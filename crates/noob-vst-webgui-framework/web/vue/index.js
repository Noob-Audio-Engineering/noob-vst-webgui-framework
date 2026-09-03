/**
 * `@noob-audio-engineering/noob-vst-webgui-framework/vue` — the optional Vue 3 layer: a reactive bridge plus a few
 * generic components. Requires `vue` in the consuming app.
 *
 *   import { useNoobVstWebguiFramework, useParam, Knob, Popover } from '@noob-audio-engineering/noob-vst-webgui-framework/vue';
 *
 * What is exported, and where it is documented:
 *
 * * **Bridge** (`./useNoobVstWebguiFramework.js`): `configureClient`, `getClient`,
 *   `useNoobVstWebguiFramework`, `useParam`, `hasParam`, `useStream`, `hasStream`, `send`,
 *   `useStore`, `useStoredRef`, `stateToJson`, `loadState`.
 * * **Value helpers** (`./values.js`): `freqToNote`, `midiToFreq`,
 *   `noteName`, `noteToFreq`, `noteLabel`, `parseValue`.
 * * **Components** (`./components/`): `Knob` (rotary control bound to a
 *   handle), `Popover` (anchored panel), `ContextMenu` (positioned menu),
 *   `LevelMeter` (Vue wrapper over the canvas meter). They are unstyled
 *   beyond a few `--noob-vst-webgui-framework-*` CSS variables so they fit any theme.
 *
 * The canvas components (`Spectrum`, `EqCurve`, `Scope`, `Keyboard`, ...)
 * are framework-agnostic and live in `@noob-audio-engineering/noob-vst-webgui-framework/components`; use them
 * from `onMounted` with a template ref, as `LevelMeter.vue` does.
 *
 * Every consumer of this module must resolve the same copy of `vue` as the
 * app (Vite: `resolve.dedupe: ['vue']` and `preserveSymlinks: true` when
 * the package is linked); see `crates/noob-vst-webgui-framework/web/README.md`.
 */
export {
  configureClient,
  getClient,
  useNoobVstWebguiFramework,
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
} from './useNoobVstWebguiFramework.js';
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
