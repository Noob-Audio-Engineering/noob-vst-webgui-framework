# Rust API tour

A guided walk through the three framework crates, in the order you meet
them when building a plug-in. Signatures are abbreviated; rustdoc
(`cargo doc --no-deps --workspace --open`) has the full text, the `# Errors`
and `# Real-time` notes, and examples. Thread annotations use the names from
[ARCHITECTURE.md](ARCHITECTURE.md#threads).

## vst3-web-stratum

### Declaring: `ParamSpec`, `StreamSpec`

```rust
ParamSpec::new(id, name)
    .range(min, max)        // plain range; default 0..1
    .default(plain)
    .unit("Hz") .group("filter")
    .log()                  // taper: logarithmic between min and max
    .skew(factor)           // taper: power curve (nih-plug style skew)
    .with_table(Vec<f32>)   // taper: explicit 65-point normalized→plain table
    .steps(n)               // discrete with n positions (2 for a toggle)
    .toggle()               // steps(2), range 0..1
    .labels(["Off", "On"])  // one label per step, sent in the manifest
    .not_automatable()      // hint for hosts and pages
```

`ParamSpec::normalize(plain)` and `denormalize(norm)` implement the taper;
`table(n)` samples it. `Taper::{Linear, Log, Skew(f32), Table(Vec<f32>)}`
is the enum behind it. `ParamManifest` and `ParamStore` are the manifest
record and the atomic store; you rarely touch them directly.

```rust
StreamSpec::new(id, capacity)      // capacity = max samples per frame
    .name("Output")
    .kind(StreamKind::{Spectrum, Meter, Waveform, Raw})
    .channels(2)
    .meta(json!({ "sample_rate": 48000 }))
    .sticky()                      // replay the last frame to late clients
```

### Building: `Vst3WebStratumBuilder` → `Vst3WebStratum`

```rust
let bridge = Vst3WebStratum::builder("My Plug-in")
    .meta(json!({ "vendor": "…", "sample_rate": 48000.0 }))   // free-form, sent in the manifest
    .param(spec) / .params(iter)
    .stream(spec)
    .ui_queue(4096)      // parameter changes waiting for the pump
    .host_queue(1024)    // edits waiting for the host
    .build();
```

`Vst3WebStratum` is cheap to clone (an `Arc`); every clone is the same bridge.

### The host side of `Vst3WebStratum` (any thread except audio)

| Method | Purpose |
|---|---|
| `name()`, `manifest_json()`, `param_count()`, `stream_count()` | introspection |
| `index_of(id)`, `spec(i)`, `specs()`, `stream_specs()` | resolve ids once, at start-up |
| `param(i)`, `param_norm(i)` | read the current value |
| `set_param(i, plain)`, `set_param_norm(i, norm)` | the plug-in or host changed a value; fans out to clients |
| `sync_all_params()` | resend everything (after a state restore) |
| `drain_edits(|EditEvent| …)` | edits from pages: `{ index, value, phase: Begin/Perform/End, client }` |
| `set_edit_hook(Option<EditHook>)` | alternative to draining: called on the net thread per edit |
| `send_json(topic, Value)` | a `{ t: "msg" }` text frame to every client |
| `poll_message()`, `requeue_message(m)` | messages from pages: `{ topic, data, client }` |
| `push_event(UiEvent)`, `drain_ui_events(…)` | events to and from pages, host-side end |
| `store_get / store_set / store_snapshot / store_json / store_replace / store_load_json / set_store_hook` | the UI store |
| `take_audio()`, `return_audio(h)` | the single `AudioHandle` |
| `now_us()`, `dropped_ui_changes()` | clock and diagnostics |

### The audio side: `AudioHandle`

Exactly one exists per bridge. Everything on it is wait-free or lock-free and
allocation-free:

| Method | Purpose |
|---|---|
| `param(i)`, `param_norm(i)` | atomic load |
| `set_param_norm(i, norm)` | for parameters the audio thread owns (rare) |
| `publish(stream, |buf| n)` | fill a frame in place; returns whether a frame was taken |
| `publish_slice(stream, &[f32])` | copy a slice as a frame |
| `drain_events(|UiEvent| …)` | notes and controllers from pages |
| `send_event(UiEvent)` | notes and controllers to pages (key lights) |
| `now_ns()`, `stream_count()` | |

`UiEvent { kind, channel, a, b, value, offset }` with `event_kind::{NOTE_ON,
NOTE_OFF, CONTROL, PITCH_BEND, AFTERTOUCH, PROGRAM, CUSTOM}` and constructors
`UiEvent::note_on(ch, note, vel)`, `note_off(ch, note, vel)`.

### Serving: `ServerConfig`, `serve`, `ServerHandle`

```rust
let cfg = ServerConfig::default()
    .prefer_port(4242)                     // or .port(n) / .ephemeral() / .port_policy(PortPolicy::for_name(name))
    .discovery(true)                       // write the instance record (default on)
    .assets_dir("web/dist")                // or .embedded(&[(path, bytes)]) or Assets::Lookup(fn)
    .echo_edits(true)                      // echo a client's own edits back to it
    .wake(WakeMode::Unpark)                // or WakeMode::Poll
    .poll_interval(Duration::from_millis(1));
let server = vst3_web_stratum::serve(&bridge, cfg)?;
server.url(); server.ws_url(); server.port(); server.addr(); server.client_count();
server.shutdown();                          // or drop it
```

`Assets::{None, Dir(PathBuf), Embedded(&'static [(&str, &[u8])]),
Lookup(fn(&str) -> Option<&'static [u8]>)}`. The client library is always
served under `/vst3-web-stratum/` (`CLIENT_ASSETS`), so a page can import
`/vst3-web-stratum/vst3-web-stratum.js` without bundling.

`PortPolicy::{Fixed(u16), Ephemeral, Probe { base, span }}` and
`PortPolicy::for_name(&str)`.

### Discovery (`vst3_web_stratum::discovery`)

`Instance { name, pid, port, url, started, protocol }`, `Instance::new(name,
port)`, `dir()`, `publish(&Instance)`, `unpublish(&path)`, `list_files()`,
`probe(port, timeout)`, `list_live(timeout)`. `serve` calls `publish` and
`ServerHandle` calls `unpublish`; you call `list_live` when you want to know
what else is running (the server does so for `/instances`).

### Persistence for standalones: `FileStore`

```rust
let store = FileStore::attach(&bridge, FileStore::default_path("my-app"));
loop { store.flush()?; /* … */ }     // writes only when something changed
```

### The wire codec (`vst3_web_stratum::wire`)

`Kind` (frame kinds), `Frame` (decoded view), `ParamValuesWriter`,
`ParamEditWriter`, `EventsWriter`, `encode_stream_f32`, the header and entry
length constants, `PROTOCOL_VERSION`. You need these only for a client in
another language or a custom transport; the server and `@elyerinfox/vst3-web-stratum` use
them for you. [WIRE.md](WIRE.md) documents the bytes.

### Real-time primitives (`vst3_web_stratum::rt`)

`AtomicF32` and `mailbox()` (a wait-free single-writer, single-reader triple
buffer returning `MailboxWriter` / `MailboxReader`). Reusable in your own
DSP for anything "latest wins".

## vst3-web-stratum-nih

```rust
let (editor, bridge) = Vst3WebStratumEditor::with_builder(
    "My Plug-in", params.as_ref(), streams(48_000.0),
    EditorConfig::new(1000, 640).assets(Assets::Lookup(ui_lookup)),
    |b| b.meta(json!({ … })),
);
let audio = bridge.take_audio();
params.ui_store.attach(&bridge);
// Plugin::editor: Some(Box::new(self.editor.handle()))
```

| Item | Purpose |
|---|---|
| `mirror_params(&dyn Params) -> Vec<(ParamSpec, ParamPtr)>` | nih-plug parameters as vst3-web-stratum specs with 65-point tables, in `param_map` order |
| `EditorConfig { width, height, assets, port: Option<PortPolicy>, discovery, devtools, echo_edits, forward_interval, min_size, max_size }` | builders `.assets .port .port_policy .discovery .devtools .size_limits` |
| `Vst3WebStratumEditor::{new, with_builder, handle, bridge, ensure_server, url, size}` | one per plug-in instance; the server starts lazily on the first editor open |
| `EditorHandle: nih_plug::Editor` | what `Plugin::editor` returns; `spawn` embeds the web view, installs the UI timer, syncs from the host |
| `StoreSlot::{new, attach, serialize_into, deserialize_from}`, `StoreSlot::KEY` | persist the UI store in plug-in state |

Messages the adapter consumes: `resize` `{ width, height }` and
`fullscreen` `{ on, width?, height? }`. Everything else is left in the queue
for `Vst3WebStratum::poll_message`. `EditorHandle` also implements the
host-to-plugin side, `can_resize` / `check_size_constraint` / `set_size`,
which exist only in the patched nih-plug this workspace builds against (see
[DEVELOPMENT.md](DEVELOPMENT.md)).

## vst3-web-stratum-webview

| Item | Purpose |
|---|---|
| `RawParent::{win32(hwnd), appkit(ns_view), x11(window)}` | wrap the host's window handle |
| `WebViewOptions { url, width, height, devtools, background, init_script }` | `WebViewOptions::new(url, w, h)` |
| `EmbeddedWebView::new(&RawParent, WebViewOptions)` → `.resize .navigate .eval .open_devtools .inner` | the child web view |
| `UiTimer::new(interval, FnMut()) -> Option<UiTimer>` | a native GUI-thread timer (Windows today; `None` elsewhere) |
| `Error::{Unsupported, Wry}` | |

## Conventions across the crates

* Indices are `usize` on the Rust side and `u16` on the wire; ids are
  strings resolved once.
* Normalized values are `f32` in `0..1`; plain values are `f32` in the
  parameter's unit.
* Everything that can fail on the audio thread returns `bool` (published /
  queued or not) rather than `Result`, and never logs there.
* `Vst3WebStratum` and `AudioHandle` are `Send`; `AudioHandle` is not `Sync` and is
  meant to live on one thread. `EmbeddedWebView` and `UiTimer` are neither
  and must stay on the GUI thread.
