<script setup>
/**
 * Output options popover (manual §20), opened from the output button in the
 * bottom bar: global bypass, phase invert, auto gain, the output meter
 * toggle, pan mode, the output-gain knob with the pan ring, and sliders
 * for gain scale and pan.
 *
 * Parameters: `bypass`, `phase_invert`, `auto_gain`, `pan_mode` (L/R or
 * M/S), `output_gain`, `output_pan`, `gain_scale` (scales every band's
 * gain, 0–200 %). `ui.meterVisible` shows or hides the meter column in
 * App.vue. No props or emits.
 *
 * The range inputs send begin / set / end gestures like the knobs do, and
 * double-click resets to the default, so host automation sees the same
 * thing whichever control was used.
 */
import { ui, useGlobals } from '../composables/useVst3WebStratum.js';
import { Knob } from '@elyerinfox/vst3-web-stratum/vue';

const g = useGlobals();
</script>

<template>
  <div class="p-3 w-[320px] flex flex-col gap-3">
    <div class="flex items-center gap-1.5">
      <button class="chip danger" :class="{ on: g.bypass.on }" title="Global bypass" @click="g.bypass.toggle()">Bypass</button>
      <button class="chip blue" :class="{ on: g.phaseInvert.on }" title="Phase invert" @click="g.phaseInvert.toggle()">ø</button>
      <button class="chip yellow" :class="{ on: g.autoGain.on }" title="Auto gain: static make-up gain estimated from the EQ curve" @click="g.autoGain.toggle()">A</button>
      <button class="chip" :class="{ on: ui.meterVisible }" title="Show output level meter" @click="ui.meterVisible = !ui.meterVisible">Meter</button>
      <div class="ml-auto flex items-center gap-1 text-[11px]">
        <span class="text-slate-500">Pan</span>
        <button v-for="(l, i) in g.panMode.labels" :key="l" class="chip" :class="{ on: g.panMode.index === i }" @click="g.panMode.setIndex(i)">{{ l }}</button>
      </div>
    </div>
    <div class="flex items-center gap-4">
      <Knob :p="g.outputGain" :ring="g.outputPan" :size="84" label="Output" ring-color="#58a6ff" />
      <div class="flex-1 flex flex-col gap-1">
        <div class="flex justify-between text-[10px] uppercase tracking-wider text-slate-500"><span>Gain scale</span><span class="tabular text-slate-300">{{ g.gainScale.text }}</span></div>
        <input
          type="range"
          min="0"
          max="1"
          step="0.005"
          class="w-full accent-[var(--color-accent)]"
          :value="g.gainScale.norm"
          @pointerdown="g.gainScale.begin()"
          @input="g.gainScale.set(Number($event.target.value))"
          @pointerup="g.gainScale.end()"
          @dblclick="g.gainScale.reset()"
        />
        <div class="flex justify-between text-[10px] uppercase tracking-wider text-slate-500 mt-1"><span>Pan ({{ g.panMode.label }})</span><span class="tabular text-slate-300">{{ g.outputPan.text }}</span></div>
        <input
          type="range"
          min="0"
          max="1"
          step="0.005"
          class="w-full accent-[#58a6ff]"
          :value="g.outputPan.norm"
          @pointerdown="g.outputPan.begin()"
          @input="g.outputPan.set(Number($event.target.value))"
          @pointerup="g.outputPan.end()"
          @dblclick="g.outputPan.reset()"
        />
      </div>
    </div>
    <div class="text-[10px] text-slate-500">Double-click a knob to type a value · Ctrl+click resets · drag the output button in the bottom bar to change gain directly.</div>
  </div>
</template>

<style scoped>
@reference '../style.css';
.chip {
  @apply rounded px-2 py-0.5 text-[11px] border border-white/10 bg-white/[0.04] text-slate-300 cursor-pointer hover:bg-white/[0.08] transition-colors;
}
.chip.on {
  @apply bg-accent/90 text-ink-950 border-transparent font-semibold;
}
.chip.danger.on {
  @apply bg-red-500 text-white;
}
.chip.blue.on {
  @apply bg-sky-400 text-ink-950;
}
.chip.yellow.on {
  @apply bg-amber-300 text-ink-950;
}
</style>
