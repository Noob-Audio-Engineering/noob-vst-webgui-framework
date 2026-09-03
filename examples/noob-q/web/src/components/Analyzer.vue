<script setup>
/**
 * The interactive display (manual §3): the centre of the UI. Everything
 * here is a layer stacked over the same box, bottom to top:
 *
 *   1. `preEl` / `postEl` / `scEl` — three framework `Spectrum` canvases
 *      (input, output, external side-chain), faded in and out with the
 *      analyzer switches. They draw the gray dB scale on the right.
 *   2. `eqEl` — the framework `EqCurve` (SVG): the yellow response curve,
 *      one node per enabled band, the per-band curves, the yellow dB scale
 *      on the left, and all node gestures (drag, wheel, double-click, the
 *      Alt / Ctrl / Shift modifiers, dragging the curve itself to create a
 *      band). A dashed white path is inserted into its SVG for the
 *      DSP-reported response (`curve` stream), so the browser-side filter
 *      model can be checked against the Rust one by eye.
 *   3. `rectEl` — Shift-drag rectangle selection of several bands.
 *   4. `sketchEl` — EQ Sketch: draw a curve left to right and it is fitted
 *      into bands on release (`fitSketch`). Armed from the pencil button in
 *      App.vue, or automatically while no band exists.
 *   5. `grabEl` — Spectrum Grab: the peaks of the frozen spectrum become
 *      handles; dragging one creates a Bell there and sets its gain.
 *   6. `ParamDisplay` — the value pop-up under the primary band's node.
 *
 * Parameters: every band's fifteen handles (`allBands()`), and the
 * analyzer / display globals (`analyzer_*`, `display_range`,
 * `piano_display`, `bypass`, `gain_q`). Streams: `spectrum_pre`,
 * `spectrum_post`, `spectrum_sc` (each subscribed only while its switch is
 * on, so a hidden spectrum costs no bandwidth or CPU), `curve` (sticky, the
 * reference line), `band_dyn` (live dynamic gain per band, feeds the curve
 * and the band panel) and `band_level` (per-band trigger level, feeds the
 * threshold meter).
 *
 * Selection lives in `ui.selected` / `ui.primary` as 1-based band numbers;
 * the EqCurve works with 0-based indices, hence the `+ 1` / `- 1` at every
 * boundary. Hover feeds `ui.hover` (band number) and `ui.hoverFreq` (cursor
 * frequency, drawn by FreqScale).
 *
 * Emits nothing; exposes `enterGrab(permanent)` and `leaveGrab()` for the
 * G key and Escape handled in App.vue.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { EqCurve, Spectrum, SLOPE_NAMES } from '@elyerinfox/vst3-web-stratum/components';
import {
  PLACEMENTS,
  SHAPES,
  allBands,
  bandToJson,
  createBand,
  deleteBand,
  getClient,
  selectBands,
  setBand,
  ui,
  useBand,
  useGlobals,
  useVst3WebStratum,
  useStream,
} from '../composables/useVst3WebStratum.js';
import { noteLabel } from '../notes.js';
import ParamDisplay from './ParamDisplay.vue';
import { ContextMenu } from '@elyerinfox/vst3-web-stratum/vue';

const { manifest } = useVst3WebStratum();
const sr = computed(() => manifest.value?.meta?.sample_rate || 48000);
const g = useGlobals();
const bands = allBands();

const preEl = ref(null);
const postEl = ref(null);
const scEl = ref(null);
const eqEl = ref(null);
const grabEl = ref(null);
const sketchEl = ref(null);
const rectEl = ref(null);
const menu = ref({ open: false, x: 0, y: 0, items: [] });
const primaryPos = ref({ x: 0, y: 0 });
const paramDisplay = ref(null);

let pre = null;
let post = null;
let sc = null;
let eq = null;
let ref_ = null;
let offs = [];

const RANGE_DB = [3, 6, 12, 30];
const SPEED_MS = [600, 300, 150, 70, 30];
const TILT = [0, 1.5, 3, 4.5, 6];
const AN_RANGE = [60, 90, 120];

function rangeDb() {
  return RANGE_DB[g.displayRange.index] || 12;
}

// -- band creation ----------------------------------------------------------

/**
 * Create a band from a gesture on the display (EqCurve's `onCreateBand`,
 * the background menu, or a sketch). `type` is the shape the EqCurve
 * suggests from where the gesture started (a cut near the edges, a shelf
 * a bit further in, a bell elsewhere), `freq` / `db` the position, `alt`
 * means "make it dynamic" (the gain goes into the dynamic range instead),
 * `fromCurve` means the user dragged the yellow curve itself, in which
 * case the band starts at 0 dB and the ongoing drag sets its gain.
 * Returns the 0-based band index for the EqCurve, or null when all 24
 * slots are in use.
 */
function createAt({ type, freq, db, alt, shift, fromCurve }) {
  const shape = { peak: 0, lowshelf: 1, highpass: 2, highshelf: 3, lowpass: 4, notch: 5, bandpass: 6 }[type] ?? 0;
  const v = { shape, freq, gain: fromCurve ? 0 : db, q: 1, slope: 1 };
  if (alt) {
    v.dynOn = true;
    v.dynRange = fromCurve ? 0 : db;
    v.gain = 0;
  }
  if (alt && shift) v.dynOn = true; // spectral bands are not implemented; make a dynamic one
  const n = createBand(v);
  return n == null ? null : n - 1;
}

// -- context menu -----------------------------------------------------------

function bandMenu(i, e) {
  const b = useBand(i + 1);
  const targets = () => (ui.selected.includes(i + 1) ? ui.selected : [i + 1]);
  const items = [
    { label: 'Copy', hint: targets().length > 1 ? `${targets().length} bands` : '', action: () => (ui.clipboard = targets().map((n) => bandToJson(n))) },
    { label: 'Paste', disabled: !ui.clipboard, action: paste },
    { divider: true },
    { label: b.dynOn.on ? 'Make Static' : 'Make Dynamic', disabled: !b.canDyn, action: () => setBand(i + 1, { dynOn: !b.dynOn.on, dynRange: b.dynOn.on ? 0 : b.gain.plain || -3 }) },
    { label: 'Make Spectral', disabled: true, hint: 'not implemented' },
    { divider: true },
    ...PLACEMENTS.map((p, k) => ({ label: p, checked: b.place.index === k, action: () => targets().forEach((n) => useBand(n).place.setIndex(k)) })),
    { label: 'Reset Placement', action: () => targets().forEach((n) => useBand(n).place.setIndex(0)) },
    { divider: true },
    { label: b.on.on ? 'Bypass band' : 'Enable band', action: () => targets().forEach((n) => useBand(n).on.toggle()) },
    { label: 'Delete', action: () => { targets().forEach((n) => deleteBand(n)); selectBands([]); } },
  ];
  menu.value = { open: true, x: e.clientX, y: e.clientY, items };
}
function bgMenu(e) {
  e.preventDefault();
  const items = [
    { label: 'Copy all bands', action: () => (ui.clipboard = bands.filter((b) => b.on.on).map((b) => bandToJson(b.n))) },
    { label: 'Paste', disabled: !ui.clipboard, action: paste },
    { divider: true },
    { label: 'Add band here', action: () => { const r = eqEl.value.getBoundingClientRect(); createAt({ type: eq.shapeForPosition(e.clientX - r.left, e.clientY - r.top), freq: eq.freqForX(e.clientX - r.left), db: eq.dbForY(e.clientY - r.top) }); } },
    { label: 'Delete all bands', action: () => { bands.forEach((b) => b.on.on && deleteBand(b.n)); selectBands([]); } },
  ];
  menu.value = { open: true, x: e.clientX, y: e.clientY, items };
}
function paste() {
  if (!ui.clipboard) return;
  const created = [];
  for (const v of ui.clipboard) {
    const n = createBand(v);
    if (n == null) break;
    created.push(n);
  }
  if (created.length) selectBands(created, created[0]);
}
function shapeMenuFor(i, kind) {
  const b = useBand(i + 1);
  const r = primaryPos.value;
  const host = eqEl.value.getBoundingClientRect();
  const items =
    kind === 'shape'
      ? SHAPES.map((s, k) => ({ label: s, checked: b.shape.index === k, action: () => b.shape.setIndex(k) }))
      : kind === 'slope'
        ? SLOPE_NAMES.map((s, k) => ({ label: `${s}/oct`, checked: b.slope.index === k, action: () => b.slope.setIndex(k) }))
        : null;
  if (items) menu.value = { open: true, x: host.left + r.x, y: host.top + r.y + 40, items };
  else bandMenu(i, { clientX: host.left + r.x, clientY: host.top + r.y + 40 });
}

// -- spectrum grab ------------------------------------------------------------

const grab = ref({ peaks: [], drag: null });
let grabTimer = null;
let lastPointer = null;
/** True when the pointer is on the spectrum trace or inside its filled area. */
function overSpectrum(p) {
  const spec = g.anPost.on ? post : g.anPre.on ? pre : null;
  if (!spec || !p) return false;
  const v = spec.valueAt(p.freq);
  if (Number.isNaN(v)) return false;
  return p.y >= spec.yForDb(v) - 12;
}
function armGrab(p) {
  clearTimeout(grabTimer);
  if (!ui.spectrumGrab || ui.grab.active || !(g.anPre.on || g.anPost.on)) return;
  if (!overSpectrum(p)) return;
  grabTimer = setTimeout(() => enterGrab(false), 1500);
}
function enterGrab(permanent) {
  const spec = g.anPost.on ? post : pre;
  if (!spec) return;
  ui.grab = { active: true, permanent };
  spec.setFrozen(true);
  eq.setDimmed(true);
  grab.value.peaks = spec.peaks({ minDb: spec.opts.minDb + 20, max: 24 });
}
function leaveGrab() {
  clearTimeout(grabTimer);
  if (!ui.grab.active) return;
  ui.grab = { active: false, permanent: false };
  for (const s of [pre, post]) if (s && !g.anFreeze.on) s.setFrozen(false);
  eq.setDimmed(false);
  grab.value.peaks = [];
  grab.value.drag = null;
}
function onGrabDown(e, p) {
  e.preventDefault();
  e.stopPropagation();
  const n = createBand({ shape: 0, freq: p.freq, gain: 0, q: 2.5 });
  if (n == null) return;
  const b = useBand(n);
  grab.value.drag = { id: e.pointerId, y0: e.clientY, b };
  grabEl.value.setPointerCapture(e.pointerId);
  b.gain.begin();
  selectBands([n], n);
}
function onGrabMove(e) {
  const d = grab.value.drag;
  if (d && e.pointerId === d.id) {
    const dy = e.clientY - d.y0;
    d.b.gain.setPlain(eq.dbForY(eq.yForDb(0) + dy));
    return;
  }
  // A hover grab ends as soon as the pointer leaves the spectrum; a
  // permanent one (click-and-hold / G key) stays until the background is clicked.
  if (!ui.grab.permanent && grabEl.value) {
    const r = grabEl.value.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    if (!overSpectrum({ freq: eq.freqForX(x), y })) leaveGrab();
  }
}
function onGrabUp(e) {
  const d = grab.value.drag;
  if (!d || e.pointerId !== d.id) return;
  d.b.gain.end();
  grab.value.drag = null;
  if (!ui.grab.permanent) leaveGrab();
}
function onGrabBgDown(e) {
  if (e.target === grabEl.value) leaveGrab();
}

// -- EQ sketch -------------------------------------------------------------------

const sketch = ref({ active: false, points: [] });
function sketchAvailable() {
  return ui.sketchArmed || !bands.some((b) => b.on.on);
}
function onSketchDown(e) {
  if (e.button !== 0) return;
  const r = sketchEl.value.getBoundingClientRect();
  sketch.value = { active: true, points: [[e.clientX - r.left, e.clientY - r.top]], id: e.pointerId };
  sketchEl.value.setPointerCapture(e.pointerId);
}
function onSketchMove(e) {
  const s = sketch.value;
  if (!s.active || e.pointerId !== s.id) return;
  const r = sketchEl.value.getBoundingClientRect();
  const x = e.clientX - r.left;
  const y = e.clientY - r.top;
  // Moving back erases what was drawn to the right of the cursor.
  while (s.points.length > 1 && s.points[s.points.length - 1][0] > x) s.points.pop();
  s.points.push([x, y]);
}
function onSketchUp(e) {
  const s = sketch.value;
  if (!s.active || e.pointerId !== s.id) return;
  s.active = false;
  ui.sketchArmed = false;
  fitSketch(s.points);
  s.points = [];
}
/** Turn a drawn path into bands: one per excursion away from 0 dB. */
function fitSketch(points) {
  if (points.length < 6) return;
  const y0 = eq.yForDb(0);
  const dead = 4;
  const segments = [];
  let cur = null;
  for (const [x, y] of points) {
    const sign = y < y0 - dead ? 1 : y > y0 + dead ? -1 : 0;
    if (sign === 0) {
      if (cur) segments.push(cur);
      cur = null;
      continue;
    }
    if (!cur || cur.sign !== sign) {
      if (cur) segments.push(cur);
      cur = { sign, pts: [] };
    }
    cur.pts.push([x, y]);
  }
  if (cur) segments.push(cur);
  const w = eqEl.value.clientWidth;
  const created = [];
  for (const seg of segments) {
    if (seg.pts.length < 2) continue;
    const ext = seg.pts.reduce((m, p) => (seg.sign > 0 ? (p[1] < m[1] ? p : m) : p[1] > m[1] ? p : m));
    const x0 = seg.pts[0][0];
    const x1 = seg.pts[seg.pts.length - 1][0];
    const freq = eq.freqForX(ext[0]);
    const gain = eq.dbForY(ext[1]);
    const bwOct = Math.max(0.2, Math.log2(eq.freqForX(x1) / eq.freqForX(x0)));
    let v;
    if (x0 < w * 0.06 && seg.sign < 0) v = { shape: 2, freq: eq.freqForX(x1), gain: 0, q: 0.707, slope: 3 };
    else if (x1 > w * 0.94 && seg.sign < 0) v = { shape: 4, freq: eq.freqForX(x0), gain: 0, q: 0.707, slope: 3 };
    else if (x0 < w * 0.06) v = { shape: 1, freq: eq.freqForX(x1), gain, q: 0.7 };
    else if (x1 > w * 0.94) v = { shape: 3, freq: eq.freqForX(x0), gain, q: 0.7 };
    else v = { shape: 0, freq, gain, q: Math.max(0.3, Math.min(10, 1.41 / bwOct)) };
    const n = createBand(v);
    if (n == null) break;
    created.push(n);
  }
  if (created.length) selectBands(created, created[created.length - 1]);
}
const sketchPath = computed(() => sketch.value.points.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`).join(' '));

// -- rectangle selection -----------------------------------------------------------

const rect = ref(null);
function onRectDown(e) {
  if (e.button !== 0 || !e.shiftKey) return;
  const r = rectEl.value.getBoundingClientRect();
  rect.value = { x0: e.clientX - r.left, y0: e.clientY - r.top, x1: e.clientX - r.left, y1: e.clientY - r.top, id: e.pointerId };
  rectEl.value.setPointerCapture(e.pointerId);
}
function onRectMove(e) {
  if (!rect.value || e.pointerId !== rect.value.id) return;
  const r = rectEl.value.getBoundingClientRect();
  rect.value.x1 = e.clientX - r.left;
  rect.value.y1 = e.clientY - r.top;
}
function onRectUp(e) {
  if (!rect.value || e.pointerId !== rect.value.id) return;
  const { x0, y0, x1, y1 } = rect.value;
  const [xa, xb] = [Math.min(x0, x1), Math.max(x0, x1)];
  const [ya, yb] = [Math.min(y0, y1), Math.max(y0, y1)];
  const hit = bands
    .filter((b) => b.on.on)
    .filter((b) => {
      const x = eq.xForFreq(b.freq.plain);
      const y = eq.yForDb(b.hasGain ? b.gain.plain : 0);
      return x >= xa && x <= xb && y >= ya && y <= yb;
    })
    .map((b) => b.n);
  rect.value = null;
  if (hit.length) selectBands(hit, hit[0]);
}
const rectStyle = computed(() => {
  const r = rect.value;
  if (!r) return null;
  return { left: `${Math.min(r.x0, r.x1)}px`, top: `${Math.min(r.y0, r.y1)}px`, width: `${Math.abs(r.x1 - r.x0)}px`, height: `${Math.abs(r.y1 - r.y0)}px` };
});

// -- lifecycle -----------------------------------------------------------------------

/**
 * Push the analyzer parameters into the three `Spectrum` instances: dB
 * range, fall-off speed and tilt from the enum indices (the `RANGE_DB` /
 * `SPEED_MS` / `TILT` / `AN_RANGE` tables mirror the parameter labels),
 * freeze, and, per spectrum, subscribe or unsubscribe its stream and fade
 * its layer so an unused spectrum costs nothing on the wire.
 */
function applyAnalyzerSettings() {
  const range = AN_RANGE[g.anRange.index] || 90;
  for (const s of [pre, post, sc]) {
    if (!s) continue;
    s.setDbRange(-range, 6);
    s.setReleaseMs(SPEED_MS[g.anSpeed.index] ?? 150);
    s.setTilt(TILT[g.anTilt.index] ?? 4.5);
    s.setFrozen(g.anFreeze.on);
  }
  useStream('spectrum_pre').subscribe({ enabled: g.anPre.on });
  useStream('spectrum_post').subscribe({ enabled: g.anPost.on });
  if (getClient().hasStream('spectrum_sc')) useStream('spectrum_sc').subscribe({ enabled: g.anSc.on });
  if (preEl.value) preEl.value.style.opacity = g.anPre.on ? '1' : '0';
  if (postEl.value) postEl.value.style.opacity = g.anPost.on ? '1' : '0';
  if (scEl.value) scEl.value.style.opacity = g.anSc.on ? '1' : '0';
}

function updatePrimaryPos() {
  if (ui.primary == null || !eq) return;
  const b = useBand(ui.primary);
  primaryPos.value = { x: eq.xForFreq(b.freq.plain), y: eq.yForDb(b.hasGain ? b.gain.plain : 0) };
}

onMounted(() => {
  const specOpts = { minHz: ui.zoom.min, maxHz: ui.zoom.max, minDb: -90, maxDb: 6, grid: false, slopeDbPerOct: 4.5, textColor: 'rgba(148,163,184,0.45)' };
  // The gray scale on the right is the analyzer's; the yellow one on the left is the EQ's.
  pre = new Spectrum(preEl.value, useStream('spectrum_pre'), { ...specOpts, dbScale: 'right', color: 'rgba(148,163,184,0.55)', fillColor: 'rgba(148,163,184,0.10)', lineWidth: 1 });
  post = new Spectrum(postEl.value, useStream('spectrum_post'), { ...specOpts, dbScale: 'right', color: 'rgba(88,196,255,0.85)', fillColor: 'rgba(88,196,255,0.14)', lineWidth: 1.25 });
  if (getClient().hasStream('spectrum_sc')) {
    sc = new Spectrum(scEl.value, useStream('spectrum_sc'), { ...specOpts, color: 'rgba(255,120,120,0.8)', fillColor: 'rgba(255,120,120,0.06)', lineWidth: 1, fill: false });
  }
  eq = new EqCurve(eqEl.value, {
    sampleRate: sr.value,
    minHz: ui.zoom.min,
    maxHz: ui.zoom.max,
    rangeDb: rangeDb(),
    points: 240,
    nodeRadius: 9,
    gainQ: g.gainQ.param,
    dynGain: (i) => ui.dynGains[i],
    bands: bands.map((b) => ({
      type: b.shape.param,
      freq: b.freq.param,
      gain: b.gain.param,
      q: b.q.param,
      slope: b.slope.param,
      placement: b.place.param,
      enabled: b.on.param,
      dynOn: b.dynOn.param,
      dynRange: b.dynRange.param,
      solo: b.solo.param,
    })),
    onSelect: (sel, primary) => {
      selectBands(sel.map((i) => i + 1), primary == null ? null : primary + 1);
      updatePrimaryPos();
    },
    onHover: (i) => (ui.hover = i == null ? null : i + 1),
    onCreateBand: createAt,
    onBandContextMenu: bandMenu,
    onBandDblClick: (i) => {
      selectBands([i + 1], i + 1);
      requestAnimationFrame(() => paramDisplay.value?.edit?.());
    },
    onCycleShape: (i) => useBand(i + 1).shape.setIndex((useBand(i + 1).shape.index + 1) % SHAPES.length),
    onCycleSlope: (i) => useBand(i + 1).slope.setIndex((useBand(i + 1).slope.index + 1) % SLOPE_NAMES.length),
    onPointer: (p) => {
      ui.hoverFreq = p ? p.freq : null;
      lastPointer = p;
      if (p) armGrab(p);
      else clearTimeout(grabTimer);
    },
  });

  // DSP-reported response as a dashed reference line.
  ref_ = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  ref_.setAttribute('fill', 'none');
  ref_.setAttribute('stroke', 'rgba(255,255,255,0.3)');
  ref_.setAttribute('stroke-dasharray', '3 5');
  eq.svg.insertBefore(ref_, eq.svg.children[1]);
  const curve = useStream('curve');
  offs.push(
    curve.on((d) => {
      const meta = curve.meta;
      const n = d.length;
      let path = '';
      for (let i = 0; i < n; i++) {
        const f = meta.min_hz * Math.pow(meta.max_hz / meta.min_hz, i / (n - 1));
        if (f < ui.zoom.min || f > ui.zoom.max) continue;
        path += (path ? 'L' : 'M') + eq.xForFreq(f).toFixed(1) + ' ' + eq.yForDb(d[i]).toFixed(1);
      }
      ref_.setAttribute('d', path);
    }),
  );
  offs.push(
    useStream('band_dyn').on((d) => {
      ui.dynGains = d;
      if (bands.some((b) => b.isDynamic)) eq.update();
    }),
  );
  if (getClient().hasStream('band_level')) offs.push(useStream('band_level').on((d) => (ui.dynLevels = d)));

  applyAnalyzerSettings();
  // Auto-adjust display range when a band leaves it (manual §3.7).
  offs.push(
    getClient().on('edit', () => {
      if (!ui.autoRange) return;
      const r = rangeDb();
      const maxAbs = bands.filter((b) => b.on.on && b.hasGain).reduce((m, b) => Math.max(m, Math.abs(b.gain.plain), b.isDynamic ? Math.abs(b.gain.plain + b.dynRange.plain) : 0), 0);
      if (maxAbs > r) {
        const next = RANGE_DB.findIndex((x) => x >= maxAbs);
        if (next > g.displayRange.index) g.displayRange.setIndex(next < 0 ? RANGE_DB.length - 1 : next);
      }
    }),
  );
  updatePrimaryPos();
});

watch(() => [g.anPre.on, g.anPost.on, g.anSc?.on, g.anRange.index, g.anSpeed.index, g.anTilt.index, g.anFreeze.on], applyAnalyzerSettings);
watch(() => g.displayRange.index, () => { eq?.setRangeDb(rangeDb()); updatePrimaryPos(); });
watch(
  () => [ui.zoom.min, ui.zoom.max],
  () => {
    eq?.setRange(ui.zoom.min, ui.zoom.max);
    for (const s of [pre, post, sc]) s?.setRange(ui.zoom.min, ui.zoom.max);
    updatePrimaryPos();
  },
);
watch(() => [ui.selected.join(','), ui.primary], () => {
  if (!eq) return;
  eq.selected = new Set(ui.selected.map((n) => n - 1));
  eq.primary = ui.primary == null ? null : ui.primary - 1;
  eq.update();
  updatePrimaryPos();
});
watch(
  () => (ui.primary ? [useBand(ui.primary).freq.norm, useBand(ui.primary).gain.norm, useBand(ui.primary).shape.index] : null),
  updatePrimaryPos,
);
const anySolo = computed(() => bands.some((b) => b.on.on && b.solo.on));
watch(anySolo, (s) => eq?.setDimmed(s || ui.grab.active));

onBeforeUnmount(() => {
  offs.forEach((f) => f());
  pre?.destroy();
  post?.destroy();
  sc?.destroy();
  eq?.destroy();
});

const hoverText = computed(() => {
  if (ui.hover == null) return '';
  const b = useBand(ui.hover);
  const f = g.piano?.on ? noteLabel(b.freq.plain) : b.freq.text;
  return `${SHAPES[b.shape.index]} · ${f}${b.hasGain ? ' · ' + b.gain.text : ''} · Q ${b.q.text}${b.hasSlope ? ' · ' + b.slope.label + '/oct' : ''}`;
});
defineExpose({ enterGrab, leaveGrab });
</script>

<template>
  <div class="relative w-full h-full overflow-hidden bg-gradient-to-b from-ink-900 to-ink-950" @contextmenu="bgMenu">
    <div ref="preEl" class="absolute inset-0 transition-opacity duration-200 pointer-events-none"></div>
    <div ref="postEl" class="absolute inset-0 transition-opacity duration-200 pointer-events-none"></div>
    <div ref="scEl" class="absolute inset-0 transition-opacity duration-200 pointer-events-none"></div>
    <div ref="eqEl" class="absolute inset-0" :class="{ 'opacity-40': g.bypass.on }"></div>

    <!-- shift-drag rectangle selection layer -->
    <div ref="rectEl" class="absolute inset-0" :class="rect ? 'pointer-events-auto' : 'pointer-events-none'" @pointerdown.capture="onRectDown" @pointermove="onRectMove" @pointerup="onRectUp" @pointercancel="onRectUp">
      <div v-if="rectStyle" class="absolute border border-accent/70 bg-accent/10" :style="rectStyle"></div>
    </div>

    <!-- EQ sketch layer -->
    <svg
      v-if="sketchAvailable() || sketch.active"
      ref="sketchEl"
      class="absolute inset-0 w-full h-full cursor-crosshair"
      :class="ui.sketchArmed || sketch.active ? 'pointer-events-auto' : 'pointer-events-none'"
      @pointerdown="onSketchDown"
      @pointermove="onSketchMove"
      @pointerup="onSketchUp"
      @pointercancel="onSketchUp"
    >
      <path :d="sketchPath" fill="none" stroke="#ffd166" stroke-width="2" stroke-dasharray="6 4" />
    </svg>

    <!-- spectrum grab layer -->
    <div
      v-if="ui.grab.active"
      ref="grabEl"
      class="absolute inset-0"
      :class="ui.grab.permanent ? 'ring-2 ring-inset ring-sky-400/60' : ''"
      @pointerdown="onGrabBgDown"
      @pointermove="onGrabMove"
      @pointerup="onGrabUp"
      @pointercancel="onGrabUp"
    >
      <div
        v-for="p in grab.peaks"
        :key="p.freq"
        class="absolute -translate-x-1/2 -translate-y-full flex flex-col items-center cursor-ns-resize"
        :style="{ left: `${p.x}px`, top: `${p.y}px` }"
        @pointerdown="onGrabDown($event, p)"
      >
        <span class="text-[10px] tabular text-slate-100 bg-ink-950/70 px-1 rounded">{{ g.piano?.on ? noteLabel(p.freq) : p.freq >= 1000 ? `${(p.freq / 1000).toFixed(2)}k` : `${p.freq.toFixed(0)}` }}</span>
        <span class="w-2.5 h-2.5 rounded-full bg-white shadow" />
      </div>
      <div class="absolute top-2 left-1/2 -translate-x-1/2 text-[11px] text-sky-300 pointer-events-none">Spectrum Grab · drag a peak to create a band{{ ui.grab.permanent ? ' · click the background to exit' : '' }}</div>
    </div>

    <ParamDisplay
      v-if="ui.primary && ui.showParamDisplay && ui.selected.length === 1 && !ui.grab.active"
      ref="paramDisplay"
      :band="ui.primary"
      :x="primaryPos.x"
      :y="primaryPos.y"
      @menu="(k) => shapeMenuFor(ui.primary - 1, k)"
    />

    <div v-if="hoverText && !ui.selected.length" class="absolute top-2 left-1/2 -translate-x-1/2 text-[11px] text-slate-400 tabular pointer-events-none bg-ink-950/60 px-2 rounded">{{ hoverText }}</div>
    <div v-if="g.bypass.on" class="absolute top-0 left-0 right-0 h-0.5 bg-red-500/80 pointer-events-none"></div>
    <div v-if="!bands.some((b) => b.on.on) && !sketch.active" class="absolute bottom-3 left-1/2 -translate-x-1/2 text-[11px] text-slate-500 pointer-events-none">
      Drag left-to-right to sketch a curve · double-click or drag the yellow line to add a band · Alt for a dynamic band
    </div>
    <ContextMenu :open="menu.open" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu.open = false" />
  </div>
</template>
