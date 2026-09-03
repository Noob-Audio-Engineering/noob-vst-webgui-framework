<script setup>
/**
 * Segmented control for a discrete parameter: one button per step, bound
 * to a `useParam` handle (ratio buttons, meter modes, filter types).
 *
 * Usage:
 *
 *   <Segmented :p="ratio" />                       <!-- labels from the manifest -->
 *   <Segmented :p="meterMode" :labels="['GR', '+4', '+8']" vertical />
 *
 * Props:
 * - `p` (object, required): a discrete parameter handle from `useParam`
 *   (`steps` ≥ 2). The handle's `labels` name the buttons unless `labels`
 *   overrides them.
 * - `labels` (string[]): custom button labels, one per step.
 * - `vertical` (boolean, default false): stack the buttons.
 * - `disabled` (boolean, default false).
 *
 * Emits: nothing; clicking sends a full gesture through `p.setIndex`.
 *
 * Styling: none. The root has class `noob-vst-webgui-framework-segmented` (plus
 * `vertical` / `disabled`), each button `noob-vst-webgui-framework-segment` and
 * `is-on` when selected; the page styles them however it likes (a row of
 * square push buttons with lamps, a rotary-switch legend, plain tabs).
 */
import { computed } from 'vue';

const props = defineProps({
  p: { type: Object, required: true },
  labels: { type: Array, default: null },
  vertical: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
});

const items = computed(() => {
  const n = Math.max(2, props.p.spec?.steps || (props.labels ? props.labels.length : 2));
  return Array.from({ length: n }, (_, i) => (props.labels && props.labels[i]) ?? props.p.labels?.[i] ?? String(i));
});
function pick(i) {
  if (props.disabled) return;
  props.p.begin();
  props.p.setIndex(i);
  props.p.end();
}
</script>

<template>
  <div class="noob-vst-webgui-framework-segmented" :class="{ vertical, disabled }" role="radiogroup">
    <button
      v-for="(label, i) in items"
      :key="i"
      type="button"
      class="noob-vst-webgui-framework-segment"
      :class="{ 'is-on': p.index === i }"
      role="radio"
      :aria-checked="p.index === i"
      :disabled="disabled"
      @click="pick(i)"
    >
      {{ label }}
    </button>
  </div>
</template>
