//! nih-plug adapter: a [`nih_plug::prelude::Editor`] whose window *is* the
//! operating system's web view, showing a page the plugin serves itself over
//! noob-vst-webgui-framework.
//!
//! # What this crate does
//!
//! `noob-vst-webgui-framework` gives a plug-in a parameter mirror, telemetry streams and a
//! local WebSocket server; `noob-vst-webgui-framework-webview` embeds the OS web view in a host
//! window. This crate joins the two to nih-plug:
//!
//! * [`mirror_params`] turns a nih-plug [`Params`] object into noob-vst-webgui-framework
//!   [`ParamSpec`]s (same ids, groups, units, 65-point value tables, labels
//!   for discrete parameters) so the page can render any parameter without
//!   knowing its formula.
//! * [`NoobVstWebguiFrameworkEditor`] owns the bridge and the server, mirrors host changes
//!   into it, and forwards page edits back to the host as proper
//!   begin / perform / end gestures from the GUI thread.
//! * [`EditorHandle`] is the [`Editor`] you return from `Plugin::editor`.
//!   Spawning it starts the server (once), creates the embedded web view in
//!   the host's window, or opens the page in the system browser when the
//!   platform cannot host a web view.
//! * [`StoreSlot`] saves the page's key-value store (`client.store`:
//!   presets, favourites, view settings) inside the plug-in state, so it
//!   travels with the session and is restored with it.
//!
//! # Quick start
//!
//! A complete plug-in is about this much code (see `examples/noob-q` and
//! `examples/noob-wave` for two real ones):
//!
//! ```ignore
//! use std::collections::BTreeMap;
//! use std::sync::Arc;
//! use nih_plug::prelude::*;
//! use noob_vst_webgui_framework::{Assets, AudioHandle, NoobVstWebguiFramework, StreamKind, StreamSpec};
//! use noob_vst_webgui_framework_nih::{EditorConfig, StoreSlot, NoobVstWebguiFrameworkEditor};
//!
//! // The page, built with Vite into web/dist and embedded in the binary.
//! static UI: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/dist");
//! fn ui_lookup(path: &str) -> Option<&'static [u8]> { UI.get_file(path).map(|f| f.contents()) }
//!
//! struct MyParams {
//!     cutoff: FloatParam,
//!     drive: FloatParam,
//!     /// Not a parameter: the page's own state, saved with the plug-in state.
//!     ui_store: StoreSlot,
//! }
//!
//! unsafe impl Params for MyParams {
//!     fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
//!         vec![
//!             ("cutoff".into(), self.cutoff.as_ptr(), "filter".into()),
//!             ("drive".into(), self.drive.as_ptr(), "filter".into()),
//!         ]
//!     }
//!     fn serialize_fields(&self) -> BTreeMap<String, String> {
//!         let mut m = BTreeMap::new();
//!         self.ui_store.serialize_into(&mut m);
//!         m
//!     }
//!     fn deserialize_fields(&self, m: &BTreeMap<String, String>) {
//!         self.ui_store.deserialize_from(m);
//!     }
//! }
//!
//! struct MyPlugin {
//!     params: Arc<MyParams>,
//!     editor: Arc<NoobVstWebguiFrameworkEditor>,
//!     bridge: NoobVstWebguiFramework,
//!     audio: Option<AudioHandle>,
//! }
//!
//! impl Default for MyPlugin {
//!     fn default() -> Self {
//!         let params = Arc::new(MyParams { /* ... */ ui_store: StoreSlot::new() });
//!         let streams = vec![
//!             StreamSpec::new("meter", 2).kind(StreamKind::Meter).channels(2),
//!             StreamSpec::new("spectrum", 1025).kind(StreamKind::Spectrum),
//!         ];
//!         let (editor, bridge) = NoobVstWebguiFrameworkEditor::with_builder(
//!             "My Plugin",
//!             params.as_ref(),
//!             streams,
//!             EditorConfig::new(1000, 640).assets(Assets::Lookup(ui_lookup)),
//!             |b| b.meta(serde_json::json!({ "vendor": "Me", "sample_rate": 48_000.0 })),
//!         );
//!         let audio = bridge.take_audio();      // Some(..) exactly once
//!         params.ui_store.attach(&bridge);      // any time; before or after state restore
//!         MyPlugin { params, editor, bridge, audio }
//!     }
//! }
//!
//! impl Plugin for MyPlugin {
//!     // ... NAME, VENDOR, AUDIO_IO_LAYOUTS, SysExMessage, BackgroundTask ...
//!     fn params(&self) -> Arc<dyn Params> { self.params.clone() }
//!
//!     fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
//!         Some(Box::new(self.editor.handle()))
//!     }
//!
//!     fn initialize(&mut self, _: &AudioIOLayout, cfg: &BufferConfig, _: &mut impl InitContext<Self>) -> bool {
//!         // Anything the page should know that is not a parameter:
//!         self.bridge.send_json("sample_rate", serde_json::json!({ "sample_rate": cfg.sample_rate }));
//!         true
//!     }
//!
//!     fn process(&mut self, buffer: &mut Buffer, _: &mut AuxiliaryBuffers, _: &mut impl ProcessContext<Self>) -> ProcessStatus {
//!         // ... DSP using self.params ...
//!         if let Some(audio) = self.audio.as_mut() {
//!             audio.publish_slice(0, &[peak_l, peak_r]);   // wait-free, latest wins
//!         }
//!         ProcessStatus::Normal
//!     }
//! }
//! ```
//!
//! On the page, `@noob-audio-engineering/noob-vst-webgui-framework` connects to `/ws` on the same origin, receives
//! the manifest with every parameter, and its knobs send gestures back; see
//! the repository's `docs/GETTING-STARTED.md`.
//!
//! # Lifecycle
//!
//! 1. **Construction** ([`NoobVstWebguiFrameworkEditor::new`] /
//!    [`with_builder`](NoobVstWebguiFrameworkEditor::with_builder)): the
//!    parameters are mirrored, the bridge is built and the server
//!    configuration is prepared. Nothing is bound yet, so a plug-in that is
//!    scanned by a host but never shown costs no socket.
//! 2. **First `spawn`** (host opens the editor): the server starts
//!    ([`NoobVstWebguiFrameworkEditor::ensure_server`]), the mirror is refreshed from the
//!    host's current values, the web view is created inside the host's
//!    window and loads the server's URL. If the web view cannot be created
//!    the URL is opened in the system browser instead.
//! 3. **While open**: host changes arrive through
//!    [`Editor::param_value_changed`] / [`Editor::param_values_changed`] and
//!    are pushed to every connected page; page edits are queued and
//!    forwarded to the host from a UI-thread timer; `resize` and
//!    `fullscreen` messages resize the window, and a resize by the host (a
//!    frame drag) reaches the web view through [`Editor::set_size`].
//! 4. **Window closed**: the web view and timer are dropped. The server
//!    keeps running so a browser tab that is also connected keeps working;
//!    its edits are then forwarded directly from the network thread.
//! 5. **Reopened**: step 2 again, without restarting the server.
//! 6. **Plug-in dropped**: the last [`NoobVstWebguiFramework`] handle goes away, the server
//!    shuts down and the discovery record is removed.
//!
//! # Threads
//!
//! | thread | owned by | what happens here |
//! |---|---|---|
//! | host GUI thread | host | `spawn`, `param_value_changed`, the [`UiTimer`] callback that forwards edits and handles `resize`, web view creation / resize / drop |
//! | audio thread | host | `process`: reads parameters through nih-plug as usual, publishes streams with [`AudioHandle`](noob_vst_webgui_framework::AudioHandle) (wait-free) |
//! | `noob-vst-webgui-framework-pump` | noob-vst-webgui-framework | fans parameter changes and stream frames out to clients |
//! | `noob-vst-webgui-framework-net` | noob-vst-webgui-framework | the WebSocket server; receives edits, messages and store writes |
//!
//! Host → page: `param_value_changed` (GUI thread) stores the value in the
//! bridge and wakes the pump, which sends it to every client in the next
//! cycle (tens of microseconds on a local socket).
//!
//! Page → host: an edit frame arrives on the network thread and is queued.
//! While the editor window is open, a native UI-thread timer
//! ([`noob_vst_webgui_framework_webview::UiTimer`], every [`EditorConfig::forward_interval`])
//! drains the queue and calls `GuiContext::raw_begin_set_parameter` /
//! `raw_set_parameter_normalized` / `raw_end_set_parameter`, which is what
//! the VST3 and CLAP specifications ask for. Where no such timer exists
//! (platforms other than Windows today) or once the window is closed but a
//! browser tab is still connected, edits are forwarded directly from the
//! network thread through the bridge's edit hook; every major host tolerates
//! that.
//!
//! # Parameter mirroring
//!
//! [`mirror_params`] walks `Params::param_map()` in order, so noob-vst-webgui-framework's
//! parameter *index* is the position in that list and the *id* is the string
//! the plug-in chose (implement `param_map` by hand if you want ids that
//! match a standalone build and the page, as the examples do). For each
//! parameter it samples `preview_plain` at 65 evenly spaced normalized
//! values; the page interpolates that table to convert between normalized
//! and plain values and to draw scales, so skewed, logarithmic and custom
//! nih-plug ranges all render correctly with no formula duplicated in
//! JavaScript. Discrete parameters with 2 to 64 steps also get their labels
//! (`normalized_value_to_string` per step); `NON_AUTOMATABLE` parameters are
//! flagged so a page can hide them from automation-related UI.
//!
//! # Messages the page can send
//!
//! `client.send(topic, data)` on the page delivers a JSON message to the
//! bridge. The adapter handles two topics itself:
//!
//! * `"resize"` with `{ "width": w, "height": h }` — clamped to
//!   [`EditorConfig::size_limits`], then the host is asked to resize the
//!   editor window and, if it agrees, the web view is resized to match and
//!   the size is remembered under [`WINDOW_STORE_KEY`].
//! * `"fullscreen"` with `{ "on": bool, "width"?, "height"? }` — on, the
//!   window grows to the monitor's work area (the page's `width` / `height`
//!   are the fallback where the work area is unknown); off, it returns to
//!   the size it had before.
//!
//! Resizing also works the other way round: [`Editor::can_resize`] answers
//! `true` unless the size limits pin one size, the host negotiates through
//! [`Editor::check_size_constraint`] and hands the final size to
//! [`Editor::set_size`], after which the web view follows on the next timer
//! tick and the size is remembered like a page request. Upstream nih-plug
//! has no host-to-plugin resizing; this workspace builds against a patched
//! fork that adds those three `Editor` methods (see `DEVELOPMENT.md`).
//!
//! Every other topic is left in the queue for the plug-in to read with
//! [`NoobVstWebguiFramework::poll_message`] from any non-real-time thread (a nih-plug
//! background task, or a timer of your own). The adapter stops draining at
//! the first foreign message, so a page that sends custom topics needs the
//! plug-in to poll them, or `resize` requests queued behind them wait.
//!
//! # Ports and instances
//!
//! By default every instance probes a small port range derived from the
//! plug-in name ([`PortPolicy::for_name`]): the first free port in that
//! range is taken, so several instances (of this plug-in or of other noob-vst-webgui-framework
//! plug-ins) never collide, and an instance usually gets the same origin
//! next session, which keeps the browser's own storage attached to it. Each
//! instance also writes a discovery record (`noob_vst_webgui_framework::discovery`) and
//! answers `GET /instance` and `GET /instances`, so tools and other
//! instances can find it. Override with [`EditorConfig::port`] or
//! [`EditorConfig::port_policy`]; switch the record off with
//! [`EditorConfig::discovery`].
//!
//! # UI state
//!
//! State the page keeps in `client.store` (presets, favourites, view
//! settings) is saved with the plug-in state: put a [`StoreSlot`] in your
//! `Params`, forward `serialize_fields` / `deserialize_fields` to it and
//! call [`StoreSlot::attach`] once the bridge exists. Restores that happen
//! before `attach` are kept and applied then, so construction order does not
//! matter.
//!
//! # Hosts
//!
//! The adapter follows the nih-plug `Editor` contract, which the VST3 and
//! CLAP wrappers implement for every host nih-plug supports. As of this
//! writing the examples have been exercised through their standalone
//! binaries and compile-checked as plug-ins; a run inside a DAW is still
//! pending, so treat host-specific behaviour (resize negotiation in both
//! directions, DPI) as unverified.

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nih_plug::prelude::*;
use noob_vst_webgui_framework::bridge::EditEvent;
use noob_vst_webgui_framework::{
    Assets, EditPhase, NoobVstWebguiFramework, ParamSpec, PortPolicy, ServerConfig, ServerHandle,
    StreamSpec,
};
use noob_vst_webgui_framework_webview::{
    EmbeddedWebView, RawParent, UiTimer, WebViewOptions, monitor_work_area,
};

/// UI-store key under which the adapter remembers the editor size the page
/// last asked for (`{ "width": w, "height": h }`, logical pixels). Written
/// on every applied `resize` message and every host-driven resize (not on
/// fullscreen requests) and read
/// when the editor is spawned, so a plug-in reopens at the size the user
/// chose; it travels with the plug-in state through the `StoreSlot`.
pub const WINDOW_STORE_KEY: &str = "window";

/// Build noob-vst-webgui-framework [`ParamSpec`]s from a nih-plug [`Params`] object, in
/// `param_map()` order, paired with the [`ParamPtr`] each one came from.
///
/// For every parameter this captures:
/// * the id and group from `param_map`, the name and (trimmed) unit;
/// * a **65-point table** of `preview_plain(i / 64)` for `i` in `0..=64`,
///   which becomes the parameter's [`Taper`](noob_vst_webgui_framework::Taper) on the
///   page, so any nih-plug range (linear, skewed, reversed, custom) converts
///   and draws correctly without a formula in JavaScript;
/// * the plain default value;
/// * the step count (`step_count + 1`, or `0` for continuous parameters),
///   and for 2 to 64 steps the label of every step from
///   `normalized_value_to_string`, so enums and booleans show their names;
/// * the `NON_AUTOMATABLE` flag.
///
/// Called by [`NoobVstWebguiFrameworkEditor::with_builder`]; public so a custom editor or a
/// tool can mirror parameters the same way.
///
/// The returned pointers are only valid while `params` is alive; nih-plug
/// keeps the `Params` object alive for the whole life of the plug-in
/// instance, which is what the editor relies on (see the `Send` / `Sync`
/// note on [`NoobVstWebguiFrameworkEditor`]).
pub fn mirror_params(params: &dyn Params) -> Vec<(ParamSpec, ParamPtr)> {
    params
        .param_map()
        .into_iter()
        .map(|(id, ptr, group)| {
            // SAFETY: the pointers come from a live `Params` object that the
            // plugin keeps alive for as long as this editor exists.
            let (name, unit, default_norm, step_count, flags) = unsafe {
                (
                    ptr.name().to_string(),
                    ptr.unit().trim().to_string(),
                    ptr.default_normalized_value(),
                    ptr.step_count(),
                    ptr.flags(),
                )
            };
            let table: Vec<f32> = (0..65)
                .map(|i| unsafe { ptr.preview_plain(i as f32 / 64.0) })
                .collect();
            let steps = step_count.map(|s| s as u32 + 1).unwrap_or(0);
            let labels: Vec<String> = if (2..=64).contains(&steps) {
                (0..steps)
                    .map(|i| unsafe {
                        ptr.normalized_value_to_string(i as f32 / (steps - 1) as f32, false)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let mut spec = ParamSpec::new(id, name)
                .unit(unit)
                .group(group)
                .with_table(table)
                .steps(steps);
            spec.default = unsafe { ptr.preview_plain(default_norm) };
            spec.labels = labels;
            if flags.contains(ParamFlags::NON_AUTOMATABLE) {
                spec = spec.not_automatable();
            }
            (spec, ptr)
        })
        .collect()
}

/// How the editor window, the server and the edit forwarding behave. Build
/// with [`new`](Self::new) and chain the builder methods, or set the public
/// fields directly.
///
/// ```ignore
/// EditorConfig::new(1180, 720)
///     .assets(Assets::Lookup(ui_lookup))   // the embedded Vite build
///     .size_limits((820, 500), (1920, 1200))
///     .devtools(true)
/// ```
pub struct EditorConfig {
    /// Initial editor width in logical pixels; also what `Editor::size`
    /// reports until the page requests another size.
    pub width: u32,
    /// Initial editor height in logical pixels.
    pub height: u32,
    /// Where the page comes from: [`Assets::Lookup`] (files embedded with
    /// `include_dir`, the normal choice for a shipped plug-in),
    /// [`Assets::Dir`] (a directory on disk, handy while developing),
    /// [`Assets::Embedded`] (a static slice of files), or [`Assets::None`]
    /// (only `/ws` and the built-in `/noob-vst-webgui-framework/*` client library are served;
    /// point a browser or a Vite dev server at it). Default: `Assets::None`.
    pub assets: Assets,
    /// How the server picks its port. `None` (the default) probes a small
    /// range derived from the plug-in name ([`PortPolicy::for_name`]), so
    /// every instance gets its own port and the page's origin stays stable
    /// across sessions.
    pub port: Option<PortPolicy>,
    /// Write a discovery record so tools and other instances can find this
    /// one (`GET /instances`, `tools/instances.mjs`). Default: `true`.
    pub discovery: bool,
    /// Enable the web view's developer tools. Default: `true` in debug
    /// builds, `false` in release builds.
    pub devtools: bool,
    /// Echo a client's own edits back to it. Lets the page measure round-trip
    /// latency and keeps several windows of one instance in sync; costs one
    /// small frame per edit. Default: `true`.
    pub echo_edits: bool,
    /// How often the UI-thread timer forwards queued edits to the host and
    /// handles `resize` messages. Default: 3 ms (the OS timer resolution is
    /// the real lower bound; see [`noob_vst_webgui_framework_webview::UiTimer`]).
    pub forward_interval: Duration,
    /// Smallest size the page may request via a `resize` message, as
    /// `(width, height)` in logical pixels. Default: `(480, 320)`.
    pub min_size: (u32, u32),
    /// Largest size the page may request. Default: `(7680, 4320)`.
    pub max_size: (u32, u32),
}

impl EditorConfig {
    /// A configuration for a `width` × `height` window with every other
    /// field at its documented default.
    pub fn new(width: u32, height: u32) -> Self {
        EditorConfig {
            width,
            height,
            assets: Assets::None,
            port: None,
            discovery: true,
            devtools: cfg!(debug_assertions),
            echo_edits: true,
            forward_interval: Duration::from_millis(3),
            min_size: (480, 320),
            max_size: (7680, 4320),
        }
    }
    /// Where the page comes from; see [`EditorConfig::assets`].
    pub fn assets(mut self, assets: Assets) -> Self {
        self.assets = assets;
        self
    }
    /// Insist on one port (`0` = ephemeral). Prefer the default probing
    /// policy unless you know only one instance will ever run: a fixed port
    /// makes the second instance fail to start its server (the editor then
    /// logs the error and shows nothing).
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(if port == 0 {
            PortPolicy::Ephemeral
        } else {
            PortPolicy::Fixed(port)
        });
        self
    }
    /// Use an explicit [`PortPolicy`] (for example a probe range shared by a
    /// family of plug-ins).
    pub fn port_policy(mut self, policy: PortPolicy) -> Self {
        self.port = Some(policy);
        self
    }
    /// Publish (or not) a discovery record; see [`EditorConfig::discovery`].
    pub fn discovery(mut self, on: bool) -> Self {
        self.discovery = on;
        self
    }
    /// Enable or disable the web view's developer tools.
    pub fn devtools(mut self, on: bool) -> Self {
        self.devtools = on;
        self
    }
    /// Bounds for sizes the page may request with a `resize` message, as
    /// `(width, height)` pairs in logical pixels.
    pub fn size_limits(mut self, min: (u32, u32), max: (u32, u32)) -> Self {
        self.min_size = min;
        self.max_size = max;
        self
    }
}

/// The editor. Create once per plugin instance; hand [`handle`](Self::handle)
/// to nih-plug every time it asks for an editor.
///
/// Owns the bridge ([`NoobVstWebguiFramework`]), the mirrored parameter pointers, the
/// current window size and the server (started lazily on the first
/// `spawn`, then kept for the life of the plug-in). All methods are safe to
/// call from any thread; the web view itself is created and dropped inside
/// [`Editor::spawn`] on the host's GUI thread and never stored here.
pub struct NoobVstWebguiFrameworkEditor {
    bridge: NoobVstWebguiFramework,
    /// One pointer per mirrored parameter, in noob-vst-webgui-framework index order.
    ptrs: Vec<ParamPtr>,
    width: AtomicU32,
    height: AtomicU32,
    min_size: (u32, u32),
    max_size: (u32, u32),
    devtools: bool,
    /// The running server, once [`ensure_server`](Self::ensure_server) has
    /// started it.
    server: Mutex<Option<ServerHandle>>,
    /// The configuration for the server, consumed when it starts.
    pending: Mutex<Option<ServerConfig>>,
    forward_interval: Duration,
    /// The GUI context from the most recent spawn; kept so edits from a
    /// detached browser tab still reach the host after the window closes.
    context: Mutex<Option<Arc<dyn GuiContext>>>,
    /// The size to go back to when the page leaves fullscreen.
    restore: Mutex<Option<(u32, u32)>>,
    /// Set by [`Editor::set_size`] when the host resized the window; the
    /// UI-thread timer then resizes the web view and remembers the size.
    host_resized: AtomicBool,
}

// SAFETY: every field is Send + Sync except `ptrs`, a Vec of `ParamPtr`,
// which is a raw pointer into the plugin's `Params` object. nih-plug requires
// `Params: Send + Sync`, holds its own `Arc` to it for the life of the plugin
// instance, and only ever lets us touch the pointers through `unsafe`
// methods whose contract is exactly that the object is alive. The editor is
// dropped with the plugin, so the pointers never dangle.
unsafe impl Send for NoobVstWebguiFrameworkEditor {}
unsafe impl Sync for NoobVstWebguiFrameworkEditor {}

impl NoobVstWebguiFrameworkEditor {
    /// Mirror `params`, declare `streams`, and prepare the server (it starts
    /// lazily on the first `spawn`). Returns the editor and the bridge; call
    /// `bridge.take_audio()` for the audio-thread handle.
    ///
    /// `name` is shown in the manifest, used to derive the default port range
    /// and written to the discovery record. Call once, from
    /// `Default::default()` of your plug-in.
    pub fn new(
        name: &str,
        params: &dyn Params,
        streams: Vec<StreamSpec>,
        cfg: EditorConfig,
    ) -> (Arc<NoobVstWebguiFrameworkEditor>, NoobVstWebguiFramework) {
        Self::with_builder(name, params, streams, cfg, |b| b)
    }

    /// Like [`new`](Self::new) but lets you add metadata or extra (plugin-side,
    /// non-host) parameters to the bridge before it is built.
    ///
    /// `customize` receives the [`NoobVstWebguiFrameworkBuilder`](noob_vst_webgui_framework::NoobVstWebguiFrameworkBuilder)
    /// after the mirrored parameters and `streams` have been added. Typical
    /// uses: `b.meta(json!({...}))` for values the page needs at load time
    /// (vendor, version, sample rate, ranges), or extra `b.param(..)` for
    /// UI-only settings that should not be host parameters. Extra parameters
    /// get indices after the mirrored ones and their edits are never
    /// forwarded to the host.
    ///
    /// # Panics
    ///
    /// Panics (from the builder) if more than 65 535 parameters or streams
    /// are declared.
    pub fn with_builder(
        name: &str,
        params: &dyn Params,
        streams: Vec<StreamSpec>,
        cfg: EditorConfig,
        customize: impl FnOnce(
            noob_vst_webgui_framework::NoobVstWebguiFrameworkBuilder,
        ) -> noob_vst_webgui_framework::NoobVstWebguiFrameworkBuilder,
    ) -> (Arc<NoobVstWebguiFrameworkEditor>, NoobVstWebguiFramework) {
        let mirrored = mirror_params(params);
        let mut b = NoobVstWebguiFramework::builder(name);
        let mut ptrs = Vec::with_capacity(mirrored.len());
        for (spec, ptr) in mirrored {
            b = b.param(spec);
            ptrs.push(ptr);
        }
        for s in streams {
            b = b.stream(s);
        }
        let bridge = customize(b).build();
        let policy = cfg.port.unwrap_or_else(|| PortPolicy::for_name(name));
        let server_cfg = ServerConfig {
            assets: cfg.assets,
            ..ServerConfig::default()
                .port_policy(policy)
                .discovery(cfg.discovery)
                .echo_edits(cfg.echo_edits)
        };
        let editor = Arc::new(NoobVstWebguiFrameworkEditor {
            bridge: bridge.clone(),
            ptrs,
            width: AtomicU32::new(cfg.width),
            height: AtomicU32::new(cfg.height),
            min_size: cfg.min_size,
            max_size: cfg.max_size,
            devtools: cfg.devtools,
            server: Mutex::new(None),
            pending: Mutex::new(Some(server_cfg)),
            forward_interval: cfg.forward_interval,
            context: Mutex::new(None),
            restore: Mutex::new(None),
            host_resized: AtomicBool::new(false),
        });
        (editor, bridge)
    }

    /// Something to hand to nih-plug from `Plugin::editor`. Cheap; make a new
    /// one every time nih-plug asks.
    pub fn handle(self: &Arc<Self>) -> EditorHandle {
        EditorHandle(self.clone())
    }

    /// The bridge, for sending messages (`send_json`), polling page messages
    /// (`poll_message`) or reading the UI store.
    pub fn bridge(&self) -> &NoobVstWebguiFramework {
        &self.bridge
    }

    /// Start the server if it is not running; returns its URL.
    ///
    /// Normally called by `spawn`, but a plug-in may call it earlier (for
    /// example to print the URL, or to let a browser connect before the
    /// editor is ever opened). Returns `None` if the server could not start
    /// (port policy exhausted, or the server already failed once: the
    /// configuration is consumed by the first attempt); the failure is
    /// logged through `nih_log!`.
    pub fn ensure_server(&self) -> Option<String> {
        let mut g = self.server.lock().ok()?;
        if g.is_none() {
            let cfg = self.pending.lock().ok()?.take()?;
            match noob_vst_webgui_framework::serve(&self.bridge, cfg) {
                Ok(s) => *g = Some(s),
                Err(e) => {
                    nih_log!("bridge: could not start server: {e}");
                    return None;
                }
            }
        }
        g.as_ref().map(|s| s.url())
    }

    /// The page URL, if the server is running (e.g. to show in a fallback
    /// UI or a log). `None` before the first `spawn`.
    pub fn url(&self) -> Option<String> {
        self.server.lock().ok()?.as_ref().map(|s| s.url())
    }

    /// The current editor size in logical pixels: the configured size, or
    /// the last size the page requested with a `resize` message.
    pub fn size(&self) -> (u32, u32) {
        (
            self.width.load(Ordering::Relaxed),
            self.height.load(Ordering::Relaxed),
        )
    }

    /// Apply the configured size limits to a requested size.
    fn clamp_size(&self, w: u32, h: u32) -> (u32, u32) {
        (
            w.clamp(self.min_size.0, self.max_size.0),
            h.clamp(self.min_size.1, self.max_size.1),
        )
    }

    /// Copy every parameter's current (unmodulated) value from the host into
    /// the bridge, which pushes the ones that changed to the clients.
    fn sync_from_host(&self) {
        for (i, ptr) in self.ptrs.iter().enumerate() {
            // SAFETY: see `mirror_params`.
            let v = unsafe { ptr.unmodulated_normalized_value() };
            self.bridge.set_param_norm(i, v);
        }
    }

    /// A closure that turns one page edit into the matching host call. Edits
    /// whose index is not a mirrored parameter (extra builder parameters)
    /// are ignored.
    fn forwarder(&self, ctx: Arc<dyn GuiContext>) -> Arc<dyn Fn(EditEvent) + Send + Sync> {
        let ptrs = self.ptrs.clone();
        Arc::new(move |e: EditEvent| {
            let Some(ptr) = ptrs.get(e.index as usize) else {
                return;
            };
            // SAFETY: see `mirror_params`.
            unsafe {
                match e.phase {
                    EditPhase::Begin => ctx.raw_begin_set_parameter(*ptr),
                    EditPhase::Perform => ctx.raw_set_parameter_normalized(*ptr, e.value),
                    EditPhase::End => ctx.raw_end_set_parameter(*ptr),
                }
            }
        })
    }

    /// Forward edits straight from the network thread (no UI-thread timer
    /// available, or the window is closed). Uses the most recent GUI
    /// context; with none yet, edits are left queued.
    fn install_direct_hook(&self) {
        let ctx = self.context.lock().ok().and_then(|g| g.clone());
        match ctx {
            Some(ctx) => self.bridge.set_edit_hook(Some(self.forwarder(ctx))),
            None => self.bridge.set_edit_hook(None),
        }
    }
}

/// What `Plugin::editor` returns: a thin [`Editor`] over a shared
/// [`NoobVstWebguiFrameworkEditor`]. Get one from [`NoobVstWebguiFrameworkEditor::handle`].
pub struct EditorHandle(Arc<NoobVstWebguiFrameworkEditor>);

/// What `spawn` hands back to nih-plug: the open window's web view and the
/// UI-thread timer. Dropping it (the host closed the editor) tears both down
/// and switches edit forwarding to the direct hook.
struct Instance {
    editor: Arc<NoobVstWebguiFrameworkEditor>,
    _webview: Rc<RefCell<Option<EmbeddedWebView>>>,
    _timer: Option<UiTimer>,
}

// SAFETY: nih-plug requires `Box<dyn Any + Send>` from `spawn`, but only so
// it can store the box; it drops the editor instance on the GUI thread that
// created it and never touches the contents from anywhere else. The web
// view and the timer are therefore only ever used on their own thread.
unsafe impl Send for Instance {}

impl Drop for Instance {
    fn drop(&mut self) {
        // The window is gone; keep serving detached browser tabs.
        self.editor.install_direct_hook();
    }
}

impl Editor for EditorHandle {
    /// Open the editor in the host's window (GUI thread).
    ///
    /// Starts the server if needed, refreshes the mirror from the host,
    /// installs the UI-thread timer that forwards edits and handles
    /// `resize`, then creates the embedded web view. If the platform cannot
    /// host one, or the server did not start, the page URL is opened in the
    /// system browser (or nothing happens, respectively) and the failure is
    /// logged.
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let ed = &self.0;
        if let Ok(mut g) = ed.context.lock() {
            *g = Some(context.clone());
        }
        ed.sync_from_host();
        let url = ed.ensure_server();
        // Reopen at the size the page last asked for, if the state has one.
        if let Some(v) = ed.bridge.store_get(WINDOW_STORE_KEY) {
            let sw = v.get("width").and_then(|x| x.as_f64()).unwrap_or(0.0) as u32;
            let sh = v.get("height").and_then(|x| x.as_f64()).unwrap_or(0.0) as u32;
            if sw != 0 && sh != 0 {
                let (sw, sh) = ed.clamp_size(sw, sh);
                ed.width.store(sw, Ordering::Relaxed);
                ed.height.store(sh, Ordering::Relaxed);
            }
        }
        let (w, h) = ed.size();

        let webview: Rc<RefCell<Option<EmbeddedWebView>>> = Rc::new(RefCell::new(None));

        // Edits and UI messages: queued on the network thread, handled on
        // the UI thread by a native timer.
        let forward = ed.forwarder(context.clone());
        let parent_raw = raw_parent(&parent);
        let timer = {
            let bridge = ed.bridge.clone();
            let editor = ed.clone();
            let webview = webview.clone();
            let ctx = context.clone();
            UiTimer::new(ed.forward_interval, move || {
                bridge.drain_edits(|e| forward(e));
                // The host resized the window (see `Editor::set_size`): the
                // web view follows and the size is remembered like a page
                // request.
                if editor.host_resized.swap(false, Ordering::AcqRel) {
                    let (w, h) = editor.size();
                    if let Some(wv) = webview.borrow().as_ref()
                        && let Err(e) = wv.resize(w, h)
                    {
                        nih_log!("noob-vst-webgui-framework: resize: {e}");
                    }
                    let _ = bridge.store_set(
                        WINDOW_STORE_KEY,
                        serde_json::json!({ "width": w, "height": h }),
                    );
                }
                // Take every queued message: `resize` requests are ours (only
                // the newest one in a batch matters, a drag sends many), the
                // rest go back in their original order for the plug-in.
                let mut others = Vec::new();
                let mut newest: Option<(u32, u32)> = None;
                let mut fullscreen: Option<(bool, Option<(u32, u32)>)> = None;
                let dims = |m: &noob_vst_webgui_framework::Message| {
                    let w = m.data.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
                    let h = m.data.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
                    (w != 0 && h != 0).then_some((w, h))
                };
                while let Some(m) = bridge.poll_message() {
                    if m.topic == "resize" {
                        if let Some(size) = dims(&m) {
                            newest = Some(size);
                        }
                    } else if m.topic == "fullscreen" {
                        // `{ "on": bool, "width"?, "height"? }`: the page's own
                        // idea of the screen size is the fallback.
                        let on = m.data.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
                        fullscreen = Some((on, dims(&m)));
                    } else {
                        others.push(m);
                    }
                }
                for m in others.into_iter().rev() {
                    bridge.requeue_message(m);
                }
                // Ask the host for a size and follow with the web view.
                let apply = |w: u32, h: u32, persist: bool| {
                    let (w, h) = editor.clamp_size(w, h);
                    if (w, h) == editor.size() {
                        return;
                    }
                    editor.width.store(w, Ordering::Relaxed);
                    editor.height.store(h, Ordering::Relaxed);
                    if ctx.request_resize()
                        && let Some(wv) = webview.borrow().as_ref()
                        && let Err(e) = wv.resize(w, h)
                    {
                        nih_log!("noob-vst-webgui-framework: resize: {e}");
                    }
                    if persist {
                        // Remembered with the plug-in state, so the editor
                        // reopens at this size (see `WINDOW_STORE_KEY`).
                        let _ = bridge.store_set(
                            WINDOW_STORE_KEY,
                            serde_json::json!({ "width": w, "height": h }),
                        );
                    }
                };
                if let Some((w, h)) = newest {
                    apply(w, h, true);
                }
                match fullscreen {
                    Some((true, fallback)) => {
                        if let Ok(mut r) = editor.restore.lock()
                            && r.is_none()
                        {
                            *r = Some(editor.size());
                        }
                        let target = parent_raw.and_then(|p| monitor_work_area(&p)).or(fallback);
                        if let Some((w, h)) = target {
                            apply(w, h, false);
                        }
                    }
                    Some((false, _)) => {
                        let back = editor.restore.lock().ok().and_then(|mut r| r.take());
                        if let Some((w, h)) = back {
                            apply(w, h, true);
                        }
                    }
                    None => {}
                }
            })
        };
        if timer.is_some() {
            ed.bridge.set_edit_hook(None);
        } else {
            ed.install_direct_hook();
        }

        match (url.as_deref(), parent_raw) {
            (Some(url), Some(p)) => {
                let mut opts = WebViewOptions::new(url, w, h);
                opts.devtools = ed.devtools;
                match EmbeddedWebView::new(&p, opts) {
                    Ok(wv) => *webview.borrow_mut() = Some(wv),
                    Err(e) => {
                        nih_log!(
                            "bridge: embedded web view unavailable ({e}); opening {url} in the system browser"
                        );
                        open_in_browser(url);
                    }
                }
            }
            (Some(url), None) => {
                nih_log!("bridge: unsupported parent window; opening {url} in the system browser");
                open_in_browser(url);
            }
            (None, _) => {}
        }

        Box::new(Instance {
            editor: ed.clone(),
            _webview: webview,
            _timer: timer,
        })
    }

    /// The size the host should give the window: see [`NoobVstWebguiFrameworkEditor::size`].
    fn size(&self) -> (u32, u32) {
        self.0.size()
    }

    /// Always `false`: sizes are logical pixels and the web view applies the
    /// monitor's scale factor itself.
    fn set_scale_factor(&self, _factor: f32) -> bool {
        // The web view handles DPI itself.
        false
    }

    /// `true` unless [`EditorConfig::size_limits`] pins one size: the host
    /// may resize the window (a frame drag, a host-side size menu) and the
    /// page follows through [`set_size`](Self::set_size).
    fn can_resize(&self) -> bool {
        self.0.min_size != self.0.max_size
    }

    /// The host proposes a size: answer with it clamped to
    /// [`EditorConfig::size_limits`].
    fn check_size_constraint(&self, width: u32, height: u32) -> (u32, u32) {
        self.0.clamp_size(width, height)
    }

    /// The host resized the window (GUI thread). The size is clamped and
    /// recorded, and the UI-thread timer resizes the web view and writes it
    /// to the `window` store key on its next tick, so the page reopens at it
    /// like a size it asked for itself. Always accepted.
    fn set_size(&self, width: u32, height: u32) -> bool {
        let ed = &self.0;
        let (w, h) = ed.clamp_size(width, height);
        if (w, h) != ed.size() {
            ed.width.store(w, Ordering::Relaxed);
            ed.height.store(h, Ordering::Relaxed);
            ed.host_resized.store(true, Ordering::Release);
        }
        true
    }

    /// One parameter changed on the host side (automation, another UI, a
    /// preset): mirror it into the bridge, which sends it to every page.
    fn param_value_changed(&self, id: &str, normalized_value: f32) {
        if let Some(i) = self.0.bridge.index_of(id) {
            self.0.bridge.set_param_norm(i, normalized_value);
        }
    }

    /// Modulation is not mirrored; the page shows unmodulated values.
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    /// Many parameters changed at once (state load): re-sync all of them.
    fn param_values_changed(&self) {
        self.0.sync_from_host();
    }
}

/// Translate nih-plug's parent handle into the web view crate's. `None` for
/// a null Win32 / AppKit handle.
fn raw_parent(parent: &ParentWindowHandle) -> Option<RawParent> {
    match *parent {
        ParentWindowHandle::Win32Hwnd(hwnd) => RawParent::win32(hwnd),
        ParentWindowHandle::AppKitNsView(view) => RawParent::appkit(view),
        ParentWindowHandle::X11Window(id) => Some(RawParent::x11(id)),
    }
}

/// Open `url` in the user's default browser, as the fallback when no web
/// view can be embedded. Failures are logged, not returned.
fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        nih_log!("bridge: could not open browser: {e}");
    }
}

// ---------------------------------------------------------------------------
// UI store persistence
// ---------------------------------------------------------------------------

/// Persists the page's key-value store (`client.store`, see
/// [`NoobVstWebguiFramework::store_json`]) inside the plug-in's state. Put one in your
/// `Params` struct (it is not a parameter), call [`attach`](Self::attach)
/// once the bridge exists, and forward `Params::serialize_fields` /
/// `Params::deserialize_fields` to it:
///
/// ```ignore
/// unsafe impl Params for MyParams {
///     fn param_map(&self) -> Vec<(String, ParamPtr, String)> { /* ... */ }
///     fn serialize_fields(&self) -> BTreeMap<String, String> {
///         let mut m = BTreeMap::new();
///         self.ui_store.serialize_into(&mut m);
///         m
///     }
///     fn deserialize_fields(&self, m: &BTreeMap<String, String>) {
///         self.ui_store.deserialize_from(m);
///     }
/// }
/// ```
///
/// # How restores are ordered
///
/// nih-plug may call `deserialize_fields` before the plug-in has built its
/// bridge (state is often loaded right after construction), and it may call
/// it again at any later time (the user loads a preset or a session). The
/// slot handles both:
/// * before [`attach`](Self::attach): the JSON is kept as *pending* and
///   applied when `attach` runs, and [`serialize_into`](Self::serialize_into)
///   writes the pending JSON back out unchanged, so a state saved before the
///   bridge existed is never lost;
/// * after `attach`: the JSON goes straight into the bridge, which replaces
///   the store and sends `store.all` to every connected page.
///
/// The store is written under the single key [`StoreSlot::KEY`] as one JSON
/// object; an empty store writes nothing, so states of plug-ins that never
/// used the store stay as they were.
///
/// All methods take `&self` and are safe from any thread (nih-plug calls them
/// from the GUI or a state-loading thread).
#[derive(Default)]
pub struct StoreSlot {
    inner: Mutex<StoreSlotInner>,
}

/// The slot's state: the bridge once attached, and JSON restored before
/// that.
#[derive(Default)]
struct StoreSlotInner {
    bridge: Option<NoobVstWebguiFramework>,
    pending: Option<String>,
}

impl StoreSlot {
    /// The key used inside the plug-in's persistent fields.
    pub const KEY: &'static str = "noob_vst_webgui_framework_ui_store";

    /// An unattached, empty slot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind to the bridge; applies any state restored before this call.
    ///
    /// Call once, right after building the bridge. Unreadable pending JSON
    /// is logged and dropped rather than failing the plug-in.
    pub fn attach(&self, bridge: &NoobVstWebguiFramework) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if let Some(json) = g.pending.take()
            && let Err(e) = bridge.store_load_json(&json)
        {
            nih_log!("bridge: ignoring unreadable UI store: {e}");
        }
        g.bridge = Some(bridge.clone());
    }

    /// Put the store into the map nih-plug persists (nothing if it is empty).
    ///
    /// Writes the live store when attached, otherwise whatever was restored
    /// but not yet applied.
    pub fn serialize_into(&self, fields: &mut BTreeMap<String, String>) {
        let Ok(g) = self.inner.lock() else {
            return;
        };
        let json = match (&g.bridge, &g.pending) {
            (Some(s), _) => s.store_json(),
            (None, Some(p)) => p.clone(),
            (None, None) => return,
        };
        if json != "{}" {
            fields.insert(Self::KEY.to_string(), json);
        }
    }

    /// Restore the store from the map nih-plug loaded. A map without the
    /// key (a state saved before the page kept anything) empties the store.
    ///
    /// When attached, the bridge is updated immediately and every connected
    /// page receives the new contents; otherwise the JSON waits for
    /// [`attach`](Self::attach). Malformed JSON is logged and ignored.
    pub fn deserialize_from(&self, fields: &BTreeMap<String, String>) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        let json = fields
            .get(Self::KEY)
            .cloned()
            .unwrap_or_else(|| "{}".to_string());
        match &g.bridge {
            Some(s) => {
                if let Err(e) = s.store_load_json(&json) {
                    nih_log!("bridge: ignoring unreadable UI store: {e}");
                }
            }
            None => g.pending = Some(json),
        }
    }
}
