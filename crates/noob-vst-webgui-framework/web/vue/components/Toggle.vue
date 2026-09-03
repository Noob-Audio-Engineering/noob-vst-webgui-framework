<script setup>
/**
 * Two-state switch for a toggle parameter, bound to a `useParam` handle:
 * power, bypass, Limit/Compress, sidechain filter on/off.
 *
 * Usage:
 *
 *   <Toggle :p="power" :labels="['OFF', 'ON']" />
 *   <Toggle :p="mode" :labels="['COMPRESS', 'LIMIT']" variant="rocker" />
 *   <Toggle :p="bypass" variant="button">BYPASS</Toggle>
 *
 * Props:
 * - `p` (object, required): a toggle handle from `useParam` (`isToggle`).
 * - `labels` ([off, on], default ['Off', 'On']): text beside the switch;
 *   for `variant="button"` the slot is the button text instead.
 * - `variant` ('switch' | 'rocker' | 'button', default 'switch'): a sliding
 *   switch, a two-position rocker with both labels printed, or a latching
 *   push button that shows `is-on`.
 * - `vertical` (boolean, default false): rocker orientation.
 * - `disabled` (boolean, default false).
 *
 * Emits: nothing; clicking sends a full gesture through `p.setOn`.
 *
 * Styling: none. Root class `noob-vst-webgui-framework-toggle` plus the variant
 * (`switch`, `rocker`, `button`) and `is-on`; inside, `lbl`, `track`, `knob`
 * for the switch and `pos` for the rocker positions. The page styles them.
 */
const props = defineProps({
  p: { type: Object, required: true },
  labels: { type: Array, default: () => ['Off', 'On'] },
  variant: { type: String, default: 'switch' },
  vertical: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
});
function set(on) {
  if (props.disabled) return;
  props.p.begin();
  props.p.setOn(on);
  props.p.end();
}
function flip() {
  set(!props.p.on);
}
</script>

<template>
  <button
    v-if="variant === 'button'"
    type="button"
    class="noob-vst-webgui-framework-toggle button"
    :class="{ 'is-on': p.on, disabled }"
    :aria-pressed="p.on"
    :disabled="disabled"
    @click="flip"
  >
    <slot>{{ p.on ? labels[1] : labels[0] }}</slot>
  </button>
  <div v-else-if="variant === 'rocker'" class="noob-vst-webgui-framework-toggle rocker" :class="{ 'is-on': p.on, vertical, disabled }" role="radiogroup">
    <button type="button" class="pos" :class="{ 'is-on': !p.on }" role="radio" :aria-checked="!p.on" :disabled="disabled" @click="set(false)">{{ labels[0] }}</button>
    <button type="button" class="pos" :class="{ 'is-on': p.on }" role="radio" :aria-checked="p.on" :disabled="disabled" @click="set(true)">{{ labels[1] }}</button>
  </div>
  <label v-else class="noob-vst-webgui-framework-toggle switch" :class="{ 'is-on': p.on, disabled }">
    <span class="lbl">{{ labels[0] }}</span>
    <button type="button" class="track" role="switch" :aria-checked="p.on" :disabled="disabled" @click="flip"><span class="knob"></span></button>
    <span class="lbl">{{ labels[1] }}</span>
  </label>
</template>
