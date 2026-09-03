# vst3-web-stratum-nih

The [nih-plug](https://github.com/robbert-vdh/nih-plug) adapter for
[vst3-web-stratum](../../README.md): a plug-in `Editor`
whose window is the operating system's web view, showing a page the plug-in
serves itself over vst3-web-stratum. Build the page with whatever you like (the
examples use Vue + Tailwind through Vite); the plug-in ships it embedded and
talks to it over a local WebSocket with sub-millisecond latency.

## What you get

- `mirror_params`: every nih-plug parameter becomes a vst3-web-stratum parameter with
  the same id, group, unit, default, step labels and a 65-point value table,
  so the page renders any range correctly without duplicating formulas.
- `Vst3WebStratumEditor`: owns the bridge and the server (started lazily when the
  editor is first opened), mirrors host changes to every connected page, and
  forwards page edits back to the host as begin / perform / end gestures
  from the GUI thread.
- `EditorHandle`: the `Editor` you return from `Plugin::editor`. It creates
  the embedded web view in the host's window, or opens the page in the
  system browser where that is not possible.
- `StoreSlot`: saves the page's key-value store (presets, favourites, view
  settings) inside the plug-in state, so it is restored with the session.
- Port probing and discovery out of the box: instances never collide, keep
  a stable origin between sessions, and can be listed with
  `GET /instances` or `node tools/instances.mjs`.

## Requirements

- nih-plug from git (the workspace pins a revision), VST3 and/or CLAP
  export as usual with `nih_export_vst3!` / `nih_export_clap!`.
- A page to serve. During development `Assets::Dir` can point at a Vite
  build (or run the Vite dev server against the plug-in's server); for a
  release, embed `web/dist` with `include_dir` and `Assets::Lookup`.
- The platform requirements of [`vst3-web-stratum-webview`](../vst3-web-stratum-webview/README.md):
  WebView2 on Windows (ships with Windows 11 / Edge), WKWebView on macOS,
  WebKitGTK development packages on Linux at build time.

## Quick start

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use nih_plug::prelude::*;
use vst3_web_stratum::{Assets, AudioHandle, Vst3WebStratum, StreamKind, StreamSpec};
use vst3_web_stratum_nih::{EditorConfig, StoreSlot, Vst3WebStratumEditor};

static UI: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/dist");
fn ui_lookup(path: &str) -> Option<&'static [u8]> { UI.get_file(path).map(|f| f.contents()) }

struct MyParams { cutoff: FloatParam, ui_store: StoreSlot }

unsafe impl Params for MyParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        vec![("cutoff".into(), self.cutoff.as_ptr(), "filter".into())]
    }
    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        self.ui_store.serialize_into(&mut m);
        m
    }
    fn deserialize_fields(&self, m: &BTreeMap<String, String>) { self.ui_store.deserialize_from(m); }
}

struct MyPlugin { params: Arc<MyParams>, editor: Arc<Vst3WebStratumEditor>, bridge: Vst3WebStratum, audio: Option<AudioHandle> }

impl Default for MyPlugin {
    fn default() -> Self {
        let params = Arc::new(MyParams { /* ... */ ui_store: StoreSlot::new() });
        let streams = vec![StreamSpec::new("meter", 2).kind(StreamKind::Meter).channels(2)];
        let (editor, bridge) = Vst3WebStratumEditor::with_builder(
            "My Plugin", params.as_ref(), streams,
            EditorConfig::new(1000, 640).assets(Assets::Lookup(ui_lookup)),
            |b| b.meta(serde_json::json!({ "vendor": "Me" })),
        );
        let audio = bridge.take_audio();
        params.ui_store.attach(&bridge);
        MyPlugin { params, editor, bridge, audio }
    }
}

impl Plugin for MyPlugin {
    // ... constants, params(), initialize() ...
    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(self.editor.handle()))
    }
    fn process(&mut self, buffer: &mut Buffer, _: &mut AuxiliaryBuffers, _: &mut impl ProcessContext<Self>) -> ProcessStatus {
        // ... DSP ...
        if let Some(audio) = self.audio.as_mut() { audio.publish_slice(0, &[peak_l, peak_r]); }
        ProcessStatus::Normal
    }
}
```

On the page, `@elyerinfox/vst3-web-stratum` (or `@elyerinfox/vst3-web-stratum/vue`) connects to `/ws` on
the same origin, receives every parameter from the manifest, and its
controls send gestures back. The complete walk-through, including the Vite
project, is in [Getting started](../../docs/GETTING-STARTED.md).

## Configuration

`EditorConfig::new(width, height)` with builders:

| builder | default | meaning |
|---|---|---|
| `.assets(Assets)` | `Assets::None` | where the page comes from (`Lookup` for `include_dir`, `Dir` for a folder, `None` to serve only `/ws`) |
| `.port(n)` / `.port_policy(p)` | probe a range derived from the plug-in name | fixed port, ephemeral (`0`), or an explicit `PortPolicy` |
| `.discovery(bool)` | `true` | write a discovery record for `/instances` |
| `.devtools(bool)` | `true` in debug builds | web view developer tools |
| `.size_limits(min, max)` | `(480, 320)` to `(7680, 4320)` | bounds for sizes the page may request |
| field `echo_edits` | `true` | echo a client's own edits back (latency measurement, multi-window sync) |
| field `forward_interval` | 3 ms | period of the UI-thread timer that forwards edits |

## How it works

- **Host to page**: `Editor::param_value_changed` (GUI thread) writes into
  the bridge; the pump thread sends the change to every client.
- **Page to host**: edits arrive on the network thread and are queued. While
  the window is open a native UI-thread timer drains them and calls
  `GuiContext::raw_begin_set_parameter` / `raw_set_parameter_normalized` /
  `raw_end_set_parameter`, as the VST3 and CLAP specs ask. Without such a
  timer (non-Windows platforms today) or after the window closed while a
  browser tab is still connected, edits are forwarded directly from the
  network thread.
- **Resize**: the page sends `client.send('resize', { width, height })`;
  the request is clamped, the host is asked, and the web view follows.
  `fullscreen` `{ on }` grows the window to the monitor's work area and
  back. The host may resize the window too (a frame drag): `can_resize`
  answers `true` unless `size_limits` pins one size, `check_size_constraint`
  clamps, and `set_size` records the size for the timer to apply. Either way
  the size is remembered under the `window` store key and the editor
  reopens at it. Host-driven resizing needs the patched nih-plug this
  workspace builds against (see `docs/DEVELOPMENT.md`).
- **Other messages** the page sends stay queued for the plug-in to read
  with `Vst3WebStratum::poll_message` from a non-real-time thread.
- **State**: the UI store is written as one JSON object under the key
  `vst3_web_stratum_ui_store` in the plug-in's persistent fields. Restores that
  arrive before the bridge exists are kept and applied on `attach`.

## Status

Compile-checked as VST3 and CLAP; exercised through the examples' standalone
binaries and a browser. A run inside a DAW is still pending, so resize
negotiation (in both directions) and DPI behaviour in specific hosts are
unverified.

## Further reading

- [Getting started](../../docs/GETTING-STARTED.md)
- [Architecture](../../docs/ARCHITECTURE.md): threads, data flow,
  real-time guarantees
- [Wire format](../../docs/WIRE.md)
- API docs: `cargo doc --no-deps -p vst3-web-stratum-nih --open`
