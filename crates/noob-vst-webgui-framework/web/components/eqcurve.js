/**
 * EqCurve — SVG parametric-EQ display with draggable band nodes.
 *
 * Each band is `{ type, freq, gain, q, slope, placement, enabled, dynOn,
 * dynRange, solo }` where every field may be a value *or* a noob-vst-webgui-framework Param.
 * Params are live: the curve follows host automation, and dragging a node
 * edits the Params with proper begin/end gestures.
 *
 * Filter math mirrors the noob-q engine (`examples/noob-q/src/dsp/filters.rs`):
 * RBJ cookbook biquads, first-order sections for odd slopes, Butterworth
 * cascades for steep cuts, cascaded shelves for steep shelves.
 *
 * Gestures (Windows keys; Cmd on macOS):
 *   drag node            frequency + gain (Q for gain-less shapes)
 *   Ctrl+drag vertical   Q
 *   Alt+drag             constrain to one axis
 *   Shift                fine
 *   wheel                Q (slope steps for cuts)
 *   Ctrl+wheel           gain        Alt+wheel  dynamic range
 *   Alt+click            toggle band Ctrl+Alt+click  cycle shape
 *   Alt+Shift+click      cycle slope
 *   double-click node    onBandDblClick (value entry)
 *   right-click node     onBandContextMenu
 *   click/Ctrl-click     select / add to selection; Shift-click  range
 *   drag the yellow curve or double-click empty space  onCreateBand
 *
 * ## Data model
 *
 * `opts.bands` is an array of band descriptors (see `EqBand` below). Every
 * field may be a plain value or a noob-vst-webgui-framework `Param`; `bandValues(i)` resolves
 * a band to numbers and canonical strings each time it is needed, so there
 * is no cached state to keep in sync. Enum-like fields (`type`, `slope`,
 * `placement`) accept a string, a number (index) or a Param, and a Param's
 * `labels` are matched by name so the page never has to know the plugin's
 * enum order — a Param labelled "Low Cut" resolves to `'highpass'`.
 *
 * Because the curve is computed from the Params, it always shows what the
 * plugin actually has: host automation, another window, undo, all update
 * it through the Param subscriptions.
 *
 * ## Coordinates
 *
 * `x = (ln f − ln minHz) / (ln maxHz − ln minHz) · width` (log frequency,
 * 10 Hz … 30 kHz by default) and `y = height − (dB + rangeDb) / (2·rangeDb)
 * · height` (linear, symmetric ±`rangeDb`). `xForFreq` / `freqForX` /
 * `yForDb` / `dbForY` are public so pages can overlay a spectrum, a piano
 * roll or a pointer read-out in the same space. The SVG `viewBox` equals
 * the element size in CSS pixels, so user units are pixels.
 *
 * ## Rendering
 *
 * Responses are sampled at `points + 1` log-spaced frequencies cached per
 * size (`_freqs`, `_xs`). Per band: a coloured line and a fill to 0 dB
 * (`showBands`), the node group (hit circle, node, number label) at
 * `(freq, gain)` (or `(freq, 0)` for gain-less shapes), and the dynamic-
 * range indicator (a bar from the static gain to `gain + dynRange` with a
 * dot at the current dynamic gain from `opts.dynGain(i)`). Then the
 * composite curve (sum of enabled bands), its fill, an invisible 14 px wide
 * hit path over it, and the dashed preview. A disabled band that is not
 * selected is hidden entirely. Rendering is coalesced to one
 * `requestAnimationFrame` per change (`_schedule`); `update()` requests one
 * explicitly (e.g. when the dynamic-gain stream ticks).
 *
 * ## Selection
 *
 * `selected` (a `Set` of indices) and `primary` (anchor for Shift-range
 * selection) are public. Dragging a node drags every enabled selected band
 * together; wheel over a selected band applies to the whole selection.
 * `onSelect(indices, primary)` reports changes; `select(null)` clears.
 *
 * ## Styling
 *
 * CSS variables: `--noob-vst-webgui-framework-grid`, `--noob-vst-webgui-framework-grid-strong` (0 dB line),
 * `--noob-vst-webgui-framework-text-dim` (scale labels), `--noob-vst-webgui-framework-curve` (composite curve
 * and fill). Band colours come from `band.color`, else
 * `PLACEMENT_COLORS[placement]`. Classes: root `.noob-vst-webgui-framework-eq`, `.grid`
 * (`.minor`), `.zero`, `.band` / `.band-fill` (`.off`, `.hot`), `.curve`,
 * `.curve-fill`, `.curve-hit`, `.node` (`.off`, `.selected`), `.label`,
 * `.hit`, `.dyn-line`, `.dyn-dot`, `.dyn-hit`, `.preview`.
 *
 * ## Lockstep with the Rust engine
 *
 * `biquad`, `onePole`, `magnitudeDb`, `butterworthQ`, `effectiveQ` and the
 * cascade rules in `bandCoefs` reproduce `Coefs::rbj`, `Coefs::one_pole_*`,
 * `Coefs::magnitude_db`, `butterworth_q`, the gain-Q rule and `design_band`
 * in `examples/noob-q/src/dsp/filters.rs`, and `FilterTypes` / `SLOPE_ORDERS`
 * mirror `Kind` and `SLOPE_ORDERS` there. The plugin also publishes its own
 * computed curve on a sticky stream; the browser-side math exists so
 * dragging feels instantaneous and so previews / EQ-match fitting can run
 * without a round trip. Any change to a formula, a slope table or a type
 * index must be made on both sides or the two curves will drift apart.
 *
 * @typedef {object} EqBand
 * @property {string|number|object} [type='peak'] Filter type: a `FilterTypes` id or alias (`'Low Cut'`, `'bell'`, …), an index into `FilterTypes`, or a Param (labels matched by name, else `plain` as index).
 * @property {number|object} [freq=1000] Centre / corner frequency in Hz (Param or number).
 * @property {number|object} [gain=0] Gain in dB (Param or number); ignored for gain-less types.
 * @property {number|object} [q=1] Quality factor (Param or number).
 * @property {number|string|object} [slope=1] Index into `SLOPE_NAMES` / `SLOPE_ORDERS`, a name prefix (`'24'`), or a Param.
 * @property {string|object} [placement='stereo'] `'stereo'|'left'|'right'|'mid'|'side'` or a Param; picks the colour.
 * @property {boolean|object} [enabled=true] Band active (Param or boolean). A Param here is toggled by Alt-click.
 * @property {boolean|object} [dynOn=false] Dynamic EQ active for this band.
 * @property {number|object} [dynRange=0] Dynamic range in dB, drawn as a bar from the static gain; Alt-wheel and dragging the bar edit it.
 * @property {boolean|object} [solo=false] Solo flag (resolved by `bandValues`; the display leaves solo dimming to the page via `setDimmed`).
 * @property {string} [color] Explicit colour, overriding the placement colour.
 */
import { injectStyle, plainOf } from '../noob-vst-webgui-framework.js';

const SVG = 'http://www.w3.org/2000/svg';
/** Frequencies that get a vertical grid line (1-2-3… per decade, 10 Hz to 30 kHz). */
const GRID_HZ = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 20000, 30000];
/** The subset of `GRID_HZ` drawn with a stronger line (the rest are `.minor`). */
const LABEL_HZ = new Set([10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]);

const CSS = `
.noob-vst-webgui-framework-eq{position:relative;width:100%;height:100%;user-select:none;-webkit-user-select:none;touch-action:none;
  font:10px system-ui,sans-serif}
.noob-vst-webgui-framework-eq svg{display:block;width:100%;height:100%;overflow:visible}
.noob-vst-webgui-framework-eq .grid{stroke:var(--noob-vst-webgui-framework-grid,rgba(255,255,255,.07));stroke-width:1}
.noob-vst-webgui-framework-eq .grid.minor{stroke:var(--noob-vst-webgui-framework-grid,rgba(255,255,255,.035))}
.noob-vst-webgui-framework-eq .zero{stroke:var(--noob-vst-webgui-framework-grid-strong,rgba(255,255,255,.2));stroke-width:1}
.noob-vst-webgui-framework-eq text{fill:var(--noob-vst-webgui-framework-text-dim,rgba(255,255,255,.4))}
.noob-vst-webgui-framework-eq .band{fill:none;stroke-width:1.2;opacity:.55}
.noob-vst-webgui-framework-eq .band.off{opacity:.15;stroke-dasharray:3 3}
.noob-vst-webgui-framework-eq .band-fill{opacity:.09;transition:opacity .12s}
.noob-vst-webgui-framework-eq .band-fill.hot{opacity:.26}
.noob-vst-webgui-framework-eq .band-fill.off{opacity:.03}
.noob-vst-webgui-framework-eq .curve{fill:none;stroke:var(--noob-vst-webgui-framework-curve,#ffd166);stroke-width:2.2;stroke-linejoin:round}
.noob-vst-webgui-framework-eq .curve-fill{fill:var(--noob-vst-webgui-framework-curve,#ffd166);opacity:.09}
.noob-vst-webgui-framework-eq .curve-hit{fill:none;stroke:transparent;stroke-width:14;cursor:ns-resize}
.noob-vst-webgui-framework-eq .node{cursor:move;stroke:#fff;stroke-width:1.5;transition:r .1s}
.noob-vst-webgui-framework-eq .node.off{fill-opacity:.12}
.noob-vst-webgui-framework-eq .node.selected{stroke-width:2.5}
.noob-vst-webgui-framework-eq g.band-g:hover .node{filter:brightness(1.2)}
.noob-vst-webgui-framework-eq .label{pointer-events:none;font-weight:600;fill:#fff;font-size:10px}
.noob-vst-webgui-framework-eq .hit{fill:transparent;cursor:move}
.noob-vst-webgui-framework-eq .dyn-line{stroke-width:2;stroke-linecap:round;opacity:.7;cursor:ns-resize}
.noob-vst-webgui-framework-eq .dyn-dot{stroke:#fff;stroke-width:1;pointer-events:none}
.noob-vst-webgui-framework-eq .dyn-hit{stroke:transparent;stroke-width:12;cursor:ns-resize}
.noob-vst-webgui-framework-eq .preview{fill:none;stroke:rgba(255,255,255,.35);stroke-width:1.5;stroke-dasharray:4 4;pointer-events:none}
`;

/**
 * Canonical filter type ids, in the plugin's enum order (`Kind` in
 * `filters.rs`, the `Shape` parameter in noob-q): the index of a name here
 * is the plain value of a shape Param. Use `normalizeType` to map user-
 * facing names and aliases onto these ids.
 *
 * | id          | shape            | uses gain | uses slope |
 * |-------------|------------------|-----------|------------|
 * | `peak`      | bell             | yes       | no         |
 * | `lowshelf`  | low shelf        | yes       | yes        |
 * | `highpass`  | low cut          | no        | yes        |
 * | `highshelf` | high shelf       | yes       | yes        |
 * | `lowpass`   | high cut         | no        | yes        |
 * | `notch`     | notch            | no        | no         |
 * | `bandpass`  | band pass        | no        | no         |
 * | `tiltshelf` | tilt shelf       | yes       | yes        |
 * | `flattilt`  | flat tilt        | yes       | no         |
 * | `allpass`   | all pass         | no        | no         |
 * @type {readonly string[]}
 */
export const FilterTypes = ['peak', 'lowshelf', 'highpass', 'highshelf', 'lowpass', 'notch', 'bandpass', 'tiltshelf', 'flattilt', 'allpass'];

/**
 * Slope names as the plugin declares them (the labels of a slope Param), and
 * the filter order for each: `6 dB/oct` per order, so `'24 dB'` is order 4
 * and `'Brickwall'` is order 32 (192 dB/oct). Both arrays mirror
 * `SLOPE_NAMES` / `SLOPE_ORDERS` in `filters.rs`; a slope value is an index
 * into them.
 * @type {readonly string[]}
 */
export const SLOPE_NAMES = ['6 dB', '12 dB', '18 dB', '24 dB', '30 dB', '36 dB', '48 dB', '72 dB', '96 dB', 'Brickwall'];
/** Filter order for each entry of `SLOPE_NAMES`. @type {readonly number[]} */
export const SLOPE_ORDERS = [1, 2, 3, 4, 5, 6, 8, 12, 16, 32];

/**
 * Curve colours by stereo placement (manual §27): yellow for stereo, white
 * / red for left / right, green / blue for mid / side. A band's `color`
 * field overrides this.
 * @type {Readonly<Record<'stereo'|'left'|'right'|'mid'|'side', string>>}
 */
export const PLACEMENT_COLORS = {
  stereo: '#ffd166',
  left: '#f1f5f9',
  right: '#ff5c5c',
  mid: '#3ddc84',
  side: '#58a6ff',
};

/**
 * Q of the k-th (1-based) second-order section of an order-N Butterworth
 * filter: `Q_k = 1 / (2·sin((2k − 1)·π / (2N)))`. Cascading the ⌊N/2⌋
 * sections (plus a first-order section when N is odd) at the same corner
 * gives a maximally flat response that is −3 dB at the corner for every N.
 * For N = 2 this is `1/√2`; for N = 4, `0.541` and `1.307`.
 * @param {number} order Filter order N (1..32 here).
 * @param {number} k Section number, 1 … ⌊N/2⌋.
 * @returns {number}
 */
export function butterworthQ(order, k) {
  return 1 / (2 * Math.sin(((2 * k - 1) * Math.PI) / (2 * order)));
}

/**
 * RBJ "Audio EQ Cookbook" biquad coefficients, normalized so `a0 = 1`
 * (mirrors `Coefs::rbj`). With `w0 = 2π·f/sr` (f clamped to 1 Hz … 0.499·sr),
 * `α = sin(w0) / (2Q)` (Q clamped to ≥ 0.001) and `A = 10^(gainDb/40)`:
 *
 * * `lowpass`  — b = [(1−cos)/2, 1−cos, (1−cos)/2], a = [1+α, −2cos, 1−α]
 * * `highpass` — b = [(1+cos)/2, −(1+cos), (1+cos)/2], same a
 * * `bandpass` — b = [α, 0, −α] (constant 0 dB peak gain), same a
 * * `notch`    — b = [1, −2cos, 1], same a
 * * `allpass`  — b = [1−α, −2cos, 1+α], same a
 * * `peak`     — b = [1+αA, −2cos, 1−αA], a = [1+α/A, −2cos, 1−α/A]
 * * `lowshelf` / `highshelf` — the cookbook shelving forms with
 *   `2√A·α` as the slope term, so Q = 1/√2 is the steepest monotonic shelf
 *   and larger Q adds a resonant overshoot at the corner.
 *
 * Unknown types fall back to `peak`. Gain is ignored for the gain-less types.
 *
 * @param {string} type One of `FilterTypes` (not an alias; see `normalizeType`).
 * @param {number} freq Hz
 * @param {number} gainDb dB (shelves and peak only)
 * @param {number} q Quality factor
 * @param {number} sampleRate Hz
 * @returns {{b0:number, b1:number, b2:number, a1:number, a2:number}} Normalized coefficients (`a0` folded in).
 */
export function biquad(type, freq, gainDb, q, sampleRate) {
  const w0 = (2 * Math.PI * Math.min(Math.max(freq, 1), sampleRate * 0.499)) / sampleRate;
  const cs = Math.cos(w0);
  const sn = Math.sin(w0);
  const A = Math.pow(10, gainDb / 40);
  const alpha = sn / (2 * Math.max(q, 1e-3));
  let b0, b1, b2, a0, a1, a2;
  switch (type) {
    case 'lowpass':
      b0 = (1 - cs) / 2; b1 = 1 - cs; b2 = (1 - cs) / 2;
      a0 = 1 + alpha; a1 = -2 * cs; a2 = 1 - alpha;
      break;
    case 'highpass':
      b0 = (1 + cs) / 2; b1 = -(1 + cs); b2 = (1 + cs) / 2;
      a0 = 1 + alpha; a1 = -2 * cs; a2 = 1 - alpha;
      break;
    case 'bandpass':
      b0 = alpha; b1 = 0; b2 = -alpha;
      a0 = 1 + alpha; a1 = -2 * cs; a2 = 1 - alpha;
      break;
    case 'notch':
      b0 = 1; b1 = -2 * cs; b2 = 1;
      a0 = 1 + alpha; a1 = -2 * cs; a2 = 1 - alpha;
      break;
    case 'allpass':
      b0 = 1 - alpha; b1 = -2 * cs; b2 = 1 + alpha;
      a0 = 1 + alpha; a1 = -2 * cs; a2 = 1 - alpha;
      break;
    case 'lowshelf': {
      const sq = 2 * Math.sqrt(A) * alpha;
      b0 = A * (A + 1 - (A - 1) * cs + sq);
      b1 = 2 * A * (A - 1 - (A + 1) * cs);
      b2 = A * (A + 1 - (A - 1) * cs - sq);
      a0 = A + 1 + (A - 1) * cs + sq;
      a1 = -2 * (A - 1 + (A + 1) * cs);
      a2 = A + 1 + (A - 1) * cs - sq;
      break;
    }
    case 'highshelf': {
      const sq = 2 * Math.sqrt(A) * alpha;
      b0 = A * (A + 1 + (A - 1) * cs + sq);
      b1 = -2 * A * (A - 1 + (A + 1) * cs);
      b2 = A * (A + 1 + (A - 1) * cs - sq);
      a0 = A + 1 - (A - 1) * cs + sq;
      a1 = 2 * (A - 1 - (A + 1) * cs);
      a2 = A + 1 - (A - 1) * cs - sq;
      break;
    }
    case 'peak':
    default:
      b0 = 1 + alpha * A; b1 = -2 * cs; b2 = 1 - alpha * A;
      a0 = 1 + alpha / A; a1 = -2 * cs; a2 = 1 - alpha / A;
      break;
  }
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
}

/**
 * First-order section (6 dB/oct) from the bilinear transform of the analog
 * RC prototype, with `k = tan(π·f/sr)` (mirrors `Coefs::one_pole_lp/hp`):
 * low-pass `H(z) = k(1 + z⁻¹) / ((1 + k) + (k − 1)z⁻¹)`, high-pass
 * `H(z) = (1 − z⁻¹) / ((1 + k) + (k − 1)z⁻¹)`. Returned in the same
 * five-coefficient shape as `biquad` (with `b2 = a2 = 0`) so the two can be
 * mixed in one cascade; used for the odd orders in `bandCoefs`.
 * @param {'lowpass'|'highpass'} type Anything other than `'lowpass'` is treated as high-pass.
 * @param {number} freq Hz (clamped to 1 Hz … 0.499·sr)
 * @param {number} sampleRate Hz
 * @returns {{b0:number, b1:number, b2:number, a1:number, a2:number}}
 */
export function onePole(type, freq, sampleRate) {
  const k = Math.tan((Math.PI * Math.min(Math.max(freq, 1), sampleRate * 0.499)) / sampleRate);
  const n = 1 / (1 + k);
  if (type === 'lowpass') return { b0: k * n, b1: k * n, b2: 0, a1: (k - 1) * n, a2: 0 };
  return { b0: n, b1: -n, b2: 0, a1: (k - 1) * n, a2: 0 };
}

/**
 * Magnitude response of a normalized biquad at `freq`, in dB (mirrors
 * `Coefs::magnitude_db`). Evaluates `H(e^{jw})` with `w = 2π·f/sr`:
 * numerator `b0 + b1·e^{−jw} + b2·e^{−2jw}`, denominator
 * `1 + a1·e^{−jw} + a2·e^{−2jw}`, and returns
 * `10·log10(|N|² / |D|²)` with both magnitudes floored at 1e-30 so a notch
 * bottoms out at about −300 dB instead of `-Infinity`.
 * @param {{b0:number, b1:number, b2:number, a1:number, a2:number}} c
 * @param {number} freq Hz
 * @param {number} sampleRate Hz
 * @returns {number} dB
 */
export function magnitudeDb(c, freq, sampleRate) {
  const w = (2 * Math.PI * freq) / sampleRate;
  const c1 = Math.cos(w);
  const s1 = Math.sin(w);
  const c2 = Math.cos(2 * w);
  const s2 = Math.sin(2 * w);
  const nr = c.b0 + c.b1 * c1 + c.b2 * c2;
  const ni = c.b1 * s1 + c.b2 * s2;
  const dr = 1 + c.a1 * c1 + c.a2 * c2;
  const di = c.a1 * s1 + c.a2 * s2;
  return 10 * Math.log10(Math.max(nr * nr + ni * ni, 1e-30) / Math.max(dr * dr + di * di, 1e-30));
}

/**
 * Gain-Q interaction (Bell only, the "Gain-Q" switch of the manual): the
 * effective Q grows with the magnitude of the gain,
 * `Q · (1 + |gain| / 30)`, so a bell narrows as it is boosted or cut
 * further — 30 dB doubles the Q. Off, or for any other type, Q is returned
 * unchanged. Must match the rule in `design_band`.
 * @param {string} type A `FilterTypes` id.
 * @param {number} q User Q.
 * @param {number} gainDb dB
 * @param {boolean} gainQ Whether the interaction is enabled.
 * @returns {number} Effective Q.
 */
export function effectiveQ(type, q, gainDb, gainQ) {
  return gainQ && type === 'peak' ? q * (1 + Math.abs(gainDb) / 30) : q;
}

/**
 * All second-order (and first-order) sections for one band, mirroring
 * `design_band` in Rust. The cascade is built per type:
 *
 * * `peak` — one RBJ peak section at (`freq`, `gainDb`, effective Q).
 * * `notch`, `bandpass`, `allpass` — one section at gain 0.
 * * `lowshelf`, `highshelf` — `n = clamp(order/2, 1..8)` identical shelf
 *   sections, each carrying `gainDb / n`, so steeper slopes keep the total
 *   shelf gain while the transition sharpens.
 * * `tiltshelf` — `n` pairs (same `n`) of a low shelf at `−g` and a high
 *   shelf at `+g` with `g = gainDb / (2n)`, pivoting at `freq`: the total
 *   tilt across the spectrum is `gainDb`.
 * * `flattilt` — one such pair with a fixed, very low Q of 0.18, which
 *   spreads the transition over the whole audible range (a "flat" tilt).
 * * `lowpass`, `highpass` (cuts) — a Butterworth of order `N =
 *   SLOPE_ORDERS[slope]`: `⌊N/2⌋` sections with `butterworthQ(N, k)`,
 *   except the last section whose Q is scaled by the band's Q relative to
 *   `1/√2` (`Q_k · q / √½`, clamped 0.05..40) so the user's Q adds or
 *   removes resonance at the corner while lower sections stay put; plus one
 *   `onePole` section when N is odd (6, 18, 30 dB).
 *
 * `slope` is an index into `SLOPE_ORDERS` (clamped); it only matters for
 * the types that use it. Gain-less types ignore `gainDb`.
 *
 * @param {string} type A `FilterTypes` id.
 * @param {number} freq Hz
 * @param {number} gainDb dB
 * @param {number} q Quality factor (before the gain-Q rule)
 * @param {number} slope Index into `SLOPE_ORDERS`.
 * @param {number} sampleRate Hz
 * @param {object} [opts]
 * @param {boolean} [opts.gainQ=false] Apply `effectiveQ`.
 * @returns {{b0:number, b1:number, b2:number, a1:number, a2:number}[]} Sections to cascade; pass to `bandDb`.
 */
export function bandCoefs(type, freq, gainDb, q, slope, sampleRate, { gainQ = false } = {}) {
  const order = SLOPE_ORDERS[Math.max(0, Math.min(SLOPE_ORDERS.length - 1, slope | 0))];
  q = effectiveQ(type, q, gainDb, gainQ);
  switch (type) {
    case 'peak':
      return [biquad('peak', freq, gainDb, q, sampleRate)];
    case 'notch':
    case 'bandpass':
    case 'allpass':
      return [biquad(type, freq, 0, q, sampleRate)];
    case 'lowshelf':
    case 'highshelf': {
      const n = Math.max(1, Math.min(8, (order / 2) | 0));
      const c = biquad(type, freq, gainDb / n, q, sampleRate);
      return Array.from({ length: n }, () => c);
    }
    case 'tiltshelf':
    case 'flattilt': {
      const n = type === 'flattilt' ? 1 : Math.max(1, Math.min(8, (order / 2) | 0));
      const qq = type === 'flattilt' ? 0.18 : q;
      const g = gainDb / (2 * n);
      const lo = biquad('lowshelf', freq, -g, qq, sampleRate);
      const hi = biquad('highshelf', freq, g, qq, sampleRate);
      const out = [];
      for (let i = 0; i < n; i++) out.push(lo, hi);
      return out;
    }
    case 'lowpass':
    case 'highpass': {
      const n2 = (order / 2) | 0;
      const odd = order % 2;
      const out = [];
      for (let k = 1; k <= n2; k++) {
        let qk = butterworthQ(order, k);
        if (k === n2) qk = Math.max(0.05, Math.min(40, (qk * q) / Math.SQRT1_2));
        out.push(biquad(type, freq, 0, qk, sampleRate));
      }
      if (odd) out.push(onePole(type, freq, sampleRate));
      return out;
    }
    default:
      return [biquad('peak', freq, gainDb, q, sampleRate)];
  }
}

/**
 * Response of a cascade at one frequency: the sum of the sections'
 * `magnitudeDb` (cascaded magnitudes multiply, so dB add). Mirrors
 * `band_magnitude_db`.
 * @param {{b0:number, b1:number, b2:number, a1:number, a2:number}[]} coefs From `bandCoefs`.
 * @param {number} freq Hz
 * @param {number} sampleRate Hz
 * @returns {number} dB
 */
export function bandDb(coefs, freq, sampleRate) {
  let db = 0;
  for (const c of coefs) db += magnitudeDb(c, freq, sampleRate);
  return db;
}

/**
 * Lower-cased, letters-only spellings that `normalizeType` accepts, mapped
 * to `FilterTypes` ids. Covers the manual's names ("Low Cut", "High Cut",
 * "Bell"), engineering abbreviations (LPF/HPF/BPF/APF) and a few synonyms.
 */
const TYPE_ALIASES = {
  peak: 'peak', peaking: 'peak', bell: 'peak', parametric: 'peak',
  lowshelf: 'lowshelf', lshelf: 'lowshelf', lowshelving: 'lowshelf',
  highshelf: 'highshelf', hshelf: 'highshelf', highshelving: 'highshelf',
  lowpass: 'lowpass', lpf: 'lowpass', lp: 'lowpass', highcut: 'lowpass',
  highpass: 'highpass', hpf: 'highpass', hp: 'highpass', lowcut: 'highpass',
  notch: 'notch', bandstop: 'notch', bandreject: 'notch',
  bandpass: 'bandpass', bpf: 'bandpass', bp: 'bandpass',
  tiltshelf: 'tiltshelf', tilt: 'tiltshelf',
  flattilt: 'flattilt',
  allpass: 'allpass', apf: 'allpass',
};

/**
 * Map a human or plugin-facing filter name onto a `FilterTypes` id:
 * accepts `'Low Shelf'`, `'low-cut'`, `'LPF'`, `'bell'`, `'Band Pass'`, …
 * (case, spaces and punctuation are ignored). Unknown names become
 * `'peak'`.
 * @param {string} s
 * @returns {string} A `FilterTypes` id.
 */
export function normalizeType(s) {
  const key = String(s).toLowerCase().replace(/[^a-z]/g, '');
  return TYPE_ALIASES[key] || 'peak';
}

/** Types whose node sits at the band gain and whose gain is editable (vertical drag, Ctrl-wheel). */
export const GAIN_TYPES = new Set(['peak', 'lowshelf', 'highshelf', 'tiltshelf', 'flattilt']);
/** The two cut types (low cut = `highpass`, high cut = `lowpass`); plain wheel steps their slope. */
export const CUT_TYPES = new Set(['lowpass', 'highpass']);
/** Types for which the slope parameter has an effect (see `bandCoefs`). */
export const SLOPE_TYPES = new Set(['lowpass', 'highpass', 'lowshelf', 'highshelf', 'tiltshelf']);
/** Types that can run as dynamic EQ bands (gain-carrying and single-corner). */
export const DYN_TYPES = new Set(['peak', 'lowshelf', 'highshelf', 'flattilt']);

/** Duck-typed test for a noob-vst-webgui-framework Param (anything with an `on` subscription). */
const isParam = (v) => v && typeof v === 'object' && typeof v.on === 'function';

/**
 * Index of a labelled Param's current label (`round(value · (steps − 1))`),
 * or `null` when the Param has no labels.
 * @param {object} v A Param.
 * @returns {number|null}
 */
function labelIndex(v) {
  const labels = v.spec && v.spec.labels;
  if (labels && labels.length) return Math.max(0, Math.min(labels.length - 1, Math.round(v.value * (v.spec.steps - 1))));
  return null;
}
/**
 * Resolve a band's `type` field: string → alias lookup; Param with labels →
 * alias lookup of the current label; other Param → `FilterTypes[plain]`;
 * missing → `'peak'`.
 * @returns {string} A `FilterTypes` id.
 */
function typeOf(v) {
  if (v == null) return 'peak';
  if (typeof v === 'string') return normalizeType(v);
  if (isParam(v)) {
    const i = labelIndex(v);
    if (i != null) return normalizeType(v.spec.labels[i]);
    return FilterTypes[Math.round(v.plain)] || 'peak';
  }
  return 'peak';
}
/**
 * Resolve a band's `slope` field to an index into `SLOPE_ORDERS`: number as
 * is; string by prefix match on `SLOPE_NAMES` (`'24'` → 3); Param by label
 * index, else its plain value; missing → 1 (12 dB).
 * @returns {number}
 */
function slopeIndexOf(v) {
  if (v == null) return 1;
  if (typeof v === 'number') return v | 0;
  if (typeof v === 'string') return Math.max(0, SLOPE_NAMES.findIndex((s) => s.toLowerCase().startsWith(v.toLowerCase())));
  if (isParam(v)) {
    const i = labelIndex(v);
    if (i != null) return i;
    return Math.round(v.plain);
  }
  return 1;
}
/**
 * Resolve a band's `placement` field to one of the `PLACEMENT_COLORS` keys:
 * string lower-cased; Param by label, else by plain index in
 * stereo/left/right/mid/side order; missing → `'stereo'`.
 * @returns {string}
 */
function placementOf(v) {
  if (v == null) return 'stereo';
  if (typeof v === 'string') return v.toLowerCase();
  if (isParam(v)) {
    const i = labelIndex(v);
    if (i != null) return v.spec.labels[i].toLowerCase();
    return ['stereo', 'left', 'right', 'mid', 'side'][Math.round(v.plain)] || 'stereo';
  }
  return 'stereo';
}
/**
 * Resolve a boolean-ish band field: booleans as is, Params / numbers by
 * `plain ≥ 0.5`, missing → `dflt`.
 * @returns {boolean}
 */
function boolOf(v, dflt) {
  if (v == null) return dflt;
  if (typeof v === 'boolean') return v;
  return plainOf(v) >= 0.5;
}
/** `setPlain` on a field only if it is a Param (plain values are read-only from the display's point of view). */
const setParam = (p, plain) => isParam(p) && p.setPlain(plain);

/**
 * Parametric EQ display and editor.
 *
 * Public fields: `el` (root `<div>`), `svg`, `opts`, `bands` (the array
 * passed in), `selected` (`Set<number>`), `primary` (`number|null`),
 * `hovered` (`number|null`), and the `gainQ` getter.
 */
export class EqCurve {
  /**
   * @param {HTMLElement} container Element the display is appended to; decides the size.
   * @param {object} opts
   * @param {EqBand[]} opts.bands Band descriptors; the array is kept by reference but its length is fixed at construction.
   * @param {number} [opts.sampleRate=48000] Sample rate the responses are computed at (should match the plugin's).
   * @param {number} [opts.minHz=10] Left edge.
   * @param {number} [opts.maxHz=30000] Right edge.
   * @param {number} [opts.rangeDb=12] Vertical range is ±rangeDb.
   * @param {number} [opts.points=256] Frequencies per curve (`points + 1` samples, log-spaced).
   * @param {boolean|object} [opts.gainQ=false] Gain-Q interaction: a boolean or a Param (≥ 0.5 = on).
   * @param {(i:number)=>number} [opts.dynGain] Returns the current dynamic gain of band `i` in dB (from the plugin's band-gain stream); positions the dot on the dynamic-range bar.
   * @param {boolean} [opts.grid=true] Draw the frequency / dB grid.
   * @param {boolean} [opts.showBands=true] Draw each band's own curve and fill (nodes are always drawn).
   * @param {number} [opts.nodeRadius=8] Node radius in px (selected nodes are 2 px larger; the hit area 8 px larger).
   * @param {(sel:number[], primary:number|null)=>void} [opts.onSelect] Selection changed.
   * @param {(i:number|null)=>void} [opts.onHover] Pointer entered (`i`) / left (`null`) a node.
   * @param {(hit:{type:string, freq:number, db:number, alt:boolean, shift:boolean, x:number, y:number, fromCurve?:boolean})=>number|null} [opts.onCreateBand]
   *   Asked to create a band at a position: `type` is the suggested shape (`shapeForPosition`, or low/high shelf / peak when dragged from the curve), `db` the gain to give it. Return the new band's index to select it (and, from the curve, to start dragging it), or `null`.
   * @param {(i:number, ev:MouseEvent)=>void} [opts.onBandContextMenu] Right-click on a node.
   * @param {(i:number)=>void} [opts.onBandDblClick] Double-click on a node (e.g. open value entry).
   * @param {(i:number)=>void} [opts.onCycleShape] Ctrl+Alt-click on a node.
   * @param {(i:number)=>void} [opts.onCycleSlope] Alt+Shift-click on a node.
   * @param {(hover:{freq:number, db:number, x:number, y:number}|null)=>void} [opts.onPointer] Pointer position over the display in plot units, `null` when it leaves.
   * @example
   * const eq = new EqCurve(el, {
   *   sampleRate: 48000,
   *   bands: ids.map((n) => ({
   *     type: client.param(`b${n}_shape`), freq: client.param(`b${n}_freq`), gain: client.param(`b${n}_gain`),
   *     q: client.param(`b${n}_q`), slope: client.param(`b${n}_slope`), enabled: client.param(`b${n}_on`),
   *   })),
   *   onCreateBand: ({ type, freq, db }) => addBand(type, freq, db),
   * });
   * client.stream('band_dyn').on(() => eq.update());
   */
  constructor(container, opts) {
    injectStyle('noob-vst-webgui-framework-eq-css', CSS);
    this.opts = {
      sampleRate: 48000,
      minHz: 10,
      maxHz: 30000,
      rangeDb: 12,
      points: 256,
      gainQ: false,
      dynGain: null,
      grid: true,
      showBands: true,
      nodeRadius: 8,
      onSelect: null,
      onHover: null,
      onCreateBand: null,
      onBandContextMenu: null,
      onBandDblClick: null,
      onCycleShape: null,
      onCycleSlope: null,
      onPointer: null,
      ...opts,
    };
    this.bands = opts.bands || [];
    this.selected = new Set();
    this.primary = null;
    this.hovered = null;
    this._dimAll = false;

    const root = (this.el = document.createElement('div'));
    root.className = 'noob-vst-webgui-framework-eq';
    const svg = (this.svg = document.createElementNS(SVG, 'svg'));
    root.appendChild(svg);
    container.appendChild(root);
    this._gGrid = document.createElementNS(SVG, 'g');
    this._gBands = document.createElementNS(SVG, 'g');
    this._curveFill = document.createElementNS(SVG, 'path');
    this._curveFill.setAttribute('class', 'curve-fill');
    this._curve = document.createElementNS(SVG, 'path');
    this._curve.setAttribute('class', 'curve');
    this._curveHit = document.createElementNS(SVG, 'path');
    this._curveHit.setAttribute('class', 'curve-hit');
    this._preview = document.createElementNS(SVG, 'path');
    this._preview.setAttribute('class', 'preview');
    this._gNodes = document.createElementNS(SVG, 'g');
    svg.append(this._gGrid, this._gBands, this._curveFill, this._curve, this._preview, this._curveHit, this._gNodes);

    this._bandEls = this.bands.map((b, i) => this._makeBand(i));

    // Empty-space and curve gestures.
    svg.addEventListener('pointerdown', (e) => this._onBgDown(e));
    svg.addEventListener('pointermove', (e) => this._onBgMove(e));
    svg.addEventListener('pointerup', (e) => this._onBgUp(e));
    svg.addEventListener('pointercancel', (e) => this._onBgUp(e));
    svg.addEventListener('pointerleave', () => {
      this._preview.setAttribute('d', '');
      if (this.opts.onPointer) this.opts.onPointer(null);
    });
    svg.addEventListener('dblclick', (e) => this._onBgDbl(e));
    svg.addEventListener('contextmenu', (e) => e.preventDefault());
    this._curveHit.addEventListener('pointerdown', (e) => this._onCurveDown(e));

    this._offs = [];
    for (const b of this.bands) {
      for (const k of ['type', 'freq', 'gain', 'q', 'slope', 'placement', 'enabled', 'dynOn', 'dynRange', 'solo']) {
        if (isParam(b[k])) this._offs.push(b[k].on(() => this._schedule()));
      }
    }
    if (isParam(this.opts.gainQ)) this._offs.push(this.opts.gainQ.on(() => this._schedule()));
    this._ro = new ResizeObserver(() => {
      this._resize();
      this._schedule();
    });
    this._ro.observe(root);
    this._container = root;
    this._freqs = null;
    this._dirty = false;
    this._resize();
    this._render();
  }

  /**
   * Build the SVG for band `i`: its curve line and fill (in the bands
   * layer, so nodes stay on top) and a node group holding the dynamic-range
   * bar (hit, line, dot), the node hit circle, the node and its number.
   * Wires the node gestures. Returns the elements `_render` updates.
   */
  _makeBand(i) {
    const fill = document.createElementNS(SVG, 'path');
    fill.setAttribute('class', 'band-fill');
    const line = document.createElementNS(SVG, 'path');
    line.setAttribute('class', 'band');
    this._gBands.append(fill, line);

    const g = document.createElementNS(SVG, 'g');
    g.setAttribute('class', 'band-g');
    const dynHit = document.createElementNS(SVG, 'line');
    dynHit.setAttribute('class', 'dyn-hit');
    const dynLine = document.createElementNS(SVG, 'line');
    dynLine.setAttribute('class', 'dyn-line');
    const dynDot = document.createElementNS(SVG, 'circle');
    dynDot.setAttribute('class', 'dyn-dot');
    dynDot.setAttribute('r', 3.5);
    const hit = document.createElementNS(SVG, 'circle');
    hit.setAttribute('class', 'hit');
    hit.setAttribute('r', this.opts.nodeRadius + 8);
    const node = document.createElementNS(SVG, 'circle');
    node.setAttribute('class', 'node');
    node.setAttribute('r', this.opts.nodeRadius);
    const label = document.createElementNS(SVG, 'text');
    label.setAttribute('class', 'label');
    label.setAttribute('text-anchor', 'middle');
    label.setAttribute('dominant-baseline', 'central');
    label.textContent = String(i + 1);
    g.append(dynHit, dynLine, dynDot, hit, node, label);
    hit.addEventListener('pointerdown', (e) => this._onNodeDown(e, i));
    node.addEventListener('pointerdown', (e) => this._onNodeDown(e, i));
    g.addEventListener('pointermove', (e) => this._onNodeMove(e, i));
    g.addEventListener('pointerup', (e) => this._onNodeUp(e, i));
    g.addEventListener('pointercancel', (e) => this._onNodeUp(e, i));
    g.addEventListener('pointerenter', () => this._hover(i));
    g.addEventListener('pointerleave', () => this._hover(null));
    g.addEventListener('wheel', (e) => this._onWheel(e, i), { passive: false });
    g.addEventListener('dblclick', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (this.opts.onBandDblClick) this.opts.onBandDblClick(i);
    });
    g.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (this.opts.onBandContextMenu) this.opts.onBandContextMenu(i, e);
    });
    dynHit.addEventListener('pointerdown', (e) => this._onDynDown(e, i));
    this._gNodes.appendChild(g);
    return { fill, line, g, node, label, dynLine, dynDot, dynHit, hit };
  }

  // -- coordinates --------------------------------------------------------

  /** Size the viewBox to the element, refresh the log-axis constants, drop the cached frequency grid, redraw the grid. */
  _resize() {
    this._w = Math.max(1, this._container.clientWidth);
    this._h = Math.max(1, this._container.clientHeight);
    this.svg.setAttribute('viewBox', `0 0 ${this._w} ${this._h}`);
    this._logMin = Math.log(this.opts.minHz);
    this._logSpan = Math.log(this.opts.maxHz) - this._logMin;
    this._freqs = null;
    if (this.opts.grid) this._drawGrid();
  }

  /**
   * Change the visible frequency range (horizontal zoom). `minHz` is
   * clamped to ≥ 1 Hz and `maxHz` to at least 1.5 × `minHz`.
   * @param {number} minHz
   * @param {number} maxHz
   */
  setRange(minHz, maxHz) {
    this.opts.minHz = Math.max(1, minHz);
    this.opts.maxHz = Math.max(this.opts.minHz * 1.5, maxHz);
    this._resize();
    this._schedule();
  }

  /**
   * Change the vertical display range (±dB): 3, 6, 12 or 30 in the manual.
   * The grid step follows (0.75 / 1.5 / 3 / 6 dB).
   * @param {number} rangeDb
   */
  setRangeDb(rangeDb) {
    this.opts.rangeDb = rangeDb;
    this._resize();
    this._schedule();
  }

  /**
   * x pixel (SVG user units = CSS px) for a frequency on the log axis.
   * @param {number} f Hz
   * @returns {number}
   */
  xForFreq(f) {
    return ((Math.log(Math.max(f, 1e-6)) - this._logMin) / this._logSpan) * this._w;
  }
  /**
   * Frequency for an x pixel (clamped to the display).
   * @param {number} x
   * @returns {number} Hz
   */
  freqForX(x) {
    return Math.exp(this._logMin + (Math.max(0, Math.min(this._w, x)) / this._w) * this._logSpan);
  }
  /**
   * y pixel for a dB value on the ±`rangeDb` axis (0 dB is the middle).
   * @param {number} db
   * @returns {number}
   */
  yForDb(db) {
    const r = this.opts.rangeDb;
    return this._h - ((db + r) / (2 * r)) * this._h;
  }
  /**
   * dB value for a y pixel (clamped to the display).
   * @param {number} y
   * @returns {number}
   */
  dbForY(y) {
    const r = this.opts.rangeDb;
    return -r + (1 - Math.max(0, Math.min(this._h, y)) / this._h) * 2 * r;
  }

  /**
   * Rebuild the grid layer: vertical lines from `GRID_HZ` (stronger at the
   * `LABEL_HZ` decades), horizontal lines and `±n dB` labels at a step that
   * depends on `rangeDb`, with the 0 dB line in the `.zero` style.
   */
  _drawGrid() {
    const g = this._gGrid;
    g.textContent = '';
    for (const f of GRID_HZ) {
      if (f < this.opts.minHz || f > this.opts.maxHz) continue;
      const x = this.xForFreq(f);
      const l = document.createElementNS(SVG, 'line');
      l.setAttribute('class', LABEL_HZ.has(f) ? 'grid' : 'grid minor');
      l.setAttribute('x1', x);
      l.setAttribute('x2', x);
      l.setAttribute('y1', 0);
      l.setAttribute('y2', this._h);
      g.appendChild(l);
    }
    const r = this.opts.rangeDb;
    const step = r >= 24 ? 6 : r >= 12 ? 3 : r >= 6 ? 1.5 : 0.75;
    for (let db = -r; db <= r + 1e-6; db += step) {
      const y = this.yForDb(db);
      const l = document.createElementNS(SVG, 'line');
      l.setAttribute('class', Math.abs(db) < 1e-6 ? 'zero' : 'grid');
      l.setAttribute('x1', 0);
      l.setAttribute('x2', this._w);
      l.setAttribute('y1', y);
      l.setAttribute('y2', y);
      g.appendChild(l);
      const t = document.createElementNS(SVG, 'text');
      t.setAttribute('x', 4);
      // Edge labels sit inside the display: the top one below its line.
      t.setAttribute('y', Math.abs(db - r) < 1e-6 ? y + 11 : y - 3);
      t.textContent = `${db > 0 ? '+' : ''}${Number.isInteger(db) ? db : db.toFixed(1)}`;
      g.appendChild(t);
    }
  }

  // -- model ----------------------------------------------------------------

  /**
   * Resolved values for band `i`: every field of the descriptor turned into
   * a number, canonical string or boolean (see the file header for the
   * rules and defaults). Cheap; called freely during rendering and drags.
   * @param {number} i
   * @returns {{type:string, freq:number, gain:number, q:number, slope:number, placement:string, enabled:boolean, dynOn:boolean, dynRange:number, solo:boolean}}
   */
  bandValues(i) {
    const b = this.bands[i];
    return {
      type: typeOf(b.type),
      freq: plainOf(b.freq ?? 1000),
      gain: plainOf(b.gain ?? 0),
      q: plainOf(b.q ?? 1),
      slope: slopeIndexOf(b.slope),
      placement: placementOf(b.placement),
      enabled: boolOf(b.enabled, true),
      dynOn: boolOf(b.dynOn, false),
      dynRange: plainOf(b.dynRange ?? 0),
      solo: boolOf(b.solo, false),
    };
  }

  /**
   * Whether the gain-Q interaction is on (a boolean option, or a Param at
   * ≥ 0.5).
   * @type {boolean}
   */
  get gainQ() {
    const g = this.opts.gainQ;
    return isParam(g) ? g.value >= 0.5 : !!g;
  }

  /**
   * Composite static response in dB at `freq`: the sum over enabled bands,
   * all placements together (the display does not separate L/R or M/S),
   * without dynamic gain. Used to seed a band created by dragging the
   * curve.
   * @param {number} freq Hz
   * @returns {number} dB
   */
  curveDb(freq) {
    let db = 0;
    const sr = this.opts.sampleRate;
    const gq = this.gainQ;
    for (let i = 0; i < this.bands.length; i++) {
      const v = this.bandValues(i);
      if (!v.enabled) continue;
      db += bandDb(bandCoefs(v.type, v.freq, v.gain, v.q, v.slope, sr, { gainQ: gq }), freq, sr);
    }
    return db;
  }

  /**
   * Change the selection and report it through `onSelect`.
   * @param {number|null} i Band index, or `null` to clear.
   * @param {'set'|'toggle'|'range'} [mode='set'] `set` selects only `i` and makes it primary; `toggle` adds / removes `i` (primary follows); `range` adds every enabled band between `primary` and `i`.
   */
  select(i, mode = 'set') {
    if (i == null) {
      this.selected.clear();
      this.primary = null;
    } else if (mode === 'toggle') {
      if (this.selected.has(i)) this.selected.delete(i);
      else this.selected.add(i);
      this.primary = this.selected.has(i) ? i : [...this.selected].pop() ?? null;
    } else if (mode === 'range' && this.primary != null) {
      const [a, b] = [Math.min(this.primary, i), Math.max(this.primary, i)];
      for (let k = a; k <= b; k++) if (this.bandValues(k).enabled) this.selected.add(k);
    } else {
      this.selected.clear();
      this.selected.add(i);
      this.primary = i;
    }
    if (this.opts.onSelect) this.opts.onSelect([...this.selected], this.primary);
    this._schedule();
  }

  /**
   * Dim every band and the curve (used while the pointer is over the
   * spectrum for "spectrum grab", or while a band is soloed).
   * @param {boolean} on
   */
  setDimmed(on) {
    this._dimAll = on;
    this._schedule();
  }

  /** Track the hovered node (`null` = none), notify `onHover`, redraw for the highlight. */
  _hover(i) {
    this.hovered = i;
    if (this.opts.onHover) this.opts.onHover(i);
    this._schedule();
  }

  /** Coalesce redraws: at most one `_render` per animation frame. */
  _schedule() {
    if (this._dirty) return;
    this._dirty = true;
    requestAnimationFrame(() => {
      this._dirty = false;
      this._render();
    });
  }

  /**
   * Request a redraw on the next animation frame, e.g. when the plugin's
   * dynamic-gain stream ticks (`opts.dynGain` is read during rendering).
   */
  update() {
    this._schedule();
  }

  // -- rendering ------------------------------------------------------------

  /**
   * Full redraw: (re)build the cached log-spaced frequency grid if needed,
   * then for every band compute its cascade once and sample it at each
   * grid point, accumulating enabled bands into the composite; position
   * the node group and dynamic-range bar; finally write the composite
   * curve, fill and hit paths. Costs `bands × points × sections`
   * `magnitudeDb` calls per frame (24 × 257 × a few) — fine at 60 fps.
   */
  _render() {
    const n = this.opts.points;
    if (!this._freqs) {
      this._freqs = new Float64Array(n + 1);
      this._xs = new Float64Array(n + 1);
      for (let i = 0; i <= n; i++) {
        const x = (i / n) * this._w;
        this._xs[i] = x;
        this._freqs[i] = this.freqForX(x);
      }
    }
    const sr = this.opts.sampleRate;
    const gq = this.gainQ;
    const total = new Float64Array(n + 1);
    const y0 = this.yForDb(0);
    const dimAll = this._dimAll;
    this.bands.forEach((b, bi) => {
      const v = this.bandValues(bi);
      const el = this._bandEls[bi];
      const color = b.color || PLACEMENT_COLORS[v.placement] || PLACEMENT_COLORS.stereo;
      el.line.setAttribute('stroke', color);
      el.fill.setAttribute('fill', color);
      el.node.setAttribute('fill', color);
      el.dynLine.setAttribute('stroke', color);
      el.dynDot.setAttribute('fill', '#ffd166');
      const present = v.enabled || this.selected.has(bi);
      el.g.style.display = present ? '' : 'none';
      if (!present) {
        el.line.setAttribute('d', '');
        el.fill.setAttribute('d', '');
        return;
      }
      const coefs = bandCoefs(v.type, v.freq, v.gain, v.q, v.slope, sr, { gainQ: gq });
      let d = '';
      for (let i = 0; i <= n; i++) {
        const db = bandDb(coefs, this._freqs[i], sr);
        if (v.enabled) total[i] += db;
        d += (i === 0 ? 'M' : 'L') + this._xs[i].toFixed(1) + ' ' + this.yForDb(db).toFixed(1);
      }
      if (this.opts.showBands) {
        el.line.setAttribute('d', d);
        el.fill.setAttribute('d', `${d} L${this._w} ${y0.toFixed(1)} L0 ${y0.toFixed(1)} Z`);
        const hot = this.selected.has(bi) || bi === this.hovered;
        el.fill.classList.toggle('hot', hot && !dimAll);
        el.fill.classList.toggle('off', !v.enabled || dimAll);
        el.line.classList.toggle('off', !v.enabled || dimAll);
      }
      const gainful = GAIN_TYPES.has(v.type);
      const hy = this.yForDb(gainful ? v.gain : 0);
      const hx = this.xForFreq(v.freq);
      el.g.setAttribute('transform', `translate(${hx.toFixed(1)} ${hy.toFixed(1)})`);
      el.g.style.opacity = dimAll ? 0.35 : 1;
      el.node.classList.toggle('off', !v.enabled);
      el.node.classList.toggle('selected', this.selected.has(bi));
      el.node.setAttribute('r', this.selected.has(bi) ? this.opts.nodeRadius + 2 : this.opts.nodeRadius);
      // Dynamic range indicator: a bar from the static gain to gain+range,
      // with a dot at the current dynamic gain.
      const showDyn = v.dynOn && gainful && Math.abs(v.dynRange) > 0.01 && v.enabled;
      if (showDyn) {
        const y2 = this.yForDb(v.gain + v.dynRange) - hy;
        el.dynLine.setAttribute('x1', 0);
        el.dynLine.setAttribute('x2', 0);
        el.dynLine.setAttribute('y1', 0);
        el.dynLine.setAttribute('y2', y2.toFixed(1));
        el.dynHit.setAttribute('x1', 0);
        el.dynHit.setAttribute('x2', 0);
        el.dynHit.setAttribute('y1', 0);
        el.dynHit.setAttribute('y2', y2.toFixed(1));
        const cur = this.opts.dynGain ? this.opts.dynGain(bi) || 0 : 0;
        el.dynDot.setAttribute('cy', (this.yForDb(v.gain + cur) - hy).toFixed(1));
        el.dynLine.style.display = '';
        el.dynDot.style.display = '';
        el.dynHit.style.display = '';
      } else {
        el.dynLine.style.display = 'none';
        el.dynDot.style.display = 'none';
        el.dynHit.style.display = 'none';
      }
    });
    let d = '';
    for (let i = 0; i <= n; i++) {
      d += (i === 0 ? 'M' : 'L') + this._xs[i].toFixed(1) + ' ' + this.yForDb(total[i]).toFixed(1);
    }
    this._curve.setAttribute('d', d);
    this._curveHit.setAttribute('d', d);
    this._curveFill.setAttribute('d', `${d} L${this._w} ${y0.toFixed(1)} L0 ${y0.toFixed(1)} Z`);
    this._curve.style.opacity = dimAll ? 0.35 : 1;
    this._curveFill.style.opacity = dimAll ? 0.03 : 0.09;
  }

  /**
   * Show a dashed preview curve for a band that would be created (drawn
   * automatically while the pointer hovers empty space; pages can call it
   * for their own previews, e.g. spectrum grab).
   * @param {string} type A `FilterTypes` id.
   * @param {number} freq Hz
   * @param {number} gainDb dB
   * @param {number} [q=1]
   * @param {number} [slope=1] Index into `SLOPE_ORDERS`.
   */
  showPreview(type, freq, gainDb, q = 1, slope = 1) {
    const sr = this.opts.sampleRate;
    const coefs = bandCoefs(type, freq, gainDb, q, slope, sr, { gainQ: this.gainQ });
    const n = this.opts.points;
    let d = '';
    for (let i = 0; i <= n; i++) {
      const db = bandDb(coefs, this._freqs[i], sr);
      d += (i === 0 ? 'M' : 'L') + this._xs[i].toFixed(1) + ' ' + this.yForDb(db).toFixed(1);
    }
    this._preview.setAttribute('d', d);
  }
  /** Remove the preview curve. */
  hidePreview() {
    this._preview.setAttribute('d', '');
  }

  /**
   * The shape Pro-Q would pick for a new band at this position (manual
   * §3.1): a notch in the bottom 10 % of the display, a low cut in the left
   * 8 %, a high cut in the right 8 %, otherwise a bell.
   * @param {number} x px
   * @param {number} y px
   * @returns {'notch'|'highpass'|'lowpass'|'peak'}
   */
  shapeForPosition(x, y) {
    const fx = x / this._w;
    const fy = y / this._h;
    if (fy > 0.9) return 'notch';
    if (fx < 0.08) return 'highpass';
    if (fx > 0.92) return 'lowpass';
    return 'peak';
  }

  // -- gestures -------------------------------------------------------------

  /** Pointer position in SVG user space (CSS px from the top-left of the svg). */
  _local(e) {
    const r = this.svg.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  /** Whether an event landed on "empty space": the svg, the grid, the curve fill or a band fill/line. */
  _isBg(e) {
    const t = e.target;
    return t === this.svg || t.parentNode === this._gGrid || t === this._curveFill || t.parentNode === this._gBands;
  }

  /**
   * Primary button on empty space: remember the press so `_onBgUp` can tell
   * a click from a drag. A plain click while something is selected just
   * deselects (consumed here); with nothing selected, or with Ctrl / Cmd,
   * the release creates a band.
   */
  _onBgDown(e) {
    if (!this._isBg(e)) return;
    if (e.button === 2) return;
    if (e.button !== 0) return;
    const [x, y] = this._local(e);
    this._bg = { x, y, id: e.pointerId, moved: false };
    // Single click on empty space with a selection: deselect. Without a
    // selection: create the previewed band.
    if (this.selected.size && !e.ctrlKey && !e.metaKey) {
      this.select(null);
      this._bg.consumed = true;
    }
  }

  /**
   * Pointer over the display: report the position (`onPointer`), track
   * whether a background press has moved more than 4 px (then it is not a
   * click), and show the create-preview while hovering empty space with no
   * drag in progress.
   */
  _onBgMove(e) {
    const [x, y] = this._local(e);
    if (this.opts.onPointer) this.opts.onPointer({ freq: this.freqForX(x), db: this.dbForY(y), x, y });
    if (this._bg) {
      if (Math.hypot(x - this._bg.x, y - this._bg.y) > 4) this._bg.moved = true;
      return;
    }
    if (this._isBg(e) && !this._drag) {
      const type = this.shapeForPosition(x, y);
      const gain = GAIN_TYPES.has(type) ? this.dbForY(y) : 0;
      this.showPreview(type, this.freqForX(x), gain, 1, 1);
    } else {
      this.hidePreview();
    }
  }

  /** Release of a background press that did not move and was not consumed: create a band there. */
  _onBgUp(e) {
    const bg = this._bg;
    this._bg = null;
    if (!bg || bg.moved || bg.consumed) return;
    if (!this._isBg(e)) return;
    const [x, y] = this._local(e);
    if (this.opts.onCreateBand && (e.ctrlKey || e.metaKey || this.selected.size === 0)) {
      this._createAt(x, y, e);
    }
  }

  /** Double-click on empty space always creates a band (even with a selection). */
  _onBgDbl(e) {
    if (!this._isBg(e)) return;
    e.preventDefault();
    const [x, y] = this._local(e);
    if (this.opts.onCreateBand) this._createAt(x, y, e);
  }

  /**
   * Ask the page for a band at a position (`shapeForPosition` decides the
   * type, gain-carrying types take the pointer's dB), hide the preview and
   * select the result.
   * @returns {number|null} The new band index, or `null`.
   */
  _createAt(x, y, e) {
    const type = this.shapeForPosition(x, y);
    const freq = this.freqForX(x);
    const db = GAIN_TYPES.has(type) ? this.dbForY(y) : 0;
    const i = this.opts.onCreateBand({ type, freq, db, alt: e.altKey, shift: e.shiftKey, x, y });
    this.hidePreview();
    if (i != null && i >= 0) this.select(i);
    return i;
  }

  /**
   * Dragging the yellow result curve creates a band and drags its gain: a
   * low shelf in the left 8 %, a high shelf in the right 8 %, else a bell,
   * seeded with the composite gain at that frequency so the curve does not
   * jump; the pointer is captured by the new band's group and a node drag
   * starts immediately (`fromCurve`).
   */
  _onCurveDown(e) {
    if (e.button !== 0 || !this.opts.onCreateBand) return;
    e.preventDefault();
    e.stopPropagation();
    const [x, y] = this._local(e);
    const fx = x / this._w;
    const type = fx < 0.08 ? 'lowshelf' : fx > 0.92 ? 'highshelf' : 'peak';
    const freq = this.freqForX(x);
    const i = this.opts.onCreateBand({ type, freq, db: this.curveDb(freq), alt: e.altKey, shift: e.shiftKey, x, y, fromCurve: true });
    if (i == null || i < 0) return;
    this.select(i);
    const el = this._bandEls[i];
    el.g.setPointerCapture(e.pointerId);
    this._startNodeDrag(e, i, x, y, true);
  }

  /**
   * Begin a node drag for band `i` and every enabled selected band: snapshot
   * each member's freq / gain / q / dynRange (deltas are applied to the
   * snapshot, so the drag is stable however the values are quantised),
   * record the modifiers (Ctrl / Cmd = Q mode, Alt = constrain to the first
   * axis moved) and open `beginEdit` on freq, gain and q of every member.
   */
  _startNodeDrag(e, i, x, y, fromCurve) {
    const v = this.bandValues(i);
    const members = [...this.selected].filter((k) => this.bandValues(k).enabled);
    if (!members.includes(i)) members.push(i);
    this._drag = {
      i,
      id: e.pointerId,
      x0: x,
      y0: y,
      fromCurve,
      members: members.map((k) => {
        const bv = this.bandValues(k);
        return { k, freq: bv.freq, gain: bv.gain, q: bv.q, dynRange: bv.dynRange };
      }),
      qMode: e.ctrlKey || e.metaKey,
      constrain: e.altKey ? null : false,
      gainful: GAIN_TYPES.has(v.type),
    };
    for (const m of this._drag.members) {
      const b = this.bands[m.k];
      if (isParam(b.freq)) b.freq.beginEdit();
      if (isParam(b.gain)) b.gain.beginEdit();
      if (isParam(b.q)) b.q.beginEdit();
    }
  }

  /**
   * Primary button on a node. Modifier clicks act and return without a
   * drag: Ctrl+Alt cycles the shape, Alt+Shift the slope, Alt toggles the
   * band. Otherwise the click updates the selection (Ctrl / Cmd toggles,
   * Shift ranges, plain selects unless already selected) and starts a drag.
   */
  _onNodeDown(e, i) {
    if (e.button === 2) return;
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const b = this.bands[i];
    // Modifier clicks (manual §3.3).
    if (e.altKey && e.ctrlKey && !e.shiftKey) {
      if (this.opts.onCycleShape) this.opts.onCycleShape(i);
      return;
    }
    if (e.altKey && e.shiftKey) {
      if (this.opts.onCycleSlope) this.opts.onCycleSlope(i);
      return;
    }
    if (e.altKey) {
      if (isParam(b.enabled)) b.enabled.set(boolOf(b.enabled, true) ? 0 : 1);
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      if (!this.selected.has(i)) this.select(i, 'toggle');
    } else if (e.shiftKey) {
      this.select(i, 'range');
    } else if (!this.selected.has(i)) {
      this.select(i);
    }
    this._bandEls[i].g.setPointerCapture(e.pointerId);
    const [x, y] = this._local(e);
    this._startNodeDrag(e, i, x, y, false);
  }

  /**
   * Node drag. Horizontal motion is converted to octaves
   * (`dx / width · log2(maxHz / minHz)`) and applied multiplicatively to
   * each member's start frequency, so a drag moves all selected bands by
   * the same musical interval. Vertical motion edits gain (through the dB
   * axis, so it follows the pointer exactly) for gain-carrying types, or Q
   * (`q · 2^(−dy/60)`: 60 px per octave of Q) for cuts / notch / band-pass
   * and whenever Ctrl / Cmd was held (Q mode also freezes frequency). Shift
   * scales the motion by 0.15; Alt locks the axis after 6 px.
   */
  _onNodeMove(e, i) {
    const d = this._drag;
    if (!d || d.i !== i || e.pointerId !== d.id) return;
    const [x, y] = this._local(e);
    const fine = e.shiftKey ? 0.15 : 1;
    let dx = (x - d.x0) * fine;
    let dy = (y - d.y0) * fine;
    if (d.constrain === null && Math.hypot(dx, dy) > 6) d.constrain = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y';
    if (d.constrain === 'x') dy = 0;
    if (d.constrain === 'y') dx = 0;
    const octaves = (dx / this._w) * (this._logSpan / Math.LN2);
    for (const m of d.members) {
      const b = this.bands[m.k];
      const bv = this.bandValues(m.k);
      if (d.qMode || !GAIN_TYPES.has(bv.type)) {
        // Vertical = Q for cuts / notch / band-pass, or when Ctrl is held.
        if (dy !== 0) setParam(b.q, m.q * Math.pow(2, -dy / 60));
        if (!d.qMode && dx !== 0) setParam(b.freq, m.freq * Math.pow(2, octaves));
      } else {
        if (dx !== 0) setParam(b.freq, m.freq * Math.pow(2, octaves));
        if (dy !== 0) {
          const dg = this.dbForY(this.yForDb(m.gain) + dy) - m.gain;
          setParam(b.gain, m.gain + dg);
        }
      }
    }
    this._schedule();
  }

  /** End of a node drag: close the gestures opened in `_startNodeDrag` for every member. */
  _onNodeUp(e, i) {
    const d = this._drag;
    if (!d || d.i !== i) return;
    this._drag = null;
    for (const m of d.members) {
      const b = this.bands[m.k];
      if (isParam(b.freq)) b.freq.endEdit();
      if (isParam(b.gain)) b.gain.endEdit();
      if (isParam(b.q)) b.q.endEdit();
    }
  }

  /**
   * Drag on the dynamic-range bar: vertical motion, read through the dB
   * axis, is added to the band's `dynRange` (Shift ×0.15) inside one
   * begin / end gesture. Listeners are attached for the drag only and
   * removed on release.
   */
  _onDynDown(e, i) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const b = this.bands[i];
    if (!isParam(b.dynRange)) return;
    this._bandEls[i].g.setPointerCapture(e.pointerId);
    const [, y] = this._local(e);
    const v = this.bandValues(i);
    this._dyn = { i, id: e.pointerId, y0: y, range: v.dynRange, gain: v.gain };
    b.dynRange.beginEdit();
    const onMove = (ev) => {
      if (!this._dyn || ev.pointerId !== this._dyn.id) return;
      const [, yy] = this._local(ev);
      const fine = ev.shiftKey ? 0.15 : 1;
      const dyDb = this.dbForY(yy) - this.dbForY(this._dyn.y0);
      setParam(b.dynRange, this._dyn.range + dyDb * fine);
      this._schedule();
    };
    const onUp = () => {
      this._dyn = null;
      b.dynRange.endEdit();
      this._bandEls[i].g.removeEventListener('pointermove', onMove);
      this._bandEls[i].g.removeEventListener('pointerup', onUp);
      this._bandEls[i].g.removeEventListener('pointercancel', onUp);
    };
    this._bandEls[i].g.addEventListener('pointermove', onMove);
    this._bandEls[i].g.addEventListener('pointerup', onUp);
    this._bandEls[i].g.addEventListener('pointercancel', onUp);
  }

  /**
   * Wheel over a node (applies to the whole selection if the node is
   * selected). Per notch, `dir = ±1`, Shift scales by 0.25:
   * plain → Q × 1.12^dir (or one slope step for cuts with a slope Param);
   * Ctrl / Cmd → gain ± 1 dB; Alt → dynamic range ± 1 dB; Ctrl+Alt → trade
   * 1 dB of gain for 1 dB of dynamic range. All notches within 180 ms share
   * one begin / end gesture per touched Param.
   */
  _onWheel(e, i) {
    e.preventDefault();
    e.stopPropagation();
    const b = this.bands[i];
    const v = this.bandValues(i);
    const dir = e.deltaY < 0 ? 1 : -1;
    const fine = e.shiftKey ? 0.25 : 1;
    const targets = this.selected.has(i) ? [...this.selected] : [i];
    const gesture = (p, fn) => {
      if (!isParam(p)) return;
      if (!this._wheelTimer) p.beginEdit();
      fn(p);
    };
    for (const k of targets) {
      const bk = this.bands[k];
      const vk = this.bandValues(k);
      if (e.altKey && (e.ctrlKey || e.metaKey)) {
        // Trade gain for dynamic range.
        gesture(bk.gain, (p) => p.setPlain(vk.gain - dir * fine));
        gesture(bk.dynRange, (p) => p.setPlain(vk.dynRange + dir * fine));
      } else if (e.altKey) {
        gesture(bk.dynRange, (p) => p.setPlain(vk.dynRange + dir * fine));
      } else if (e.ctrlKey || e.metaKey) {
        gesture(bk.gain, (p) => p.setPlain(vk.gain + dir * fine));
      } else if (CUT_TYPES.has(vk.type) && isParam(bk.slope)) {
        const n = SLOPE_NAMES.length;
        gesture(bk.slope, (p) => p.set(Math.max(0, Math.min(n - 1, vk.slope + dir)) / (n - 1)));
      } else {
        gesture(bk.q, (p) => p.setPlain(vk.q * Math.pow(1.12, dir * fine)));
      }
    }
    void b;
    void v;
    clearTimeout(this._wheelTimer);
    this._wheelTimer = setTimeout(() => {
      this._wheelTimer = null;
      for (const k of targets) {
        for (const key of ['gain', 'dynRange', 'slope', 'q']) {
          const p = this.bands[k][key];
          if (isParam(p)) p.endEdit();
        }
      }
    }, 180);
    this._schedule();
  }

  /** Unsubscribe from every Param, stop observing size and remove the element. */
  destroy() {
    for (const off of this._offs) off();
    this._ro.disconnect();
    this.el.remove();
  }
}

export default EqCurve;
