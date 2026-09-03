<script setup>
/**
 * Frequency scale under the display (manual §3.8, §12). Two modes:
 *
 * - Normal: logarithmic labels 10 Hz … 20 kHz. Drag up / down to zoom
 *   around the clicked frequency, drag sideways to scroll while zoomed,
 *   double-click to reset. The zoom is written to `ui.zoom`, which
 *   Analyzer.vue applies to the EqCurve and the spectra, so scale and
 *   display always agree.
 * - Piano (`piano` prop): the 88 keys A0–C8 drawn on the same log axis,
 *   with a coloured dot per enabled band. Click a dot to quantise the band
 *   to the nearest note; drag it to move the band while staying on notes.
 *
 * Props: `piano` (Boolean, default false). Parameters: `b<n>_freq` of every
 * enabled band (through `allBands()`), edited with begin / end gestures so
 * the host records the drag as one automation event. Reads `ui.hoverFreq`
 * and `ui.showFreqHover` to draw the frequency-on-hover line.
 *
 * The component measures its own width with a `ResizeObserver` instead of
 * receiving it as a prop: the earlier version attached the observer before
 * the element existed and stayed at a stale width, which showed up as a
 * scale that ended around 70 Hz. Measuring here keeps it in lockstep with
 * the display above whatever the window size.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { allBands, ui } from '../composables/useVst3WebStratum.js';
import { freqToNote, midiToFreq } from '../notes.js';

const props = defineProps({ piano: { type: Boolean, default: false } });
const FULL = { min: 10, max: 30000 };
const LABELS = [10, 20, 30, 50, 100, 200, 300, 500, 1000, 2000, 3000, 5000, 10000, 20000];
const el = ref(null);
// The scale measures itself, so it always matches the display above it.
const width = ref(800);
let ro = null;
onMounted(() => {
  width.value = el.value.clientWidth || 800;
  ro = new ResizeObserver(() => (width.value = el.value?.clientWidth || width.value));
  ro.observe(el.value);
});
onBeforeUnmount(() => ro?.disconnect());

const logMin = computed(() => Math.log(ui.zoom.min));
const logSpan = computed(() => Math.log(ui.zoom.max) - logMin.value);
const xFor = (f) => ((Math.log(f) - logMin.value) / logSpan.value) * width.value;
const freqFor = (x) => Math.exp(logMin.value + (x / width.value) * logSpan.value);
const fmt = (f) => (f >= 1000 ? `${f / 1000}k` : String(f));

const labels = computed(() => LABELS.filter((f) => f >= ui.zoom.min && f <= ui.zoom.max).map((f) => ({ f, x: xFor(f), t: fmt(f) })));
const hoverX = computed(() => (ui.hoverFreq && ui.showFreqHover ? xFor(ui.hoverFreq) : null));

// Piano: keys A0 (midi 21) .. C8 (midi 108).
const keys = computed(() => {
  const out = [];
  for (let m = 21; m <= 108; m++) {
    const f = midiToFreq(m);
    if (f < ui.zoom.min || f > ui.zoom.max) continue;
    const x0 = xFor(midiToFreq(m - 0.5));
    const x1 = xFor(midiToFreq(m + 0.5));
    const black = [1, 3, 6, 8, 10].includes(m % 12);
    out.push({ m, x: x0, w: Math.max(1, x1 - x0), black, c: m % 12 === 0 });
  }
  return out;
});
const bandDots = computed(() =>
  allBands()
    .filter((b) => b.on.on)
    .map((b) => ({ n: b.n, x: xFor(b.freq.plain), color: b.color, note: freqToNote(b.freq.plain) })),
);

let drag = null;
function onDown(e) {
  if (e.button !== 0) return;
  el.value.setPointerCapture(e.pointerId);
  drag = { id: e.pointerId, x: e.clientX, y: e.clientY, f: freqFor(e.offsetX), min: ui.zoom.min, max: ui.zoom.max, moved: false };
}
function onMove(e) {
  if (!drag || e.pointerId !== drag.id) return;
  const dx = e.clientX - drag.x;
  const dy = e.clientY - drag.y;
  if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
  const spanLog = Math.log(drag.max / drag.min);
  // Everything is done in log-frequency, where the scale is linear.
  // Vertical movement zooms: 120 px of drag multiplies the visible span
  // by e, clamped between 1.5× (a musical interval or so) and the full
  // 10 Hz–30 kHz range. The clicked frequency stays under the pointer
  // (`t` is its position within the span). Horizontal movement scrolls by
  // the fraction of the width moved. Finally the window is pushed back
  // inside the full range if the zoom or scroll took it out.
  const zoomFactor = Math.exp(dy / 120);
  const newSpan = Math.max(Math.log(1.5), Math.min(Math.log(FULL.max / FULL.min), spanLog * zoomFactor));
  const t = Math.log(drag.f / drag.min) / spanLog; // position of the anchor within the span
  const shift = (-dx / width.value) * newSpan;
  let lmin = Math.log(drag.f) - t * newSpan + shift;
  let lmax = lmin + newSpan;
  if (lmin < Math.log(FULL.min)) {
    lmax += Math.log(FULL.min) - lmin;
    lmin = Math.log(FULL.min);
  }
  if (lmax > Math.log(FULL.max)) {
    lmin -= lmax - Math.log(FULL.max);
    lmax = Math.log(FULL.max);
  }
  ui.zoom = { min: Math.exp(lmin), max: Math.exp(lmax) };
}
function onUp(e) {
  if (!drag || e.pointerId !== drag.id) return;
  drag = null;
}
function reset() {
  ui.zoom = { ...FULL };
}

// Piano mode: dragging a band dot moves the band from note to note
// (`freqToNote` rounds, `midiToFreq` maps back), inside one begin / end
// gesture; a click without movement just quantises the band to the nearest
// note. The pointer is captured on the scale element, so `onDotMove` /
// `onDotUp` are wired on the scale next to the zoom handlers.
let dotDrag = null;
function onDotDown(e, d) {
  e.stopPropagation();
  const b = allBands()[d.n - 1];
  el.value.setPointerCapture(e.pointerId);
  dotDrag = { id: e.pointerId, b, moved: false, x0: e.clientX };
  b.freq.begin();
}
function onDotMove(e) {
  if (!dotDrag || e.pointerId !== dotDrag.id) return;
  if (Math.abs(e.clientX - dotDrag.x0) > 2) dotDrag.moved = true;
  const f = freqFor(e.offsetX);
  dotDrag.b.freq.setPlain(midiToFreq(freqToNote(f).midi));
}
function onDotUp(e) {
  if (!dotDrag || e.pointerId !== dotDrag.id) return;
  if (!dotDrag.moved) dotDrag.b.freq.setPlain(midiToFreq(freqToNote(dotDrag.b.freq.plain).midi));
  dotDrag.b.freq.end();
  dotDrag = null;
}
</script>

<template>
  <div
    ref="el"
    class="relative h-full w-full select-none cursor-ns-resize text-[10px] text-slate-500"
    :title="piano ? 'Click a dot to quantise, drag to move on notes' : 'Drag up/down to zoom, sideways to scroll, double-click to reset'"
    @pointerdown="onDown"
    @pointermove="onMove($event); onDotMove($event)"
    @pointerup="onUp($event); onDotUp($event)"
    @pointercancel="onUp($event); onDotUp($event)"
    @dblclick="reset"
  >
    <template v-if="!piano">
      <span v-for="l in labels" :key="l.f" class="absolute top-0.5 -translate-x-1/2 tabular" :style="{ left: `${l.x}px` }">{{ l.t }}</span>
    </template>
    <template v-else>
      <div
        v-for="k in keys"
        :key="k.m"
        class="absolute top-0 bottom-0 border-r border-black/50"
        :class="k.black ? 'bg-slate-800 z-10 h-[60%]' : 'bg-slate-200'"
        :style="{ left: `${k.x}px`, width: `${k.w}px` }"
      >
        <span v-if="k.c && !k.black" class="absolute bottom-0 left-0.5 text-[8px] text-slate-600">C{{ k.m / 12 - 1 }}</span>
      </div>
      <div
        v-for="d in bandDots"
        :key="d.n"
        class="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 z-20 w-3.5 h-3.5 rounded-full border border-white/70 cursor-ew-resize text-[8px] text-ink-950 font-bold grid place-items-center"
        :style="{ left: `${d.x}px`, background: d.color }"
        :title="`Band ${d.n}: ${d.note.name} ${d.note.cents > 0 ? '+' : ''}${d.note.cents}`"
        @pointerdown="onDotDown($event, d)"
      >
        {{ d.n }}
      </div>
    </template>
    <div v-if="hoverX != null" class="absolute top-0 bottom-0 w-px bg-accent/60 pointer-events-none" :style="{ left: `${hoverX}px` }">
      <span class="absolute top-0.5 left-1 whitespace-nowrap text-accent tabular bg-ink-950/80 px-1 rounded">
        {{ ui.hoverFreq >= 1000 ? `${(ui.hoverFreq / 1000).toFixed(2)} kHz` : `${ui.hoverFreq.toFixed(1)} Hz` }}
        <span v-if="piano" class="text-slate-400"> · {{ freqToNote(ui.hoverFreq).name }}</span>
      </span>
    </div>
  </div>
</template>
