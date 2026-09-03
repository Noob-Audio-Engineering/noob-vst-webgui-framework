<script setup>
/**
 * Bottom bar (manual §1.4): MIDI Learn (stub: the host owns MIDI mapping),
 * processing mode, linear-phase resolution (only in Linear Phase mode, with
 * a warning when dynamics are unavailable), the instance button (EQ Match,
 * the list of other running instances, reset), the analyzer summary with
 * its popover, character, global bypass, the output summary with its
 * popover (drag the button vertically to change output gain directly), and
 * the size menu.
 *
 * Emits `freeze-hold` (Boolean), forwarded from AnalyzerPanel to App.vue.
 * No props.
 *
 * Parameters (through `useGlobals()`): `processing_mode`, `lp_quality`,
 * `character`, `bypass`, `output_gain`, `gain_scale`, `phase_invert`,
 * `auto_gain`, `analyzer_pre` / `analyzer_post` / `analyzer_sc` /
 * `analyzer_freeze` (summary and indicator lines).
 *
 * Popovers use a hover-to-peek / click-to-pin model: hovering a button
 * opens its panel until the pointer leaves it; clicking pins it
 * (`ui.panel`, `ui.panelSticky`). The instance list comes from the
 * server's `GET /instances` (the other instances of this plug-in, see
 * docs/WIRE.md), filtered to exclude this page's own port; picking one
 * opens its URL in a new window. The size menu sends a `resize` message,
 * which the nih-plug adapter turns into a host resize request; in the
 * standalone it resizes the browser window instead.
 */
import { computed, ref } from 'vue';
import { hasParam, send, ui, useGlobals, useVst3WebStratum } from '../composables/useVst3WebStratum.js';
import { Popover } from '@elyerinfox/vst3-web-stratum/vue';
import { ContextMenu } from '@elyerinfox/vst3-web-stratum/vue';
import AnalyzerPanel from './AnalyzerPanel.vue';
import OutputPanel from './OutputPanel.vue';
import EqMatchPanel from './EqMatchPanel.vue';

const emit = defineEmits(['freeze-hold']);
const g = useGlobals();
const { manifest } = useVst3WebStratum();
const analyzerBtn = ref(null);
const outputBtn = ref(null);
const instanceBtn = ref(null);
const menu = ref({ open: false, x: 0, y: 0, items: [] });
const standalone = computed(() => !!manifest.value?.meta?.standalone);

const analyzerSummary = computed(() => {
  const parts = [];
  if (g.anPre.on) parts.push('Pre');
  if (g.anPost.on) parts.push('Post');
  if (g.anSc?.on) parts.push('SC');
  return parts.join('+') || 'Off';
});
const dynWarning = computed(() => g.mode.index === 2 && g.quality.index >= 3);

function openMenuAt(r, items) {
  menu.value = { open: true, x: r.left, y: r.top - 10 - items.length * 30, items };
}
function openMenu(e, items) {
  openMenuAt(e.currentTarget.getBoundingClientRect(), items);
}
/** The other live instances of this plug-in, from this server's `/instances` (scoped by name). */
async function liveInstances() {
  try {
    const r = await fetch('/instances');
    return r.ok ? await r.json() : [];
  } catch {
    return [];
  }
}
function modeMenu(e) {
  openMenu(e, g.mode.labels.map((l, i) => ({ label: l, checked: g.mode.index === i, action: () => g.mode.setIndex(i), hint: i === 1 ? 'approximated' : '' })));
}
function qualityMenu(e) {
  openMenu(e, g.quality.labels.map((l, i) => ({ label: l, checked: g.quality.index === i, action: () => g.quality.setIndex(i), hint: i >= 3 ? 'no dynamic EQ' : '' })));
}
function characterMenu(e) {
  openMenu(e, g.character.labels.map((l, i) => ({ label: l, checked: g.character.index === i, action: () => g.character.setIndex(i) })));
}
async function instanceMenu(e) {
  const rect = e.currentTarget.getBoundingClientRect();
  const here = Number(location.port);
  const others = (await liveInstances()).filter((i) => i.port !== here);
  openMenuAt(rect, [
    { label: 'EQ Match…', action: () => (ui.panel = 'eqmatch') },
    { divider: true },
    ...(others.length
      ? others.map((i) => ({ label: i.name, hint: `:${i.port}  pid ${i.pid}`, action: () => window.open(i.url, '_blank') }))
      : [{ label: `No other ${manifest?.name || 'noob-q'} instances running`, disabled: true }]),
    { divider: true },
    { label: 'Reset all parameters', action: () => send('reset') },
  ]);
}
function sizeMenu(e) {
  const sizes = [
    ['Mini', 820, 500],
    ['Small', 980, 600],
    ['Medium', 1180, 720],
    ['Large', 1400, 860],
    ['Extra Large', 1680, 1020],
  ];
  openMenu(
    e,
    sizes.map(([name, w, h]) => ({
      label: name,
      hint: `${w}×${h}`,
      checked: ui.size === name,
      action: () => {
        ui.size = name;
        send('resize', { width: w, height: h });
        if (standalone.value) window.resizeTo?.(w, h);
      },
    })),
  );
}

// Drag the output button vertically to change output gain directly
// (manual §1.4): a press that moves more than 3 px becomes a gain gesture
// (300 px = the full range), a press that does not becomes a click that
// pins the output panel. `drag.moved` tells the two apart on release.
let drag = null;
function onOutDown(e) {
  if (e.button !== 0) return;
  drag = { id: e.pointerId, y: e.clientY, n: g.outputGain.norm, moved: false };
  e.currentTarget.setPointerCapture(e.pointerId);
}
function onOutMove(e) {
  if (!drag || e.pointerId !== drag.id) return;
  const dy = drag.y - e.clientY;
  if (!drag.moved && Math.abs(dy) > 3) {
    drag.moved = true;
    g.outputGain.begin();
  }
  if (drag.moved) g.outputGain.set(Math.max(0, Math.min(1, drag.n + dy / 300)));
}
function onOutUp(e) {
  if (!drag || e.pointerId !== drag.id) return;
  if (drag.moved) g.outputGain.end();
  else ui.panel = ui.panel === 'output' ? null : 'output';
  drag = null;
}
/** Hover-to-peek: open `name` while the pointer is over its button or panel, unless a panel is pinned. */
function hoverPanel(name, on) {
  if (ui.panelSticky) return;
  if (on) ui.panel = name;
  else if (ui.panel === name) ui.panel = null;
}
/** Click-to-pin: a click pins the panel open; a second click on a pinned panel's button closes it. */
function clickPanel(name) {
  if (ui.panel === name && ui.panelSticky) {
    ui.panel = null;
    ui.panelSticky = false;
  } else {
    ui.panel = name;
    ui.panelSticky = true;
  }
}
</script>

<template>
  <footer class="h-11 shrink-0 flex items-center gap-2 px-3 border-t border-white/[0.06] bg-ink-900/80 select-none text-[11px]">
    <button class="bb opacity-50 cursor-not-allowed" title="MIDI Learn: map controllers in your host; not available inside the web view" disabled>MIDI Learn ▾</button>
    <button class="bb" title="Processing mode" @click="modeMenu">{{ g.mode.label }}</button>
    <button v-if="g.mode.index === 2" class="bb" :class="{ 'text-amber-300': dynWarning }" :title="dynWarning ? 'Dynamic EQ is disabled at this resolution' : 'Linear-phase resolution'" @click="qualityMenu">{{ dynWarning ? '⚠ ' : '' }}{{ g.quality.label }}</button>

    <button ref="instanceBtn" class="bb mx-auto min-w-[140px] text-center" title="Instance: EQ Match and more" @click="instanceMenu">{{ manifest?.name || 'noob-q' }}</button>

    <button
      ref="analyzerBtn"
      class="bb relative"
      :class="{ on: ui.panel === 'analyzer' && ui.panelSticky }"
      title="Analyzer settings (hover to peek, click to keep open)"
      @pointerenter="hoverPanel('analyzer', true)"
      @click="clickPanel('analyzer')"
    >
      <span v-if="g.anFreeze.on" class="absolute -top-px left-1 right-1 h-0.5 bg-sky-400 rounded" />
      Analyzer: <b class="text-slate-100">{{ analyzerSummary }}</b>
    </button>
    <button class="bb" title="Character (saturation)" @click="characterMenu">{{ g.character.label }}</button>
    <button class="bb relative" :class="{ danger: g.bypass.on }" title="Global bypass" @click="g.bypass.toggle()">
      <span v-if="g.bypass.on" class="absolute -top-px left-1 right-1 h-0.5 bg-red-500 rounded" />
      Bypass
    </button>
    <button
      ref="outputBtn"
      class="bb relative tabular min-w-[110px] cursor-ns-resize"
      :class="{ on: ui.panel === 'output' && ui.panelSticky }"
      title="Output options (hover to peek, click to keep open, drag to change gain)"
      @pointerenter="hoverPanel('output', true)"
      @pointerdown="onOutDown"
      @pointermove="onOutMove"
      @pointerup="onOutUp"
      @pointercancel="onOutUp"
    >
      <span v-if="g.phaseInvert.on" class="absolute -top-px left-1 right-1/2 h-0.5 bg-sky-400 rounded" />
      <span v-if="g.autoGain.on" class="absolute -top-px left-1/2 right-1 h-0.5 bg-amber-300 rounded" />
      {{ g.gainScale.text }}&nbsp;&nbsp;{{ g.outputGain.text }}
    </button>
    <button class="bb" title="Size and scaling" @click="sizeMenu">⤢</button>

    <Popover :open="ui.panel === 'analyzer'" :anchor="analyzerBtn" placement="top" align="end" title="Spectrum analyzer" @close="ui.panel = null; ui.panelSticky = false">
      <div @pointerleave="hoverPanel('analyzer', false)"><AnalyzerPanel @freeze-hold="(v) => emit('freeze-hold', v)" /></div>
    </Popover>
    <Popover :open="ui.panel === 'output'" :anchor="outputBtn" placement="top" align="end" title="Output" @close="ui.panel = null; ui.panelSticky = false">
      <div @pointerleave="hoverPanel('output', false)"><OutputPanel /></div>
    </Popover>
    <Popover :open="ui.panel === 'eqmatch'" :anchor="instanceBtn" placement="top" align="center" @close="ui.panel = null">
      <EqMatchPanel @close="ui.panel = null" />
    </Popover>
    <ContextMenu :open="menu.open" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu.open = false" />
  </footer>
</template>

<style scoped>
@reference '../style.css';
.bb {
  @apply rounded px-2.5 py-1 border border-white/10 bg-white/[0.04] text-slate-300 hover:bg-white/[0.09] transition-colors leading-4;
}
.bb.on {
  @apply bg-white/[0.12] text-slate-100;
}
.bb.danger {
  @apply bg-red-500/20 text-red-300 border-red-500/40;
}
</style>
