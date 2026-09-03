<script setup>
/**
 * EQ Match (manual §18), opened from the instance button in the bottom bar.
 *
 * Step 1 — analyse: average the input spectrum (`spectrum_pre`) while
 * "Input" records, and a reference spectrum, either the external side-chain
 * (`spectrum_sc`, when the build has one) recorded the same way, or a
 * spectrum saved earlier with "Save Input As Reference Spectrum". Step 2 —
 * match: choose how many bands (1–16) should approximate the difference,
 * look at the proposal (yellow), and Finish creates the bands.
 *
 * Averaging: each spectrum frame is folded onto a 128-point log grid
 * (20 Hz … Nyquist) by power-averaging the FFT bins within a semitone of
 * each grid point (`toGrid`), then accumulated with a running mean that
 * settles after ~400 frames (`accumulate`).
 *
 * Matching (`fit`): the difference reference − input is centred on its
 * mean (a match is about shape, not level) and smoothed to about 1/6
 * octave; then bands are fitted greedily: take the largest remaining
 * deviation, read its width at half height to get a Q, add a Bell with
 * 90 % of the deviation as gain, subtract that Bell's response
 * (`bandCoefs` / `bandDb` from the framework, the same RBJ model the
 * display uses) from the residual, repeat. Deviations under 0.3 dB stop
 * the loop early.
 *
 * Saved reference spectra live in the plug-in's UI store under
 * `eqmatch.references` as `{ name, data: number[128] }`. Emits `close`.
 * No props. The side-chain spectrum stream is force-subscribed while the
 * panel is open, whatever the analyzer's SC switch says.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { bandCoefs, bandDb } from '@elyerinfox/vst3-web-stratum/components';
import { createBand, hasParam, useVst3WebStratum, useStream } from '../composables/useVst3WebStratum.js';
import { loadReferences, saveReferences } from '../presets.js';

const emit = defineEmits(['close']);
const { manifest } = useVst3WebStratum();
const sr = manifest.value?.meta?.sample_rate || 48000;
const step = ref(1);
const bands = ref(8);
const recordingIn = ref(true);
const recordingRef = ref(false);
const refSource = ref(hasParam('analyzer_sc') ? 'sc' : 'saved');
const saved = ref(loadReferences());
const savedName = ref(saved.value[0]?.name || '');
const framesIn = ref(0);
const framesRef = ref(0);
const canvas = ref(null);

const GRID = 128;
const FMIN = 20;
const FMAX = Math.min(20000, sr / 2);
const gridF = Array.from({ length: GRID }, (_, i) => FMIN * Math.pow(FMAX / FMIN, i / (GRID - 1)));
let accIn = new Float64Array(GRID);
let accRef = new Float64Array(GRID);
let proposal = [];

/**
 * Fold one spectrum frame (dB per FFT bin, bin 0 = DC) onto the 128-point
 * log grid: for each grid point, power-average the bins within ±1
 * semitone so the high end, where many bins fall between grid points, is
 * not under-sampled and the low end, where a grid point can sit between
 * bins, still gets a value. Empty ranges become -120 dB.
 */
function toGrid(d) {
  const bins = d.length;
  // The stream carries bins 0..N/2, so N = (bins - 1) * 2 and each bin is sr / N wide.
  const binHz = sr / ((bins - 1) * 2);
  const out = new Float64Array(GRID);
  for (let i = 0; i < GRID; i++) {
    const f0 = gridF[i] / Math.pow(2, 1 / 12);
    const f1 = gridF[i] * Math.pow(2, 1 / 12);
    const k0 = Math.max(1, Math.floor(f0 / binHz));
    const k1 = Math.min(bins - 1, Math.ceil(f1 / binHz));
    let sum = 0;
    let n = 0;
    for (let k = k0; k <= k1; k++) {
      sum += Math.pow(10, d[k] / 10);
      n++;
    }
    out[i] = n ? 10 * Math.log10(sum / n + 1e-12) : -120;
  }
  return out;
}
/**
 * Fold a new frame into a running average of grid spectra. The weight
 * 1 / min(count + 1, 400) makes the first frames converge quickly and then
 * turns into a slow exponential average, so a long recording keeps
 * tracking the source instead of freezing on its first seconds.
 */
function accumulate(acc, count, d) {
  const g = toGrid(d);
  for (let i = 0; i < GRID; i++) acc[i] += (g[i] - acc[i]) / Math.min(count + 1, 400);
}

let offIn = null;
let offSc = null;
onMounted(() => {
  const pre = useStream('spectrum_pre');
  offIn = pre.on((d) => {
    if (!recordingIn.value || step.value !== 1) return;
    accumulate(accIn, framesIn.value, d);
    framesIn.value++;
    draw();
  });
  if (hasParam('analyzer_sc')) {
    const sc = useStream('spectrum_sc');
    sc.subscribe({ enabled: true });
    offSc = sc.on((d) => {
      if (!recordingRef.value || refSource.value !== 'sc' || step.value !== 1) return;
      accumulate(accRef, framesRef.value, d);
      framesRef.value++;
      draw();
    });
  }
  draw();
});
onBeforeUnmount(() => {
  offIn?.();
  offSc?.();
});

const haveIn = computed(() => framesIn.value > 10);
const haveRef = computed(() => (refSource.value === 'sc' ? framesRef.value > 10 : !!savedName.value));
const canMatch = computed(() => haveIn.value && haveRef.value);

function refCurve() {
  if (refSource.value === 'sc') return accRef;
  const s = saved.value.find((x) => x.name === savedName.value);
  return s ? Float64Array.from(s.data) : accRef;
}
function difference() {
  const r = refCurve();
  const diff = new Float64Array(GRID);
  for (let i = 0; i < GRID; i++) diff[i] = r[i] - accIn[i];
  // Normalise around the mean so the match is about shape, not level.
  const mean = diff.reduce((a, b) => a + b, 0) / GRID;
  for (let i = 0; i < GRID; i++) diff[i] -= mean;
  // 1/6 octave smoothing (3 grid points each side at 128 pts over ~10 oct).
  const out = new Float64Array(GRID);
  for (let i = 0; i < GRID; i++) {
    let s = 0;
    let n = 0;
    for (let k = -3; k <= 3; k++) {
      const j = i + k;
      if (j < 0 || j >= GRID) continue;
      s += diff[j];
      n++;
    }
    out[i] = s / n;
  }
  return out;
}
function fit(n) {
  const target = difference();
  const residual = Float64Array.from(target);
  const out = [];
  for (let b = 0; b < n; b++) {
    let best = 0;
    for (let i = 1; i < GRID - 1; i++) if (Math.abs(residual[i]) > Math.abs(residual[best])) best = i;
    const gain = residual[best];
    if (Math.abs(gain) < 0.3) break;
    // Width where the deviation falls to half: gives Q.
    let lo = best;
    let hi = best;
    while (lo > 0 && Math.sign(residual[lo]) === Math.sign(gain) && Math.abs(residual[lo]) > Math.abs(gain) / 2) lo--;
    while (hi < GRID - 1 && Math.sign(residual[hi]) === Math.sign(gain) && Math.abs(residual[hi]) > Math.abs(gain) / 2) hi++;
    const bwOct = Math.max(0.15, Math.log2(gridF[hi] / gridF[lo]));
    const q = Math.max(0.3, Math.min(12, 1.41 / bwOct));
    const freq = gridF[best];
    const g = Math.max(-18, Math.min(18, gain * 0.9));
    out.push({ shape: 0, freq, gain: g, q });
    const coefs = bandCoefs('peak', freq, g, q, 1, sr);
    for (let i = 0; i < GRID; i++) residual[i] -= bandDb(coefs, gridF[i], sr);
  }
  return out;
}
function match() {
  proposal = fit(bands.value);
  step.value = 2;
  draw();
}
function finish() {
  for (const p of proposal) if (createBand(p) == null) break;
  emit('close');
}
function saveReference() {
  if (!haveIn.value) return;
  const name = window.prompt('Save the current input spectrum as reference:', `Reference ${saved.value.length + 1}`);
  if (!name) return;
  const list = saved.value.filter((s) => s.name !== name);
  list.push({ name, data: Array.from(accIn) });
  saved.value = list;
  savedName.value = name;
  refSource.value = 'saved';
  saveReferences(list);
}
function draw() {
  const c = canvas.value;
  if (!c) return;
  const w = (c.width = c.clientWidth * (window.devicePixelRatio || 1));
  const h = (c.height = c.clientHeight * (window.devicePixelRatio || 1));
  const ctx = c.getContext('2d');
  ctx.clearRect(0, 0, w, h);
  const y0 = h / 2;
  const yFor = (db) => y0 - (db / 24) * (h / 2);
  ctx.strokeStyle = 'rgba(255,255,255,0.12)';
  ctx.beginPath();
  ctx.moveTo(0, y0);
  ctx.lineTo(w, y0);
  ctx.stroke();
  const line = (arr, color, width) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    ctx.beginPath();
    for (let i = 0; i < GRID; i++) {
      const x = (i / (GRID - 1)) * w;
      const y = yFor(arr[i]);
      if (i) ctx.lineTo(x, y);
      else ctx.moveTo(x, y);
    }
    ctx.stroke();
  };
  if (haveIn.value) {
    const mean = accIn.reduce((a, b) => a + b, 0) / GRID;
    line(accIn.map((v) => v - mean), 'rgba(148,163,184,0.7)', 1);
  }
  if (haveRef.value) {
    const r = refCurve();
    const mean = r.reduce((a, b) => a + b, 0) / GRID;
    line(r.map((v) => v - mean), 'rgba(255,92,92,0.8)', 1);
  }
  if (canMatch.value) line(difference(), '#fff', 2);
  if (step.value === 2 && proposal.length) {
    const sum = new Float64Array(GRID);
    for (const p of proposal) {
      const coefs = bandCoefs('peak', p.freq, p.gain, p.q, 1, sr);
      for (let i = 0; i < GRID; i++) sum[i] += bandDb(coefs, gridF[i], sr);
    }
    line(sum, '#ffd166', 2);
  }
}
</script>

<template>
  <div class="w-[520px] p-3 flex flex-col gap-2 text-[12px]">
    <div class="flex items-center gap-2">
      <span class="font-semibold">EQ Match</span>
      <span class="text-slate-500">{{ step === 1 ? 'Step 1 · choose reference' : 'Step 2 · match' }}</span>
      <button class="ml-auto text-slate-500 hover:text-slate-200" @click="emit('close')">×</button>
    </div>
    <canvas ref="canvas" class="w-full h-[150px] rounded bg-ink-950/60"></canvas>
    <template v-if="step === 1">
      <div class="flex items-center gap-2">
        <button class="chip" :class="{ on: recordingIn }" @click="recordingIn = !recordingIn">{{ recordingIn ? '⏸ Input' : '● Input' }}</button>
        <span class="text-slate-500 tabular">{{ framesIn }} frames</span>
        <span class="mx-2 text-slate-600">|</span>
        <select v-model="refSource" class="sel">
          <option v-if="hasParam('analyzer_sc')" value="sc">Reference: Sidechain</option>
          <option value="saved">Reference: saved spectrum</option>
        </select>
        <select v-if="refSource === 'saved'" v-model="savedName" class="sel">
          <option v-for="s in saved" :key="s.name" :value="s.name">{{ s.name }}</option>
          <option v-if="!saved.length" value="">(No Saved Reference Spectrums)</option>
        </select>
        <button v-if="refSource === 'sc'" class="chip" :class="{ on: recordingRef }" @click="recordingRef = !recordingRef">{{ recordingRef ? '⏸ Ref' : '● Ref' }}</button>
      </div>
      <div class="flex items-center gap-2">
        <button class="chip" :disabled="!haveIn" @click="saveReference">Save Input As Reference Spectrum…</button>
        <span v-if="!haveIn" class="text-amber-300">Waiting for input audio…</span>
        <button class="chip on ml-auto" :disabled="!canMatch" @click="match">Match ›</button>
      </div>
    </template>
    <template v-else>
      <div class="flex items-center gap-3">
        <span class="text-slate-500">Number of bands</span>
        <input type="range" min="1" max="16" step="1" v-model.number="bands" class="flex-1 accent-[var(--color-accent)]" @input="proposal = fit(bands); draw()" />
        <span class="tabular w-6">{{ bands }}</span>
      </div>
      <div class="flex items-center gap-2">
        <button class="chip" @click="step = 1; draw()">‹ Analyze</button>
        <span class="text-slate-500">{{ proposal.length }} band{{ proposal.length === 1 ? '' : 's' }} proposed (yellow)</span>
        <button class="chip on ml-auto" @click="finish">Finish</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
@reference '../style.css';
.chip {
  @apply rounded px-2 py-0.5 text-[11px] border border-white/10 bg-white/[0.04] text-slate-300 cursor-pointer hover:bg-white/[0.08] transition-colors disabled:opacity-40;
}
.chip.on {
  @apply bg-accent/90 text-ink-950 border-transparent font-semibold;
}
.sel {
  @apply rounded bg-ink-700 border border-white/10 px-2 py-1 text-[11px];
}
</style>
