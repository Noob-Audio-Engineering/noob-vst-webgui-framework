//! noob-vst-webgui-framework: a low-latency local WebSocket bridge between an audio
//! plug-in (or any real-time process) and a UI rendered in a browser or an
//! embedded web view.
//!
//! The plug-in keeps doing its work in Rust. The page gets a live mirror of
//! every parameter, a firehose of telemetry (`f32` frames: meters, spectra,
//! response curves, waveforms) and a channel for events (notes, controllers),
//! and sends parameter *gestures* and events back. Everything travels over
//! one WebSocket on the loopback interface in a compact little-endian binary
//! format ([`wire`], reference in `docs/WIRE.md`); JSON is used only for the
//! one-time manifest and for rare control messages.
//!
//! # Roles and threads
//!
//! A bridge is shared by three or four threads, each with its own handle:
//!
//! | thread | handle | what it does |
//! |---|---|---|
//! | audio (real-time) | [`AudioHandle`] | reads parameters, publishes stream frames, drains UI events, sends events |
//! | host / GUI (plug-in side) | [`NoobVstWebguiFramework`] | pushes host parameter changes, drains edits and messages, owns the UI store |
//! | `noob-vst-webgui-framework-pump` (spawned by `serve`) | internal | wakes when something was published, encodes each frame once, hands it to every client |
//! | `noob-vst-webgui-framework-net` (spawned by `serve`) | internal | single-threaded tokio + axum: HTTP, WebSocket, decoding of inbound frames |
//!
//! Inbound edits are applied to the parameter store *on the network thread*,
//! so the audio thread sees them on its next block with no extra hop; the
//! same edit is then queued for the host ([`NoobVstWebguiFramework::drain_edits`]) or handed
//! straight to an [`EditHook`], and fanned out to every client (the sender
//! gets it back flagged as an echo, so a page can measure its own latency).
//!
//! # Real-time contract
//!
//! Everything reachable through [`AudioHandle`] is wait-free after
//! construction:
//!
//! * parameter reads and writes are relaxed atomics ([`rt::AtomicF32`]);
//! * stream frames go through single-producer / single-consumer triple
//!   buffers ([`rt::mailbox`]): the audio thread always has a slot to write
//!   into, and a slow reader only ever loses *intermediate* frames;
//! * events use bounded lock-free queues (`crossbeam_queue::ArrayQueue`); a
//!   full queue drops the event and reports `false` instead of waiting;
//! * waking the pump is a `Mutex::try_lock` (skipped if contended) plus
//!   `Thread::unpark`, which is an atomic swap and, only if the pump is
//!   actually asleep, one lightweight wake syscall. `WakeMode::Poll` removes
//!   even that.
//!
//! Nothing on the audio path allocates: every buffer is sized in
//! [`NoobVstWebguiFrameworkBuilder::build`]. The [`NoobVstWebguiFramework`] handle, by contrast, takes
//! short uncontended mutexes (the JSON queues, the UI store, the hooks) and
//! is meant for the host, GUI or a worker thread, never the audio callback.
//!
//! # Quick start
//!
//! ```no_run
//! use noob_vst_webgui_framework::{NoobVstWebguiFramework, ParamSpec, StreamSpec, StreamKind, ServerConfig};
//!
//! let bridge = NoobVstWebguiFramework::builder("MyPlugin")
//!     .param(ParamSpec::new("cutoff", "Cutoff").range(20.0, 20000.0).log().default(1000.0).unit("Hz"))
//!     .param(ParamSpec::new("bypass", "Bypass").toggle())
//!     .stream(StreamSpec::new("meter", 2).kind(StreamKind::Meter).channels(2))
//!     .build();
//!
//! // Audio thread: read params, publish telemetry. Never blocks.
//! let mut audio = bridge.take_audio().unwrap();
//! let cutoff_hz = audio.param(0);
//! audio.publish_slice(0, &[0.5, 0.4]);
//!
//! // Anywhere else: start the server, hand the URL to a web view or browser.
//! let server = noob_vst_webgui_framework::serve(&bridge, ServerConfig::default().prefer_port(4242)).unwrap();
//! println!("{}", server.url());
//!
//! // Host loop: forward gestures to the host, answer messages from the page.
//! bridge.drain_edits(|e| println!("param {} {:?} -> {}", e.index, e.phase, e.value));
//! while let Some(m) = bridge.poll_message() {
//!     println!("client {} sent {} {}", m.client, m.topic, m.data);
//! }
//! ```
//!
//! # Feature flags
//!
//! * `server` (default): the HTTP / WebSocket server (`server` module),
//!   instance discovery (`discovery` module) and `FileStore::default_path`.
//!   Without it the crate is just the protocol ([`wire`]), the parameter
//!   store ([`params`]), the telemetry primitives ([`stream`], [`rt`]) and
//!   the bridge ([`bridge`]): enough to drive a custom transport or to write
//!   tests that never open a socket.
//!
//! # Module map
//!
//! * [`bridge`]: [`NoobVstWebguiFramework`] / [`AudioHandle`] and the state they share; the
//!   UI store; the edit, event and message queues.
//! * [`params`]: [`ParamSpec`] declarations, tapers, the lock-free value
//!   store and the manifest form of a parameter.
//! * [`stream`]: [`StreamSpec`] declarations and the frame type.
//! * [`rt`]: the real-time primitives, [`rt::AtomicF32`] and the triple
//!   buffer mailbox.
//! * [`wire`]: frame encoding and decoding, shared with the browser client.
//! * `server`: `serve`, `ServerConfig`, `PortPolicy`, `Assets`, `WakeMode`
//!   and the pump thread.
//! * `discovery`: per-user instance records and `/instance` probing.
//! * [`store_file`]: [`FileStore`], file-backed UI store persistence for
//!   standalone hosts.
//!
//! The matching browser library is `@noob-audio-engineering/noob-vst-webgui-framework` (`crates/noob-vst-webgui-framework/web/noob-vst-webgui-framework.js` in the
//! repository, also served by the server under `/noob-vst-webgui-framework/`); the nih-plug
//! adapter is the `noob-vst-webgui-framework-nih` crate.

pub mod bridge;
pub mod params;
pub mod rt;
pub mod store_file;
pub mod stream;
pub mod wire;

#[cfg(feature = "server")]
pub mod discovery;
#[cfg(feature = "server")]
pub mod server;

pub use bridge::{
    AudioHandle, EditEvent, EditHook, Message, NoobVstWebguiFramework,
    NoobVstWebguiFrameworkBuilder, ParamChange, StoreHook,
};
pub use params::{ParamSpec, Taper};
pub use store_file::FileStore;
pub use stream::{StreamKind, StreamSpec};
pub use wire::{EditPhase, UiEvent, event_kind};

#[cfg(feature = "server")]
pub use discovery::Instance;
#[cfg(feature = "server")]
pub use server::{Assets, PortPolicy, ServerConfig, ServerHandle, WakeMode, serve};
