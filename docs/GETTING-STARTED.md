# Getting started

This tutorial builds a small but complete plug-in on vst3-web-stratum: **Hello
Gain**, a stereo gain with a tone control, a level meter and an oscilloscope,
whose editor is a Vue page in the OS web view. You will end up with the same
shape the two examples in this repository have:

```
hello-gain/
  Cargo.toml
  src/
    lib.rs          the nih-plug plug-in (feature "plugin")
    engine.rs       the DSP and the stream declarations, shared by both binaries
    bin/standalone.rs   a dev server with a fake audio thread
  web/              a Vite + Vue project; `npm run build` writes web/dist
```

If you prefer to read finished code, `examples/noob-wave` is this tutorial at
full scale. Every API mentioned here is documented in rustdoc
(`cargo doc --open`) and in [RUST-API.md](RUST-API.md); the browser side is
in [../crates/vst3-web-stratum/web/README.md](../crates/vst3-web-stratum/web/README.md).

## 0. Prerequisites

* Rust stable (edition 2024) with `cargo`; Node 20 or newer.
* Windows 10/11 (WebView2 is built in), macOS 11+ (WKWebView), or Linux with
  `libwebkit2gtk-4.1-dev` and `libgtk-3-dev`.
* This repository checked out next to your project, or vendored; the crates
  are not on crates.io (see [DEVELOPMENT.md](DEVELOPMENT.md#release-checklist)).

## 1. The crate

```toml
# hello-gain/Cargo.toml
[package]
name = "hello-gain"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib", "lib"]

[features]
default = []
plugin = ["dep:nih_plug", "dep:vst3-web-stratum-nih", "dep:include_dir"]

[dependencies]
vst3-web-stratum = { path = "../vst3-web-stratum/crates/vst3-web-stratum" }
serde_json = "1"
log = "0.4"
env_logger = "0.11"
# plug-in only
nih_plug = { git = "https://github.com/robbert-vdh/nih-plug.git", optional = true }
vst3-web-stratum-nih = { path = "../vst3-web-stratum/crates/vst3-web-stratum-nih", optional = true }
include_dir = { version = "0.7", optional = true }
```

The `plugin` feature keeps the standalone free of nih-plug, so you can
iterate on DSP and UI without a host and without `web/dist` existing.

## 2. Parameters and streams

Parameters and streams are declared once and used by both the standalone and
the plug-in. Put them in `src/engine.rs`.

```rust
// src/engine.rs
use serde_json::json;
use vst3_web_stratum::{AudioHandle, ParamSpec, StreamKind, StreamSpec};

pub const SCOPE_LEN: usize = 512;

/// Stream indices, in the order `streams()` declares them.
pub const METER: usize = 0;
pub const SCOPE: usize = 1;

/// The parameters as bridge specs (the standalone uses these directly; the
/// plug-in mirrors its nih-plug parameters instead, with the same ids).
pub fn param_specs() -> Vec<ParamSpec> {
    vec![
        ParamSpec::new("gain", "Gain").range(-24.0, 24.0).default(0.0).unit("dB").group("main"),
        ParamSpec::new("tone", "Tone").range(20.0, 20_000.0).log().default(1_000.0).unit("Hz").group("main"),
        ParamSpec::new("bypass", "Bypass").toggle().group("main"),
    ]
}

pub fn streams(sample_rate: f32) -> Vec<StreamSpec> {
    vec![
        StreamSpec::new("meter", 2).name("Output").kind(StreamKind::Meter).channels(2).meta(json!({ "layout": "peak_l,peak_r", "db": true })),
        StreamSpec::new("scope", SCOPE_LEN).name("Output").kind(StreamKind::Waveform).meta(json!({ "sample_rate": sample_rate })),
    ]
}

/// Indices of the parameters, resolved once by id so the audio thread never
/// looks anything up by string.
pub struct Ix { pub gain: usize, pub tone: usize, pub bypass: usize }

pub fn indices(s: &vst3_web_stratum::Vst3WebStratum) -> Ix {
    let ix = |id: &str| s.index_of(id).expect(id);
    Ix { gain: ix("gain"), tone: ix("tone"), bypass: ix("bypass") }
}

/// The DSP. Owns its buffers; `process` neither allocates nor blocks.
pub struct Engine {
    sample_rate: f32,
    lp_l: f32,
    lp_r: f32,
    scope: Vec<f32>,
    scope_pos: usize,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        Engine { sample_rate, lp_l: 0.0, lp_r: 0.0, scope: vec![0.0; SCOPE_LEN], scope_pos: 0 }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
    }

    /// Process one block in place and publish telemetry.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], audio: &mut AudioHandle, ix: &Ix) {
        let gain = 10f32.powf(audio.param(ix.gain) / 20.0);
        let bypass = audio.param(ix.bypass) >= 0.5;
        // One-pole low-pass at the tone frequency.
        let k = 1.0 - (-2.0 * std::f32::consts::PI * audio.param(ix.tone) / self.sample_rate).exp();
        let (mut peak_l, mut peak_r) = (0.0f32, 0.0f32);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            if !bypass {
                self.lp_l += k * (*a - self.lp_l);
                self.lp_r += k * (*b - self.lp_r);
                *a = self.lp_l * gain;
                *b = self.lp_r * gain;
            }
            peak_l = peak_l.max(a.abs());
            peak_r = peak_r.max(b.abs());
            self.scope[self.scope_pos] = 0.5 * (*a + *b);
            self.scope_pos = (self.scope_pos + 1) % SCOPE_LEN;
        }
        let db = |p: f32| 20.0 * p.max(1e-9).log10();
        audio.publish_slice(METER, &[db(peak_l), db(peak_r)]);
        if self.scope_pos == 0 {
            audio.publish_slice(SCOPE, &self.scope);
        }
    }
}
```

Two things to notice. `audio.param(i)` is an atomic load of the **plain**
value; it costs nothing and may be called per block or per sample.
`publish_slice` copies into a wait-free triple buffer and wakes the network
side; the copy is the only work done on the audio thread.

## 3. The standalone

A standalone is a plain binary: build the bridge from the specs, run a fake
audio thread, start the server, and loop on the host side.

```rust
// src/bin/standalone.rs
use std::thread;
use std::time::{Duration, Instant};

use hello_gain::engine::{self, Engine};
use serde_json::json;
use vst3_web_stratum::{FileStore, ServerConfig, Vst3WebStratum};

const SR: f32 = 48_000.0;
const BLOCK: usize = 256;

fn main() {
    env_logger::init();
    let bridge = {
        let mut b = Vst3WebStratum::builder("Hello Gain")
            .meta(json!({ "vendor": "You", "sample_rate": SR, "standalone": true }))
            .params(engine::param_specs());
        for s in engine::streams(SR) {
            b = b.stream(s);
        }
        b.build()
    };
    let ix = engine::indices(&bridge);
    let mut audio = bridge.take_audio().expect("one audio handle");

    // The fake audio thread: a sine through the engine at block rate.
    thread::Builder::new().name("fake-audio".into()).spawn(move || {
        let mut engine = Engine::new(SR);
        let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        let mut phase = 0.0f32;
        let period = Duration::from_secs_f64(BLOCK as f64 / SR as f64);
        let mut next = Instant::now();
        loop {
            for i in 0..BLOCK {
                phase = (phase + 220.0 / SR) % 1.0;
                l[i] = (phase * std::f32::consts::TAU).sin() * 0.5;
                r[i] = l[i];
            }
            engine.process(&mut l, &mut r, &mut audio, &ix);
            next += period;
            if let Some(d) = next.checked_duration_since(Instant::now()) { thread::sleep(d); }
        }
    }).unwrap();

    // Page state (presets, view settings) persists in a file between runs.
    let store = FileStore::attach(&bridge, FileStore::default_path("hello-gain"));

    let server = vst3_web_stratum::serve(&bridge, ServerConfig::default().prefer_port(4250).assets_dir("web/dist")).expect("serve");
    println!("open {}", server.url());

    // The host side: forward edits (a real host would record automation),
    // answer messages, report status.
    let mut last = Instant::now();
    loop {
        bridge.drain_edits(|e| log::debug!("edit #{} {:?} -> {:.3}", e.index, e.phase, e.value));
        while let Some(m) = bridge.poll_message() {
            log::info!("message {} {}", m.topic, m.data);
        }
        if last.elapsed() > Duration::from_secs(1) {
            last = Instant::now();
            bridge.send_json("status", json!({ "clients": server.client_count() }));
        }
        store.flush().ok();
        thread::sleep(Duration::from_millis(5));
    }
}
```

Run it with `cargo run --bin standalone`, and
until `web/dist` exists it serves a 404 for the page but the WebSocket is
live: `node ../vst3-web-stratum/tools/bench.mjs 4250` already works.

## 4. The page

### Vanilla, no build step

The server embeds the client library at `/vst3-web-stratum/`, so the smallest page is
one HTML file in `web/dist`:

```html
<!doctype html>
<meta charset="utf-8">
<div id="gain"></div><div id="tone"></div><div id="meter" style="height:120px"></div>
<script type="module">
  import { Vst3WebStratumClient } from '/vst3-web-stratum/vst3-web-stratum.js';
  import { Knob, Meter } from '/vst3-web-stratum/components/index.js';

  const client = new Vst3WebStratumClient(null, { pingIntervalMs: 500 });
  client.on('manifest', () => {
    new Knob(document.getElementById('gain'), client.param('gain'), { size: 64 });
    new Knob(document.getElementById('tone'), client.param('tone'), { size: 64 });
    new Meter(document.getElementById('meter'), client.stream('meter'), { minDb: -60, maxDb: 6 });
  });
</script>
```

`client.param(id)` returns a `Param` handle: `value` (normalized), `plain`,
`format()`, `set(norm)`, `setPlain(v)`, `beginEdit()` / `endEdit()`, and
`on(fn)` for changes from anywhere else. `client.stream(id).on(frame => …)`
delivers each frame as a `Float32Array` view without copying.
`crates/vst3-web-stratum/web/examples/vanilla/index.html` is a larger version of this page.

### Vue + Vite

```sh
cd hello-gain && mkdir web && cd web && npm init -y
npm install vue @elyerinfox/vst3-web-stratum@file:../../vst3-web-stratum/web
npm install -D vite @vitejs/plugin-vue tailwindcss @tailwindcss/vite
```

```js
// web/vite.config.js
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';

const serverPort = process.env.VST3_WEB_STRATUM_PORT || '4250';
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  base: './',
  resolve: { preserveSymlinks: true, dedupe: ['vue'] },   // the library is a file: link
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022' },
  server: {
    fs: { allow: [fileURLToPath(new URL('../../', import.meta.url))] },
    proxy: {
      '/ws': { target: `ws://127.0.0.1:${serverPort}`, ws: true },
      '/instance': { target: `http://127.0.0.1:${serverPort}` },
    },
  },
});
```

```html
<!-- web/index.html -->
<!doctype html><meta charset="utf-8"><div id="app"></div><script type="module" src="/src/main.js"></script>
```

```js
// web/src/main.js
import { createApp } from 'vue';
import App from './App.vue';
import './style.css';          // @import "tailwindcss"; @source "../../../vst3-web-stratum/web/vue";
createApp(App).mount('#app');
```

```vue
<!-- web/src/App.vue -->
<script setup>
import { onMounted, ref } from 'vue';
import { Knob, useParam, useVst3WebStratum, useStream } from '@elyerinfox/vst3-web-stratum/vue';
import { Meter } from '@elyerinfox/vst3-web-stratum/components';

const { ready, connected, stats, status } = useVst3WebStratum();
const meterEl = ref(null);
onMounted(() => new Meter(meterEl.value, useStream('meter'), { minDb: -60, maxDb: 6 }));
</script>

<template>
  <main class="p-4 flex gap-6 items-center text-slate-200 bg-slate-900 min-h-screen">
    <template v-if="ready">
      <Knob :p="useParam('gain')" :size="72" label="Gain" />
      <Knob :p="useParam('tone')" :size="72" label="Tone" />
      <button class="px-3 py-1 rounded border" :class="{ 'bg-red-500': useParam('bypass').on }" @click="useParam('bypass').toggle()">Bypass</button>
    </template>
    <div ref="meterEl" class="w-8 h-32"></div>
    <span class="ml-auto text-xs opacity-60">{{ connected ? 'online' : 'offline' }} · echo {{ (stats.echoAvgMs * 1000).toFixed(0) }} µs · clients {{ status?.clients ?? '-' }}</span>
  </main>
</template>
```

`useVst3WebStratum()` gives you connection state, the manifest, statistics and the
undo history; `useParam(id)` returns a reactive handle (`plain`, `text`,
`on`, `set`, `toggle`, `begin`, `end`, `reset`, …) that every component
asking for the same id shares. Call it once `ready` is true.

```sh
VST3_WEB_STRATUM_PORT=4250 npm run dev     # hot reload against the running standalone
npm run build                     # writes web/dist for the standalone and the plug-in
```

## 5. The plug-in

With nih-plug the host owns the parameters. The adapter mirrors them into
vst3-web-stratum with the same ids, forwards gestures to the host on the GUI thread,
and persists the UI store in the plug-in state.

```rust
// src/lib.rs
pub mod engine;

#[cfg(feature = "plugin")]
mod plugin {
    use std::collections::BTreeMap;
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use include_dir::{Dir, include_dir};
    use nih_plug::prelude::*;
    use vst3_web_stratum::{Assets, AudioHandle, Vst3WebStratum};
    use vst3_web_stratum_nih::{EditorConfig, StoreSlot, Vst3WebStratumEditor};

    use crate::engine::{self, Engine, Ix};

    static UI: Dir = include_dir!("$CARGO_MANIFEST_DIR/web/dist");
    fn ui_lookup(path: &str) -> Option<&'static [u8]> { UI.get_file(path).map(|f| f.contents()) }

    pub struct HelloParams {
        pub gain: FloatParam,
        pub tone: FloatParam,
        pub bypass: BoolParam,
        pub ui_store: StoreSlot,
    }

    impl Default for HelloParams {
        fn default() -> Self {
            HelloParams {
                gain: FloatParam::new("Gain", 0.0, FloatRange::Linear { min: -24.0, max: 24.0 }).with_unit(" dB"),
                tone: FloatParam::new("Tone", 1000.0, FloatRange::Skewed { min: 20.0, max: 20_000.0, factor: FloatRange::skew_factor(-2.0) }).with_unit(" Hz"),
                bypass: BoolParam::new("Bypass", false),
                ui_store: StoreSlot::new(),
            }
        }
    }

    // By hand so the ids match the standalone and the page.
    unsafe impl Params for HelloParams {
        fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
            vec![
                ("gain".into(), self.gain.as_ptr(), "main".into()),
                ("tone".into(), self.tone.as_ptr(), "main".into()),
                ("bypass".into(), self.bypass.as_ptr(), "main".into()),
            ]
        }
        fn serialize_fields(&self) -> BTreeMap<String, String> {
            let mut m = BTreeMap::new();
            self.ui_store.serialize_into(&mut m);
            m
        }
        fn deserialize_fields(&self, m: &BTreeMap<String, String>) {
            self.ui_store.deserialize_from(m);
        }
    }

    pub struct HelloGain {
        params: Arc<HelloParams>,
        editor: Arc<Vst3WebStratumEditor>,
        bridge: Vst3WebStratum,
        audio: Option<AudioHandle>,
        ix: Ix,
        engine: Engine,
    }

    impl Default for HelloGain {
        fn default() -> Self {
            let params = Arc::new(HelloParams::default());
            let (editor, bridge) = Vst3WebStratumEditor::with_builder(
                "Hello Gain",
                params.as_ref(),
                engine::streams(48_000.0),
                EditorConfig::new(720, 420).assets(Assets::Lookup(ui_lookup)),
                |b| b.meta(serde_json::json!({ "vendor": "You", "sample_rate": 48_000.0 })),
            );
            let audio = bridge.take_audio();
            params.ui_store.attach(&bridge);
            let ix = engine::indices(&bridge);
            HelloGain { params, editor, bridge, audio, ix, engine: Engine::new(48_000.0) }
        }
    }

    impl Plugin for HelloGain {
        const NAME: &'static str = "Hello Gain";
        const VENDOR: &'static str = "You";
        const URL: &'static str = "";
        const EMAIL: &'static str = "";
        const VERSION: &'static str = env!("CARGO_PKG_VERSION");
        const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        }];
        const SAMPLE_ACCURATE_AUTOMATION: bool = false;
        type SysExMessage = ();
        type BackgroundTask = ();

        fn params(&self) -> Arc<dyn Params> { self.params.clone() }

        fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
            Some(Box::new(self.editor.handle()))
        }

        fn initialize(&mut self, _: &AudioIOLayout, cfg: &BufferConfig, _: &mut impl InitContext<Self>) -> bool {
            self.engine.set_sample_rate(cfg.sample_rate);
            self.bridge.send_json("sample_rate", serde_json::json!({ "sample_rate": cfg.sample_rate }));
            true
        }

        fn process(&mut self, buffer: &mut Buffer, _: &mut AuxiliaryBuffers, _: &mut impl ProcessContext<Self>) -> ProcessStatus {
            let Some(audio) = self.audio.as_mut() else { return ProcessStatus::Normal };
            let (a, b) = buffer.as_slice().split_at_mut(1);
            self.engine.process(&mut *a[0], &mut *b[0], audio, &self.ix);
            ProcessStatus::Normal
        }
    }

    impl Vst3Plugin for HelloGain {
        const VST3_CLASS_ID: [u8; 16] = *b"HelloGainVst3Web";
        const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
    }

    impl ClapPlugin for HelloGain {
        const CLAP_ID: &'static str = "dev.you.hello-gain";
        const CLAP_DESCRIPTION: Option<&'static str> = Some("Hello Gain");
        const CLAP_MANUAL_URL: Option<&'static str> = None;
        const CLAP_SUPPORT_URL: Option<&'static str> = None;
        const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
    }

    nih_export_vst3!(HelloGain);
    nih_export_clap!(HelloGain);
}
```

Notes on what the adapter does for you:

* `mirror_params` samples every nih-plug range at 65 points, so the page
  formats and scales `tone` exactly like the host does, without knowing
  nih-plug's skew formula.
* Values in `Engine::process` come from `audio.param(...)`, that is, from the
  vst3-web-stratum mirror, which the adapter keeps in sync with the host through the
  editor callbacks. If you prefer nih-plug's smoothed values, read
  `self.params.gain.smoothed` instead; both are correct, the mirror is the
  one the page sees.
* The server starts on the first editor open, on a port probed from the
  plug-in name, and writes a discovery record. Nothing to configure.
* The `StoreSlot` makes `client.store` survive with the session.

Build it after the page:

```sh
(cd web && npm run build)
cargo build --release --features plugin
# bundle with nih-plug's xtask, see the README section "Build the plug-ins"
```

## 6. What to do next

* **Publish state on change** and mark the stream `.sticky()` (a filter
  curve, a wavetable) so a late window gets it at once.
* **Events**: `AudioHandle::drain_events` / `send_event` carry notes and
  controllers in 12-byte records; `client.noteOn(note, velocity)` sends them.
  The synth example wires an on-screen keyboard this way.
* **Undo, redo, A/B**: `useVst3WebStratum().history` in Vue, `new History(client)`
  in vanilla JS.
* **Your own look**: `useKnobGesture` gives any SVG the drag, wheel and
  keyboard behaviour of a knob; `useNeedle` gives a meter face you draw the
  ballistics of a VU meter; `Segmented` and `Toggle` are unstyled controls;
  `Timeline` and `LinePlot` chart history and curves in your colours. The
  compressor lab example is built that way.
* **Presets**: `stateToJson()` and `loadState(values)` snapshot and restore
  every parameter in one frame; keep user presets in `client.store`.
* **Resize**: `useWindowSize` and the unstyled `ResizeGrip` from the Vue
  layer, or `client.send('resize', { width, height })` by hand; the adapter
  clamps and forwards it to the host, and a resize from the host's side (a
  frame drag) reaches the page the same way. `toggleFullscreen` asks for
  the whole monitor.
* **Several instances**, ports and discovery: [MULTI-INSTANCE.md](MULTI-INSTANCE.md).
* **Latency**: [PERFORMANCE.md](PERFORMANCE.md), and `tools/bench.mjs`.
* **The protocol**, if you want a client in another language:
  [WIRE.md](WIRE.md).
