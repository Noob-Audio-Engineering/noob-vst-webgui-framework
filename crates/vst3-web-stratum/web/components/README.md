# `@elyerinfox/vst3-web-stratum/components`

Dependency-free controls and visualisers for a vst3-web-stratum-bridged plug-in UI.
Each one binds directly to the objects the client hands out — a `Param`
from `client.param(id)` or a `Stream` from `client.stream(id)` — and needs no
framework. The Vue layer (`@elyerinfox/vst3-web-stratum/vue`) wraps a few of them; the
noob-q and noob-wave examples use most of them, either directly or through
their own Vue components.

```js
// with a bundler and @elyerinfox/vst3-web-stratum linked
import { Knob, Meter, Spectrum, EqCurve, Scope, Keyboard, WavetableView, Envelope } from '@elyerinfox/vst3-web-stratum/components';

// or straight from the plug-in's server, no build step
import { Knob, Meter } from '/vst3-web-stratum/components/index.js';
```

Contents: [Conventions](#conventions) · [Knob](#knob) · [Meter](#meter) ·
[Spectrum](#spectrum) · [EqCurve](#eqcurve) · [Filter helpers](#filter-design-helpers) ·
[Scope](#scope) · [Keyboard](#keyboard) · [WavetableView](#wavetableview) ·
[Envelope](#envelope) · [CSS variables](#css-variables)

## Conventions

* **Constructor `(container, source, opts)`.** The component appends its own
  root element (`<svg>`, `<canvas>` or `<div>`) to `container` and fills it
  (`width: 100%; height: 100%`). Size the container; a `ResizeObserver`
  follows later changes. Canvas components render at `devicePixelRatio` and
  draw in CSS pixels.
* **Live binding.** A component that takes a `Param` subscribes to it and
  redraws when the plug-in changes the value (host automation, another
  window, undo). One that takes a `Stream` redraws on every frame. Redraws
  are coalesced to one `requestAnimationFrame` per change.
* **Gestures, not values.** Every pointer, wheel or keyboard edit is wrapped
  in `beginEdit()` … `set()` … `endEdit()` on the Param, so the host records
  automation as one gesture. Wheel gestures stay open for 150–180 ms after
  the last notch. Shift makes every gesture fine.
* **Param-or-value fields.** Where a component takes a value that may or may
  not be live (`EqCurve` bands, `WavetableView.position`, the `Envelope`
  stages), it accepts a `Param`, a number, or a getter function; only Params
  are editable.
* **`destroy()`** unsubscribes, stops the animation loop and removes the root
  element. Call it when the component leaves the page.
* **Styling** comes from CSS custom properties (see the [table](#css-variables))
  plus per-instance colour options. Each component injects its stylesheet
  once per page.

Stream frames are `Float32Array` views into the received message and are
never reused, so a component may keep a reference to the last one. A page
that hides a component should also throttle or stop its stream with
`stream.subscribe({ maxHz })` / `subscribe({ enabled: false })`; nothing in
the component does that for you.

---

## Knob

`knob.js` — SVG rotary control for one Param.

```js
const knob = new Knob(el, client.param('cutoff'), { size: 64 });
```

**Constructor** `new Knob(container, param, opts?)`

| option        | default            | meaning |
|---------------|--------------------|---------|
| `size`        | `72`               | Diameter in px |
| `label`       | `param.name`       | Text under the knob (`''` for none) |
| `showValue`   | `true`             | Show `param.format()` under the arc |
| `bipolar`     | `param.isBipolar`  | Draw the value arc from 12 o'clock instead of the track start |
| `sensitivity` | `200`              | Pixels of vertical drag for the full range |
| `sweep`       | `270`              | Track extent in degrees, centred on 12 o'clock |
| `color`       | —                  | Accent colour for this knob (sets `--vst3-web-stratum-accent` on the root) |
| `format`      | —                  | `(plain) => string`, replaces `param.format()` |

**Gestures:** vertical drag (Shift ×0.1); wheel ±0.02 (Shift ±0.002; one step
for discrete Params); double-click resets to the default; arrow keys ±0.01
(Shift ±0.1; one step when discrete), Home / End, Backspace / Delete reset.
Discrete Params snap and send only when the step changes.

**Methods:** `destroy()`.
**Fields:** `el` (focusable root, `role="slider"` with ARIA values), `param`, `opts`.
**Style:** `--vst3-web-stratum-accent`, `--vst3-web-stratum-text`, `--vst3-web-stratum-track`, `--vst3-web-stratum-knob-body`; classes `.vst3-web-stratum-knob .track/.value/.ind/.body/.val/.lbl`.

---

## Meter

`meter.js` — canvas peak / RMS level meter for one Stream.

```js
const meter = new Meter(el, client.stream('meter_out'), { minDb: -48 });
clipButton.onclick = () => meter.resetClip();
```

**Stream frame:** `[peak_0 … peak_{ch−1}, rms_0 … rms_{ch−1}?]`, linear
amplitude (1.0 = 0 dBFS), one block peak per channel. RMS is optional and
detected from the frame length (`≥ 2·channels`).

**Constructor** `new Meter(container, stream, opts?)`

| option          | default                  | meaning |
|-----------------|--------------------------|---------|
| `channels`      | `stream.channels`        | Bars to draw |
| `minDb`, `maxDb`| `-60`, `6`               | Scale (linear in dB) |
| `orientation`   | `'vertical'`             | or `'horizontal'` |
| `decayDbPerSec` | `24`                     | Fall rate after a peak (attack is instant) |
| `holdMs`        | `1200`                   | Peak-hold time, then it falls at twice the decay rate |
| `gap`           | `3`                      | px between channel bars |
| `colors`        | green, yellow, red, red  | `[low, mid, hot, clip]`; zones split at −12 dB and 0 dB |
| `background`    | `rgba(255,255,255,.06)`  | Fill behind each bar |

**Methods:** `resetClip()` clears the latched clip markers; `destroy()`.
**Fields:** `canvas`, `stream`, `opts`.
The animation loop runs continuously because ballistics move without frames.

---

## Spectrum

`spectrum.js` — canvas spectrum analyser for one Stream of per-bin magnitudes.

```js
const an = new Spectrum(el, client.stream('spectrum_post'), { minDb: -90, maxDb: 0, slopeDbPerOct: 4.5, dbScale: 'right' });
an.setReleaseMs(300);
freeze.onclick = () => an.setFrozen(!an.frozen);
```

**Stream frame:** `fftSize / 2 + 1` values, bin `k` at `k · sampleRate / fftSize`
Hz, dB unless the stream meta has `db: false`. Defaults are read from the
stream meta: `sample_rate`, `fft_size` (else inferred as `(bins − 1) · 2`), `db`.

**Constructor** `new Spectrum(container, stream, opts?)`

| option           | default                   | meaning |
|------------------|---------------------------|---------|
| `sampleRate`     | meta / `48000`            | Hz |
| `fftSize`        | meta / inferred           | For the bin → Hz mapping |
| `isDb`           | meta `db !== false`       | Input already in dB |
| `minHz`, `maxHz` | `20`, `sampleRate / 2`    | Log frequency axis |
| `minDb`, `maxDb` | `-90`, `0`                | Linear dB axis |
| `releaseMs`      | `120`                     | Per-bin smoothing release (the "speed") |
| `attackMs`       | `0`                       | Per-bin smoothing attack (0 = instant) |
| `slopeDbPerOct`  | `0`                       | Display tilt, pivoting at 1 kHz |
| `dbScale`        | `'none'`                  | `'left'` / `'right'` draws a labelled dB column |
| `grid`, `fill`   | `true`, `true`            | |
| `color`, `fillColor`, `gridColor`, `textColor`, `lineWidth` | sky blue … | |

**How it draws:** one column per CSS pixel; where a column covers several bins
it takes the maximum (narrow peaks never vanish), where a bin spans several
columns it interpolates, and below bin 1 it holds bin 1 so the trace reaches
the left edge. Smoothing is a one-pole in dB per bin stepped with the
plug-in's frame timestamps, so the decay looks the same at any frame rate.

**Methods**

| method                         | effect |
|--------------------------------|--------|
| `setRange(minHz, maxHz)`       | Zoom (clamped: ≥ 1 Hz, `max ≥ 1.5·min`) |
| `setDbRange(minDb, maxDb)`     | Vertical range |
| `setReleaseMs(ms)`             | Smoothing speed |
| `setTilt(dbPerOct)`            | Display tilt |
| `setFrozen(on)` / `frozen`     | Freeze: no decay, peak-hold of everything since |
| `xForFreq(f)`, `freqForX(x)`, `yForDb(db)`, `dbForY(y)` | Plot ↔ pixel mapping, for overlays and pointer read-outs |
| `valueAt(freq)`                | Displayed level at a frequency (dB, or `NaN`) |
| `peaks({ minDistanceOct, minDb, max })` | Local maxima as `{ freq, db, x, y }`, loudest first, thinned by spacing — for "spectrum grab" |
| `destroy()`                    | |

---

## EqCurve

`eqcurve.js` — SVG parametric-EQ display with draggable band nodes, previews,
selection, dynamic-range bars and the full Pro-Q style gesture set.

```js
const eq = new EqCurve(el, {
  sampleRate: 48000,
  rangeDb: 12,
  bands: [1, 2, 3].map((n) => ({
    type: client.param(`b${n}_shape`),   // labelled enum Param: labels are matched by name
    freq: client.param(`b${n}_freq`),
    gain: client.param(`b${n}_gain`),
    q: client.param(`b${n}_q`),
    slope: client.param(`b${n}_slope`),
    enabled: client.param(`b${n}_on`),
    dynOn: client.param(`b${n}_dyn_on`),
    dynRange: client.param(`b${n}_dyn_range`),
  })),
  gainQ: client.param('gain_q'),
  dynGain: (i) => dynGains[i],           // from the plug-in's band-gain stream
  onCreateBand: ({ type, freq, db }) => addBand(type, freq, db),
  onSelect: (sel, primary) => showPanel(primary),
});
client.stream('band_dyn').on((d) => { dynGains = d; eq.update(); });
```

**Band descriptor** (`opts.bands[i]`; every field may be a value or a Param)

| field       | default     | accepts |
|-------------|-------------|---------|
| `type`      | `'peak'`    | a `FilterTypes` id or any alias (`'Low Cut'`, `'bell'`, `'LPF'`), an index into `FilterTypes`, or a Param (labels matched by name, else `plain` as index) |
| `freq`      | `1000`      | Hz |
| `gain`      | `0`         | dB (ignored for gain-less types) |
| `q`         | `1`         | |
| `slope`     | `1` (12 dB) | index into `SLOPE_NAMES` / `SLOPE_ORDERS`, a name prefix, or a Param |
| `placement` | `'stereo'`  | `'stereo' \| 'left' \| 'right' \| 'mid' \| 'side'` or a Param; picks the colour |
| `enabled`   | `true`      | boolean or Param (Alt-click toggles a Param) |
| `dynOn`     | `false`     | dynamic EQ on |
| `dynRange`  | `0`         | dB; drawn as a bar from the static gain, draggable, Alt-wheel |
| `solo`      | `false`     | resolved for the page; dimming is the page's call (`setDimmed`) |
| `color`     | —           | explicit colour instead of the placement colour |

**Options**

| option       | default | meaning |
|--------------|---------|---------|
| `sampleRate` | `48000` | Responses are computed at this rate; match the plug-in |
| `minHz`, `maxHz` | `10`, `30000` | Log axis |
| `rangeDb`    | `12`    | Vertical range ±dB (grid step follows) |
| `points`     | `256`   | Frequencies per curve |
| `gainQ`      | `false` | Gain-Q interaction: boolean or Param |
| `dynGain`    | —       | `(i) => dB`, current dynamic gain per band |
| `grid`, `showBands` | `true` | |
| `nodeRadius` | `8`     | px |

**Callbacks:** `onSelect(indices, primary)`, `onHover(i | null)`,
`onCreateBand({ type, freq, db, alt, shift, x, y, fromCurve? }) → index | null`,
`onBandContextMenu(i, event)`, `onBandDblClick(i)`, `onCycleShape(i)`,
`onCycleSlope(i)`, `onPointer({ freq, db, x, y } | null)`.

**Gestures** (Ctrl = Cmd on macOS)

| input                          | effect |
|--------------------------------|--------|
| drag node                      | frequency + gain (Q instead of gain for cuts / notch / band-pass); all selected bands move together |
| Ctrl+drag                      | Q only |
| Alt+drag                       | constrain to the first axis moved |
| Shift                          | fine (×0.15 drag, ×0.25 wheel) |
| wheel                          | Q (slope steps for cuts) |
| Ctrl+wheel / Alt+wheel         | gain / dynamic range; Ctrl+Alt+wheel trades one for the other |
| Alt+click / Ctrl+Alt+click / Alt+Shift+click | toggle band / `onCycleShape` / `onCycleSlope` |
| click, Ctrl+click, Shift+click | select / toggle in selection / range select |
| click empty space              | deselect; with nothing selected (or with Ctrl) create a band |
| double-click empty space       | create a band |
| drag the composite curve       | create a band there and drag its gain |
| drag the dynamic-range bar     | edit `dynRange` |
| right-click node               | `onBandContextMenu` |
| double-click node              | `onBandDblClick` |

**Methods**

| method | effect |
|--------|--------|
| `bandValues(i)` | Resolved `{ type, freq, gain, q, slope, placement, enabled, dynOn, dynRange, solo }` |
| `curveDb(freq)` | Composite static response in dB |
| `select(i, mode = 'set' \| 'toggle' \| 'range')`, `select(null)` | Selection |
| `setDimmed(on)` | Dim everything (spectrum grab, solo) |
| `setRange(minHz, maxHz)`, `setRangeDb(rangeDb)` | Zoom / vertical range |
| `xForFreq`, `freqForX`, `yForDb`, `dbForY` | Plot ↔ pixel mapping |
| `showPreview(type, freq, gainDb, q?, slope?)`, `hidePreview()` | Dashed preview curve |
| `shapeForPosition(x, y)` | The shape a new band would get there |
| `update()` | Redraw on the next frame (call when `dynGain` inputs change) |
| `destroy()` | |

**Fields:** `el`, `svg`, `opts`, `bands`, `selected` (`Set`), `primary`, `hovered`, `gainQ` (getter).
**Style:** `--vst3-web-stratum-grid`, `--vst3-web-stratum-grid-strong`, `--vst3-web-stratum-text-dim`, `--vst3-web-stratum-curve`; band colours from `PLACEMENT_COLORS` or `band.color`.

### Filter design helpers

Exported from `eqcurve.js` (and the barrel) so pages can compute responses
without an `EqCurve` — the EQ-match fitter in noob-q does. They reproduce
`examples/noob-q/src/dsp/filters.rs` exactly and must be changed together
with it.

| export | what |
|--------|------|
| `FilterTypes` | `['peak','lowshelf','highpass','highshelf','lowpass','notch','bandpass','tiltshelf','flattilt','allpass']` — index = the plug-in's shape enum value |
| `SLOPE_NAMES`, `SLOPE_ORDERS` | `'6 dB' … '96 dB', 'Brickwall'` ↔ orders `1 … 16, 32` |
| `PLACEMENT_COLORS` | colours for stereo / left / right / mid / side |
| `GAIN_TYPES`, `CUT_TYPES`, `SLOPE_TYPES`, `DYN_TYPES` | `Set`s classifying the types |
| `normalizeType(name)` | `'Low Cut'`, `'lpf'`, `'bell'` … → id (`'peak'` if unknown) |
| `biquad(type, freq, gainDb, q, sr)` | RBJ cookbook coefficients, normalized (`a0 = 1`): `{ b0, b1, b2, a1, a2 }` |
| `onePole(type, freq, sr)` | 6 dB/oct section by bilinear transform, same shape |
| `magnitudeDb(coefs, freq, sr)` | `10·log10(|N|²/|D|²)` of `H(e^{jw})` |
| `butterworthQ(order, k)` | `1 / (2·sin((2k−1)π / 2N))` |
| `effectiveQ(type, q, gainDb, gainQ)` | bell Q grows with `|gain|/30` when gain-Q is on |
| `bandCoefs(type, freq, gainDb, q, slope, sr, { gainQ })` | the full cascade for a band (see the JSDoc for the per-type rules) |
| `bandDb(coefs, freq, sr)` | sum of the sections' `magnitudeDb` |

```js
import { bandCoefs, bandDb } from '@elyerinfox/vst3-web-stratum/components';
const coefs = bandCoefs('peak', 1000, 6, 1.4, 1, 48000);
const at2k = bandDb(coefs, 2000, 48000); // dB
```

---

## Scope

`scope.js` — canvas oscilloscope for one Stream of interleaved time-domain samples.

```js
const scope = new Scope(el, client.stream('scope'), { fill: true, gain: 1.5 });
```

**Stream frame:** `frames × channels` samples in −1..1, interleaved
(`[l0, r0, l1, r1, …]`). Latest frame wins.

| option      | default | meaning |
|-------------|---------|---------|
| `channels`  | `stream.channels` | Interleave factor |
| `gain`      | `1`     | Vertical scale |
| `colors`    | sky, orange, green, yellow | Per channel, cycled |
| `lineWidth` | `1.5`   | |
| `fill`      | `false` | Fill to the centre line (20 % alpha) |
| `grid`, `gridColor` | `true`, `rgba(255,255,255,.08)` | Centre and ±50 % lines |

With more than two samples per pixel the trace is a min / max envelope per
column; otherwise a polyline through every sample.
**Methods:** `destroy()`. **Fields:** `canvas`, `stream`, `opts`.

---

## Keyboard

`keyboard.js` — on-screen piano keyboard that sends note events and lights the
keys the plug-in reports.

```js
const kbd = new Keyboard(el, client, { low: 36, high: 96 });
octaveDown.onclick = () => (kbd.octave -= 1);
```

**Wire:** presses call `client.noteOn(note, velocity, channel)`, releases
`client.noteOff(note, 0, channel)` (binary event frames). With `remote` on,
`client.on('event')` note-ons light keys with `.remote`, note-offs clear them.

| option     | default | meaning |
|------------|---------|---------|
| `low`, `high` | `48`, `84` | MIDI range shown |
| `velocity` | `0.8`   | For QWERTY notes and glides; pointer presses take theirs from the hit height (0.2 … 1) |
| `channel`  | `0`     | |
| `qwerty`   | `true`  | `a w s e d f t g y h u j k o l p ; '` play, `z` / `x` shift octaves; ignored while a text field has focus |
| `labels`   | `true`  | `C3`, `C4`, … on the C keys |
| `remote`   | `true`  | Light keys from plug-in events |
| `onNote`   | —       | `(note, on, velocity)` after every local press / release |

**Methods:** `setRange(low, high)`, `octave` (get / set, −4..4), `static noteName(n)` (`60 → "C4"`), `destroy()`.
**Style:** `--vst3-web-stratum-key-white`, `--vst3-web-stratum-key-black`, `--vst3-web-stratum-key-border`, `--vst3-web-stratum-accent` (held), `--vst3-web-stratum-key-remote`.

---

## WavetableView

`wavetable.js` — pseudo-3D stack of a wavetable's frames with the playing frame in front.

```js
const view = new WavetableView(el, { position: client.param('wt_position') });
client.stream('wavetable').on((d, s) => view.setTable(d, s.meta.frames));
client.stream('modulation').on((d) => view.setLivePosition(d[0]));
```

**Table:** `frames` consecutive single cycles of `data.length / frames`
samples in −1..1. Publish it on a *sticky* stream so late windows get it.

| option      | default | meaning |
|-------------|---------|---------|
| `position`  | `0`     | Morph position 0..1: Param (editable), number or getter |
| `depthX`, `depthY` | `0.45`, `0.55` | Offset of the back frame as fractions of width / height |
| `maxFrames` | `32`    | Frames drawn in the stack (subsampled evenly) |
| `color`, `stackColor`, `nearColor` | sky, grey, sky-ish | Front frame, stack, frame nearest the position |
| `draggable` | `true`  | Vertical drag (80 % of the height = full range, Shift ×0.1) and wheel (±0.03, Shift ±0.005) edit a position Param |

**Methods:** `setTable(data, frames)`, `setLivePosition(p | null)` (the modulated
position drives the front frame when set), `position` (getter), `destroy()`.

---

## Envelope

`envelope.js` — ADSR editor with three draggable handles.

```js
const p = (id) => client.param(id);
new Envelope(el, { attack: p('amp_attack'), decay: p('amp_decay'), sustain: p('amp_sustain'), release: p('amp_release') });
```

**Inputs:** `attack`, `decay`, `release` in **seconds** and `sustain` as a
**level 0..1** — each a Param, a number or a getter (wrap a 0..100 % Param in
a getter / adapter; the component does not rescale). Only Params are editable.

| option         | default | meaning |
|----------------|---------|---------|
| `maxTime`      | `4`     | Seconds each time stage may occupy on screen |
| `sustainWidth` | `0.22`  | Fraction of the width for the sustain plateau |
| `color`        | sky     | Line, fill, handles |
| `onHover`, `stageIndicator` | — | Reserved, not yet used |

Times use a square-root scale (`px = sqrt(t / maxTime) · stageWidth`) so 1 ms
stays visible next to 4 s. Handles: **attack** (horizontal), **decay**
(horizontal = decay, vertical = sustain), **release** (horizontal); Shift ×0.2.
**Methods:** `destroy()`. **Fields:** `el`, `svg`, `opts`.
**Style:** `--vst3-web-stratum-grid`, `--vst3-web-stratum-text-dim`.

---

## CSS variables

| variable                 | used by                         | default |
|--------------------------|---------------------------------|---------|
| `--vst3-web-stratum-accent`       | Knob arc / focus, Keyboard held keys | `#5ac8fa` |
| `--vst3-web-stratum-text`         | Knob indicator and value        | `#e6e6e6` |
| `--vst3-web-stratum-text-dim`     | EqCurve / Envelope labels       | `rgba(255,255,255,.4)` |
| `--vst3-web-stratum-track`        | Knob track                      | `rgba(255,255,255,.14)` |
| `--vst3-web-stratum-knob-body`    | Knob disc                       | `rgba(255,255,255,.06)` |
| `--vst3-web-stratum-grid`         | EqCurve / Envelope grid         | `rgba(255,255,255,.07)` |
| `--vst3-web-stratum-grid-strong`  | EqCurve 0 dB line               | `rgba(255,255,255,.2)` |
| `--vst3-web-stratum-curve`        | EqCurve composite curve         | `#ffd166` |
| `--vst3-web-stratum-key-white`, `--vst3-web-stratum-key-black`, `--vst3-web-stratum-key-border`, `--vst3-web-stratum-key-remote` | Keyboard | slate / dark / navy / `#ffd166` |

Canvas components (Meter, Spectrum, Scope, WavetableView) take their colours
from constructor options instead, since a canvas cannot read CSS variables
without extra work.
