/**
 * Noob-Q specifics on top of the generic `@elyerinfox/vst3-web-stratum/vue` bridge: band
 * helpers, global handles and the UI-only state. Everything generic
 * (client, reactive params, history, presets state) lives in the framework.
 *
 * Rules of use:
 * - `useVst3WebStratum()` may be called any time; it creates the client on first
 *   use and returns the connection state (`ready`, `connected`, `manifest`,
 *   `stats`, `status`, `history`, `historyState`).
 * - Everything that returns parameter handles (`useBand`, `allBands`,
 *   `useGlobals`, and the helpers built on them) needs the manifest, so
 *   call it only once `ready` is true. App.vue guards this by rendering
 *   its children under `v-if="ready"`.
 * - Handles are cached per id (in the framework) and per band here, so any
 *   number of components can ask for the same band and share one
 *   subscription.
 * - Multi-parameter changes go through `client.setMany`, which sends one
 *   frame for all of them; that keeps a "create band" or "paste" atomic on
 *   the wire and a single undo step.
 */
import { computed, reactive } from 'vue';
import {
  getClient,
  hasParam,
  loadState as loadStateGeneric,
  send,
  stateToJson as stateToJsonGeneric,
  useParam,
  useVst3WebStratum as useVst3WebStratumGeneric,
  useStream,
} from '@elyerinfox/vst3-web-stratum/vue';

export { getClient, hasParam, send, useParam, useStream };

/** Parameters that belong to the UI or the demo source, not the preset. */
const NOT_PRESET = (id) =>
  id.startsWith('src_') || id.startsWith('sc_') || id.startsWith('analyzer_') || id === 'display_range' || id === 'piano_display';

let wired = false;
/**
 * The generic connection state, plus Noob-Q's "preset modified" tracking:
 * any completed local edit marks the current preset as modified, and a new
 * manifest (reconnect) clears the flag. Safe to call before `ready`.
 * @returns {{ client, history, historyState, ready, connected, manifest, status, stats, modified }}
 */
export function useVst3WebStratum() {
  const s = useVst3WebStratumGeneric();
  if (!wired) {
    wired = true;
    s.client.on('edit', () => (ui.preset.modified = true));
    s.client.on('manifest', () => (ui.preset.modified = false));
  }
  return s;
}

/**
 * The preset-relevant state as `{ id: plain }`: every parameter except the
 * UI-only and demo-source ones (`NOT_PRESET`). This is what Save As and
 * Copy put in a preset.
 * @returns {Object.<string, number>}
 */
export function stateToJson() {
  return stateToJsonGeneric({ skip: NOT_PRESET });
}

/**
 * Load `{ id: plain }` in one frame. Parameters not listed are reset to
 * their defaults (pass `{ reset: false }` to leave them), UI-only and
 * demo-source parameters are never touched, and the preset is marked clean.
 * @param {Object.<string, number>} values
 * @param {{ reset?: boolean }} [opts]
 */
export function loadState(values, opts = {}) {
  loadStateGeneric(values, { skip: NOT_PRESET, ...opts });
  ui.preset.modified = false;
}

// ---------------------------------------------------------------------------
// Bands
// ---------------------------------------------------------------------------

/**
 * Handle name → parameter-id suffix for one band. Band `n`'s ids are
 * `b<n>_<suffix>` (`b3_freq`, `b3_dyn_range`, …), matching the Rust
 * `param_map` in `src/plugin.rs` and `build_bridge` in `src/dsp/mod.rs`.
 */
export const BAND_KEYS = {
  on: 'on',
  shape: 'shape',
  freq: 'freq',
  gain: 'gain',
  q: 'q',
  slope: 'slope',
  place: 'place',
  solo: 'solo',
  dynOn: 'dyn_on',
  dynRange: 'dyn_range',
  dynThr: 'dyn_thr',
  dynAuto: 'dyn_auto',
  dynAttack: 'dyn_attack',
  dynRelease: 'dyn_release',
  dynSc: 'dyn_sc',
};

/** Shape labels by `b<n>_shape` index (the Rust `Shape` enum order). */
export const SHAPES = ['Bell', 'Low Shelf', 'Low Cut', 'High Shelf', 'High Cut', 'Notch', 'Band Pass', 'Tilt Shelf', 'Flat Tilt', 'All Pass'];
/** The same shapes by the filter-type id the framework's EqCurve / `bandCoefs` use. */
export const SHAPE_IDS = ['peak', 'lowshelf', 'highpass', 'highshelf', 'lowpass', 'notch', 'bandpass', 'tiltshelf', 'flattilt', 'allpass'];
/** Shapes that have a gain control: Bell, Low Shelf, High Shelf, Tilt Shelf, Flat Tilt. */
export const GAIN_SHAPES = new Set([0, 1, 3, 7, 8]);
/** Shapes that can be dynamic (gain modulated by level): Bell, the two shelves, Flat Tilt. */
export const DYN_SHAPES = new Set([0, 1, 3, 8]);
/** Shapes with a slope control: shelves, cuts and Tilt Shelf. */
export const SLOPE_SHAPES = new Set([1, 2, 3, 4, 7]);
/** Low Cut and High Cut. */
export const CUT_SHAPES = new Set([2, 4]);
/** Placement labels by `b<n>_place` index, and the node colour for each (Pro-Q's yellow / white / red / green / blue). */
export const PLACEMENTS = ['Stereo', 'Left', 'Right', 'Mid', 'Side'];
export const PLACEMENT_COLORS = ['#ffd166', '#f1f5f9', '#ff5c5c', '#3ddc84', '#58a6ff'];

const bandCache = new Map();

/** All fifteen handles of band `n` (1-based), as one reactive object. */
export function useBand(n) {
  let b = bandCache.get(n);
  if (b) return b;
  b = reactive({ n });
  for (const [key, suffix] of Object.entries(BAND_KEYS)) b[key] = useParam(`b${n}_${suffix}`);
  b.color = computed(() => PLACEMENT_COLORS[b.place.index] || PLACEMENT_COLORS[0]);
  b.hasGain = computed(() => GAIN_SHAPES.has(b.shape.index));
  b.canDyn = computed(() => DYN_SHAPES.has(b.shape.index));
  b.hasSlope = computed(() => SLOPE_SHAPES.has(b.shape.index));
  b.isCut = computed(() => CUT_SHAPES.has(b.shape.index));
  b.isDynamic = computed(() => b.dynOn.on && Math.abs(b.dynRange.plain) > 0.01);
  bandCache.set(n, b);
  return b;
}

/** Number of bands the plug-in declared in its manifest meta (24 for Noob-Q). */
export function bandCount() {
  return useVst3WebStratumGeneric().manifest.value?.meta?.bands || 24;
}

/** Every band's reactive object, 1..bandCount(), as a plain array (index 0 = band 1). */
export function allBands() {
  return Array.from({ length: bandCount() }, (_, i) => useBand(i + 1));
}

/** Lowest-numbered disabled band, or null when all 24 are in use. */
export function firstFreeBand() {
  for (let n = 1; n <= bandCount(); n++) if (!useBand(n).on.on) return n;
  return null;
}

/** Configure band `n` in one frame. `v` holds plain values / indices. */
export function setBand(n, v) {
  const b = useBand(n);
  const edits = [];
  const put = (h, val) => edits.push([h.param, val]);
  if (v.shape != null) put(b.shape, b.shape.toNorm(v.shape));
  if (v.freq != null) put(b.freq, b.freq.toNorm(v.freq));
  if (v.gain != null) put(b.gain, b.gain.toNorm(v.gain));
  if (v.q != null) put(b.q, b.q.toNorm(v.q));
  if (v.slope != null) put(b.slope, b.slope.toNorm(v.slope));
  if (v.place != null) put(b.place, b.place.toNorm(v.place));
  if (v.dynOn != null) put(b.dynOn, v.dynOn ? 1 : 0);
  if (v.dynRange != null) put(b.dynRange, b.dynRange.toNorm(v.dynRange));
  if (v.dynThr != null) put(b.dynThr, b.dynThr.toNorm(v.dynThr));
  if (v.dynAuto != null) put(b.dynAuto, v.dynAuto ? 1 : 0);
  if (v.dynAttack != null) put(b.dynAttack, b.dynAttack.toNorm(v.dynAttack));
  if (v.dynRelease != null) put(b.dynRelease, b.dynRelease.toNorm(v.dynRelease));
  if (v.dynSc != null) put(b.dynSc, v.dynSc ? 1 : 0);
  if (v.solo != null) put(b.solo, v.solo ? 1 : 0);
  if (v.on != null) put(b.on, v.on ? 1 : 0);
  getClient().setMany(edits);
  return n;
}

/** Create a band in the first free slot. Returns its number or null. */
export function createBand(v) {
  const n = firstFreeBand();
  if (n == null) return null;
  setBand(n, {
    shape: 0,
    gain: 0,
    q: 1,
    slope: 1,
    place: 0,
    dynOn: false,
    dynRange: 0,
    dynAuto: true,
    dynSc: false,
    solo: false,
    ...v,
    on: true,
  });
  return n;
}

/**
 * "Delete" a band: there is no slot list to remove from, so the band is
 * disabled (and un-soloed) and its slot becomes free for `createBand`.
 * Its other settings stay in the plug-in state until the slot is reused.
 */
export function deleteBand(n) {
  const b = useBand(n);
  getClient().setMany([
    [b.on.param, 0],
    [b.solo.param, 0],
  ]);
}

/** Plain values of a band as a plain object (for copy / presets). */
export function bandToJson(n) {
  const b = useBand(n);
  return {
    on: b.on.on,
    shape: b.shape.index,
    freq: b.freq.plain,
    gain: b.gain.plain,
    q: b.q.plain,
    slope: b.slope.index,
    place: b.place.index,
    dynOn: b.dynOn.on,
    dynRange: b.dynRange.plain,
    dynThr: b.dynThr.plain,
    dynAuto: b.dynAuto.on,
    dynAttack: b.dynAttack.plain,
    dynRelease: b.dynRelease.plain,
    dynSc: b.dynSc.on,
  };
}

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

let globals = null;
/**
 * Every global parameter as a reactive handle under a short name. Entries
 * are `null` when the server does not have that parameter (the demo
 * source and the side-chain analyzer only exist in some builds), so
 * templates guard with `g.anSc?.on`. Built once; needs the manifest.
 *
 *   bypass, outputGain, gainScale, autoGain, outputPan, panMode,
 *   phaseInvert, mode (processing_mode), quality (lp_quality), character,
 *   gainQ, anPre / anPost / anSc / anRes / anRange / anSpeed / anTilt /
 *   anFreeze (analyzer_*), displayRange, piano (piano_display),
 *   srcKind / srcFreq / srcLevel / scKind / scLevel (standalone only)
 */
export function useGlobals() {
  if (globals) return globals;
  const g = (id) => (hasParam(id) ? useParam(id) : null);
  globals = reactive({
    bypass: g('bypass'),
    outputGain: g('output_gain'),
    gainScale: g('gain_scale'),
    autoGain: g('auto_gain'),
    outputPan: g('output_pan'),
    panMode: g('pan_mode'),
    phaseInvert: g('phase_invert'),
    mode: g('processing_mode'),
    quality: g('lp_quality'),
    character: g('character'),
    gainQ: g('gain_q'),
    anPre: g('analyzer_pre'),
    anPost: g('analyzer_post'),
    anSc: g('analyzer_sc'),
    anRes: g('analyzer_resolution'),
    anRange: g('analyzer_range'),
    anSpeed: g('analyzer_speed'),
    anTilt: g('analyzer_tilt'),
    anFreeze: g('analyzer_freeze'),
    displayRange: g('display_range'),
    piano: g('piano_display'),
    srcKind: g('src_kind'),
    srcFreq: g('src_freq'),
    srcLevel: g('src_level'),
    scKind: g('sc_kind'),
    scLevel: g('sc_level'),
  });
  return globals;
}

// ---------------------------------------------------------------------------
// UI-only state
// ---------------------------------------------------------------------------

/**
 * UI-only state shared by the components. None of it is sent to the
 * plug-in (view settings that should persist are parameters:
 * `display_range`, `piano_display`, `analyzer_*`).
 */
export const ui = reactive({
  /** Selected band numbers (1-based) and the primary one (gets the panel and the value pop-up). */
  selected: [],
  primary: null,
  /** Band under the pointer, and the cursor frequency for the scale's hover line. */
  hover: null,
  hoverFreq: null,
  /** Visible frequency range of the display and the scale, in Hz. */
  zoom: { min: 10, max: 30000 },
  /** Help-menu options (manual §1.1) and the analyzer's Spectrum Grab switch. */
  showParamDisplay: true,
  autoRange: true,
  showFreqHover: true,
  spectrumGrab: true,
  /** Output meter column visible; EQ Sketch armed from the pencil button. */
  meterVisible: true,
  sketchArmed: false,
  /** Spectrum Grab state: `permanent` = entered with G / click-and-hold, stays until the background is clicked. */
  grab: { active: false, permanent: false },
  /** Which popover is open ('presets' | 'analyzer' | 'output' | 'eqmatch' | null) and whether it is pinned. */
  panel: null,
  panelSticky: false,
  /** Current preset: `index` is its position in factory + user order, -1 when unknown. */
  preset: { name: 'Default Setting', modified: false, index: -1 },
  /** Copied bands (`bandToJson` objects) for the band menu's Paste. */
  clipboard: null,
  fullscreen: false,
  /** Name of the size menu entry in use ('Mini' … 'Extra Large'). */
  size: 'Medium',
  /** Latest `band_dyn` (dynamic gain in dB per band) and `band_level` (trigger level in dB per band) frames. */
  dynGains: new Float32Array(24),
  dynLevels: new Float32Array(24),
});

/** Set the selection; the primary band defaults to the last one listed. */
export function selectBands(list, primary = null) {
  ui.selected = [...list];
  ui.primary = primary ?? (list.length ? list[list.length - 1] : null);
}
