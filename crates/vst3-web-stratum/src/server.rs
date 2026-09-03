//! The local HTTP + WebSocket server and the pump thread that feeds it.
//!
//! Two threads are spawned by [`serve`]:
//!
//! * `vst3-web-stratum-net`: a single-threaded tokio runtime running axum. Accepts
//!   connections on the loopback interface, serves the UI assets and the
//!   built-in client library, and handles inbound frames (edits, events,
//!   pings, subscriptions) directly on the socket task with no extra hops.
//! * `vst3-web-stratum-pump`: a plain thread that sleeps until the audio thread
//!   publishes something (see [`WakeMode`]), then drains the parameter
//!   change queue, the outbound event and text queues and every stream
//!   mailbox, encodes each frame once, and hands the bytes to each client's
//!   writer task with `try_send`, never blocking.
//!
//! # Routes
//!
//! | path | what |
//! |---|---|
//! | `GET /ws` | the WebSocket (see `docs/WIRE.md`) |
//! | `GET /instance` | this server's [`discovery::Instance`] as JSON |
//! | `GET /instances` | every live instance on the machine, validated |
//! | `GET /vst3-web-stratum/<file>` | the browser library baked into the binary ([`CLIENT_ASSETS`]) |
//! | anything else | the configured [`Assets`]; `/` maps to `index.html` |
//!
//! Every response is `Cache-Control: no-store`; paths containing `..`, `\`
//! or `:` are refused.
//!
//! # Per-client state and delivery
//!
//! Each connection gets a `u16` id (never `0`), a bounded outbound queue
//! ([`ServerConfig::send_queue`]) drained by its own writer task, a
//! per-stream throttle set through `Subscribe` frames, and a *needs full
//! sync* flag. The pump uses `try_send` everywhere: a parameter or event
//! frame that does not fit sets the flag and the client receives a complete
//! `ParamValues` snapshot on the next cycle, so it can never drift; a stream
//! frame that does not fit is simply skipped for that client. Sticky
//! streams keep their last encoded frame in the client registry and replay
//! it during the handshake.
//!
//! # Security model
//!
//! There is no authentication. The server only ever binds the loopback
//! interface ([`ServerConfig::ip`]); anything on the machine that can open a
//! loopback socket can drive the plug-in, which is the same trust level as
//! the plug-in's own process.
//!
//! # Lifecycle
//!
//! [`serve`] binds the port synchronously (so the caller learns it right
//! away), publishes the discovery record, starts both threads and returns a
//! [`ServerHandle`]. Dropping or [`shutdown`](ServerHandle::shutdown)-ing
//! the handle removes the record, stops the pump (returning the stream
//! readers to the bridge so a new server can be started later), closes every
//! socket and joins both threads.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener as StdListener};
use std::path::PathBuf;

use crate::discovery;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message as WsMessage, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::serve::ListenerExt;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};

use crate::bridge::{Message, ParamChange, Shared, Vst3WebStratum};
use crate::wire::{self, Frame, PARAM_FLAG_ECHO};

/// How the pump thread learns that the audio thread published something.
///
/// In both modes the pump also wakes on [`ServerConfig::poll_interval`], so
/// nothing is ever stuck; the mode only decides whether the audio thread
/// nudges it sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeMode {
    /// The audio thread calls `Thread::unpark` on the pump after every
    /// publish. That is an atomic swap plus, only if the pump is actually
    /// asleep, one lightweight wake syscall (`WakeByAddressSingle` /
    /// `futex`). Lowest latency; the default.
    Unpark,
    /// The audio thread never touches the pump; the pump wakes every
    /// [`ServerConfig::poll_interval`] on its own. Use this if your
    /// real-time policy forbids any syscall on the audio thread.
    Poll,
}

/// Where the UI's files come from. Paths are matched exactly against the
/// request path without its leading slash; `/` and paths ending in `/`
/// become `index.html`. Content types come from the file extension.
pub enum Assets {
    /// Serve nothing but `/ws` and the built-in client library under
    /// `/vst3-web-stratum/`. Point a dev server at it, or use this while prototyping.
    None,
    /// Serve files from a directory, re-read on every request (edit and
    /// refresh). For development.
    Dir(PathBuf),
    /// Serve files baked into the binary as `(path, bytes)` pairs, e.g. via
    /// `include_bytes!`. Paths are relative, no leading slash; `index.html`
    /// answers `/`. For shipping a plugin.
    Embedded(&'static [(&'static str, &'static [u8])]),
    /// Serve files from a lookup function, e.g. an `include_dir!` tree:
    /// `Assets::Lookup(|p| UI.get_file(p).map(|f| f.contents()))`.
    Lookup(fn(&str) -> Option<&'static [u8]>),
}

/// How the server picks its port.
///
/// The choice matters beyond collisions: the page's *origin* is
/// `http://127.0.0.1:<port>`, and the browser keys its own storage
/// (localStorage, IndexedDB, permissions) by origin. A stable port keeps
/// that storage attached to the same plug-in from one session to the next.
///
/// ```
/// use vst3_web_stratum::PortPolicy;
///
/// let p = PortPolicy::for_name("my-plugin");
/// let PortPolicy::Probe { base, span } = p else { unreachable!() };
/// assert!((49_152..64_152).contains(&base));
/// assert_eq!(span, 64);
/// assert_eq!(p, PortPolicy::for_name("my-plugin"));      // deterministic
/// assert_ne!(p, PortPolicy::for_name("other-plugin"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortPolicy {
    /// Let the OS pick a free port. Every instance differs; the page's
    /// origin (and so its browser storage) changes on every launch.
    Ephemeral,
    /// Exactly this port; fail if it is taken.
    Fixed(u16),
    /// Try `base`, `base + 1`, … `base + span - 1` in turn, then fall back to
    /// an ephemeral port. The first instance of a plug-in lands on the same
    /// port every time, later instances take the next free ones.
    Probe { base: u16, span: u16 },
}

impl PortPolicy {
    /// A probe policy whose base is a stable hash of `name` inside the
    /// dynamic port range, so different plug-ins keep out of each other's
    /// way and the same plug-in is predictable: FNV-1a of the name into
    /// `49152..=64151`, span 64. Two names may still collide (15000 slots);
    /// probing then just makes them share a range.
    pub fn for_name(name: &str) -> PortPolicy {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        PortPolicy::Probe {
            base: 49_152 + (h % 15_000) as u16,
            span: 64,
        }
    }
}

/// Everything [`serve`] needs. Start from [`ServerConfig::default`] and
/// chain the builders; every field is also public for direct construction.
///
/// ```
/// use std::time::Duration;
/// use vst3_web_stratum::{ServerConfig, WakeMode};
///
/// let cfg = ServerConfig::default()
///     .prefer_port(4242)           // 4242, or the next free one up to 4273
///     .assets_dir("web/dist")      // re-read on every request while developing
///     .discovery(true)
///     .wake(WakeMode::Unpark)
///     .poll_interval(Duration::from_millis(1));
/// assert!(cfg.echo_edits);
/// ```
///
/// Defaults: loopback, ephemeral port, discovery on, no assets, echoes on,
/// `Unpark`, 1 ms, 256-message send queue, 1 MiB max message.
pub struct ServerConfig {
    /// Never a non-loopback address: there is no authentication. Default
    /// `127.0.0.1`.
    pub ip: IpAddr,
    /// How to pick the port. Default [`PortPolicy::Ephemeral`]; plug-ins
    /// should prefer [`PortPolicy::for_name`], standalones
    /// [`prefer_port`](Self::prefer_port).
    pub port: PortPolicy,
    /// Advertise this instance in the per-user discovery directory and
    /// answer `/instances`. Default `true`.
    pub discovery: bool,
    /// Where the page's files come from. Default [`Assets::None`].
    pub assets: Assets,
    /// Send a client's own edits back to it flagged as echoes. Lets the UI
    /// measure round-trip latency; costs one tiny frame per edit. Default
    /// `true`.
    pub echo_edits: bool,
    /// How the audio thread wakes the pump. Default [`WakeMode::Unpark`].
    pub wake: WakeMode,
    /// Pump wake period in `Poll` mode, and the fallback timeout in `Unpark`
    /// mode. Note that on Windows the fallback timeout is subject to the
    /// system timer resolution unless the host has raised it. Default 1 ms.
    pub poll_interval: Duration,
    /// Per-client outbound queue length, in messages. When it fills, stream
    /// frames are skipped and parameter frames trigger a full resync.
    /// Default 256.
    pub send_queue: usize,
    /// Largest inbound WebSocket message accepted; larger ones close the
    /// connection. Default 1 MiB (enough for the largest `store.set`).
    pub max_message_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            ip: IpAddr::from([127, 0, 0, 1]),
            port: PortPolicy::Ephemeral,
            discovery: true,
            assets: Assets::None,
            echo_edits: true,
            wake: WakeMode::Unpark,
            poll_interval: Duration::from_millis(1),
            send_queue: 256,
            max_message_size: 1 << 20,
        }
    }
}

impl ServerConfig {
    /// Serve the page from a directory on disk ([`Assets::Dir`]).
    pub fn assets_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.assets = Assets::Dir(dir.into());
        self
    }
    /// Serve the page from `(path, bytes)` pairs baked into the binary
    /// ([`Assets::Embedded`]).
    pub fn embedded(mut self, files: &'static [(&'static str, &'static [u8])]) -> Self {
        self.assets = Assets::Embedded(files);
        self
    }
    /// Exactly this port (fails if taken); `0` means ephemeral.
    pub fn port(mut self, port: u16) -> Self {
        self.port = if port == 0 {
            PortPolicy::Ephemeral
        } else {
            PortPolicy::Fixed(port)
        };
        self
    }
    /// Try this port first, then the next 31, then anything free.
    pub fn prefer_port(mut self, port: u16) -> Self {
        self.port = PortPolicy::Probe {
            base: port,
            span: 32,
        };
        self
    }
    /// Let the OS pick a free port ([`PortPolicy::Ephemeral`]).
    pub fn ephemeral(mut self) -> Self {
        self.port = PortPolicy::Ephemeral;
        self
    }
    /// Use an explicit [`PortPolicy`].
    pub fn port_policy(mut self, policy: PortPolicy) -> Self {
        self.port = policy;
        self
    }
    /// Turn the discovery record on or off.
    pub fn discovery(mut self, on: bool) -> Self {
        self.discovery = on;
        self
    }
    /// Set the [`WakeMode`].
    pub fn wake(mut self, wake: WakeMode) -> Self {
        self.wake = wake;
        self
    }
    /// Set the pump's poll period / fallback timeout.
    pub fn poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }
    /// Turn edit echoes on or off.
    pub fn echo_edits(mut self, echo: bool) -> Self {
        self.echo_edits = echo;
        self
    }
}

/// The browser-side library, baked in and served under `/vst3-web-stratum/`, so a
/// page can `import '/vst3-web-stratum/vst3-web-stratum.js'` with no bundler. The files are
/// the ones in `web/` at the repository root at build time.
pub const CLIENT_ASSETS: &[(&str, &[u8])] = &[
    (
        "vst3-web-stratum.js",
        include_bytes!("../web/vst3-web-stratum.js"),
    ),
    (
        "components/knob.js",
        include_bytes!("../web/components/knob.js"),
    ),
    (
        "components/meter.js",
        include_bytes!("../web/components/meter.js"),
    ),
    (
        "components/spectrum.js",
        include_bytes!("../web/components/spectrum.js"),
    ),
    (
        "components/eqcurve.js",
        include_bytes!("../web/components/eqcurve.js"),
    ),
    (
        "components/scope.js",
        include_bytes!("../web/components/scope.js"),
    ),
    (
        "components/index.js",
        include_bytes!("../web/components/index.js"),
    ),
];

// ---------------------------------------------------------------------------
// Client registry
// ---------------------------------------------------------------------------

/// Sentinel in `Client::subs`: the client turned this stream off.
const SUB_DISABLED: u32 = u32::MAX;

/// One connected client, shared between its socket task (which decodes
/// inbound frames and updates `subs`) and the pump (which sends).
struct Client {
    /// Connection id, `1..`, handed out by `Registry::next_id`.
    id: u16,
    /// Outbound queue; the writer task drains it into the socket.
    tx: mpsc::Sender<WsMessage>,
    /// Per stream: minimum interval between frames in µs; `SUB_DISABLED` off.
    subs: Box<[AtomicU32]>,
    /// Per stream: when the pump last sent a frame, in µs. Pump-owned.
    last_sent: Box<[AtomicU64]>,
    /// Set when a parameter frame had to be dropped; the pump then sends a
    /// full snapshot so the client cannot drift.
    needs_full_sync: AtomicBool,
}

impl Client {
    fn new(id: u16, tx: mpsc::Sender<WsMessage>, streams: usize) -> Self {
        Client {
            id,
            tx,
            subs: (0..streams).map(|_| AtomicU32::new(0)).collect(),
            last_sent: (0..streams).map(|_| AtomicU64::new(0)).collect(),
            needs_full_sync: AtomicBool::new(false),
        }
    }
}

/// The set of connected clients. The socket tasks add and remove; the pump
/// keeps a private snapshot and refreshes it only when `generation`
/// changes, so the hot loop never takes the `clients` lock while nobody is
/// connecting or leaving.
struct Registry {
    clients: Mutex<Vec<Arc<Client>>>,
    /// Bumped on every add / remove; the pump compares it to its copy.
    generation: AtomicU64,
    /// Next client id; `0` is skipped because it means "the plug-in side".
    next_id: AtomicU16,
    /// Cached `clients.len()` for `ServerHandle::client_count`.
    count: AtomicUsize,
    /// Last encoded frame of every sticky stream, replayed to new clients.
    sticky: Mutex<Vec<Option<Bytes>>>,
}

impl Registry {
    fn new(streams: usize) -> Self {
        Registry {
            clients: Mutex::new(Vec::new()),
            generation: AtomicU64::new(1),
            next_id: AtomicU16::new(1),
            count: AtomicUsize::new(0),
            sticky: Mutex::new(vec![None; streams]),
        }
    }
    fn remember(&self, stream: usize, frame: &Bytes) {
        if let Ok(mut g) = self.sticky.lock()
            && let Some(slot) = g.get_mut(stream)
        {
            *slot = Some(frame.clone());
        }
    }
    fn sticky_frames(&self) -> Vec<Bytes> {
        self.sticky
            .lock()
            .map(|g| g.iter().flatten().cloned().collect())
            .unwrap_or_default()
    }
    fn next_id(&self) -> u16 {
        loop {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id != 0 {
                return id;
            }
        }
    }
    fn add(&self, c: Arc<Client>) {
        if let Ok(mut g) = self.clients.lock() {
            g.push(c);
            self.count.store(g.len(), Ordering::Relaxed);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }
    fn remove(&self, id: u16) {
        if let Ok(mut g) = self.clients.lock() {
            g.retain(|c| c.id != id);
            self.count.store(g.len(), Ordering::Relaxed);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }
    fn snapshot(&self, gen_seen: &mut u64, into: &mut Vec<Arc<Client>>) {
        let current = self.generation.load(Ordering::Acquire);
        if current != *gen_seen {
            if let Ok(g) = self.clients.lock() {
                into.clear();
                into.extend(g.iter().cloned());
            }
            *gen_seen = current;
        }
    }
}

// ---------------------------------------------------------------------------
// Pump thread
// ---------------------------------------------------------------------------

/// The pump's copy of the relevant `ServerConfig` fields.
struct PumpCfg {
    wake: WakeMode,
    poll_interval: Duration,
    echo_edits: bool,
}

/// Body of the `vst3-web-stratum-pump` thread. One cycle:
///
/// 1. Sleep until woken (`park_timeout`) or until the poll interval passes.
/// 2. Gather work without touching any client: drain up to 4096 parameter
///    changes, take the outbound texts, count dirty stream mailboxes, and
///    batch up to 512 plug-in events into one `EventsOut` frame. An idle
///    wake ends here.
/// 3. Refresh the client snapshot if the registry changed. With no clients,
///    consume the mailboxes anyway (remembering sticky frames) and go back
///    to sleep.
/// 4. Parameter changes: one `ParamValues` frame *per client*, because the
///    ECHO flag depends on who is receiving. A full queue marks the client
///    for a full sync.
/// 5. Full snapshots for clients marked in an earlier cycle.
/// 6. Streams: each dirty mailbox is encoded once into shared `Bytes` and
///    sent to every client whose throttle allows it. A full queue skips the
///    frame.
/// 7. The events frame to everyone (full queue marks for full sync).
/// 8. Texts, skipping the client each one originated from.
///
/// Nothing here blocks on a client: every send is `try_send`. On exit the
/// stream readers go back to the bridge.
fn pump_loop(shared: Arc<Shared>, registry: Arc<Registry>, stop: Arc<AtomicBool>, cfg: PumpCfg) {
    let mut readers = shared.take_stream_readers().unwrap_or_default();
    let nstreams = readers.len();
    let mut clients: Vec<Arc<Client>> = Vec::new();
    let mut gen_seen = 0u64;
    let mut changes: Vec<ParamChange> = Vec::with_capacity(512);
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut texts: Vec<(String, u16)> = Vec::new();

    while !stop.load(Ordering::Acquire) {
        match cfg.wake {
            WakeMode::Unpark => thread::park_timeout(cfg.poll_interval),
            WakeMode::Poll => thread::sleep(cfg.poll_interval),
        }
        if stop.load(Ordering::Acquire) {
            break;
        }

        // Gather work first so an idle wake costs almost nothing.
        changes.clear();
        while let Some(c) = shared.to_ui.pop() {
            changes.push(c);
            if changes.len() >= 4096 {
                break;
            }
        }
        texts.clear();
        if let Ok(mut q) = shared.outbound_json.lock() {
            texts.extend(q.drain(..));
        }
        let mut dirty_streams = 0usize;
        for r in &readers {
            if r.is_dirty() {
                dirty_streams += 1;
            }
        }
        // Events from the plugin for the UI: one frame for everyone.
        let mut events_frame: Option<Bytes> = None;
        if !shared.from_audio_events.is_empty() {
            let mut w = wire::EventsWriter::begin(&mut buf, true);
            while let Some(e) = shared.from_audio_events.pop() {
                w.push(e);
                if w.len() >= 512 {
                    break;
                }
            }
            if w.finish() > 0 {
                events_frame = Some(Bytes::copy_from_slice(&buf));
            }
        }
        if changes.is_empty() && texts.is_empty() && dirty_streams == 0 && events_frame.is_none() {
            continue;
        }

        registry.snapshot(&mut gen_seen, &mut clients);
        if clients.is_empty() {
            // Nobody is listening: consume the mailboxes and drop the
            // changes on the floor, but keep sticky frames for later.
            for (si, r) in readers.iter_mut().enumerate() {
                if let Some(frame) = r.read()
                    && shared.stream_specs[si].sticky
                {
                    wire::encode_stream_f32(
                        &mut buf,
                        si as u16,
                        frame.seq,
                        frame.ts_ns,
                        frame.samples(),
                    );
                    registry.remember(si, &Bytes::copy_from_slice(&buf));
                }
            }
            continue;
        }
        let now_us = shared.now_us() as u64;

        // 1. Parameter changes. Flags differ per client (echo), so the frame
        //    is built per client; batches are small.
        if !changes.is_empty() {
            for c in &clients {
                let mut w = wire::ParamValuesWriter::begin(&mut buf);
                for ch in &changes {
                    if ch.origin == c.id {
                        if !cfg.echo_edits {
                            continue;
                        }
                        w.push(ch.index, ch.flags | PARAM_FLAG_ECHO, ch.value);
                    } else {
                        w.push(ch.index, ch.flags, ch.value);
                    }
                }
                if w.finish() == 0 {
                    continue;
                }
                match c
                    .tx
                    .try_send(WsMessage::Binary(Bytes::copy_from_slice(&buf)))
                {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        c.needs_full_sync.store(true, Ordering::Relaxed);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {}
                }
            }
        }

        // 2. Full snapshots for clients that fell behind.
        for c in &clients {
            if c.needs_full_sync.swap(false, Ordering::Relaxed) {
                encode_full_snapshot(&shared, &mut buf);
                if c.tx
                    .try_send(WsMessage::Binary(Bytes::copy_from_slice(&buf)))
                    .is_err()
                {
                    c.needs_full_sync.store(true, Ordering::Relaxed);
                }
            }
        }

        // 3. Streams. Encoded once, shared by reference across clients.
        if dirty_streams > 0 {
            for (si, r) in readers.iter_mut().enumerate() {
                let Some(frame) = r.read() else { continue };
                wire::encode_stream_f32(
                    &mut buf,
                    si as u16,
                    frame.seq,
                    frame.ts_ns,
                    frame.samples(),
                );
                let bytes = Bytes::copy_from_slice(&buf);
                if shared.stream_specs[si].sticky {
                    registry.remember(si, &bytes);
                }
                for c in &clients {
                    let min = c.subs[si].load(Ordering::Relaxed);
                    if min == SUB_DISABLED {
                        continue;
                    }
                    let last = c.last_sent[si].load(Ordering::Relaxed);
                    if min != 0 && now_us.saturating_sub(last) < min as u64 {
                        continue;
                    }
                    // Telemetry is disposable: if the client's queue is
                    // full, this frame is simply skipped.
                    if c.tx.try_send(WsMessage::Binary(bytes.clone())).is_ok() {
                        c.last_sent[si].store(now_us, Ordering::Relaxed);
                    }
                }
            }
        }

        // 4. Plugin -> UI events. Not disposable: a dropped note-off would
        //    leave a key lit, so a full queue forces a full sync instead.
        if let Some(bytes) = &events_frame {
            for c in &clients {
                if c.tx.try_send(WsMessage::Binary(bytes.clone())).is_err() {
                    c.needs_full_sync.store(true, Ordering::Relaxed);
                }
            }
        }

        // 5. Ad-hoc JSON (skipping the client a message originated from).
        for (t, exclude) in &texts {
            let msg = WsMessage::Text(Utf8Bytes::from(t.clone()));
            for c in &clients {
                if c.id == *exclude {
                    continue;
                }
                let _ = c.tx.try_send(msg.clone());
            }
        }
    }

    let _ = nstreams;
    shared.return_stream_readers(readers);
}

/// A `ParamValues` frame with every parameter and no flags: the connect-time
/// snapshot and the resync frame.
fn encode_full_snapshot(shared: &Shared, buf: &mut Vec<u8>) {
    let mut w = wire::ParamValuesWriter::begin(buf);
    for i in 0..shared.params.len() {
        w.push(i as u16, 0, shared.params.get_normalized(i));
    }
    w.finish();
}

// ---------------------------------------------------------------------------
// HTTP / WebSocket
// ---------------------------------------------------------------------------

/// axum state shared by every handler.
struct AppState {
    shared: Arc<Shared>,
    registry: Arc<Registry>,
    assets: Assets,
    /// `ServerConfig::send_queue`.
    send_queue: usize,
    /// `ServerConfig::max_message_size`.
    max_message_size: usize,
    /// Flips to `true` on shutdown; every socket task watches it.
    shutdown: watch::Receiver<bool>,
    /// What `/instance` answers.
    instance: discovery::Instance,
}

/// `GET /instance`: who this server is (used by discovery to validate files).
async fn instance_handler(State(st): State<Arc<AppState>>) -> Response {
    json_response(serde_json::to_string(&st.instance).unwrap_or_default())
}

/// `GET /instances`: the live instances of **this plug-in** (same `name`)
/// for this user, each validated by probing its discovery record.
///
/// Instance features are scoped to one product on purpose: an EQ's
/// instance list shows the other copies of that EQ, not every app on the
/// machine that happens to use the same bridge. `?all=1` lifts the
/// restriction for tooling.
async fn instances_handler(State(st): State<Arc<AppState>>, uri: axum::http::Uri) -> Response {
    let all = uri
        .query()
        .map(|q| {
            q.split('&')
                .any(|kv| matches!(kv, "all" | "all=1" | "all=true"))
        })
        .unwrap_or(false);
    let name = st.instance.name.clone();
    let list = tokio::task::spawn_blocking(move || {
        let mut live = discovery::list_live(Duration::from_millis(250));
        if !all {
            live.retain(|i| i.name == name);
        }
        live
    })
    .await
    .unwrap_or_default();
    json_response(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()))
}

/// `200` with a JSON body, `no-store`.
fn json_response(body: String) -> Response {
    let mut r = body.into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// `GET /ws`: upgrade and hand the socket to [`handle_socket`].
async fn ws_handler(State(st): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(st.max_message_size)
        .on_upgrade(move |socket| handle_socket(socket, st))
}

/// One client's lifetime: handshake (Hello, manifest, snapshot, sticky
/// frames, `store.all`), registration, then a writer task draining the
/// outbound queue and this task decoding inbound messages until the socket
/// closes or the server shuts down. Registration happens *after* the
/// handshake so the pump never interleaves a broadcast with it.
async fn handle_socket(socket: WebSocket, st: Arc<AppState>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<WsMessage>(st.send_queue);
    let id = st.registry.next_id();
    let nstreams = st.shared.stream_specs.len();
    let client = Arc::new(Client::new(id, tx.clone(), nstreams));

    // Handshake: Hello, manifest, then a full parameter snapshot.
    let mut buf = Vec::with_capacity(4096);
    wire::encode_hello(&mut buf, st.shared.params.len() as u16, nstreams as u16, id);
    let hello = WsMessage::Binary(Bytes::copy_from_slice(&buf));
    let manifest = WsMessage::Text(Utf8Bytes::from(st.shared.manifest_json.clone()));
    encode_full_snapshot(&st.shared, &mut buf);
    let snapshot = WsMessage::Binary(Bytes::copy_from_slice(&buf));
    for m in [hello, manifest, snapshot] {
        if sink.send(m).await.is_err() {
            return;
        }
    }
    // Sticky streams: replay the latest frame so state-like data (a
    // response curve, a wavetable) is there before its next change.
    for bytes in st.registry.sticky_frames() {
        if sink.send(WsMessage::Binary(bytes)).await.is_err() {
            return;
        }
    }
    // The UI store, so the page can hydrate before it renders.
    if sink
        .send(WsMessage::Text(Utf8Bytes::from(st.shared.store_all_json())))
        .await
        .is_err()
    {
        return;
    }

    st.registry.add(client.clone());
    log::info!("bridge: client {id} connected");

    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut shutdown = st.shutdown.clone();
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(WsMessage::Binary(b))) => handle_binary(&st, &client, &b),
                    Some(Ok(WsMessage::Text(t))) => handle_text(&st, &client, t.as_str()),
                    Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
        }
    }

    st.registry.remove(id);
    drop(tx);
    drop(client);
    writer.abort();
    log::info!("bridge: client {id} disconnected");
}

/// An inbound binary frame, handled right here on the socket task: edits go
/// straight into the parameter store, events into the audio queue, a Ping
/// is answered immediately (ahead of anything the pump has queued), and a
/// Subscribe updates this client's throttle. Anything else is logged and
/// ignored.
fn handle_binary(st: &AppState, client: &Client, b: &[u8]) {
    match Frame::decode(b) {
        Ok(Frame::ParamEdits(edits)) => {
            for e in edits.iter() {
                st.shared
                    .apply_client_edit(e.index, e.value, e.phase, client.id);
            }
        }
        Ok(Frame::Events(events)) => {
            for e in events.iter() {
                if !st.shared.push_ui_event(e) {
                    log::debug!("bridge: event queue full, dropped {e:?}");
                }
            }
        }
        Ok(Frame::Ping { client_time }) => {
            let mut buf = Vec::with_capacity(20);
            wire::encode_pong(&mut buf, client_time, st.shared.now_us());
            let _ = client.tx.try_send(WsMessage::Binary(Bytes::from(buf)));
        }
        Ok(Frame::Subscribe {
            stream,
            min_interval_us,
            enabled,
        }) => {
            if let Some(s) = client.subs.get(stream as usize) {
                s.store(
                    if enabled {
                        min_interval_us.min(SUB_DISABLED - 1)
                    } else {
                        SUB_DISABLED
                    },
                    Ordering::Relaxed,
                );
            }
        }
        Ok(other) => {
            log::debug!("bridge: ignoring unexpected frame from client: {other:?}");
        }
        Err(e) => {
            log::warn!("bridge: bad frame from client {}: {e}", client.id);
        }
    }
}

/// An inbound text frame. Only `{"t":"msg",...}` objects are accepted. The
/// UI store topics (`store.set`, `store.all`) are served here; every other
/// topic is queued for the plug-in with the sender's id.
fn handle_text(st: &AppState, client: &Client, text: &str) {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("bridge: bad JSON from client {}: {e}", client.id);
            return;
        }
    };
    if v.get("t").and_then(|t| t.as_str()) == Some("msg") {
        let topic = v
            .get("topic")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let data = v.get("data").cloned().unwrap_or(serde_json::Value::Null);
        match topic.as_str() {
            // The UI store is handled here, not by the plugin.
            "store.set" => {
                let key = data
                    .get("key")
                    .and_then(|k| k.as_str())
                    .unwrap_or("")
                    .to_string();
                let value = data
                    .get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                match st.shared.store_set(&key, value.clone()) {
                    Ok(()) => {
                        // Tell the other clients (the sender already has it).
                        let text = serde_json::json!({ "t": "msg", "topic": "store.changed", "data": { "key": key, "value": value } }).to_string();
                        if let Ok(mut q) = st.shared.outbound_json.lock() {
                            q.push_back((text, client.id));
                        }
                        st.shared.wake();
                    }
                    Err(why) => {
                        let text = serde_json::json!({ "t": "msg", "topic": "store.error", "data": { "key": key, "error": why } }).to_string();
                        let _ = client.tx.try_send(WsMessage::Text(Utf8Bytes::from(text)));
                    }
                }
            }
            "store.all" => {
                let _ = client
                    .tx
                    .try_send(WsMessage::Text(Utf8Bytes::from(st.shared.store_all_json())));
            }
            _ => st.shared.push_inbound(Message {
                topic,
                data,
                client: client.id,
            }),
        }
    }
}

/// Content type from the file extension; `application/octet-stream` when
/// unknown.
fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// `200` with a static file body, typed from `path`, `no-store` so an
/// edited page is picked up on refresh.
fn file_response(path: &str, body: Bytes) -> Response {
    let mut r = body.into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(mime_for(path)),
    );
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// What `/` shows with [`Assets::None`], so a bare server is not a 404.
const NO_ASSETS_PAGE: &str = "<!doctype html><meta charset=utf-8><title>bridge</title>\
<body style=\"font:14px system-ui;padding:2em;color:#ddd;background:#111\">\
<h2>bridge is running</h2><p>No UI assets are configured. The WebSocket endpoint is \
<code>/ws</code> and the client library is served at <code>/vst3-web-stratum/vst3-web-stratum.js</code>.</p>";

/// The fallback route: `/vst3-web-stratum/<file>` from [`CLIENT_ASSETS`], everything
/// else from the configured [`Assets`]. Refuses `..`, `\` and `:` in paths
/// with `400` before touching the file system.
async fn static_handler(State(st): State<Arc<AppState>>, uri: Uri) -> Response {
    let path = uri.path();
    if let Some(rest) = path.strip_prefix("/vst3-web-stratum/") {
        if let Some((_, body)) = CLIENT_ASSETS.iter().find(|(p, _)| *p == rest) {
            return file_response(rest, Bytes::from_static(body));
        }
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut rel = path.trim_start_matches('/').to_string();
    if rel.is_empty() || rel.ends_with('/') {
        rel.push_str("index.html");
    }
    let rel = rel.as_str();
    if rel.contains("..") || rel.contains('\\') || rel.contains(':') {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match &st.assets {
        Assets::None => {
            if rel == "index.html" {
                file_response("index.html", Bytes::from_static(NO_ASSETS_PAGE.as_bytes()))
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
        Assets::Embedded(files) => match files.iter().find(|(p, _)| *p == rel) {
            Some((_, body)) => file_response(rel, Bytes::from_static(body)),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Assets::Lookup(lookup) => match lookup(rel) {
            Some(body) => file_response(rel, Bytes::from_static(body)),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        Assets::Dir(dir) => match tokio::fs::read(dir.join(rel)).await {
            Ok(bytes) => file_response(rel, Bytes::from(bytes)),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// A running server. Dropping it shuts everything down (see the module docs
/// for the order); keep it alive for as long as the UI should be reachable.
pub struct ServerHandle {
    addr: SocketAddr,
    /// Tells the pump loop to exit.
    stop: Arc<AtomicBool>,
    /// Tells every socket task and the axum server to exit.
    shutdown_tx: watch::Sender<bool>,
    pump: Option<JoinHandle<()>>,
    net: Option<JoinHandle<()>>,
    registry: Arc<Registry>,
    shared: Arc<Shared>,
    /// The discovery record to remove on stop, if one was written.
    discovery_path: Option<PathBuf>,
}

impl ServerHandle {
    /// The bound socket address (`127.0.0.1:<port>`).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    /// The bound port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }
    /// `http://127.0.0.1:<port>/`
    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }
    /// `ws://127.0.0.1:<port>/ws`
    pub fn ws_url(&self) -> String {
        format!("ws://{}/ws", self.addr)
    }
    /// Number of connected clients right now (past the handshake).
    pub fn client_count(&self) -> usize {
        self.registry.count.load(Ordering::Relaxed)
    }
    /// Stop both threads and close every connection. Same as dropping the
    /// handle, spelled out.
    pub fn shutdown(mut self) {
        self.stop_inner();
    }
    /// Idempotent teardown shared by `shutdown` and `Drop`: unpublish,
    /// stop and join the pump, signal and join the network thread.
    fn stop_inner(&mut self) {
        if let Some(p) = self.discovery_path.take() {
            discovery::unpublish(&p);
        }
        self.stop.store(true, Ordering::Release);
        self.shared.register_pump(None);
        if let Some(p) = self.pump.take() {
            p.thread().unpark();
            let _ = p.join();
        }
        let _ = self.shutdown_tx.send(true);
        if let Some(n) = self.net.take() {
            let _ = n.join();
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

/// Bind a listener according to `policy`. For `Probe`, `AddrInUse` moves on
/// to the next port and any other error aborts; when the whole span is busy
/// an ephemeral port is used with a warning.
fn bind_with_policy(ip: IpAddr, policy: PortPolicy) -> io::Result<StdListener> {
    match policy {
        PortPolicy::Ephemeral => StdListener::bind(SocketAddr::new(ip, 0)),
        PortPolicy::Fixed(port) => StdListener::bind(SocketAddr::new(ip, port)),
        PortPolicy::Probe { base, span } => {
            for i in 0..span.max(1) {
                let port = base.saturating_add(i);
                match StdListener::bind(SocketAddr::new(ip, port)) {
                    Ok(l) => {
                        if i > 0 {
                            log::info!("bridge: port {base} busy, using {port}");
                        }
                        return Ok(l);
                    }
                    Err(e) if e.kind() == io::ErrorKind::AddrInUse => continue,
                    Err(e) => return Err(e),
                }
            }
            log::warn!(
                "bridge: ports {base}..{} all busy, using an ephemeral port",
                base.saturating_add(span)
            );
            StdListener::bind(SocketAddr::new(ip, 0))
        }
    }
}

/// Bind the listener, spawn the network and pump threads, and return once the
/// port is known. Call from any non-audio thread. A bridge can be served
/// again after its handle is dropped (the pump gives the stream readers
/// back), but not by two servers at once: the second pump would find no
/// readers and deliver no streams.
///
/// # Errors
///
/// The port could not be bound (a taken [`PortPolicy::Fixed`] port, or a
/// non-`AddrInUse` failure while probing), the listener could not be made
/// non-blocking, or a thread could not be spawned.
pub fn serve(bridge: &Vst3WebStratum, cfg: ServerConfig) -> io::Result<ServerHandle> {
    let listener = bind_with_policy(cfg.ip, cfg.port)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let shared = bridge.shared.clone();
    let instance = discovery::Instance::new(&shared.name, addr.port());
    let discovery_path = if cfg.discovery {
        discovery::publish(&instance)
    } else {
        None
    };
    let registry = Arc::new(Registry::new(shared.stream_specs.len()));
    let stop = Arc::new(AtomicBool::new(false));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let pump = {
        let shared = shared.clone();
        let registry = registry.clone();
        let stop = stop.clone();
        let pcfg = PumpCfg {
            wake: cfg.wake,
            poll_interval: cfg.poll_interval,
            echo_edits: cfg.echo_edits,
        };
        thread::Builder::new()
            .name("vst3-web-stratum-pump".into())
            .spawn(move || pump_loop(shared, registry, stop, pcfg))?
    };
    shared.register_pump(Some(pump.thread().clone()));

    let state = Arc::new(AppState {
        shared: shared.clone(),
        registry: registry.clone(),
        assets: cfg.assets,
        send_queue: cfg.send_queue,
        max_message_size: cfg.max_message_size,
        shutdown: shutdown_rx.clone(),
        instance: instance.clone(),
    });

    let net = thread::Builder::new()
        .name("vst3-web-stratum-net".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("bridge: failed to build runtime: {e}");
                    return;
                }
            };
            let mut shutdown_rx = shutdown_rx;
            rt.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(l) => l,
                    Err(e) => {
                        log::error!("bridge: listener: {e}");
                        return;
                    }
                };
                let app = Router::new()
                    .route("/ws", get(ws_handler))
                    .route("/instance", get(instance_handler))
                    .route("/instances", get(instances_handler))
                    .fallback(static_handler)
                    .with_state(state);
                // Nagle would hold 12-byte edit frames back for up to 40 ms:
                // the single biggest latency sink, so it goes off first.
                let listener = listener.tap_io(|stream| {
                    if let Err(e) = stream.set_nodelay(true) {
                        log::warn!("bridge: TCP_NODELAY: {e}");
                    }
                });
                let server = axum::serve(listener, app);
                tokio::select! {
                    r = server => {
                        if let Err(e) = r { log::error!("bridge: server: {e}"); }
                    }
                    _ = async {
                        loop {
                            if *shutdown_rx.borrow() { break; }
                            if shutdown_rx.changed().await.is_err() { break; }
                        }
                    } => {}
                }
            });
            rt.shutdown_timeout(Duration::from_millis(500));
        })?;

    log::info!("bridge: serving {} on http://{addr}/", shared.name);
    Ok(ServerHandle {
        addr,
        stop,
        shutdown_tx,
        pump: Some(pump),
        net: Some(net),
        registry,
        shared,
        discovery_path,
    })
}
