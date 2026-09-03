/**
 * Factory presets: `{ id: plain }` maps. Anything not listed loads at its
 * default, so an empty `values` object is the "Default Setting" canvas.
 * User presets, favourites and EQ Match reference spectra live in the
 * plug-in's UI store (`client.store`): they persist with the plug-in state
 * and every window of the instance sees them.
 */
import { getClient } from '@elyerinfox/vst3-web-stratum/vue';

/**
 * @typedef {Object} Preset
 * @property {string} name          Shown in the top bar and the browser; unique within its folder.
 * @property {string} [author]      Shown in the details column.
 * @property {string[]} [tags]      Searchable keywords.
 * @property {string} [description] One line for the details column.
 * @property {Object.<string, number>} values
 *   Parameter id → plain value (Hz, dB, enum index, 0/1 for toggles).
 *   Only the parameters a preset changes need to be listed; `loadState`
 *   resets every other parameter to its default, and skips the UI-only and
 *   demo-source parameters entirely.
 */

/**
 * Build the `values` entries for one enabled band.
 * @param {number} n       band number, 1-based
 * @param {number} shape   shape index (see the table below)
 * @param {number} freq    Hz
 * @param {number} [gain]  dB
 * @param {number} [q]     quality factor
 * @param {Object} [extra] any other `b<n>_<key>` value: `slope`, `place`, `dyn_on`, `dyn_range`, …
 */
const band = (n, shape, freq, gain = 0, q = 1, extra = {}) => {
  const o = { [`b${n}_on`]: 1, [`b${n}_shape`]: shape, [`b${n}_freq`]: freq, [`b${n}_gain`]: gain, [`b${n}_q`]: q };
  for (const [k, v] of Object.entries(extra)) o[`b${n}_${k}`] = v;
  return o;
};
/** Combine several `band()` results (and loose `{ id: value }` extras) into one `values` object. */
const merge = (...parts) => Object.assign({}, ...parts);
// Enum indices used below (they match the Rust `Shape` / `SlopeParam` /
// `PlacementParam` enums and the manifest's `labels`):
// shape indices: 0 Bell, 1 Low Shelf, 2 Low Cut, 3 High Shelf, 4 High Cut, 5 Notch, 6 Band Pass, 7 Tilt Shelf, 8 Flat Tilt, 9 All Pass
// slope indices: 0 6dB, 1 12dB, 2 18dB, 3 24dB, 4 30dB, 5 36dB, 6 48dB, 7 72dB, 8 96dB, 9 Brickwall
// placement indices: 0 Stereo, 1 Left, 2 Right, 3 Mid, 4 Side
// other globals used here: processing_mode 0 Zero Latency / 1 Natural / 2 Linear Phase,
// lp_quality 0 Low … 4 Maximum, character 0 Clean / 1 Subtle / 2 Warm, gain_q 0/1

/**
 * Factory presets in browser order. The first entry is the empty canvas
 * the plug-in starts on. Loading one resets every parameter it does not
 * mention, so "Clean" really is just a low cut.
 * @type {Preset[]}
 */
export const FACTORY_PRESETS = [
  { name: 'Default Setting', author: 'Ely Erin Fox', tags: ['default', 'clean', 'start'], description: 'An empty canvas.', values: {} },
  { name: 'Clean', author: 'Ely Erin Fox', tags: ['clean'], description: 'Gentle low cut only.', values: merge(band(1, 2, 30, 0, 0.707, { slope: 3 })) },
  {
    name: 'Drums',
    author: 'Ely Erin Fox',
    tags: ['drums', 'mix'],
    description: 'Tight lows, a scoop in the mud, snap on top.',
    values: merge(band(1, 2, 35, 0, 0.707, { slope: 3 }), band(2, 0, 70, 2.5, 1.4), band(3, 0, 320, -3.5, 1.2), band(4, 0, 4200, 2.5, 1.6), band(5, 3, 9000, 2, 0.7)),
  },
  {
    name: 'Vocal Presence',
    author: 'Ely Erin Fox',
    tags: ['vocal', 'mix'],
    description: 'Clears mud, adds presence with a dynamic dip on harsh peaks.',
    values: merge(
      band(1, 2, 90, 0, 0.707, { slope: 3 }),
      band(2, 0, 260, -2.5, 1.3),
      band(3, 0, 3000, 2.5, 1.0),
      band(4, 0, 6500, 0, 2.5, { dyn_on: 1, dyn_range: -5 }),
      band(5, 3, 11000, 1.5, 0.7),
    ),
  },
  {
    name: 'De-mud',
    author: 'Ely Erin Fox',
    tags: ['mix', 'dynamic'],
    description: 'Dynamic cut around 250 Hz that only acts when the mud builds up.',
    values: merge(band(1, 0, 250, 0, 1.1, { dyn_on: 1, dyn_range: -6 })),
  },
  { name: 'Air', author: 'Ely Erin Fox', tags: ['master', 'shelf'], description: 'A touch of top with a broad high shelf.', values: merge(band(1, 3, 12000, 2, 0.5)) },
  {
    name: 'Mastering Tilt',
    author: 'Ely Erin Fox',
    tags: ['master', 'tilt'],
    description: 'Flat tilt around 650 Hz plus mid/side width on top.',
    values: merge(band(1, 8, 650, 1.5, 1), band(2, 3, 8000, 1, 0.7, { place: 4 }), { gain_q: 0 }),
  },
  { name: 'Low Cut', author: 'Ely Erin Fox', tags: ['cut'], description: '24 dB/oct low cut at 80 Hz.', values: merge(band(1, 2, 80, 0, 0.707, { slope: 3 })) },
  { name: 'High Cut', author: 'Ely Erin Fox', tags: ['cut'], description: '24 dB/oct high cut at 12 kHz.', values: merge(band(1, 4, 12000, 0, 0.707, { slope: 3 })) },
  { name: 'High Cut Brickwall', author: 'Ely Erin Fox', tags: ['cut', 'linear'], description: 'Brickwall high cut at 16 kHz in linear phase.', values: merge(band(1, 4, 16000, 0, 0.707, { slope: 9 }), { processing_mode: 2, lp_quality: 1 }) },
  { name: 'Low Shelf', author: 'Ely Erin Fox', tags: ['shelf'], description: '+3 dB below 120 Hz.', values: merge(band(1, 1, 120, 3, 0.7)) },
  { name: 'High Shelf', author: 'Ely Erin Fox', tags: ['shelf'], description: '+3 dB above 6 kHz.', values: merge(band(1, 3, 6000, 3, 0.7)) },
  { name: 'High Shelf Brickwall', author: 'Ely Erin Fox', tags: ['shelf', 'steep'], description: 'Very steep high shelf (96 dB/oct cascade).', values: merge(band(1, 3, 6000, 4, 0.7, { slope: 8 })) },
  { name: 'Low Boost', author: 'Ely Erin Fox', tags: ['boost'], description: 'Bell boost at 80 Hz.', values: merge(band(1, 0, 80, 4, 1.2)) },
  { name: 'High Boost', author: 'Ely Erin Fox', tags: ['boost'], description: 'Bell boost at 8 kHz.', values: merge(band(1, 0, 8000, 4, 1.0)) },
  {
    name: 'Flat 5 Bands',
    author: 'Ely Erin Fox',
    tags: ['flat', 'template'],
    description: 'Five bells at 0 dB, ready to grab.',
    values: merge(band(1, 0, 100), band(2, 0, 400), band(3, 0, 1200), band(4, 0, 3500), band(5, 0, 10000)),
  },
  {
    name: 'Flat 7 Bands',
    author: 'Ely Erin Fox',
    tags: ['flat', 'template'],
    description: 'Seven bells at 0 dB.',
    values: merge(band(1, 0, 60), band(2, 0, 150), band(3, 0, 400), band(4, 0, 1000), band(5, 0, 2500), band(6, 0, 6000), band(7, 0, 14000)),
  },
  { name: 'Notch Hum', author: 'Ely Erin Fox', tags: ['notch', 'repair'], description: 'Narrow notches at 50 Hz and its first harmonic.', values: merge(band(1, 5, 50, 0, 30), band(2, 5, 100, 0, 30)) },
  {
    name: 'Telephone',
    author: 'Ely Erin Fox',
    tags: ['fx'],
    description: 'Band-limited to 300 Hz – 3.4 kHz with steep cuts.',
    values: merge(band(1, 2, 300, 0, 0.707, { slope: 6 }), band(2, 4, 3400, 0, 0.707, { slope: 6 }), band(3, 0, 1500, 3, 1)),
  },
  {
    name: 'Side Sparkle',
    author: 'Ely Erin Fox',
    tags: ['master', 'stereo'],
    description: 'Highs on the sides only, lows kept in the middle.',
    values: merge(band(1, 3, 9000, 2.5, 0.6, { place: 4 }), band(2, 2, 120, 0, 0.707, { slope: 3, place: 4 })),
  },
  {
    name: 'Warm Character',
    author: 'Ely Erin Fox',
    tags: ['character'],
    description: 'Warm saturation with a gentle tilt.',
    values: merge(band(1, 7, 800, -2, 0.7), { character: 2 }),
  },
];

export const USER_KEY = 'presets.user';
export const FAV_KEY = 'presets.favorites';
export const REF_KEY = 'eqmatch.references';

/** Guard against a store value that is not the array we expect (an older or foreign page wrote it). */
const list = (v) => (Array.isArray(v) ? v : []);

/** User presets in the store; empty until hydration and when nothing was saved. @returns {Preset[]} */
export function loadUserPresets() {
  return list(getClient().store.get(USER_KEY, []));
}
/** Replace the user presets; persisted by the plug-in and pushed to its other windows. @param {Preset[]} presets */
export function saveUserPresets(presets) {
  getClient().store.set(USER_KEY, presets);
}
/** Favourite preset names. @returns {Set<string>} */
export function loadFavorites() {
  return new Set(list(getClient().store.get(FAV_KEY, [])));
}
/** @param {Set<string>} set */
export function saveFavorites(set) {
  getClient().store.set(FAV_KEY, [...set]);
}
/** Saved EQ Match reference spectra, `{ name, data: number[128] }` on the panel's log grid. */
export function loadReferences() {
  return list(getClient().store.get(REF_KEY, []));
}
/** @param {{ name: string, data: number[] }[]} refs */
export function saveReferences(refs) {
  getClient().store.set(REF_KEY, refs);
}
/**
 * Re-run `fn(key)` whenever one of these keys changes: another window saved
 * a preset, or the host restored the plug-in's state (`key === null`).
 * Returns an unsubscribe function.
 */
export function onPresetStoreChange(fn) {
  return getClient().store.on('*', (k) => {
    if (k == null || k === USER_KEY || k === FAV_KEY || k === REF_KEY) fn(k);
  });
}
