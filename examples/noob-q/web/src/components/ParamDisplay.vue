<script setup>
/**
 * The EQ parameter display (manual §3.6): the small pop-up under the
 * primary band's node with bypass, the frequency / gain / Q read-outs,
 * slope (cut shapes), hold-to-solo, shape, the band menu and delete.
 *
 * Props: `band` (Number, required, 1-based), `x` / `y` (Number, the node's
 * position in display pixels; the pop-up is centred under it). Emits
 * `menu(kind)` with `'shape'`, `'slope'` or `'band'` so Analyzer.vue can
 * open the matching context menu at the node (`shape` is declared for
 * symmetry but the shape menu also goes through `menu`).
 *
 * Each value read-out is a tiny control of its own: drag vertically
 * (Shift = fine), mouse wheel (Shift = fine; wheel edits are wrapped in one
 * begin / end gesture that closes 150 ms after the last tick), or
 * double-click to type. Typed values go through the framework's
 * `parseValue`, which understands units and note names ("2k", "A#3",
 * "-3.5"). Tab commits and moves to the next field, Escape cancels.
 * Frequency is shown as a note name when the piano display is on.
 *
 * Exposes `edit()` so a double-click on the node (Analyzer.vue) starts
 * typing the frequency straight away.
 */
import { computed, nextTick, ref } from 'vue';
import { SHAPES, deleteBand, ui, useBand, useGlobals } from '../composables/useVst3WebStratum.js';
import { noteLabel, parseValue } from '../notes.js';

const props = defineProps({ band: { type: Number, required: true }, x: { type: Number, default: 0 }, y: { type: Number, default: 0 } });
const emit = defineEmits(['menu', 'shape']);
const b = computed(() => useBand(props.band));
const g = useGlobals();
const editing = ref(null); // 'freq' | 'gain' | 'q'
const editText = ref('');
const input = ref(null);

const fields = computed(() => {
  const list = [{ key: 'freq', h: b.value.freq, text: g.piano?.on ? noteLabel(b.value.freq.plain) : b.value.freq.text }];
  if (b.value.hasGain) list.push({ key: 'gain', h: b.value.gain, text: b.value.gain.text });
  list.push({ key: 'q', h: b.value.q, text: b.value.q.text });
  return list;
});

let drag = null;
function onDown(e, f) {
  if (e.button !== 0) return;
  e.currentTarget.setPointerCapture(e.pointerId);
  drag = { id: e.pointerId, y: e.clientY, h: f.h, n: f.h.norm };
  f.h.begin();
}
function onMove(e) {
  if (!drag || e.pointerId !== drag.id) return;
  const fine = e.shiftKey ? 0.1 : 1;
  const dy = drag.y - e.clientY;
  drag.y = e.clientY;
  drag.n = Math.max(0, Math.min(1, drag.n + (dy / 200) * fine));
  drag.h.set(drag.n);
}
function onUp(e) {
  if (!drag || e.pointerId !== drag.id) return;
  drag.h.end();
  drag = null;
}
let wheelTimer = null;
/**
 * Wheel over a value: 1 % of the normalized range per notch (0.2 % with
 * Shift). A wheel has no natural start and end, so the first notch opens a
 * begin gesture and a timer closes it 150 ms after the last notch; the
 * host then records the whole scroll as one automation edit.
 */
function onWheel(e, f) {
  e.preventDefault();
  if (!wheelTimer) f.h.begin();
  clearTimeout(wheelTimer);
  f.h.set(f.h.norm + (e.deltaY < 0 ? 1 : -1) * (e.shiftKey ? 0.002 : 0.01));
  wheelTimer = setTimeout(() => {
    wheelTimer = null;
    f.h.end();
  }, 150);
}
async function startEdit(f) {
  editing.value = f.key;
  editText.value = String(+f.h.plain.toFixed(2));
  await nextTick();
  input.value?.focus();
  input.value?.select();
}
/** Parse the typed text into the field being edited; `next` (Tab) moves on to the following field. */
function commit(next) {
  const key = editing.value;
  if (!key) return;
  const f = fields.value.find((x) => x.key === key);
  const v = parseValue(editText.value, f.h);
  if (!Number.isNaN(v)) f.h.setPlain(v);
  editing.value = null;
  if (next) {
    const i = fields.value.findIndex((x) => x.key === key);
    startEdit(fields.value[(i + 1) % fields.value.length]);
  }
}
function solo(on) {
  b.value.solo.setOn(on);
}
defineExpose({ edit: () => startEdit(fields.value[0]) });
</script>

<template>
  <div
    class="absolute z-20 flex items-center gap-1 rounded-lg border border-white/10 bg-ink-800/95 backdrop-blur px-1.5 py-1 text-[11px] shadow-lg shadow-black/40 -translate-x-1/2"
    :style="{ left: `${x}px`, top: `${y + 16}px` }"
    @pointerdown.stop
    @dblclick.stop
  >
    <button class="pd-btn" :class="b.on.on ? 'text-emerald-300' : 'text-red-400'" title="Bypass band (Alt+click node)" @click="b.on.toggle()">⏻</button>
    <template v-for="f in fields" :key="f.key">
      <input
        v-if="editing === f.key"
        ref="input"
        v-model="editText"
        class="w-16 text-center bg-ink-950 border border-accent rounded px-1 outline-none tabular"
        @keydown.enter.stop="commit(false)"
        @keydown.tab.prevent.stop="commit(true)"
        @keydown.escape.stop="editing = null"
        @keydown.stop
        @blur="commit(false)"
      />
      <span
        v-else
        class="tabular cursor-ns-resize px-1 rounded hover:bg-white/[0.08]"
        :title="`${f.h.name}: drag, wheel, or double-click to type`"
        @pointerdown="onDown($event, f)"
        @pointermove="onMove"
        @pointerup="onUp"
        @pointercancel="onUp"
        @wheel="onWheel($event, f)"
        @dblclick="startEdit(f)"
      >
        {{ f.text }}
      </span>
    </template>
    <button v-if="b.isCut" class="pd-btn tabular" title="Slope" @click="emit('menu', 'slope')">{{ b.slope.label }}/oct</button>
    <button class="pd-btn" :class="b.solo.on ? 'text-accent' : ''" title="Hold to solo" @pointerdown="solo(true)" @pointerup="solo(false)" @pointerleave="solo(false)">🎧</button>
    <button class="pd-btn" title="Shape" @click="emit('menu', 'shape')">{{ SHAPES[b.shape.index] }}</button>
    <button class="pd-btn" title="Band menu" @click="emit('menu', 'band')">▾</button>
    <button class="pd-btn text-slate-500 hover:text-red-400" title="Delete band" @click="deleteBand(band); ui.selected = []; ui.primary = null">×</button>
  </div>
</template>

<style scoped>
@reference '../style.css';
.pd-btn {
  @apply px-1 rounded hover:bg-white/[0.08] leading-5;
}
</style>
