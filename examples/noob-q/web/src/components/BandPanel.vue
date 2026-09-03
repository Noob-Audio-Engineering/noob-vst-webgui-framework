<script setup>
/**
 * Floating band controls (manual §5 and §6), shown at the bottom of the
 * display while a band is selected. Row one: bypass, shape, slope,
 * frequency, gain with the dynamic-range ring, gain-Q, Q, previous / next
 * band, delete, stereo placement, split. Row two (gain shapes only): the
 * dynamics panel with the on/off switch, range read-out, and, once
 * expanded, threshold (automatic or manual, with the live trigger level),
 * external side-chain, attack, release, audition, and clear.
 *
 * Props: `band` (Number, 1-based band number, `null` hides the panel).
 * Emits: `close` (declared for the parent; deletion clears the selection
 * itself through `selectBands([])`).
 *
 * Parameters: all `b<n>_*` handles of the shown band; when several bands are
 * selected, discrete changes (shape, slope, placement, bypass) apply to every
 * selected band through `forSelected`, while the knobs edit the primary
 * band only. Globals: `gain_q` (the G·Q button), `processing_mode` and
 * `lp_quality` (dynamics are unavailable at the two highest linear-phase
 * resolutions, mirrored in `dynDisabled`), `b1_dyn_sc` presence tells
 * whether the build has a side-chain input. Reads `ui.dynGains` and
 * `ui.dynLevels` (from the `band_dyn` / `band_level` streams, filled by
 * Analyzer.vue) for the live dynamic gain and the trigger level meter.
 *
 * The threshold control is a custom meter with an invisible, rotated
 * `<input type="range">` on top of it so it stays a native, accessible
 * slider; dragging it switches `dyn_auto` off. Collapsing the expanded
 * panel restores automatic threshold and default attack / release, as the
 * manual specifies (§6.2).
 */
import { computed, ref } from 'vue';
import {
  PLACEMENTS,
  SHAPES,
  allBands,
  bandToJson,
  createBand,
  deleteBand,
  getClient,
  hasParam,
  selectBands,
  ui,
  useBand,
  useGlobals,
} from '../composables/useVst3WebStratum.js';
import { Knob } from '@elyerinfox/vst3-web-stratum/vue';
import { ContextMenu } from '@elyerinfox/vst3-web-stratum/vue';

const props = defineProps({ band: { type: Number, default: null } });
const emit = defineEmits(['close']);
const g = useGlobals();
const b = computed(() => (props.band ? useBand(props.band) : null));
const selectedBands = computed(() => ui.selected.map((n) => useBand(n)));
const expanded = ref(false);
const menu = ref({ open: false, x: 0, y: 0, items: [] });
const hasSc = hasParam('b1_dyn_sc');
const level = computed(() => (props.band ? ui.dynLevels[props.band - 1] : -120));
const dynGain = computed(() => (props.band ? ui.dynGains[props.band - 1] : 0));

function openMenu(e, items) {
  const r = e.currentTarget.getBoundingClientRect();
  menu.value = { open: true, x: r.left, y: r.top - 8 - items.length * 28, items };
}
function shapeMenu(e) {
  openMenu(
    e,
    SHAPES.map((s, i) => ({ label: s, checked: b.value.shape.index === i, action: () => forSelected((x) => x.shape.setIndex(i)) })),
  );
}
function slopeMenu(e) {
  openMenu(
    e,
    b.value.slope.labels.map((s, i) => ({ label: `${s}/oct`, checked: b.value.slope.index === i, action: () => forSelected((x) => x.slope.setIndex(i)) })),
  );
}
function placementMenu(e) {
  const colors = ['#ffd166', '#f1f5f9', '#ff5c5c', '#3ddc84', '#58a6ff'];
  openMenu(e, [
    ...PLACEMENTS.map((p, i) => ({ label: p, color: colors[i], checked: b.value.place.index === i, action: () => forSelected((x) => x.place.setIndex(i)) })),
    { divider: true },
    { label: 'Split into L + R', action: () => split(1, 2) },
    { label: 'Split into M + S', action: () => split(3, 4) },
  ]);
}
/** Apply `fn` to every selected band, or to the shown band when nothing else is selected. */
function forSelected(fn) {
  const list = selectedBands.value.length ? selectedBands.value : [b.value];
  for (const x of list) fn(x);
}
/**
 * Split the band into two channel-specific copies (manual §5.4): the
 * existing band gets placement `a`, a new band with identical settings
 * gets placement `c` (1 + 2 = Left + Right, 3 + 4 = Mid + Side), and both
 * end up selected.
 */
function split(a, c) {
  const src = bandToJson(props.band);
  b.value.place.setIndex(a);
  const n = createBand({ ...src, place: c });
  if (n) selectBands([props.band, n], props.band);
}
/** Select the previous (`-1`) or next (`+1`) enabled band, wrapping around. */
function step(dir) {
  const on = allBands().filter((x) => x.on.on).map((x) => x.n);
  if (!on.length) return;
  const i = on.indexOf(props.band);
  const next = on[(i + dir + on.length) % on.length];
  selectBands([next], next);
}
function remove() {
  const list = ui.selected.length ? [...ui.selected] : [props.band];
  for (const n of list) deleteBand(n);
  selectBands([]);
}
function clearDynamics() {
  getClient().setMany([
    [b.value.dynRange.param, b.value.dynRange.toNorm(0)],
    [b.value.dynOn.param, 0],
  ]);
}
function onRangeInput(e) {
  b.value.dynThr.set(Number(e.target.value));
}
function toggleExpanded() {
  expanded.value = !expanded.value;
  if (!expanded.value) {
    // Collapsing reverts to automatic behaviour (manual §6.2).
    getClient().setMany([
      [b.value.dynAuto.param, 1],
      [b.value.dynAttack.param, b.value.dynAttack.spec.default_norm],
      [b.value.dynRelease.param, b.value.dynRelease.spec.default_norm],
    ]);
  }
}
// Threshold marker position (0..1 of the meter height; drawn at the top in automatic mode).
const thrPos = computed(() => (b.value ? b.value.dynThr.norm : 0));
// Live trigger level from the `band_level` stream, -60..0 dB mapped onto the meter.
const lvlPos = computed(() => (b.value ? Math.max(0, Math.min(1, (level.value + 60) / 60)) : 0));
// Linear Phase at Very High / Maximum resolution has no dynamic EQ (manual §6): the FIR is too long to modulate.
const dynDisabled = computed(() => g.mode.index === 2 && g.quality.index >= 3);
</script>

<template>
  <transition name="pop">
    <div
      v-if="b"
      class="absolute left-1/2 bottom-3 -translate-x-1/2 z-20 flex flex-col gap-2 rounded-xl border border-white/10 bg-ink-800/95 backdrop-blur px-3 py-2 shadow-2xl shadow-black/60"
      @pointerdown.stop
      @dblclick.stop
    >
      <div class="flex items-center gap-2.5">
        <button class="btn" :class="b.on.on ? 'text-emerald-300' : 'text-red-400 bg-red-500/10'" title="Bypass band" @click="forSelected((x) => x.on.toggle())">⏻</button>
        <button class="btn min-w-[78px]" title="Shape" @click="shapeMenu">{{ SHAPES[b.shape.index] }}</button>
        <button class="btn min-w-[60px] tabular" :class="{ 'opacity-40': !b.hasSlope }" :disabled="!b.hasSlope" title="Slope" @click="slopeMenu">{{ b.slope.label }}/oct</button>
        <Knob :p="b.freq" :size="54" label="Freq" :color="b.color" />
        <Knob v-if="b.hasGain" :p="b.gain" :ring="b.canDyn ? b.dynRange : null" :size="62" label="Gain" :color="b.color" />
        <button v-if="b.shape.index === 0" class="btn text-[10px]" :class="{ on: g.gainQ.on }" title="Gain-Q interaction (Bell): Q narrows as gain grows" @click="g.gainQ.toggle()">G·Q</button>
        <Knob :p="b.q" :size="54" label="Q" :color="b.color" :disabled="b.slope.index === 0 && b.isCut" />
        <div class="flex flex-col items-center text-[10px] text-slate-500">
          <div class="flex items-center gap-1">
            <button class="btn" title="Previous band" @click="step(-1)">‹</button>
            <span class="w-7 h-7 rounded-full grid place-items-center text-[12px] font-bold text-ink-950" :style="{ background: b.color }">{{ b.n }}</span>
            <button class="btn" title="Next band" @click="step(1)">›</button>
          </div>
          <span v-if="ui.selected.length > 1">{{ ui.selected.length }} selected</span>
        </div>
        <button class="btn min-w-[64px]" title="Stereo placement" @click="placementMenu"><span class="inline-block w-2 h-2 rounded-full mr-1" :style="{ background: b.color }" />{{ PLACEMENTS[b.place.index] }}</button>
        <button class="btn" title="Split into two channel-specific copies" @click="split(1, 2)">✂</button>
        <button class="btn text-slate-500 hover:text-red-400" title="Delete band" @click="remove">×</button>
      </div>

      <div v-if="b.canDyn" class="flex items-center gap-2.5 pt-2 border-t border-white/[0.07]">
        <button class="btn" :class="b.dynOn.on ? 'on' : ''" title="Dynamics on/off" :disabled="dynDisabled" @click="b.dynOn.toggle()">Dyn</button>
        <span class="text-[10px] text-slate-500 w-28">
          <template v-if="dynDisabled">Not at this linear-phase resolution</template>
          <template v-else-if="b.dynOn.on">range <b class="text-slate-200 tabular">{{ b.dynRange.text }}</b> · now <b class="text-amber-300 tabular">{{ dynGain.toFixed(1) }} dB</b></template>
          <template v-else>Turn the ring to make this band dynamic</template>
        </span>
        <button class="btn" :class="{ on: expanded }" title="Expand: custom threshold, attack, release" @click="toggleExpanded">»</button>
        <template v-if="expanded">
          <div class="flex flex-col items-center gap-0.5">
            <div class="relative w-3 h-12 rounded bg-white/[0.06] overflow-hidden" title="Threshold: drag; top = automatic">
              <div class="absolute left-0 right-0 bottom-0 bg-emerald-400/50" :style="{ height: `${lvlPos * 100}%` }" />
              <div class="absolute left-0 right-0 h-0.5 bg-amber-300" :style="{ bottom: `${(b.dynAuto.on ? 1 : thrPos) * 100}%` }" />
            </div>
            <input type="range" min="0" max="1" step="0.005" class="w-14 -rotate-90 origin-center absolute opacity-0 cursor-ns-resize h-12" :value="thrPos" @pointerdown="b.dynThr.begin()" @input="b.dynAuto.setOn(false); onRangeInput($event)" @pointerup="b.dynThr.end()" />
            <button class="text-[9px]" :class="b.dynAuto.on ? 'text-amber-300' : 'text-slate-500'" title="Automatic threshold" @click="b.dynAuto.toggle()">{{ b.dynAuto.on ? 'A' : b.dynThr.text }}</button>
          </div>
          <button v-if="hasSc" class="btn text-[10px]" :class="{ on: b.dynSc.on }" title="Trigger from the external side-chain" @click="b.dynSc.toggle()">Ext SC</button>
          <Knob :p="b.dynAttack" :size="44" label="Attack" />
          <Knob :p="b.dynRelease" :size="44" label="Release" />
          <button class="btn" title="Audition the trigger signal (hold)" @pointerdown="b.solo.setOn(true)" @pointerup="b.solo.setOn(false)" @pointerleave="b.solo.setOn(false)">🎧</button>
        </template>
        <button class="btn text-slate-500 ml-auto" title="Clear dynamics (back to a static band)" @click="clearDynamics">×</button>
      </div>
      <ContextMenu :open="menu.open" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu.open = false" />
    </div>
  </transition>
</template>

<style scoped>
@reference '../style.css';
.btn {
  @apply rounded px-2 py-1 text-[11px] border border-white/10 bg-white/[0.04] text-slate-200 hover:bg-white/[0.09] disabled:opacity-40 disabled:hover:bg-white/[0.04] transition-colors leading-4;
}
.btn.on {
  @apply bg-accent/90 text-ink-950 border-transparent font-semibold;
}
.pop-enter-active,
.pop-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px);
}
</style>
