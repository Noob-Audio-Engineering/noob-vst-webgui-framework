<script setup>
/**
 * Rotary control bound to a reactive `useParam()` handle: vertical drag
 * (Shift = fine), wheel, double-click for text entry, Ctrl/Cmd+click resets,
 * arrow keys. An optional `ring` handle draws a second, bipolar value around
 * the knob. Themed through CSS variables (`--noob-vst-webgui-framework-accent`, `--noob-vst-webgui-framework-text`).
 *
 * Usage:
 *
 *   <Knob :p="useParam('cutoff')" />
 *   <Knob :p="gain" :ring="dynRange" label="Gain" :size="64" />
 *
 * Props:
 * - `p` (ParamHandle, required): the handle to control; `norm`, `text`,
 *   `name`, `isDiscrete`, `isBipolar`, `labels` are read from it.
 * - `ring` (ParamHandle, default null): a second handle drawn as a thinner
 *   bipolar arc outside the main track (a dynamic range around a gain).
 * - `size` (number, default 52): rendered width and height in CSS px.
 * - `label` (string, default null): caption; `p.name` when null.
 * - `bipolar` (boolean, default null): draw the value arc from the centre
 *   instead of the left end; `p.isBipolar` when null.
 * - `color` (string, default null): stroke of the value arc; the
 *   `--noob-vst-webgui-framework-accent` variable (fallback `#5ac8fa`) when null.
 * - `ringColor` (string, default '#ff5c5c'): stroke of the ring arc.
 * - `showValue` (boolean, default true): show the formatted value under the
 *   knob (hidden while the text field is open).
 * - `sensitivity` (number, default 180): pixels of vertical drag for a full
 *   sweep; Shift divides the speed by ten.
 * - `disabled` (boolean, default false): ignore input and dim the control.
 *
 * Emits: nothing; every change goes through the handle (and so to the
 * plug-in and any other window).
 *
 * Pointer: left-button drag changes `p` with pointer capture, wrapped in
 * `p.begin()` / `p.end()` so the host records one gesture. Alt + drag also
 * moves `ring` in the opposite direction. Ctrl/Cmd + click resets `p`.
 * Wheel steps by 2 % (Shift 0.2 %, discrete parameters one step) and
 * coalesces into one gesture that ends 150 ms after the last tick.
 * Double-click opens the text field.
 *
 * Keyboard (the control is focusable, `role="slider"`): Arrow keys step by
 * 1 % (Shift 10 %, discrete parameters one step), Home / End go to the
 * ends, Backspace / Delete reset to the default, Enter opens the text
 * field. In the field: Enter commits, Escape cancels, blur commits.
 * Typed text is parsed with `parseValue()` (`1k`, `A4`, `50%`, `2x`,
 * `250ms`); for enumerations a label prefix is matched.
 *
 * CSS variables honoured: `--noob-vst-webgui-framework-accent` (arc, focus ring, field
 * border), `--noob-vst-webgui-framework-text` (pointer line, value), `--noob-vst-webgui-framework-text-dim`
 * (label), `--noob-vst-webgui-framework-bg` (text field background).
 */
import { computed, nextTick, ref } from 'vue';
import { parseValue } from '../values.js';

const props = defineProps({
  p: { type: Object, required: true },
  ring: { type: Object, default: null },
  size: { type: Number, default: 52 },
  label: { type: String, default: null },
  bipolar: { type: Boolean, default: null },
  color: { type: String, default: null },
  ringColor: { type: String, default: '#ff5c5c' },
  showValue: { type: Boolean, default: true },
  sensitivity: { type: Number, default: 180 },
  disabled: { type: Boolean, default: false },
});

const SWEEP = 270;
const R = 38;
const R_RING = 47;

function polar(r, deg) {
  const a = ((deg - 90) * Math.PI) / 180;
  return [50 + r * Math.cos(a), 50 + r * Math.sin(a)];
}
function arc(r, a0, a1) {
  if (a1 < a0) [a0, a1] = [a1, a0];
  if (a1 - a0 < 0.01) return '';
  const [x0, y0] = polar(r, a0);
  const [x1, y1] = polar(r, a1);
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}

const isBipolar = computed(() => (props.bipolar == null ? props.p.isBipolar : props.bipolar));
const angle = computed(() => -SWEEP / 2 + SWEEP * props.p.norm);
const trackPath = arc(R, -SWEEP / 2, SWEEP / 2);
const ringTrack = arc(R_RING, -SWEEP / 2, SWEEP / 2);
const valuePath = computed(() => (isBipolar.value ? arc(R, 0, angle.value) : arc(R, -SWEEP / 2, angle.value)));
const ringAngle = computed(() => (props.ring ? -SWEEP / 2 + SWEEP * props.ring.norm : 0));
const ringPath = computed(() => (props.ring ? arc(R_RING, 0, ringAngle.value) : ''));
const stroke = computed(() => props.color || 'var(--noob-vst-webgui-framework-accent, #5ac8fa)');
const el = ref(null);
const editing = ref(false);
const editText = ref('');
const input = ref(null);

let drag = null;
let wheelTimer = null;

function onDown(e) {
  if (props.disabled || e.button !== 0) return;
  e.preventDefault();
  if (e.ctrlKey || e.metaKey) {
    props.p.reset();
    return;
  }
  el.value.focus();
  el.value.setPointerCapture(e.pointerId);
  drag = { y: e.clientY, n: props.p.norm, id: e.pointerId, ringN: props.ring ? props.ring.norm : 0, alt: e.altKey };
  props.p.begin();
  if (props.ring && drag.alt) props.ring.begin();
}
function onMove(e) {
  if (!drag || e.pointerId !== drag.id) return;
  const fine = e.shiftKey ? 0.1 : 1;
  const dy = drag.y - e.clientY;
  drag.y = e.clientY;
  const step = (dy / props.sensitivity) * fine;
  drag.n = Math.max(0, Math.min(1, drag.n + step));
  if (props.p.isDiscrete) {
    const last = props.p.spec.steps - 1;
    const snapped = Math.round(drag.n * last) / last;
    if (snapped !== props.p.norm) props.p.set(snapped);
  } else {
    props.p.set(drag.n);
  }
  if (props.ring && drag.alt) {
    drag.ringN = Math.max(0, Math.min(1, drag.ringN - step));
    props.ring.set(drag.ringN);
  }
}
function onUp(e) {
  if (!drag || e.pointerId !== drag.id) return;
  const wasAlt = drag.alt;
  drag = null;
  props.p.end();
  if (props.ring && wasAlt) props.ring.end();
}
function onWheel(e) {
  if (props.disabled) return;
  e.preventDefault();
  const step = props.p.isDiscrete ? 1 / (props.p.spec.steps - 1) : e.shiftKey ? 0.002 : 0.02;
  if (!wheelTimer) props.p.begin();
  clearTimeout(wheelTimer);
  props.p.set(props.p.norm + (e.deltaY < 0 ? step : -step));
  wheelTimer = setTimeout(() => {
    wheelTimer = null;
    props.p.end();
  }, 150);
}
function onKey(e) {
  const p = props.p;
  if (editing.value) return;
  const step = p.isDiscrete ? 1 / (p.spec.steps - 1) : e.shiftKey ? 0.1 : 0.01;
  let n = null;
  switch (e.key) {
    case 'ArrowUp':
    case 'ArrowRight':
      n = p.norm + step;
      break;
    case 'ArrowDown':
    case 'ArrowLeft':
      n = p.norm - step;
      break;
    case 'Home':
      n = 0;
      break;
    case 'End':
      n = 1;
      break;
    case 'Backspace':
    case 'Delete':
      n = p.spec.default_norm;
      break;
    case 'Enter':
      startEdit();
      return;
    default:
      return;
  }
  e.preventDefault();
  e.stopPropagation();
  p.set(n);
}
async function startEdit() {
  if (props.disabled) return;
  editing.value = true;
  editText.value = props.p.isDiscrete ? props.p.label || String(props.p.plain) : String(+props.p.plain.toFixed(3));
  await nextTick();
  input.value?.focus();
  input.value?.select();
}
function commitEdit() {
  if (!editing.value) return;
  editing.value = false;
  const p = props.p;
  if (p.labels.length) {
    const i = p.labels.findIndex((l) => l.toLowerCase().startsWith(editText.value.trim().toLowerCase()));
    if (i >= 0) p.setIndex(i);
    return;
  }
  const v = parseValue(editText.value, p);
  if (!Number.isNaN(v)) p.setPlain(v);
}
</script>

<template>
  <div
    ref="el"
    class="sk"
    :class="{ disabled }"
    tabindex="0"
    role="slider"
    :aria-label="label ?? p.name"
    :aria-valuetext="p.text"
    :aria-valuenow="p.norm"
    aria-valuemin="0"
    aria-valuemax="1"
    :title="`${p.name}: ${p.text}`"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
    @pointercancel="onUp"
    @dblclick.prevent="startEdit"
    @wheel="onWheel"
    @keydown="onKey"
  >
    <svg :width="size" :height="size" viewBox="0 0 100 100" class="sk-svg">
      <template v-if="ring">
        <path :d="ringTrack" fill="none" stroke="rgba(255,255,255,0.08)" stroke-width="5" stroke-linecap="round" />
        <path :d="ringPath" fill="none" :stroke="ringColor" stroke-width="5" stroke-linecap="round" />
      </template>
      <circle cx="50" cy="50" r="27" fill="rgba(255,255,255,0.05)" />
      <path :d="trackPath" fill="none" stroke="rgba(255,255,255,0.12)" stroke-width="7" stroke-linecap="round" />
      <path :d="valuePath" fill="none" :stroke="stroke" stroke-width="7" stroke-linecap="round" />
      <line x1="50" y1="26" x2="50" y2="35" stroke="var(--noob-vst-webgui-framework-text, #e2e8f0)" stroke-width="4" stroke-linecap="round" :transform="`rotate(${angle.toFixed(2)} 50 50)`" />
    </svg>
    <input
      v-if="editing"
      ref="input"
      v-model="editText"
      class="sk-input"
      @keydown.enter.stop="commitEdit"
      @keydown.escape.stop="editing = false"
      @keydown.stop
      @blur="commitEdit"
      @pointerdown.stop
      @dblclick.stop
    />
    <div v-else-if="showValue" class="sk-value">{{ p.text }}</div>
    <div class="sk-label">{{ label ?? p.name }}</div>
  </div>
</template>

<style scoped>
.sk {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1px;
  user-select: none;
  -webkit-user-select: none;
  outline: none;
  border-radius: 6px;
  cursor: ns-resize;
  color: var(--noob-vst-webgui-framework-text, #e2e8f0);
  font: 11px/1.2 system-ui, -apple-system, 'Segoe UI', sans-serif;
}
.sk.disabled {
  opacity: 0.4;
  cursor: default;
}
.sk:focus-visible {
  box-shadow: 0 0 0 2px var(--noob-vst-webgui-framework-accent, #5ac8fa);
}
.sk-svg {
  display: block;
  overflow: visible;
}
.sk-input {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  width: 64px;
  text-align: center;
  font: 11px system-ui, sans-serif;
  background: var(--noob-vst-webgui-framework-bg, #0d1016);
  color: inherit;
  border: 1px solid var(--noob-vst-webgui-framework-accent, #5ac8fa);
  border-radius: 4px;
  padding: 2px 4px;
  outline: none;
  font-variant-numeric: tabular-nums;
}
.sk-value {
  font-variant-numeric: tabular-nums;
  line-height: 1.1;
}
.sk-label {
  font-size: 9.5px;
  line-height: 1.1;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--noob-vst-webgui-framework-text-dim, #64748b);
}
</style>
