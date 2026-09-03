<script setup>
/**
 * Analyzer settings popover (manual §17), opened from the bottom bar.
 *
 * Parameters (all non-automatable, UI-only on the plug-in side):
 * `analyzer_pre` / `analyzer_post` / `analyzer_sc` (which spectra are
 * shown; the side-chain one only exists when the build has a side-chain
 * input, `hasParam('analyzer_sc')`), `analyzer_range` (60 / 90 / 120 dB),
 * `analyzer_resolution` (FFT size, applied by the DSP), `analyzer_speed`
 * (fall-off time), `analyzer_tilt` (dB per octave slope compensation) and
 * `analyzer_freeze` (peak hold). Analyzer.vue watches these and
 * reconfigures the `Spectrum` instances.
 *
 * Emits `freeze-hold` (Boolean): pointer down / up on the Freeze button,
 * so App.vue can implement the temporary click-and-hold freeze on top of
 * the click toggle. `ui.spectrumGrab` enables hover-to-grab in the display.
 * "Show Collisions" needs other instances of the plug-in and is a stub.
 */
import { hasParam, ui, useGlobals } from '../composables/useVst3WebStratum.js';

const g = useGlobals();
const hasSc = hasParam('analyzer_sc');
const emit = defineEmits(['freeze-hold']);
</script>

<template>
  <div class="p-3 w-[300px] flex flex-col gap-2.5">
    <div class="flex items-center gap-1.5">
      <button class="chip" :class="{ on: g.anPre.on }" @click="g.anPre.toggle()">Pre</button>
      <button class="chip" :class="{ on: g.anPost.on }" @click="g.anPost.toggle()">Post</button>
      <button v-if="hasSc" class="chip" :class="{ on: g.anSc.on }" title="External spectrum: side-chain input" @click="g.anSc.toggle()">SC</button>
      <select class="sel ml-auto" :disabled="!hasSc" :value="g.anSc.on ? 1 : 0" @change="g.anSc.setOn(Number($event.target.value) === 1)">
        <option :value="0">Off</option>
        <option :value="1">Sidechain Input</option>
      </select>
    </div>
    <div class="grid grid-cols-[70px_1fr] gap-x-2 gap-y-1.5 items-center">
      <span class="lbl">Range</span>
      <div class="flex gap-1">
        <button v-for="(l, i) in g.anRange.labels" :key="l" class="chip flex-1" :class="{ on: g.anRange.index === i }" @click="g.anRange.setIndex(i)">{{ l }}</button>
      </div>
      <span class="lbl">Resolution</span>
      <div class="flex gap-1">
        <button v-for="(l, i) in g.anRes.labels" :key="l" class="chip flex-1" :class="{ on: g.anRes.index === i }" @click="g.anRes.setIndex(i)">{{ l }}</button>
      </div>
      <span class="lbl">Speed</span>
      <select class="sel" :value="g.anSpeed.index" @change="g.anSpeed.setIndex(Number($event.target.value))">
        <option v-for="(l, i) in g.anSpeed.labels" :key="l" :value="i">{{ l }}</option>
      </select>
      <span class="lbl">Tilt</span>
      <select class="sel" :value="g.anTilt.index" @change="g.anTilt.setIndex(Number($event.target.value))">
        <option v-for="(l, i) in g.anTilt.labels" :key="l" :value="i">{{ l }}</option>
      </select>
    </div>
    <div class="flex items-center gap-1.5 pt-1 border-t border-white/10">
      <button
        class="chip"
        :class="{ on: g.anFreeze.on }"
        title="Click to freeze (peak hold); click-and-hold for a temporary freeze"
        @click="g.anFreeze.toggle()"
        @pointerdown="emit('freeze-hold', true)"
        @pointerup="emit('freeze-hold', false)"
        @pointerleave="emit('freeze-hold', false)"
      >
        Freeze
      </button>
      <button class="chip" :class="{ on: ui.spectrumGrab }" title="Hover the spectrum a moment to grab peaks" @click="ui.spectrumGrab = !ui.spectrumGrab">Spectrum Grab</button>
      <button class="chip opacity-40 cursor-not-allowed" title="Needs other instances; not available in this example" disabled>Show Collisions</button>
    </div>
  </div>
</template>

<style scoped>
@reference '../style.css';
.chip {
  @apply rounded px-2 py-0.5 text-[11px] border border-white/10 bg-white/[0.04] text-slate-300 cursor-pointer hover:bg-white/[0.08] transition-colors text-center;
}
.chip.on {
  @apply bg-accent/90 text-ink-950 border-transparent font-semibold;
}
.sel {
  @apply rounded bg-ink-700 border border-white/10 px-2 py-1 text-[11px];
}
.lbl {
  @apply text-[10px] uppercase tracking-wider text-slate-500;
}
</style>
