//! The bridge: state shared between the audio thread, the host / GUI thread
//! and the network thread, with a handle for each.
//!
//! ```text
//!  audio thread            network thread              browser
//!  ───────────             ──────────────              ───────
//!  AudioHandle::publish ─► mailbox (latest wins) ─► StreamF32 frames ─► canvas
//!  AudioHandle::param  ◄── ParamStore (atomics)  ◄── ParamEdit frames ◄─ knob
//!  AudioHandle::drain_events ◄── to_audio_events ◄── Events frames    ◄─ keyboard
//!  AudioHandle::send_event ─► from_audio_events ─► EventsOut frames  ─► keyboard
//!
//!  host / GUI thread
//!  ─────────────────
//!  NoobVstWebguiFramework::set_param_norm ─► ParamStore + to_ui queue ─► ParamValues frames
//!  NoobVstWebguiFramework::drain_edits    ◄── to_host queue            ◄── ParamEdit frames
//!  NoobVstWebguiFramework::poll_message   ◄── inbound_json             ◄── {"t":"msg"} text
//!  NoobVstWebguiFramework::send_json      ─► outbound_json             ─► {"t":"msg"} text
//!  NoobVstWebguiFramework::store_*        ◄► store (JSON object)       ◄► store.* text
//! ```
//!
//! # Building
//!
//! [`NoobVstWebguiFramework::builder`] collects [`ParamSpec`]s and [`StreamSpec`]s; their
//! order fixes the `u16` indices used everywhere else. [`NoobVstWebguiFrameworkBuilder::build`]
//! allocates every queue and mailbox once and renders the manifest JSON, so
//! nothing after that allocates on the audio path.
//!
//! # Handles
//!
//! * [`NoobVstWebguiFramework`]: cheap to clone, for the host / GUI side. Takes short,
//!   uncontended mutexes.
//! * [`AudioHandle`]: exactly one per bridge ([`NoobVstWebguiFramework::take_audio`]),
//!   wait-free, meant to be moved into the audio callback.
//!
//! # Queues and what happens when they fill
//!
//! | queue | direction | capacity | when full |
//! |---|---|---|---|
//! | `to_ui` | plug-in → clients (parameter changes) | `ui_queue`, default 4096 | the change is dropped and counted ([`NoobVstWebguiFramework::dropped_ui_changes`]); clients resync from the store on their next full snapshot |
//! | `to_host` | clients → host (edits) | `host_queue`, default 1024 | the edit is dropped for the host (the store was already updated) |
//! | `to_audio_events` | clients → audio | 1024 | the event is dropped, the server logs it |
//! | `from_audio_events` | plug-in → clients | 1024 | `send_event` / `push_event` return `false` |
//! | `inbound_json` | clients → plug-in | 1024 | the oldest message is dropped |
//! | `outbound_json` | plug-in → clients | unbounded | never |
//!
//! # Hooks
//!
//! An [`EditHook`] replaces the `to_host` queue: it runs on the network
//! thread the moment an edit is decoded, which is the lowest-latency path to
//! a host. A [`StoreHook`] runs after every UI store change so an adapter
//! can mark its state dirty.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::Thread;
use std::time::Instant;

use crossbeam_queue::ArrayQueue;
use serde::Serialize;
use serde_json::Value;

use crate::params::{ParamManifest, ParamSpec, ParamStore};
use crate::rt::{MailboxReader, MailboxWriter, mailbox};
use crate::stream::{StreamFrame, StreamManifest, StreamSpec};
use crate::wire::{EditPhase, PARAM_FLAG_HOST, PROTOCOL_VERSION, UiEvent};

/// A parameter change heading to the UI. Produced by the `NoobVstWebguiFramework` and
/// `AudioHandle` setters and by client edits; consumed by the pump thread,
/// which turns a batch of them into one `ParamValues` frame per client.
#[derive(Debug, Clone, Copy)]
pub struct ParamChange {
    /// Dense parameter index.
    pub index: u16,
    /// Normalized.
    pub value: f32,
    /// Entry flags for the wire (`PARAM_FLAG_HOST` when the host changed it).
    /// The pump adds `PARAM_FLAG_ECHO` for the originating client.
    pub flags: u16,
    /// Id of the client whose edit caused this, or `0` when it came from the
    /// plugin side. The server marks the change as an echo for that client.
    pub origin: u16,
}

/// A parameter edit that arrived from a UI client. Forward it to the host
/// (`beginEdit` / `performEdit` / `endEdit`) so automation gets recorded.
/// The parameter store has already been updated when you see this.
#[derive(Debug, Clone, Copy)]
pub struct EditEvent {
    /// Dense parameter index.
    pub index: u16,
    /// Normalized.
    pub value: f32,
    /// Where in the gesture this edit sits.
    pub phase: EditPhase,
    /// Id of the client that sent it (never `0`).
    pub client: u16,
}

/// An ad-hoc JSON message from a UI client (`{"t":"msg","topic":…,"data":…}`
/// on the wire). Topics the server handles itself (`store.*`) never appear
/// here.
#[derive(Debug, Clone)]
pub struct Message {
    /// Routing key chosen by the page.
    pub topic: String,
    /// Payload; `Null` when the page sent none.
    pub data: Value,
    /// Id of the client that sent it, so a reply can be targeted or the
    /// sender excluded.
    pub client: u16,
}

/// Called on the network thread for every edit, *instead of* queueing it
/// for [`NoobVstWebguiFramework::drain_edits`]. The lowest-latency route to a host; the
/// hook must be quick and must not block, and it runs on a tokio task, so
/// it must not call into anything that requires a particular thread unless
/// that call is itself thread-safe (the nih-plug adapter forwards edits
/// from a UI-thread timer instead while its window is open).
pub type EditHook = Arc<dyn Fn(EditEvent) + Send + Sync>;

/// Called (on whichever thread changed it) after a store key changes, so a
/// plugin adapter can mark its state dirty. Receives the key and the new
/// value (`Null` for a removal). Runs with no locks held; keep it short.
pub type StoreHook = Arc<dyn Fn(&str, &Value) + Send + Sync>;

/// Largest serialized value a client may put in the store (256 KiB).
pub const STORE_MAX_VALUE_BYTES: usize = 256 * 1024;
/// Largest total serialized store (1 MiB): keys plus values.
pub const STORE_MAX_TOTAL_BYTES: usize = 1024 * 1024;

/// Everything the handles and the server share. One `Arc<Shared>` per
/// bridge. Fields are grouped by which side writes them; the mutexes are
/// only ever taken briefly and never on the audio thread (the audio thread
/// uses `try_lock` for the pump wake and nothing else).
pub(crate) struct Shared {
    /// Bridge name: manifest `name`, discovery record, log lines.
    pub name: String,
    /// Normalized parameter values and their specs. Lock-free.
    pub params: ParamStore,
    /// Stream declarations in index order (the pump reads `sticky`).
    pub stream_specs: Vec<StreamSpec>,
    /// The manifest, rendered once at build time and sent to every client.
    pub manifest_json: String,
    /// Origin of `now_ns` / `now_us`: when the bridge was built.
    pub epoch: Instant,
    /// Parameter changes waiting for the pump (plug-in and client edits).
    pub to_ui: ArrayQueue<ParamChange>,
    /// Client edits waiting for the host (unused while an edit hook is set).
    pub to_host: ArrayQueue<EditEvent>,
    /// Events from UI clients (notes, controllers) for the audio thread.
    pub to_audio_events: ArrayQueue<UiEvent>,
    /// Events from the plugin for the UI clients.
    pub from_audio_events: ArrayQueue<UiEvent>,
    /// Replaces the `to_host` queue when set.
    pub edit_hook: Mutex<Option<EditHook>>,
    /// Key-value store the page uses for its own state (presets, view
    /// settings). Persisted by the plugin adapter with the plugin state.
    pub store: Mutex<serde_json::Map<String, Value>>,
    /// Runs after every store change.
    pub store_hook: Mutex<Option<StoreHook>>,
    /// Text frames for the clients, with a client id to skip (0 = nobody).
    pub outbound_json: Mutex<VecDeque<(String, u16)>>,
    /// Messages from clients for the plug-in; capped at 1024, oldest dropped.
    pub inbound_json: Mutex<VecDeque<Message>>,
    /// The pump thread, if a server is running. Woken on every publish.
    pub pump: Mutex<Option<Thread>>,
    /// How many `to_ui` pushes failed because the queue was full.
    pub dropped_ui_changes: AtomicU32,
    /// Producer halves of the stream mailboxes; moved into the `AudioHandle`.
    stream_writers: Mutex<Option<Vec<MailboxWriter<StreamFrame>>>>,
    /// Consumer halves of the stream mailboxes; borrowed by the pump thread
    /// while a server runs.
    stream_readers: Mutex<Option<Vec<MailboxReader<StreamFrame>>>>,
}

impl Shared {
    /// Nanoseconds since the bridge was built (stream `ts_ns`).
    #[inline]
    pub fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    /// Microseconds since the bridge was built (Pong `server_time_us`).
    #[inline]
    pub fn now_us(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1e6
    }

    /// Wake the pump thread without blocking. Safe on the audio thread: a
    /// contended lock just skips the wake and the pump's timeout covers it.
    #[inline]
    pub fn wake(&self) {
        if let Ok(g) = self.pump.try_lock()
            && let Some(t) = g.as_ref()
        {
            t.unpark();
        }
    }

    /// Queue a parameter change for the pump and wake it. Lock-free; a full
    /// queue drops the change and bumps `dropped_ui_changes`.
    pub fn push_change(&self, change: ParamChange) {
        if self.to_ui.push(change).is_err() {
            self.dropped_ui_changes.fetch_add(1, Ordering::Relaxed);
        }
        self.wake();
    }

    /// Network-thread path for an edit from a client: apply it so the audio
    /// thread sees it on its next block, hand it to the host, and fan it out
    /// to every client (the originator receives it flagged as an echo).
    pub fn apply_client_edit(&self, index: u16, value: f32, phase: EditPhase, client: u16) {
        if !self.params.set_normalized(index as usize, value) {
            return;
        }
        let ev = EditEvent {
            index,
            value,
            phase,
            client,
        };
        let hook = self.edit_hook.lock().ok().and_then(|g| g.clone());
        match hook {
            Some(h) => h(ev),
            None => {
                let _ = self.to_host.push(ev);
            }
        }
        self.push_change(ParamChange {
            index,
            value,
            flags: 0,
            origin: client,
        });
    }

    /// Network-thread path for an event from a client: queue it for the
    /// audio thread. Never blocks; a full queue drops the event.
    pub fn push_ui_event(&self, e: UiEvent) -> bool {
        self.to_audio_events.push(e).is_ok()
    }

    /// Set a store key (from a client or the plugin). `Value::Null` removes
    /// it. Enforces the size caps, then runs the store hook.
    pub fn store_set(&self, key: &str, value: Value) -> Result<(), &'static str> {
        if key.is_empty() || key.len() > 128 {
            return Err("bad key");
        }
        let encoded = value.to_string();
        if encoded.len() > STORE_MAX_VALUE_BYTES {
            return Err("value too large");
        }
        {
            let mut g = self.store.lock().map_err(|_| "poisoned")?;
            let current: usize = g.iter().map(|(k, v)| k.len() + v.to_string().len()).sum();
            let existing = g.get(key).map(|v| v.to_string().len()).unwrap_or(0);
            if current - existing + key.len() + encoded.len() > STORE_MAX_TOTAL_BYTES {
                return Err("store full");
            }
            if value.is_null() {
                g.remove(key);
            } else {
                g.insert(key.to_string(), value.clone());
            }
        }
        let hook = self.store_hook.lock().ok().and_then(|g| g.clone());
        if let Some(h) = hook {
            h(key, &value);
        }
        Ok(())
    }

    /// The whole store as a `store.all` text frame (sent on connect and after
    /// a replace).
    pub fn store_all_json(&self) -> String {
        let map = self.store.lock().map(|g| g.clone()).unwrap_or_default();
        serde_json::json!({ "t": "msg", "topic": "store.all", "data": { "values": Value::Object(map) } }).to_string()
    }

    /// Queue a client message for the plug-in; the oldest is dropped past
    /// 1024 pending.
    pub fn push_inbound(&self, msg: Message) {
        if let Ok(mut q) = self.inbound_json.lock() {
            if q.len() >= 1024 {
                q.pop_front();
            }
            q.push_back(msg);
        }
    }

    /// Borrow the stream readers for a pump thread. `None` if a pump already
    /// holds them.
    pub fn take_stream_readers(&self) -> Option<Vec<MailboxReader<StreamFrame>>> {
        self.stream_readers.lock().ok()?.take()
    }

    /// Give the stream readers back when a pump thread exits, so a new
    /// server can be started on the same bridge.
    pub fn return_stream_readers(&self, readers: Vec<MailboxReader<StreamFrame>>) {
        if let Ok(mut g) = self.stream_readers.lock() {
            *g = Some(readers);
        }
    }

    /// Tell the bridge which thread to `unpark` on publish (`None` when the
    /// server stops).
    pub fn register_pump(&self, t: Option<Thread>) {
        if let Ok(mut g) = self.pump.lock() {
            *g = t;
        }
    }
}

/// Serialized shape of the manifest text frame (`"t": "manifest"`).
#[derive(Serialize)]
struct Manifest<'a> {
    t: &'static str,
    name: &'a str,
    protocol: u16,
    meta: &'a Value,
    params: Vec<ParamManifest>,
    streams: Vec<StreamManifest>,
}

/// Declare parameters and streams, then [`build`](NoobVstWebguiFrameworkBuilder::build).
///
/// ```
/// use noob_vst_webgui_framework::{ParamSpec, NoobVstWebguiFramework, StreamKind, StreamSpec};
///
/// let bridge = NoobVstWebguiFramework::builder("demo")
///     .meta(serde_json::json!({ "vendor": "Ely Erin Fox", "version": "0.1.0" }))
///     .param(ParamSpec::new("gain", "Gain").range(-24.0, 24.0).default(0.0).unit("dB"))
///     .param(ParamSpec::new("mode", "Mode").labels(["A", "B"]))
///     .stream(StreamSpec::new("meter", 2).kind(StreamKind::Meter).channels(2))
///     .build();
///
/// assert_eq!(bridge.param_count(), 2);
/// assert_eq!(bridge.index_of("mode"), Some(1));
/// assert_eq!(bridge.param(0), 0.0);          // plain default
/// assert_eq!(bridge.param_norm(0), 0.5);     // normalized default
/// assert!(bridge.manifest_json().contains("\"vendor\":\"Ely Erin Fox\""));
/// ```
pub struct NoobVstWebguiFrameworkBuilder {
    name: String,
    meta: Value,
    params: Vec<ParamSpec>,
    streams: Vec<StreamSpec>,
    ui_queue: usize,
    host_queue: usize,
}

impl NoobVstWebguiFrameworkBuilder {
    /// Start a bridge called `name` (the manifest `name`, the discovery
    /// record, the default port-probing base). Same as [`NoobVstWebguiFramework::builder`].
    pub fn new(name: impl Into<String>) -> Self {
        NoobVstWebguiFrameworkBuilder {
            name: name.into(),
            meta: Value::Null,
            params: Vec::new(),
            streams: Vec::new(),
            ui_queue: 4096,
            host_queue: 1024,
        }
    }

    /// Free-form metadata shipped in the manifest (version, vendor, layout hints).
    pub fn meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }

    /// Add a parameter. Its index is its position in declaration order.
    pub fn param(mut self, spec: ParamSpec) -> Self {
        self.params.push(spec);
        self
    }

    /// Add several parameters at once, in iteration order.
    pub fn params(mut self, specs: impl IntoIterator<Item = ParamSpec>) -> Self {
        self.params.extend(specs);
        self
    }

    /// Add a stream. Its index is its position in declaration order.
    pub fn stream(mut self, spec: StreamSpec) -> Self {
        self.streams.push(spec);
        self
    }

    /// Capacity of the plugin -> UI parameter change queue (default 4096,
    /// at least 16). Size it for the largest burst a preset load produces.
    pub fn ui_queue(mut self, n: usize) -> Self {
        self.ui_queue = n.max(16);
        self
    }

    /// Capacity of the UI -> host edit queue (default 1024, at least 16).
    /// Irrelevant when an edit hook is installed.
    pub fn host_queue(mut self, n: usize) -> Self {
        self.host_queue = n.max(16);
        self
    }

    /// Allocate every queue and mailbox, render the manifest and return the
    /// handle. Nothing on the audio path allocates after this.
    ///
    /// # Panics
    ///
    /// If more than `u16::MAX` parameters or streams were declared.
    pub fn build(self) -> NoobVstWebguiFramework {
        assert!(self.params.len() <= u16::MAX as usize, "too many params");
        assert!(self.streams.len() <= u16::MAX as usize, "too many streams");
        let params = ParamStore::new(self.params);
        let mut writers = Vec::with_capacity(self.streams.len());
        let mut readers = Vec::with_capacity(self.streams.len());
        for s in &self.streams {
            let cap = s.capacity;
            let (w, r) = mailbox(|| StreamFrame::with_capacity(cap));
            writers.push(w);
            readers.push(r);
        }
        let manifest = Manifest {
            t: "manifest",
            name: &self.name,
            protocol: PROTOCOL_VERSION,
            meta: &self.meta,
            params: params.manifest(),
            streams: self
                .streams
                .iter()
                .enumerate()
                .map(|(i, s)| StreamManifest::from_spec(i as u16, s))
                .collect(),
        };
        let manifest_json = serde_json::to_string(&manifest).expect("manifest serializes");
        NoobVstWebguiFramework {
            shared: Arc::new(Shared {
                name: self.name,
                params,
                stream_specs: self.streams,
                manifest_json,
                epoch: Instant::now(),
                to_ui: ArrayQueue::new(self.ui_queue),
                to_host: ArrayQueue::new(self.host_queue),
                to_audio_events: ArrayQueue::new(1024),
                from_audio_events: ArrayQueue::new(1024),
                edit_hook: Mutex::new(None),
                store: Mutex::new(serde_json::Map::new()),
                store_hook: Mutex::new(None),
                outbound_json: Mutex::new(VecDeque::new()),
                inbound_json: Mutex::new(VecDeque::new()),
                pump: Mutex::new(None),
                dropped_ui_changes: AtomicU32::new(0),
                stream_writers: Mutex::new(Some(writers)),
                stream_readers: Mutex::new(Some(readers)),
            }),
        }
    }
}

/// The plugin-side handle. Cheap to clone; use it from the host / GUI thread
/// (or anywhere that is not the audio thread).
///
/// What it does:
///
/// * **Parameters**: [`set_param_norm`](Self::set_param_norm) /
///   [`set_param`](Self::set_param) when the host changes something,
///   [`sync_all_params`](Self::sync_all_params) after a preset load,
///   [`drain_edits`](Self::drain_edits) or [`set_edit_hook`](Self::set_edit_hook)
///   to get client gestures to the host.
/// * **Messages**: [`send_json`](Self::send_json) out,
///   [`poll_message`](Self::poll_message) in.
/// * **Events**: [`push_event`](Self::push_event) out,
///   [`drain_ui_events`](Self::drain_ui_events) in (when not handled on the
///   audio thread).
/// * **UI store**: the `store_*` methods, plus [`set_store_hook`](Self::set_store_hook)
///   for persistence.
/// * **Introspection**: specs, counts, [`manifest_json`](Self::manifest_json).
///
/// All methods take at most a short uncontended mutex; none block on the
/// network. Do not call them from the audio callback: use [`AudioHandle`]
/// there.
#[derive(Clone)]
pub struct NoobVstWebguiFramework {
    pub(crate) shared: Arc<Shared>,
}

impl NoobVstWebguiFramework {
    /// Start declaring a bridge called `name`.
    pub fn builder(name: impl Into<String>) -> NoobVstWebguiFrameworkBuilder {
        NoobVstWebguiFrameworkBuilder::new(name)
    }

    /// The bridge name given to the builder.
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// The manifest as sent to every client on connect.
    pub fn manifest_json(&self) -> &str {
        &self.shared.manifest_json
    }

    /// Take the audio-thread handle. Only one exists; returns `None` if it
    /// was already taken. Take it on a non-audio thread and move it into the
    /// audio callback.
    pub fn take_audio(&self) -> Option<AudioHandle> {
        let writers = self.shared.stream_writers.lock().ok()?.take()?;
        let seqs = vec![0u32; writers.len()];
        Some(AudioHandle {
            shared: self.shared.clone(),
            writers,
            seqs,
        })
    }

    /// Give the audio handle back so it can be taken again (plugin reload).
    pub fn return_audio(&self, audio: AudioHandle) {
        if let Ok(mut g) = self.shared.stream_writers.lock() {
            *g = Some(audio.writers);
        }
    }

    /// Number of parameters.
    pub fn param_count(&self) -> usize {
        self.shared.params.len()
    }

    /// Number of streams.
    pub fn stream_count(&self) -> usize {
        self.shared.stream_specs.len()
    }

    /// The index of the parameter with id `id`, if any.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.shared.params.index_of(id)
    }

    /// The declaration of parameter `index`, if any.
    pub fn spec(&self, index: usize) -> Option<&ParamSpec> {
        self.shared.params.spec(index)
    }

    /// Every parameter declaration, in index order.
    pub fn specs(&self) -> &[ParamSpec] {
        self.shared.params.specs()
    }

    /// Every stream declaration, in index order.
    pub fn stream_specs(&self) -> &[StreamSpec] {
        &self.shared.stream_specs
    }

    /// Normalized value of parameter `index` (`0.0` for a bad index).
    pub fn param_norm(&self, index: usize) -> f32 {
        self.shared.params.get_normalized(index)
    }

    /// Plain value of parameter `index` (`0.0` for a bad index).
    pub fn param(&self, index: usize) -> f32 {
        self.shared.params.get_plain(index)
    }

    /// The host changed a parameter (automation, preset load, another UI).
    /// Stores it (clamped) and pushes it to every connected client with the
    /// HOST flag. A bad index is ignored. Does not notify the host back.
    pub fn set_param_norm(&self, index: usize, norm: f32) {
        if self.shared.params.set_normalized(index, norm) {
            self.shared.push_change(ParamChange {
                index: index as u16,
                value: norm.clamp(0.0, 1.0),
                flags: PARAM_FLAG_HOST,
                origin: 0,
            });
        }
    }

    /// Same as [`set_param_norm`](Self::set_param_norm) with a plain value.
    pub fn set_param(&self, index: usize, plain: f32) {
        if let Some(n) = self.shared.params.set_plain(index, plain) {
            self.shared.push_change(ParamChange {
                index: index as u16,
                value: n,
                flags: PARAM_FLAG_HOST,
                origin: 0,
            });
        }
    }

    /// Push every current value to the clients with the HOST flag (after a
    /// preset load, or when parameters were changed without going through
    /// this handle). Needs `param_count()` free slots in the UI queue.
    pub fn sync_all_params(&self) {
        for i in 0..self.shared.params.len() {
            self.shared.push_change(ParamChange {
                index: i as u16,
                value: self.shared.params.get_normalized(i),
                flags: PARAM_FLAG_HOST,
                origin: 0,
            });
        }
    }

    /// Drain edits that arrived from UI clients, oldest first, calling `f`
    /// for each. No-op if an edit hook is set. Lock-free; call it from a
    /// timer or the host's idle callback.
    pub fn drain_edits(&self, mut f: impl FnMut(EditEvent)) {
        while let Some(ev) = self.shared.to_host.pop() {
            f(ev);
        }
    }

    /// Install a hook called on the network thread for every edit, instead of
    /// queueing it for [`drain_edits`](Self::drain_edits). Lowest latency path
    /// to the host; the hook must be quick and must not block.
    pub fn set_edit_hook(&self, hook: Option<EditHook>) {
        if let Ok(mut g) = self.shared.edit_hook.lock() {
            *g = hook;
        }
    }

    /// Send an ad-hoc JSON message to every client as
    /// `{"t":"msg","topic":...,"data":...}`. Queued for the pump (the queue
    /// is unbounded) and delivered on its next cycle.
    pub fn send_json(&self, topic: &str, data: Value) {
        let text = serde_json::json!({ "t": "msg", "topic": topic, "data": data }).to_string();
        if let Ok(mut q) = self.shared.outbound_json.lock() {
            q.push_back((text, 0));
        }
        self.shared.wake();
    }

    /// Send an event to every client from a non-audio thread (e.g. host
    /// MIDI the on-screen keyboard should light up). Real-time safe too.
    /// Returns `false` if the 1024-event queue was full.
    pub fn push_event(&self, e: UiEvent) -> bool {
        let ok = self.shared.from_audio_events.push(e).is_ok();
        self.shared.wake();
        ok
    }

    /// Drain events that UI clients sent (for plugins that handle them on a
    /// non-audio thread). The audio thread normally uses
    /// [`AudioHandle::drain_events`] instead.
    pub fn drain_ui_events(&self, mut f: impl FnMut(UiEvent)) {
        while let Some(e) = self.shared.to_audio_events.pop() {
            f(e);
        }
    }

    // -- UI store -----------------------------------------------------------

    /// A value the page stored under `key` (cloned), if any.
    pub fn store_get(&self, key: &str) -> Option<Value> {
        self.shared.store.lock().ok()?.get(key).cloned()
    }

    /// Set a key from the plugin side (`Null` removes it) and tell every
    /// client with a `store.changed` message. Runs the store hook.
    ///
    /// # Errors
    ///
    /// `"bad key"` (empty or over 128 bytes), `"value too large"` (over
    /// [`STORE_MAX_VALUE_BYTES`] serialized) or `"store full"` (the store
    /// would exceed [`STORE_MAX_TOTAL_BYTES`]). Nothing is changed then.
    pub fn store_set(&self, key: &str, value: Value) -> Result<(), &'static str> {
        self.shared.store_set(key, value.clone())?;
        self.send_json(
            "store.changed",
            serde_json::json!({ "key": key, "value": value }),
        );
        Ok(())
    }

    /// The whole store, cloned.
    pub fn store_snapshot(&self) -> serde_json::Map<String, Value> {
        self.shared
            .store
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// The whole store as compact JSON text (what a plugin adapter
    /// persists). `{}` when empty.
    pub fn store_json(&self) -> String {
        Value::Object(self.store_snapshot()).to_string()
    }

    /// Replace the whole store (e.g. when the host restores plugin state)
    /// and push it to every client as `store.all`. No size checks and no
    /// store hook: the caller is the persistence layer.
    pub fn store_replace(&self, values: serde_json::Map<String, Value>) {
        if let Ok(mut g) = self.shared.store.lock() {
            *g = values;
        }
        let text = self.shared.store_all_json();
        if let Ok(mut q) = self.shared.outbound_json.lock() {
            q.push_back((text, 0));
        }
        self.shared.wake();
    }

    /// [`store_replace`](Self::store_replace) from JSON text. Valid JSON that
    /// is not an object loads as an empty store.
    ///
    /// # Errors
    ///
    /// The text is not valid JSON; the store is left untouched.
    pub fn store_load_json(&self, json: &str) -> Result<(), serde_json::Error> {
        let v: Value = serde_json::from_str(json)?;
        self.store_replace(v.as_object().cloned().unwrap_or_default());
        Ok(())
    }

    /// Install (or with `None` remove) the hook called after any store change
    /// (client or plugin side), so a persistence layer can mark itself dirty.
    pub fn set_store_hook(&self, hook: Option<StoreHook>) {
        if let Ok(mut g) = self.shared.store_hook.lock() {
            *g = hook;
        }
    }

    /// Next ad-hoc JSON message from a client, oldest first, if any. Topics
    /// the server handles itself (`store.*`) never show up here.
    ///
    /// # Something has to call this, and in a plug-in nothing does
    ///
    /// This is a queue, not a callback. A standalone has a main loop to drain
    /// it from; a plug-in has an audio thread and an editor thread and nothing
    /// else, so a message sent from the page sits here for ever unless the
    /// plug-in arranges to poll it. That makes this the worst shape of bug
    /// available: a feature built on messages works perfectly against the
    /// development server and silently does nothing inside the host, and every
    /// test that can be run on the page's side passes.
    ///
    /// So before reaching for a message, ask whether the **UI store** will do
    /// instead. [`set_store_hook`](Self::set_store_hook) fires on every client
    /// write with no polling at all, the store is already persisted with the
    /// plug-in state, and a value written there survives a reload with no
    /// editor open. A plug-in that needs the page to hand it structured data
    /// almost always wants that rather than this.
    ///
    /// Messages remain right for things that are genuinely transient and
    /// genuinely have a reader — a standalone's own commands, or a layer that
    /// polls deliberately and uses [`requeue_message`](Self::requeue_message)
    /// to leave the rest alone.
    pub fn poll_message(&self) -> Option<Message> {
        self.shared.inbound_json.lock().ok()?.pop_front()
    }

    /// Put a polled message back at the front of the queue (for a layer that
    /// only handles some topics and leaves the rest to the plugin).
    pub fn requeue_message(&self, msg: Message) {
        if let Ok(mut q) = self.shared.inbound_json.lock() {
            q.push_front(msg);
        }
    }

    /// Microseconds since the bridge was created; the clock stream timestamps
    /// and `Pong` frames use.
    pub fn now_us(&self) -> f64 {
        self.shared.now_us()
    }

    /// Number of plugin -> UI changes dropped because the queue was full
    /// since the bridge was built. A non-zero value means `ui_queue` is too
    /// small for the bursts the plug-in produces.
    pub fn dropped_ui_changes(&self) -> u32 {
        self.shared.dropped_ui_changes.load(Ordering::Relaxed)
    }
}

/// The audio-thread handle. Nothing here allocates, locks or blocks.
///
/// Exactly one exists per bridge ([`NoobVstWebguiFramework::take_audio`]). Typical use in
/// a process callback:
///
/// ```no_run
/// # use noob_vst_webgui_framework::{AudioHandle, UiEvent, event_kind};
/// # fn process(audio: &mut AudioHandle, block: &mut [f32]) {
/// let cutoff = audio.param(0);                 // plain units, one atomic load
/// audio.drain_events(|e| {                     // notes from the on-screen keyboard
///     if e.kind == event_kind::NOTE_ON { /* start a voice */ }
/// });
/// // ... render the block ...
/// audio.publish(1, |out| {                     // fill the next spectrum frame in place
///     let n = out.len().min(1025);
///     for v in &mut out[..n] { *v = -90.0; }
///     n
/// });
/// audio.publish_slice(0, &[0.5, 0.4]);         // a two-channel meter
/// # }
/// ```
pub struct AudioHandle {
    shared: Arc<Shared>,
    /// Producer half of every stream mailbox, in stream order.
    writers: Vec<MailboxWriter<StreamFrame>>,
    /// Per-stream publish counters (the frame `seq`).
    seqs: Vec<u32>,
}

impl AudioHandle {
    /// Plain value of a parameter: one relaxed load plus the taper math.
    /// `0.0` for a bad index.
    #[inline]
    pub fn param(&self, index: usize) -> f32 {
        self.shared.params.get_plain(index)
    }

    /// Normalized value of a parameter: one relaxed load. `0.0` for a bad
    /// index.
    #[inline]
    pub fn param_norm(&self, index: usize) -> f32 {
        self.shared.params.get_normalized(index)
    }

    /// Change a parameter from the audio thread (internal modulation the UI
    /// should display). Pushes the change to the clients with no flags; does
    /// not notify the host. A bad index is ignored.
    #[inline]
    pub fn set_param_norm(&self, index: usize, norm: f32) {
        if self.shared.params.set_normalized(index, norm) {
            self.shared.push_change(ParamChange {
                index: index as u16,
                value: norm.clamp(0.0, 1.0),
                flags: 0,
                origin: 0,
            });
        }
    }

    /// Fill and publish one frame of a stream. `fill` receives the slot's
    /// full capacity (holding whatever was in that slot two publishes ago)
    /// and returns how many values it wrote; the count is clamped to the
    /// capacity. Stamps `seq` and `ts_ns`, then wakes the pump. Wait-free.
    /// Returns `false` for an unknown stream index.
    ///
    /// # Non-finite values
    ///
    /// Debug builds assert that every value written is finite. A NaN or an
    /// infinity means the plug-in's own processing has come apart, and
    /// passing it on helps nobody: the wire carries it faithfully, and a
    /// page draws a blank meter or an empty curve with no clue why. Release
    /// builds do not check, so this costs a development build a pass over
    /// the frame and a shipped one nothing.
    #[inline]
    pub fn publish(&mut self, stream: usize, fill: impl FnOnce(&mut [f32]) -> usize) -> bool {
        let Some(w) = self.writers.get_mut(stream) else {
            return false;
        };
        let seq = {
            let s = &mut self.seqs[stream];
            *s = s.wrapping_add(1);
            *s
        };
        let ts = self.shared.now_ns();
        let frame = w.slot();
        let n = fill(&mut frame.data).min(frame.data.len());
        debug_assert!(
            frame.data[..n].iter().all(|v| v.is_finite()),
            "stream {stream}: published a non-finite value; the plug-in's processing has come apart"
        );
        frame.len = n;
        frame.seq = seq;
        frame.ts_ns = ts;
        w.publish();
        self.shared.wake();
        true
    }

    /// Publish a copy of `data` (truncated to the stream's capacity). See
    /// [`publish`](Self::publish).
    #[inline]
    pub fn publish_slice(&mut self, stream: usize, data: &[f32]) -> bool {
        self.publish(stream, |out| {
            let n = data.len().min(out.len());
            out[..n].copy_from_slice(&data[..n]);
            n
        })
    }

    /// Nanoseconds since the bridge was created (the clock frames are
    /// stamped with).
    #[inline]
    pub fn now_ns(&self) -> u64 {
        self.shared.now_ns()
    }

    /// Number of streams this handle can publish to.
    pub fn stream_count(&self) -> usize {
        self.writers.len()
    }

    /// Events sent by UI clients since the last call (notes from an
    /// on-screen keyboard, controllers, custom). Call once per block.
    /// Lock-free; never blocks.
    #[inline]
    pub fn drain_events(&self, mut f: impl FnMut(UiEvent)) {
        while let Some(e) = self.shared.to_audio_events.pop() {
            f(e);
        }
    }

    /// Send an event to the UI from the audio thread (a note the host
    /// played, a trigger for a visual). Lock-free; a full (1024-event) queue
    /// drops it and returns `false`.
    #[inline]
    pub fn send_event(&self, e: UiEvent) -> bool {
        let ok = self.shared.from_audio_events.push(e).is_ok();
        self.shared.wake();
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StreamKind;

    fn bridge() -> NoobVstWebguiFramework {
        NoobVstWebguiFramework::builder("test")
            .param(
                ParamSpec::new("gain", "Gain")
                    .range(-24.0, 24.0)
                    .default(0.0),
            )
            .param(
                ParamSpec::new("freq", "Freq")
                    .range(20.0, 20000.0)
                    .log()
                    .default(1000.0),
            )
            .stream(
                StreamSpec::new("meter", 2)
                    .kind(StreamKind::Meter)
                    .channels(2),
            )
            .build()
    }

    #[test]
    fn manifest_has_everything() {
        let s = bridge();
        let v: Value = serde_json::from_str(s.manifest_json()).unwrap();
        assert_eq!(v["t"], "manifest");
        assert_eq!(v["name"], "test");
        assert_eq!(v["protocol"], PROTOCOL_VERSION);
        assert_eq!(v["params"].as_array().unwrap().len(), 2);
        assert_eq!(v["params"][1]["taper"], "log");
        assert_eq!(v["streams"][0]["kind"], "meter");
        assert_eq!(v["streams"][0]["channels"], 2);
    }

    #[test]
    fn host_changes_reach_ui_queue_with_host_flag() {
        let s = bridge();
        s.set_param(0, 12.0);
        let c = s.shared.to_ui.pop().unwrap();
        assert_eq!(c.index, 0);
        assert!((c.value - 0.75).abs() < 1e-6);
        assert_eq!(c.flags, PARAM_FLAG_HOST);
        assert!((s.param(0) - 12.0).abs() < 1e-4);
    }

    #[test]
    fn client_edits_apply_queue_and_fan_out() {
        let s = bridge();
        s.shared.apply_client_edit(1, 0.5, EditPhase::Begin, 7);
        assert!((s.param_norm(1) - 0.5).abs() < 1e-6);
        let mut got = Vec::new();
        s.drain_edits(|e| got.push(e));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].client, 7);
        assert_eq!(got[0].phase, EditPhase::Begin);
        let c = s.shared.to_ui.pop().unwrap();
        assert_eq!(c.origin, 7);
        assert_eq!(c.flags, 0);
    }

    #[test]
    fn edit_hook_bypasses_queue() {
        let s = bridge();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        s.set_edit_hook(Some(Arc::new(move |e: EditEvent| {
            seen2.lock().unwrap().push(e.index);
        })));
        s.shared.apply_client_edit(0, 0.1, EditPhase::Perform, 1);
        assert_eq!(seen.lock().unwrap().as_slice(), &[0]);
        let mut n = 0;
        s.drain_edits(|_| n += 1);
        assert_eq!(n, 0);
    }

    #[test]
    fn audio_handle_publishes_and_is_unique() {
        let s = bridge();
        let mut a = s.take_audio().unwrap();
        assert!(s.take_audio().is_none());
        assert!(a.publish_slice(0, &[0.5, 0.25, 9.0]));
        assert!(!a.publish_slice(3, &[0.0]));
        let mut readers = s.shared.take_stream_readers().unwrap();
        let f = readers[0].read().unwrap();
        assert_eq!(f.seq, 1);
        assert_eq!(f.samples(), &[0.5, 0.25]);
        assert!(readers[0].read().is_none());
        s.return_audio(a);
        assert!(s.take_audio().is_some());
    }

    #[test]
    fn json_messages_round_trip_through_queues() {
        let s = bridge();
        s.send_json("preset", serde_json::json!({"name": "Init"}));
        let (text, exclude) = s.shared.outbound_json.lock().unwrap().pop_front().unwrap();
        assert_eq!(exclude, 0);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["t"], "msg");
        assert_eq!(v["topic"], "preset");
        s.shared.push_inbound(Message {
            topic: "hello".into(),
            data: Value::Null,
            client: 2,
        });
        assert_eq!(s.poll_message().unwrap().topic, "hello");
        assert!(s.poll_message().is_none());
    }
}
