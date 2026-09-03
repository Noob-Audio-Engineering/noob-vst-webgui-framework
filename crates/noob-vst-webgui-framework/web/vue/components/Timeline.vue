<script setup>
/**
 * Scrolling history chart (Vue wrapper over the canvas `Timeline`).
 *
 * Usage:
 *
 *   <div class="h-24">
 *     <Timeline :seconds="8" :series="[
 *       { stream: 'meter', index: 0, unit: 'linear', range: [-60, 6], color: '#8fa3b8', label: 'in' },
 *       { stream: 'meter', index: 2, unit: 'linear', range: [-60, 6], color: '#58c4ff', label: 'out' },
 *       { stream: 'meter', index: 4, unit: 'db', range: [-24, 0], color: '#ffb547', label: 'GR', fill: true, fillTo: 0 },
 *     ]" />
 *   </div>
 *
 * Props:
 * - `series` (array, required): as the canvas component's series, except
 *   that `stream` is the stream **id** (a string), resolved on mount.
 * - `seconds` (number, default 6): history shown.
 * - `gridSeries` (number, default 0), `gridStep` (number, default 12),
 *   `grid` (boolean, default true), `legend` (boolean, default true),
 *   `timeTicks` (boolean, default true): passed through.
 *
 * Emits: nothing. Exposes `push(series, value)` for series without a stream.
 * Colours come from `--noob-vst-webgui-framework-grid` and
 * `--noob-vst-webgui-framework-text-dim`. Mount once `ready` is true. The root
 * `<div>` fills its parent.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { Timeline as CanvasTimeline } from '../../components/timeline.js';
import { useStream } from '../useNoobVstWebguiFramework.js';

const props = defineProps({
  series: { type: Array, required: true },
  seconds: { type: Number, default: 6 },
  gridSeries: { type: Number, default: 0 },
  gridStep: { type: Number, default: 12 },
  grid: { type: Boolean, default: true },
  legend: { type: Boolean, default: true },
  timeTicks: { type: Boolean, default: true },
});

const el = ref(null);
let chart = null;
onMounted(() => {
  chart = new CanvasTimeline(el.value, {
    seconds: props.seconds,
    gridSeries: props.gridSeries,
    gridStep: props.gridStep,
    grid: props.grid,
    legend: props.legend,
    timeTicks: props.timeTicks,
    series: props.series.map((s) => ({ ...s, stream: s.stream ? useStream(s.stream) : undefined })),
  });
});
onBeforeUnmount(() => chart?.destroy());
defineExpose({ push: (i, v) => chart?.push(i, v) });
</script>

<template>
  <div ref="el" style="width: 100%; height: 100%"></div>
</template>
