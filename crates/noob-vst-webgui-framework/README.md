# noob-vst-webgui-framework

The heart of [noob-vst-webgui-framework](../../README.md): a low-latency local
WebSocket bridge between an audio plug-in (or any real-time process) and a
UI rendered in a browser or an embedded web view.

The plug-in keeps its DSP and its parameters in Rust. The page gets:

* a live mirror of every **parameter** (normalized values, with enough
  metadata in the manifest to format and scale them), and sends back
  **gestures** (begin / perform / end) so hosts record automation correctly;
* **telemetry streams**: `f32` frames (meters, spectra, curves, waveforms)
  published from the audio thread with latest-wins semantics, throttled per
  client, optionally *sticky* so late clients get the last frame at once;
* **events** both ways (notes, controllers, plugin-defined signals);
* ad-hoc **JSON messages** both ways, and a small plug-in-owned **UI store**
  for page state that should travel with the plug-in (presets, view
  settings).

Everything travels over one WebSocket on `127.0.0.1` in a compact
little-endian binary format; JSON is used only for the manifest and control
messages. The format is specified in [`docs/WIRE.md`](../../docs/WIRE.md).

## Threading model

| thread | handle | may do |
|---|---|---|
| audio (real-time) | `AudioHandle` | read parameters, publish frames, drain / send events; wait-free, no allocation |
| host / GUI | `NoobVstWebguiFramework` | push host changes, drain edits and messages, own the UI store; short uncontended mutexes |
| `noob-vst-webgui-framework-pump` | internal | wakes on publish, encodes each frame once, hands it to every client without blocking |
| `noob-vst-webgui-framework-net` | internal | single-threaded tokio + axum; HTTP, WebSocket, decoding of inbound frames |

Parameter values are relaxed atomics; streams are single-producer /
single-consumer triple buffers; events are bounded lock-free queues. The
audio thread wakes the pump with one `unpark` (an atomic swap plus, only if
the pump is asleep, a lightweight wake syscall); `WakeMode::Poll` avoids
even that. TCP_NODELAY is on so a 12-byte edit is not held back by Nagle.

Measured on Noob-Q (release build, 386 parameters): edit-to-echo
round trip p50 about 50 µs, p99 under 300 µs; ping p50 about 40 µs.

## Quick start

```rust
use noob_vst_webgui_framework::{NoobVstWebguiFramework, ParamSpec, StreamSpec, StreamKind, ServerConfig};

let bridge = NoobVstWebguiFramework::builder("MyPlugin")
    .param(ParamSpec::new("cutoff", "Cutoff").range(20.0, 20000.0).log().default(1000.0).unit("Hz"))
    .param(ParamSpec::new("bypass", "Bypass").toggle())
    .stream(StreamSpec::new("meter", 2).kind(StreamKind::Meter).channels(2))
    .build();

// Audio thread: read params, publish telemetry. Never blocks.
let mut audio = bridge.take_audio().unwrap();
let cutoff_hz = audio.param(0);
audio.publish_slice(0, &[0.5, 0.4]);

// Anywhere else: start the server, hand the URL to a web view or browser.
let server = noob_vst_webgui_framework::serve(&bridge, ServerConfig::default().prefer_port(4242)).unwrap();
println!("{}", server.url());

// Host loop: forward gestures to the host, answer messages from the page.
bridge.drain_edits(|e| println!("param {} {:?} -> {}", e.index, e.phase, e.value));
while let Some(m) = bridge.poll_message() {
    println!("client {} sent {} {}", m.client, m.topic, m.data);
}
```

The page side is the `@noob-audio-engineering/noob-vst-webgui-framework` library (`web/` inside this crate, at the repository
root), which the server also serves under `/noob-vst-webgui-framework/`:

```js
import { NoobVstWebguiFrameworkClient } from '/noob-vst-webgui-framework/noob-vst-webgui-framework.js';
const client = new NoobVstWebguiFrameworkClient();
client.on('manifest', () => {
  const cutoff = client.param('cutoff');
  cutoff.on((norm) => console.log(cutoff.format(cutoff.toPlain(norm))));
  client.stream('meter').on((frame) => draw(frame));
});
```

## Configuration

`ServerConfig` (all builders return `Self`):

| option | default | meaning |
|---|---|---|
| `ip` | `127.0.0.1` | never bind elsewhere: there is no authentication |
| `port` / `prefer_port` / `ephemeral` / `port_policy` | ephemeral | fixed port, probe a range, or let the OS choose; `PortPolicy::for_name` hashes a plug-in name into a stable base |
| `discovery` | `true` | write a per-user discovery record and answer `/instance` |
| `assets_dir` / `embedded` / `Assets::Lookup` | none | where the page's files come from |
| `echo_edits` | `true` | send a client its own edits back flagged as echoes |
| `wake` | `Unpark` | how the audio thread wakes the pump |
| `poll_interval` | 1 ms | pump period in `Poll` mode, fallback timeout in `Unpark` mode |
| `send_queue` | 256 | per-client outbound queue (messages) |
| `max_message_size` | 1 MiB | largest inbound WebSocket message |

## Features

* `server` (default): the HTTP / WebSocket server, instance discovery and
  `FileStore::default_path`. Without it the crate is the protocol, the
  parameter store, the real-time primitives and the bridge: enough for a
  custom transport or for tests that never open a socket.

## Modules

| module | contents |
|---|---|
| `bridge` | `NoobVstWebguiFramework`, `AudioHandle`, `NoobVstWebguiFrameworkBuilder`, the shared state, the UI store, edit / event / message queues |
| `params` | `ParamSpec`, `Taper`, `ParamStore`, `ParamManifest` |
| `stream` | `StreamSpec`, `StreamKind`, `StreamFrame`, `StreamManifest` |
| `rt` | `AtomicF32`, the triple-buffer `mailbox` |
| `wire` | frame kinds, encoders, the zero-copy decoder |
| `server` | `serve`, `ServerConfig`, `PortPolicy`, `Assets`, `WakeMode`, `ServerHandle`, the pump loop |
| `discovery` | `Instance`, the per-user records, `probe`, `list_live` |
| `store_file` | `FileStore` |

Run `cargo doc --open -p noob-vst-webgui-framework` for the API reference. The
integration tests in `tests/server.rs` double as a worked example of every
frame kind against a live server.

## Further reading

* [`docs/WIRE.md`](../../docs/WIRE.md): the protocol, byte by byte.
* [`docs/`](../../docs/): architecture, guides and the getting-started walkthrough.
* `noob-vst-webgui-framework-nih`: the nih-plug editor adapter (embedded web view, gesture
  forwarding, UI store persistence).
* `web/`: the browser library and its Vue layer.
