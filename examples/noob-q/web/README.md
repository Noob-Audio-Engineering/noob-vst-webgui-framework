# Noob-Q UI

The user interface of **Noob-Q**, the Pro-Q style equalizer example of
[vst3-web-stratum](../../../README.md). It is a Vue 3 +
Tailwind v4 single-page app built with Vite. Inside a DAW it renders in the
plug-in's own window, which is the operating system's web view; during
development it renders in any browser. Either way it talks to the Rust DSP
over one loopback WebSocket using the `@elyerinfox/vst3-web-stratum` client library.

Everything specific to an equalizer lives here and in the Rust crate one
level up (`examples/noob-q`, see its README). Everything generic, the client,
the reactive parameter handles, undo / redo, the knob, the spectrum and EQ
curve renderers, lives in the library at the repo root (`web/`, see
[crates/vst3-web-stratum/web/README.md](../../../crates/vst3-web-stratum/web/README.md)).

## Development workflow

Requirements: Node 20 or newer and the Rust toolchain for the standalone.

```sh
cd examples/noob-q/web
npm install          # also links @elyerinfox/vst3-web-stratum from ../../../crates/vst3-web-stratum/web (a file: dependency)
npm run build        # writes dist/, which the Rust side serves or embeds
```

Run the page against real DSP without a DAW:

```sh
# from the repo root: serves web/dist on port 4242 (or the next free port)
cargo run -p noob-q --bin noob-q-standalone -- --open
```

Hot reload while editing the UI:

```sh
# terminal 1: the standalone (keep it running; note the port it printed)
cargo run -p noob-q --bin noob-q-standalone
# terminal 2: Vite on http://localhost:5173, proxying to the standalone
cd examples/noob-q/web && VST3_WEB_STRATUM_PORT=4242 npm run dev
```

The dev server proxies `/ws` (the WebSocket) and `/instance` + `/instances`
(the discovery endpoints used by the instance menu) to `VST3_WEB_STRATUM_PORT`. The
standalone prefers 4242 and walks up if that port is taken, so use the port
from its start-up banner.

The plug-in build embeds `dist/` into the binary (`include_dir!`), so run
`npm run build` before `cargo build --features plugin`; the root README has
the full plug-in build steps.

## How the page talks to the plug-in

1. On load, `useVst3WebStratum()` creates one `Vst3WebStratumClient` that connects to
   `ws://<page origin>/ws`. The server answers with a manifest describing
   every parameter (id, name, unit, range, taper table, enum labels) and
   every telemetry stream.
2. Components ask for reactive **parameter handles** (`useParam(id)`,
   wrapped here by `useBand(n)` and `useGlobals()`). Reading `handle.plain`
   or `handle.text` is reactive; `handle.set()` / `setPlain()` /
   `setIndex()` / `toggle()` send edits, and `begin()` / `end()` bracket a
   drag so the host records one automation gesture.
3. **Streams** (`useStream(id)`) deliver telemetry as `Float32Array`s at up
   to audio-block rate: spectra, meters, the DSP's own response curve, the
   per-band dynamic gain and trigger level. A component subscribes only to
   what it shows.
4. The **UI store** (`client.store`) holds page state that should travel
   with the plug-in: user presets, favourites, EQ Match references. The
   plug-in persists it with its state and every window of the instance
   sees the same values.
5. Ad-hoc JSON **messages**: the page sends `reset` (all parameters to
   default) and `resize` (the size menu); the host sends `status` once a
   second (latency, block size, client count) and `sample_rate`.

Parameter ids: globals such as `bypass`, `output_gain`, `processing_mode`,
`analyzer_*`, `display_range`, `piano_display`, and per band
`b<n>_on`, `b<n>_shape`, `b<n>_freq`, `b<n>_gain`, `b<n>_q`, `b<n>_slope`,
`b<n>_place`, `b<n>_solo`, `b<n>_dyn_on`, `b<n>_dyn_range`, `b<n>_dyn_thr`,
`b<n>_dyn_auto`, `b<n>_dyn_attack`, `b<n>_dyn_release`, `b<n>_dyn_sc` for
`n` = 1…24. The standalone adds the demo source (`src_*`, `sc_*`); the page
uses the presence of `src_kind` to know it is not inside a plug-in.

Streams: `spectrum_pre`, `spectrum_post`, `spectrum_sc`, `meter_in`,
`meter_out`, `curve` (sticky), `band_dyn`, `band_level`.

## Component tree

```
App.vue                     layout, display buttons, global keyboard shortcuts
├── TopBar.vue              undo / redo / A-B, preset navigation, latency read-outs, help, full screen
│   └── PresetBrowser.vue   folders, search, favourites, details, Save As, Copy / Paste
├── Analyzer.vue            the display: spectra, EQ curve with band nodes, grab, sketch, rectangle select
│   └── ParamDisplay.vue    value pop-up under the selected node (drag / wheel / type)
├── BandPanel.vue           floating controls for the selected band(s), incl. the dynamics panel
├── FreqScale.vue           frequency axis: zoom / scroll, piano mode with band dots
├── BottomBar.vue           mode, resolution, instance menu, analyzer / character / bypass / output, size
│   ├── AnalyzerPanel.vue   analyzer settings popover
│   ├── OutputPanel.vue     output options popover
│   └── EqMatchPanel.vue    EQ Match: record, reference, fit bands
└── (from @elyerinfox/vst3-web-stratum/vue) Knob, Popover, ContextMenu, LevelMeter
```

Every component starts with a doc block that lists its props, emits, the
parameters and streams it touches, the store keys it uses and its gestures.

## The composable layer (`src/composables/useVst3WebStratum.js`)

| Export | Purpose |
|---|---|
| `useVst3WebStratum()` | Connection state (`ready`, `connected`, `manifest`, `stats`, `status`), `history`, plus Noob-Q's preset-modified tracking. Safe before `ready`. |
| `useBand(n)` / `allBands()` | The fifteen handles of one band (or of all 24) as one reactive object, with derived `color`, `hasGain`, `canDyn`, `hasSlope`, `isCut`, `isDynamic`. Cached. |
| `useGlobals()` | Every global handle by a short name (`bypass`, `outputGain`, `mode`, `anPre`, `displayRange`, …); `null` for parameters the server does not have. |
| `createBand(v)` / `setBand(n, v)` / `deleteBand(n)` | Create in the first free slot, configure, or disable a band, each as **one frame** (`client.setMany`) so it is atomic on the wire and one undo step. |
| `bandToJson(n)` | A band's plain values, used by copy / paste and split. |
| `stateToJson()` / `loadState(values)` | Whole-state save and load that skip the UI-only and demo-source parameters (`NOT_PRESET`). |
| `selectBands(list, primary)` | Set the selection (1-based band numbers). |
| `ui` | Reactive UI-only state, below. |
| `SHAPES`, `SHAPE_IDS`, `PLACEMENTS`, `PLACEMENT_COLORS`, `GAIN_SHAPES`, `DYN_SHAPES`, `SLOPE_SHAPES`, `CUT_SHAPES`, `BAND_KEYS` | Enum tables matching the Rust side. |

Handles need the manifest: call anything but `useVst3WebStratum()` only once
`ready` is true (App.vue renders its children under `v-if="ready"`).

### `ui` fields

| Field | Meaning |
|---|---|
| `selected`, `primary`, `hover`, `hoverFreq` | Selection (1-based band numbers), hovered band, cursor frequency for the scale |
| `zoom` | `{ min, max }` Hz shown by the display and the scale |
| `showParamDisplay`, `autoRange`, `showFreqHover`, `spectrumGrab` | Help-menu and analyzer options |
| `meterVisible`, `sketchArmed`, `grab`, `panel`, `panelSticky`, `fullscreen`, `size` | Chrome state: meter column, EQ Sketch armed, Spectrum Grab `{ active, permanent }`, which popover is open and whether it is pinned |
| `preset` | `{ name, modified, index }` of the current preset |
| `clipboard` | Copied bands (`bandToJson` objects) |
| `dynGains`, `dynLevels` | Latest `band_dyn` / `band_level` frames, shared by the display and the band panel |

## Presets and the store

`src/presets.js` holds the factory presets (`{ name, author, tags,
description, values }`, where `values` maps parameter id → plain value and
anything unlisted loads at its default) and the store-backed helpers.

| Store key | Content |
|---|---|
| `presets.user` | `Preset[]` saved with Save As |
| `presets.favorites` | `string[]` of favourite preset names |
| `eqmatch.references` | `{ name, data: number[128] }[]` reference spectra for EQ Match |

`onPresetStoreChange(fn)` fires when any of these change from elsewhere
(another window, or the host restoring the plug-in state). Nothing uses
`localStorage`: the browser profile inside a host is not a reliable place,
and it would not follow the plug-in between sessions.

## Styling

Tailwind v4 in CSS-first mode (`src/style.css`): `@theme` declares the
`ink-*` surface palette and the `accent` blue; `@source` adds the shared
Vue components of `@elyerinfox/vst3-web-stratum` to Tailwind's scan; the `--vst3-web-stratum-*`
custom properties colour the framework's canvas components. A scoped
`<style>` that uses `@apply` starts with `@reference '../style.css'`.
Numbers that change live use the `.tabular` class so text does not jitter.

## Adding things

**A new control for an existing parameter**: get its handle (`useGlobals()`
or `useBand(n)`), then either drop a `<Knob :p="handle" />` in a template or
wire a button to `handle.toggle()` / `handle.setIndex(i)`. Read
`handle.text` for the formatted value. Bracket custom drags with
`handle.begin()` / `handle.end()`.

**A new parameter**: add it on the Rust side (the `Params` struct and
`param_map` in `src/plugin.rs`, and `build_bridge` in `src/dsp/mod.rs` so
the standalone has it too), rebuild, and it appears in the manifest. Then
add it to `useGlobals()` (or `BAND_KEYS` for a per-band one), and to
`NOT_PRESET` if it is UI-only.

**A new panel**: create a component, give it the doc block the others
have, mount it from `App.vue` or a `Popover` anchored to a bottom-bar
button, and reuse the `.chip` / `.btn` recipes from a neighbouring
component's scoped style.

**A new telemetry stream**: declare it in `src/dsp/mod.rs`, publish it from
the audio thread, then `useStream(id).on(frame => …)` in the component and
unsubscribe in `onBeforeUnmount`. Mark it `sticky` on the Rust side if it is
state-like and only published on change.

## Keyboard and mouse

Undo / redo Ctrl+Z / Ctrl+Y, A/B Ctrl+B, Delete removes the selected bands,
Escape deselects / closes / exits full screen, arrow keys nudge (Shift =
fine), G toggles Spectrum Grab. In the display: double-click or drag the
yellow curve to add a band (Alt for a dynamic one), drag a node, wheel = Q,
Ctrl+wheel = gain, Alt+wheel = dynamic range, Alt+click = bypass,
Ctrl+Alt+click = cycle shape, Alt+Shift+click = cycle slope, double-click a
node to type values, Shift+drag = rectangle selection, right-click = band
or background menu, draw left-to-right = EQ Sketch, hover the spectrum =
Spectrum Grab. The scale zooms on vertical drag and scrolls on horizontal
drag.

## See also

- [`../README.md`](../README.md): the Rust crate (DSP, plug-in, standalone)
- [`../../../crates/vst3-web-stratum/web/README.md`](../../../crates/vst3-web-stratum/web/README.md): the `@elyerinfox/vst3-web-stratum`
  client library and Vue layer this UI is built on
- [`../../../docs/`](../../../docs/): the wire format, ports, the UI store
  and the rest of the framework documentation
