<script setup>
/**
 * A right-click / drop-down menu at a screen position. Items: `{ label, action, checked, disabled, divider, hint, color }`.
 *
 * Usage:
 *
 *   <ContextMenu :open="menu.open" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu.open = false" />
 *   // on contextmenu: menu = { open: true, x: e.clientX, y: e.clientY, items: [...] }
 *
 * Props:
 * - `open` (boolean, default false).
 * - `x`, `y` (number, default 0): requested top-left corner in viewport px;
 *   clamped so the menu stays on screen with a 4 px margin.
 * - `items` (array, default []): each entry is one of
 *   - `{ divider: true }`: a separator line;
 *   - `{ label, action?, checked?, disabled?, hint?, color? }`: a row.
 *     `label` is the text; `action()` runs on click; `checked` shows a tick
 *     in the left column; `disabled` greys the row out and ignores clicks;
 *     `hint` is small dim text on the right (a shortcut, a note);
 *     `color` draws a swatch dot before the label (band colours).
 *
 * Emits: `close` after an item runs, on a pointerdown outside the menu, or
 * on Escape.
 *
 * Behaviour: teleported to `body` at `z-index: 50`, positioned `fixed`,
 * re-placed on the frame after `open` / `x` / `y` change. Rows are real
 * `<button>`s, so they are keyboard-focusable. The browser context menu is
 * suppressed on the menu itself.
 *
 * CSS variables honoured: `--vst3-web-stratum-panel`, `--vst3-web-stratum-border`,
 * `--vst3-web-stratum-text`, `--vst3-web-stratum-text-dim` (hints), `--vst3-web-stratum-accent`
 * (tick).
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

const props = defineProps({
  open: { type: Boolean, default: false },
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
  items: { type: Array, default: () => [] },
});
const emit = defineEmits(['close']);
const el = ref(null);
const pos = ref({ left: 0, top: 0 });

function place() {
  const w = el.value?.offsetWidth || 200;
  const h = el.value?.offsetHeight || 200;
  pos.value = {
    left: Math.max(4, Math.min(window.innerWidth - w - 4, props.x)),
    top: Math.max(4, Math.min(window.innerHeight - h - 4, props.y)),
  };
}
function onDocDown(e) {
  if (props.open && !el.value?.contains(e.target)) emit('close');
}
function onKey(e) {
  if (props.open && e.key === 'Escape') emit('close');
}
function run(item) {
  if (item.disabled || item.divider) return;
  item.action?.();
  emit('close');
}
watch(
  () => [props.open, props.x, props.y],
  async () => {
    if (props.open) {
      await new Promise((r) => requestAnimationFrame(r));
      place();
    }
  },
);
onMounted(() => {
  document.addEventListener('pointerdown', onDocDown, true);
  document.addEventListener('keydown', onKey);
});
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocDown, true);
  document.removeEventListener('keydown', onKey);
});
const style = computed(() => ({ left: `${pos.value.left}px`, top: `${pos.value.top}px` }));
</script>

<template>
  <Teleport to="body">
    <div v-if="open" ref="el" class="cm" :style="style" @contextmenu.prevent>
      <template v-for="(it, i) in items" :key="i">
        <div v-if="it.divider" class="cm-div" />
        <button v-else class="cm-item" :disabled="it.disabled" @click="run(it)">
          <span class="cm-check">{{ it.checked ? '✓' : '' }}</span>
          <span v-if="it.color" class="cm-dot" :style="{ background: it.color }" />
          <span class="cm-label">{{ it.label }}</span>
          <span v-if="it.hint" class="cm-hint">{{ it.hint }}</span>
        </button>
      </template>
    </div>
  </Teleport>
</template>

<style scoped>
.cm {
  position: fixed;
  z-index: 50;
  min-width: 180px;
  padding: 4px 0;
  border-radius: 8px;
  border: 1px solid var(--vst3-web-stratum-border, rgba(255, 255, 255, 0.1));
  background: var(--vst3-web-stratum-panel, rgba(24, 29, 39, 0.96));
  backdrop-filter: blur(8px);
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.5);
  color: var(--vst3-web-stratum-text, #e2e8f0);
  font: 12px system-ui, -apple-system, 'Segoe UI', sans-serif;
}
.cm-div {
  margin: 4px 0;
  border-top: 1px solid var(--vst3-web-stratum-border, rgba(255, 255, 255, 0.1));
}
.cm-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 12px;
  text-align: left;
  background: none;
  border: 0;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.cm-item:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.08);
}
.cm-item:disabled {
  opacity: 0.4;
  cursor: default;
}
.cm-check {
  width: 12px;
  color: var(--vst3-web-stratum-accent, #5ac8fa);
}
.cm-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}
.cm-label {
  flex: 1;
}
.cm-hint {
  font-size: 10px;
  color: var(--vst3-web-stratum-text-dim, #64748b);
}
</style>
