<script setup>
/**
 * Resize grip for a plug-in window: drag it and the page asks the host for
 * a new editor size on every animation frame, so the window follows the
 * pointer; on release the size is remembered in the UI store (key
 * `window`) and the editor reopens at it. Built on `useWindowSize`.
 *
 * Usage:
 *
 *   <ResizeGrip class="fixed bottom-0 right-0 w-4 h-4 cursor-nwse-resize" :min="[900, 520]" :aspect="1100 / 620" />
 *
 * Props:
 * - `min` ([w, h], default [480, 320]) and `max` ([w, h], default
 *   [7680, 4320]): limits, in CSS pixels; the adapter clamps again to the
 *   plug-in's own limits.
 * - `aspect` (number, default null): lock width / height to this ratio.
 * - `storeKey` (string, default 'window'): where the size is remembered.
 *
 * Emits: nothing. Renders nothing when resizing is not available (a page in
 * a browser tab, where `manifest.meta.standalone` is true).
 *
 * Styling: none. Root class `vst3-web-stratum-resize-grip` plus
 * `is-dragging`; the page positions it (usually fixed in the bottom-right
 * corner) and draws it (a slot for your own artwork, or a background).
 */
import { useWindowSize } from '../useVst3WebStratum.js';

const props = defineProps({
  min: { type: Array, default: () => [480, 320] },
  max: { type: Array, default: () => [7680, 4320] },
  aspect: { type: Number, default: null },
  storeKey: { type: String, default: 'window' },
});
const { enabled, dragging, gripHandlers } = useWindowSize({ min: props.min, max: props.max, aspect: props.aspect, storeKey: props.storeKey });
</script>

<template>
  <div v-if="enabled" class="vst3-web-stratum-resize-grip" :class="{ 'is-dragging': dragging }" role="separator" aria-label="Resize window" v-on="gripHandlers">
    <slot />
  </div>
</template>
