<script setup>
/**
 * A panel anchored to a trigger element. Closes on outside click or Escape.
 * Themed through CSS variables (`--noob-vst-webgui-framework-panel`, `--noob-vst-webgui-framework-border`).
 *
 * Usage:
 *
 *   <button ref="btn" @click="open = !open">Analyzer</button>
 *   <Popover :open="open" :anchor="btn" placement="top" align="end" title="Analyzer" @close="open = false">
 *     <AnalyzerPanel />
 *   </Popover>
 *
 * Props:
 * - `open` (boolean, default false): shown or not; the content is not
 *   rendered while closed.
 * - `anchor` (Element, default null): the element to attach to (a template
 *   ref). Without it the panel sits at the last computed position.
 * - `placement` ('top' | 'bottom', default 'top'): above or below the anchor,
 *   6 px away.
 * - `align` ('start' | 'center' | 'end', default 'start'): horizontal
 *   alignment with the anchor's left edge, centre or right edge.
 * - `width` (number, default 0): fixed width in px; 0 lets the content size
 *   the panel.
 * - `title` (string, default ''): small uppercase heading above the slot.
 *
 * Emits: `close` on a pointerdown outside the panel and the anchor, or on
 * Escape. The parent owns `open`, so it decides whether to honour it.
 *
 * Slots: default, the panel content.
 *
 * Behaviour: teleported to `body` at `z-index: 40`, positioned `fixed` and
 * clamped to the viewport with a 6 px margin. The position is computed on
 * the frame after opening and again on window resize; a short fade / slide
 * transition plays on open and close. The context menu is suppressed
 * inside it so right-click on the panel does not show the browser menu.
 *
 * CSS variables honoured: `--noob-vst-webgui-framework-panel` (background), `--noob-vst-webgui-framework-border`,
 * `--noob-vst-webgui-framework-text`, `--noob-vst-webgui-framework-text-dim` (title).
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

const props = defineProps({
  open: { type: Boolean, default: false },
  anchor: { type: Object, default: null },
  placement: { type: String, default: 'top' }, // top | bottom
  align: { type: String, default: 'start' }, // start | center | end
  width: { type: Number, default: 0 },
  title: { type: String, default: '' },
});
const emit = defineEmits(['close']);
const el = ref(null);
const pos = ref({ left: 0, top: 0 });

function place() {
  const a = props.anchor?.getBoundingClientRect?.();
  if (!a || !el.value) return;
  const r = el.value.getBoundingClientRect();
  const w = r.width || props.width || 240;
  const h = r.height || 200;
  let left = props.align === 'center' ? a.left + a.width / 2 - w / 2 : props.align === 'end' ? a.right - w : a.left;
  let top = props.placement === 'top' ? a.top - h - 6 : a.bottom + 6;
  left = Math.max(6, Math.min(window.innerWidth - w - 6, left));
  top = Math.max(6, Math.min(window.innerHeight - h - 6, top));
  pos.value = { left, top };
}
function onDocDown(e) {
  if (!props.open) return;
  if (el.value?.contains(e.target)) return;
  if (props.anchor && props.anchor.contains && props.anchor.contains(e.target)) return;
  emit('close');
}
function onKey(e) {
  if (props.open && e.key === 'Escape') emit('close');
}
watch(
  () => props.open,
  async (o) => {
    if (o) {
      await new Promise((r) => requestAnimationFrame(r));
      place();
    }
  },
);
onMounted(() => {
  document.addEventListener('pointerdown', onDocDown, true);
  document.addEventListener('keydown', onKey);
  window.addEventListener('resize', place);
});
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocDown, true);
  document.removeEventListener('keydown', onKey);
  window.removeEventListener('resize', place);
});
const style = computed(() => ({ left: `${pos.value.left}px`, top: `${pos.value.top}px`, width: props.width ? `${props.width}px` : undefined }));
</script>

<template>
  <Teleport to="body">
    <transition name="sp-pop">
      <div v-if="open" ref="el" class="sp" :style="style" @contextmenu.prevent>
        <div v-if="title" class="sp-title">{{ title }}</div>
        <slot />
      </div>
    </transition>
  </Teleport>
</template>

<style scoped>
.sp {
  position: fixed;
  z-index: 40;
  border-radius: 12px;
  border: 1px solid var(--noob-vst-webgui-framework-border, rgba(255, 255, 255, 0.1));
  background: var(--noob-vst-webgui-framework-panel, rgba(24, 29, 39, 0.96));
  backdrop-filter: blur(8px);
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.5);
  color: var(--noob-vst-webgui-framework-text, #e2e8f0);
  font: 12px system-ui, -apple-system, 'Segoe UI', sans-serif;
}
.sp-title {
  padding: 8px 12px 4px;
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: var(--noob-vst-webgui-framework-text-dim, #64748b);
}
.sp-pop-enter-active,
.sp-pop-leave-active {
  transition: opacity 0.1s ease, transform 0.1s ease;
}
.sp-pop-enter-from,
.sp-pop-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>
