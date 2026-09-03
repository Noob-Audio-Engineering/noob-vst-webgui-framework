<script setup>
/**
 * Top bar (manual §1.1): connection dot and byline, undo / redo, A/B
 * compare and copy-to-other-slot, previous / current / next preset with the
 * preset browser popover, the live edit→echo round trip, the plug-in's
 * reported latency, the help menu and full screen.
 *
 * No props or emits. Uses the framework `History` (undo / redo / A-B live
 * in `@elyerinfox/vst3-web-stratum`, not here), `stats.echoAvgMs` (measured by the client
 * from its own edits), and the `status` message the host sends once a
 * second (`latency_ms`, `latency_samples`). Preset navigation walks the
 * factory presets followed by the user presets from the plug-in's UI store
 * (`presets.user`); the browser itself is `PresetBrowser.vue`.
 *
 * The help menu toggles the UI-only options in `ui` (`showParamDisplay`,
 * `autoRange`, `showFreqHover`) and lists every keyboard shortcut.
 */
import { computed, ref } from 'vue';
import { loadState, ui, useVst3WebStratum } from '../composables/useVst3WebStratum.js';
import { FACTORY_PRESETS, loadUserPresets } from '../presets.js';
import { Popover } from '@elyerinfox/vst3-web-stratum/vue';
import PresetBrowser from './PresetBrowser.vue';
import { ContextMenu } from '@elyerinfox/vst3-web-stratum/vue';

const { history, historyState, connected, stats, status } = useVst3WebStratum();
const presetBtn = ref(null);
const helpBtn = ref(null);
const help = ref({ open: false, x: 0, y: 0 });

const fmt = (ms) => (Number.isNaN(ms) ? '–' : ms < 1 ? `${Math.round(ms * 1000)} µs` : `${ms.toFixed(2)} ms`);
const latencyMs = computed(() => status.value?.latency_ms);

/** Factory presets followed by the user presets from the store, in navigation order. */
function allPresets() {
  return [...FACTORY_PRESETS, ...loadUserPresets()];
}
/** Load the previous (`-1`) or next (`+1`) preset relative to the current name, wrapping around. */
function stepPreset(dir) {
  const list = allPresets();
  if (!list.length) return;
  let i = list.findIndex((p) => p.name === ui.preset.name);
  i = (i + dir + list.length) % list.length;
  loadState(list[i].values || {});
  ui.preset = { name: list[i].name, modified: false, index: i };
}
function toggleFullscreen() {
  const d = document;
  if (!d.fullscreenElement) d.documentElement.requestFullscreen?.().then(() => (ui.fullscreen = true)).catch(() => {});
  else d.exitFullscreen?.().then(() => (ui.fullscreen = false));
}
const helpItems = computed(() => [
  { label: 'Show EQ Parameter Display', checked: ui.showParamDisplay, action: () => (ui.showParamDisplay = !ui.showParamDisplay) },
  { label: 'Auto-EQ Sketch', checked: true, disabled: true, hint: 'always on' },
  { label: 'Auto-Adjust Display Range', checked: ui.autoRange, action: () => (ui.autoRange = !ui.autoRange) },
  { label: 'Show Frequency On Hover', checked: ui.showFreqHover, action: () => (ui.showFreqHover = !ui.showFreqHover) },
  { divider: true },
  { label: 'Keyboard shortcuts', action: () => window.alert('Ctrl+Z / Ctrl+Y undo / redo · Ctrl+B A/B · Delete removes selected bands · Esc deselects / exits full screen · Arrow keys nudge · Shift+drag selects a rectangle · Alt+drag creates a dynamic band · Ctrl+drag node = Q · wheel = Q, Ctrl+wheel = gain, Alt+wheel = dynamic range · Alt+click node = bypass · Ctrl+Alt+click = shape · Alt+Shift+click = slope · double-click node = type values') },
  { label: 'About Noob-Q', action: () => window.alert('Noob-Q — a Pro-Q style EQ example for bridge: Rust DSP, Vue + Tailwind UI in the OS web view, one loopback WebSocket.') },
]);
</script>

<template>
  <header class="h-10 shrink-0 flex items-center gap-2 px-3 border-b border-white/[0.06] bg-ink-900/80 backdrop-blur select-none">
    <div class="flex items-center gap-2 mr-2">
      <span class="w-2 h-2 rounded-full" :class="connected ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,.8)]' : 'bg-red-500'" />
      <span class="font-bold tracking-wide text-[13px]">NOOB-Q</span>
      <span class="text-slate-600 text-[11px]">by Ely Erin Fox</span>
    </div>

    <button class="tb" :disabled="!historyState.canUndo" title="Undo (Ctrl+Z)" @click="history.undo()">↶</button>
    <button class="tb" :disabled="!historyState.canRedo" title="Redo (Ctrl+Y)" @click="history.redo()">↷</button>
    <button class="tb w-9" title="A/B compare (Ctrl+B)" @click="history.toggleAB()"><b class="text-accent">{{ historyState.ab }}</b><span class="text-slate-500">/{{ historyState.ab === 'A' ? 'B' : 'A' }}</span></button>
    <button class="tb" title="Copy the active state to the other slot" @click="history.copyToOther()">⧉</button>

    <div class="flex items-center gap-1 mx-2">
      <button class="tb" title="Previous preset" @click="stepPreset(-1)">‹</button>
      <button ref="presetBtn" class="tb min-w-[200px] text-center" :class="{ 'text-slate-400': ui.preset.modified }" title="Open the preset browser" @click="ui.panel = ui.panel === 'presets' ? null : 'presets'">
        {{ ui.preset.name }}<span v-if="ui.preset.modified"> *</span>
      </button>
      <button class="tb" title="Next preset" @click="stepPreset(1)">›</button>
    </div>

    <div class="ml-auto flex items-center gap-3 text-[11px] text-slate-500 tabular">
      <span title="Time from sending a knob edit until the plug-in echoes it back">edit→echo <b class="text-emerald-300 font-medium">{{ fmt(stats.echoAvgMs) }}</b></span>
      <span v-if="latencyMs != null" :title="`${status.latency_samples} samples of plug-in latency`">latency <b class="text-slate-300">{{ latencyMs.toFixed(1) }} ms</b></span>
    </div>

    <button ref="helpBtn" class="tb" title="Help" @click="help = { open: !help.open, x: $event.currentTarget.getBoundingClientRect().left, y: $event.currentTarget.getBoundingClientRect().bottom + 4 }">?</button>
    <button class="tb" title="Full screen (Esc to exit)" @click="toggleFullscreen">⛶</button>

    <Popover :open="ui.panel === 'presets'" :anchor="presetBtn" placement="bottom" align="center" @close="ui.panel = null">
      <PresetBrowser @close="ui.panel = null" />
    </Popover>
    <ContextMenu :open="help.open" :x="help.x" :y="help.y" :items="helpItems" @close="help.open = false" />
  </header>
</template>

<style scoped>
@reference '../style.css';
.tb {
  @apply rounded px-2 py-1 text-[11px] border border-white/10 bg-white/[0.04] text-slate-200 hover:bg-white/[0.09] disabled:opacity-30 disabled:hover:bg-white/[0.04] transition-colors leading-4;
}
</style>
