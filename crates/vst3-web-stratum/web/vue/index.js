/**
 * `@elyerinfox/vst3-web-stratum/vue` — the optional Vue 3 layer: a reactive bridge plus a few
 * generic components. Requires `vue` in the consuming app.
 *
 *   import { useVst3WebStratum, useParam, Knob, Popover } from '@elyerinfox/vst3-web-stratum/vue';
 *
 * What is exported, and where it is documented:
 *
 * * **Bridge** (`./useVst3WebStratum.js`): `configureClient`, `getClient`,
 *   `useVst3WebStratum`, `useParam`, `hasParam`, `useStream`, `hasStream`, `send`,
 *   `useStore`, `useStoredRef`, `stateToJson`, `loadState`.
 * * **Value helpers** (`./values.js`): `freqToNote`, `midiToFreq`,
 *   `noteName`, `noteToFreq`, `noteLabel`, `parseValue`.
 * * **Components** (`./components/`): `Knob` (rotary control bound to a
 *   handle), `Popover` (anchored panel), `ContextMenu` (positioned menu),
 *   `LevelMeter` (Vue wrapper over the canvas meter). They are unstyled
 *   beyond a few `--vst3-web-stratum-*` CSS variables so they fit any theme.
 *
 * The canvas components (`Spectrum`, `EqCurve`, `Scope`, `Keyboard`, ...)
 * are framework-agnostic and live in `@elyerinfox/vst3-web-stratum/components`; use them
 * from `onMounted` with a template ref, as `LevelMeter.vue` does.
 *
 * Every consumer of this module must resolve the same copy of `vue` as the
 * app (Vite: `resolve.dedupe: ['vue']` and `preserveSymlinks: true` when
 * the package is linked); see `crates/vst3-web-stratum/web/README.md`.
 */
export {
  configureClient,
  getClient,
  useVst3WebStratum,
  useParam,
  hasParam,
  useStream,
  hasStream,
  send,
  stateToJson,
  loadState,
  useStore,
  useStoredRef,
  useStreamValue,
  useStreamFrame,
  useNeedle,
  useKnobGesture,
  useWindowSize,
} from './useVst3WebStratum.js';
export { freqToNote, midiToFreq, noteName, noteToFreq, noteLabel, parseValue } from './values.js';
export { default as Knob } from './components/Knob.vue';
export { default as Popover } from './components/Popover.vue';
export { default as ContextMenu } from './components/ContextMenu.vue';
export { default as LevelMeter } from './components/LevelMeter.vue';
export { default as Timeline } from './components/Timeline.vue';
export { default as LinePlot } from './components/LinePlot.vue';
export { default as Segmented } from './components/Segmented.vue';
export { default as Toggle } from './components/Toggle.vue';
export { default as ResizeGrip } from './components/ResizeGrip.vue';
