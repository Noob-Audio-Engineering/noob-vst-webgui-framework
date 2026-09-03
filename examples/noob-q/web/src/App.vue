<script setup>
/**
 * Noob-Q root component. Owns the page layout (top bar, display, frequency
 * scale, output meter, bottom bar), the floating display buttons (display
 * range, piano display, EQ Sketch, the standalone-only demo source) and the
 * window-level keyboard shortcuts.
 *
 * Nothing but a "connecting…" placeholder renders until `ready` is true:
 * every child asks for parameter handles with `useParam`, which needs the
 * manifest the plug-in sends right after the socket opens.
 *
 * Parameters read / written (through `useGlobals()`): `display_range`,
 * `piano_display`, `analyzer_freeze`, and, only when the server exposes
 * them, the demo-source parameters `src_kind` / `src_freq` / `src_level` /
 * `sc_kind` / `sc_level`. The presence of `src_kind` is how the page tells
 * the standalone from the plug-in (`standalone`). Streams: `meter_out`
 * through the framework `LevelMeter`.
 *
 * Keyboard (window level; ignored while an input, select or textarea has
 * focus so typing a value never triggers a shortcut):
 *   Ctrl/Cmd+Z, Ctrl/Cmd+Y, Ctrl/Cmd+Shift+Z   undo / redo (framework History)
 *   Ctrl/Cmd+B                                 A/B compare
 *   Delete, Backspace                          delete the selected bands
 *   Escape                                     leave Spectrum Grab, else close the open
 *                                              panel, else exit full screen, else deselect
 *   Arrow keys (Shift = fine)                  nudge the selected bands: left/right one
 *                                              semitone, up/down 0.5 dB (gain shapes only)
 *   G                                          toggle a permanent Spectrum Grab
 *
 * `onFreezeHold` implements the analyzer's click-and-hold freeze: the bottom
 * bar reports pointer down / up, and this turns `analyzer_freeze` on only if
 * it was off, remembering that in `ui._tempFreeze`, so a freeze the user
 * clicked on deliberately is not undone when the button is released.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { allBands, deleteBand, hasParam, selectBands, ui, useGlobals, useVst3WebStratum } from './composables/useVst3WebStratum.js';
import TopBar from './components/TopBar.vue';
import Analyzer from './components/Analyzer.vue';
import BandPanel from './components/BandPanel.vue';
import BottomBar from './components/BottomBar.vue';
import FreqScale from './components/FreqScale.vue';
import { LevelMeter } from '@elyerinfox/vst3-web-stratum/vue';
import { Knob } from '@elyerinfox/vst3-web-stratum/vue';
import { ContextMenu } from '@elyerinfox/vst3-web-stratum/vue';

const { ready, connected, client, history } = useVst3WebStratum();
const analyzer = ref(null);
const rangeMenu = ref({ open: false, x: 0, y: 0 });
const sourceOpen = ref(false);
// Global handles need the manifest, so they are resolved lazily once `ready`.
const g = computed(() => (ready.value ? useGlobals() : null));
// The standalone exposes demo-source parameters the plug-in does not have.
const standalone = computed(() => ready.value && hasParam('src_kind'));

onMounted(() => window.addEventListener('keydown', onKey));
onBeforeUnmount(() => window.removeEventListener('keydown', onKey));

function onKey(e) {
  const t = e.target;
  if (t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.tagName === 'TEXTAREA')) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'z' && !e.shiftKey) {
    history.undo();
    e.preventDefault();
  } else if ((mod && e.key.toLowerCase() === 'y') || (mod && e.shiftKey && e.key.toLowerCase() === 'z')) {
    history.redo();
    e.preventDefault();
  } else if (mod && e.key.toLowerCase() === 'b') {
    history.toggleAB();
    e.preventDefault();
  } else if (e.key === 'Delete' || e.key === 'Backspace') {
    if (ui.selected.length) {
      ui.selected.forEach((n) => deleteBand(n));
      selectBands([]);
      e.preventDefault();
    }
  } else if (e.key === 'Escape') {
    if (ui.grab.active) analyzer.value?.leaveGrab();
    else if (ui.panel) {
      ui.panel = null;
      ui.panelSticky = false;
    } else if (document.fullscreenElement) document.exitFullscreen?.();
    else selectBands([]);
  } else if (e.key.startsWith('Arrow') && ui.selected.length) {
    const fine = e.shiftKey ? 0.2 : 1;
    for (const n of ui.selected) {
      const b = allBands()[n - 1];
      if (e.key === 'ArrowLeft') b.freq.setPlain(b.freq.plain / Math.pow(2, (1 / 12) * fine));
      if (e.key === 'ArrowRight') b.freq.setPlain(b.freq.plain * Math.pow(2, (1 / 12) * fine));
      if (e.key === 'ArrowUp' && b.hasGain) b.gain.setPlain(b.gain.plain + 0.5 * fine);
      if (e.key === 'ArrowDown' && b.hasGain) b.gain.setPlain(b.gain.plain - 0.5 * fine);
    }
    e.preventDefault();
  } else if (e.key.toLowerCase() === 'g' && !mod) {
    if (ui.grab.active) analyzer.value?.leaveGrab();
    else analyzer.value?.enterGrab(true);
  }
}
function onFreezeHold(on) {
  if (!g.value) return;
  if (on && !g.value.anFreeze.on) g.value.anFreeze.setOn(true), (ui._tempFreeze = true);
  if (!on && ui._tempFreeze) g.value.anFreeze.setOn(false), (ui._tempFreeze = false);
}
function openRangeMenu(e) {
  const r = e.currentTarget.getBoundingClientRect();
  rangeMenu.value = { open: true, x: r.right - 120, y: r.bottom + 4 };
}
const rangeItems = computed(() =>
  g.value ? g.value.displayRange.labels.map((l, i) => ({ label: `±${l}`, checked: g.value.displayRange.index === i, action: () => g.value.displayRange.setIndex(i) })) : [],
);
</script>

<template>
  <div class="h-full flex flex-col bg-ink-950 text-slate-200 overflow-hidden">
    <template v-if="ready">
      <TopBar />
      <main class="relative flex-1 min-h-0 flex">
        <div class="relative flex-1 min-w-0 flex flex-col">
          <div class="relative flex-1 min-h-0">
            <Analyzer ref="analyzer" />
            <BandPanel :band="ui.selected.length ? ui.primary : null" @close="selectBands([])" />
            <button class="absolute top-2 right-2 z-10 rounded px-2 py-0.5 text-[11px] border border-white/10 bg-ink-900/80 hover:bg-white/[0.1]" title="Display range" @click="openRangeMenu">
              ±{{ g.displayRange.label }} ▾
            </button>
            <div class="absolute bottom-2 left-2 z-10 flex gap-1">
              <button class="mini" :class="{ on: g.piano.on }" title="Piano display" @click="g.piano.toggle()">🎹</button>
              <button class="mini" :class="{ on: ui.sketchArmed }" title="EQ Sketch: draw a curve left to right" @click="ui.sketchArmed = !ui.sketchArmed">✎</button>
            </div>
            <div v-if="standalone" class="absolute top-2 left-2 z-10">
              <button class="mini" :class="{ on: sourceOpen }" title="Demo signal source (standalone only)" @click="sourceOpen = !sourceOpen">Source</button>
              <div v-if="sourceOpen" class="mt-1 rounded-lg border border-white/10 bg-ink-800/95 p-2 flex items-end gap-2">
                <div class="flex flex-col gap-1 text-[10px] text-slate-500">
                  <span>Main</span>
                  <select class="rounded bg-ink-700 border border-white/10 px-1 py-0.5 text-[11px] text-slate-200" :value="g.srcKind.index" @change="g.srcKind.setIndex(Number($event.target.value))">
                    <option v-for="(l, i) in g.srcKind.labels" :key="l" :value="i">{{ l }}</option>
                  </select>
                  <span>Side-chain</span>
                  <select class="rounded bg-ink-700 border border-white/10 px-1 py-0.5 text-[11px] text-slate-200" :value="g.scKind.index" @change="g.scKind.setIndex(Number($event.target.value))">
                    <option v-for="(l, i) in g.scKind.labels" :key="l" :value="i">{{ l }}</option>
                  </select>
                </div>
                <Knob :p="g.srcFreq" :size="44" />
                <Knob :p="g.srcLevel" :size="44" label="Level" />
                <Knob :p="g.scLevel" :size="44" label="SC" />
              </div>
            </div>
          </div>
          <div class="h-5 shrink-0 border-t border-white/[0.06] bg-ink-900/60">
            <FreqScale :piano="g.piano.on" />
          </div>
        </div>
        <aside v-if="ui.meterVisible" class="w-10 shrink-0 border-l border-white/[0.06] bg-ink-900/60 flex flex-col items-center py-2 gap-1">
          <div class="flex-1 w-6 min-h-0"><LevelMeter stream="meter_out" :min-db="-60" :max-db="6" /></div>
          <span class="text-[9px] uppercase tracking-wider text-slate-500">Out</span>
        </aside>
      </main>
      <BottomBar @freeze-hold="onFreezeHold" />
      <ContextMenu :open="rangeMenu.open" :x="rangeMenu.x" :y="rangeMenu.y" :items="rangeItems" @close="rangeMenu.open = false" />
    </template>
    <div v-else class="flex-1 grid place-items-center text-slate-500 text-sm">
      <div class="text-center">
        <div class="mb-1">{{ connected ? 'waiting for manifest…' : 'connecting to plug-in…' }}</div>
        <div class="text-[11px] text-slate-600 tabular">{{ client.url }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@reference './style.css';
.mini {
  @apply rounded px-2 py-0.5 text-[11px] border border-white/10 bg-ink-900/80 text-slate-300 hover:bg-white/[0.1] transition-colors;
}
.mini.on {
  @apply bg-accent/90 text-ink-950 border-transparent;
}
</style>
