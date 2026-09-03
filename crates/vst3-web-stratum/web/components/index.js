/**
 * Barrel export for the built-in vst3-web-stratum components: dependency-free
 * controls and visualisers that bind straight to the objects the client
 * hands out (`client.param(id)` → `Param`, `client.stream(id)` → `Stream`).
 *
 * Two ways to import them:
 *
 *   // with a bundler (Vite, …) and the `@elyerinfox/vst3-web-stratum` package linked in
 *   import { Knob, Meter, Spectrum, EqCurve, Scope } from '@elyerinfox/vst3-web-stratum/components';
 *
 *   // straight from the plugin's own server, no build step
 *   import { Knob, Meter } from '/vst3-web-stratum/components/index.js';
 *
 * What every component has in common:
 *
 * * **Constructor `(container, source, opts)`** — the component appends its
 *   own root element (an `<svg>`, a `<canvas>` or a `<div>`) to `container`
 *   and sizes itself to it (`width: 100%; height: 100%`), so the container
 *   decides the size. A `ResizeObserver` follows later size changes.
 * * **Live binding** — a component that takes a `Param` subscribes to it and
 *   redraws when the plugin (host automation, another window) changes the
 *   value; one that takes a `Stream` redraws on every frame. Drawing is
 *   coalesced into one `requestAnimationFrame` per change.
 * * **Gestures, not values** — a control that edits a `Param` wraps every
 *   pointer / wheel / key interaction in `beginEdit()` … `set()` …
 *   `endEdit()`, so the host records automation correctly. Wheel gestures
 *   end after a short idle time (150–180 ms). Shift makes every gesture fine.
 * * **Styling** — visual style comes from a few CSS custom properties
 *   (`--vst3-web-stratum-accent`, `--vst3-web-stratum-text`, `--vst3-web-stratum-text-dim`,
 *   `--vst3-web-stratum-grid`, `--vst3-web-stratum-grid-strong`, `--vst3-web-stratum-track`,
 *   `--vst3-web-stratum-knob-body`, `--vst3-web-stratum-curve`, `--vst3-web-stratum-key-*`) plus the
 *   colour options each constructor accepts. Component CSS is injected once
 *   per page with `injectStyle`.
 * * **`destroy()`** — unsubscribes, stops the animation loop and removes the
 *   root element. Call it when the component leaves the page; nothing is
 *   garbage-collected while a `Param` / `Stream` still references it.
 *
 * Besides the classes, `eqcurve.js` exports the filter-design helpers
 * (`biquad`, `onePole`, `magnitudeDb`, `bandCoefs`, `bandDb`, `butterworthQ`,
 * `effectiveQ`, `normalizeType`) and the constants (`FilterTypes`,
 * `SLOPE_NAMES`, `SLOPE_ORDERS`, `PLACEMENT_COLORS`, `GAIN_TYPES`,
 * `CUT_TYPES`, `SLOPE_TYPES`, `DYN_TYPES`) that pages use to draw or fit
 * responses without instantiating an `EqCurve`. See `README.md` in this
 * folder for the full reference.
 */
export { Knob } from './knob.js';
export { Meter } from './meter.js';
export { Spectrum } from './spectrum.js';
export {
  EqCurve,
  biquad,
  onePole,
  magnitudeDb,
  bandCoefs,
  bandDb,
  butterworthQ,
  effectiveQ,
  normalizeType,
  FilterTypes,
  SLOPE_NAMES,
  SLOPE_ORDERS,
  PLACEMENT_COLORS,
  GAIN_TYPES,
  CUT_TYPES,
  SLOPE_TYPES,
  DYN_TYPES,
} from './eqcurve.js';
export { Scope } from './scope.js';
export { Keyboard } from './keyboard.js';
export { WavetableView } from './wavetable.js';
export { Envelope } from './envelope.js';
export { NeedleModel } from './needle.js';
export { Timeline } from './timeline.js';
export { LinePlot } from './lineplot.js';
