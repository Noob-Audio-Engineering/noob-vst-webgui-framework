<script setup>
/**
 * XY curve chart (Vue wrapper over the canvas `LinePlot`): transfer curves,
 * responses, tables.
 *
 * Usage:
 *
 *   <div class="h-40">
 *     <LinePlot :x-range="[-60, 0]" :y-range="[-60, 0]" x-label="in dB" y-label="out dB"
 *               :series="[{ stream: 'transfer', color: '#ffb547', label: 'transfer' }, { xy: unity, color: 'rgba(255,255,255,0.2)', dash: [4, 4] }]"
 *               :marker="[inDb, outDb]" />
 *   </div>
 *
 * Props:
 * - `series` (array, required): as the canvas component's series, except
 *   that `stream` is the stream **id**; `points` and `xy` are watched, so
 *   reactive arrays redraw the plot.
 * - `xRange`, `yRange` ([min, max]): axis ranges (watched).
 * - `xStep`, `yStep` (number): grid spacing.
 * - `xLabel`, `yLabel` (string): axis captions.
 * - `marker` ([x, y] | null): operating point (watched).
 * - `grid`, `legend` (boolean, default true).
 *
 * Emits: nothing. Mount once `ready` is true when streams are used. The
 * root `<div>` fills its parent.
 */
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { LinePlot as CanvasPlot } from '../../components/lineplot.js';
import { useStream } from '../useNoobVstWebguiFramework.js';

const props = defineProps({
  series: { type: Array, required: true },
  xRange: { type: Array, default: () => [0, 1] },
  yRange: { type: Array, default: () => [0, 1] },
  xStep: { type: Number, default: undefined },
  yStep: { type: Number, default: undefined },
  xLabel: { type: String, default: '' },
  yLabel: { type: String, default: '' },
  marker: { type: Array, default: null },
  grid: { type: Boolean, default: true },
  legend: { type: Boolean, default: true },
});

const el = ref(null);
let plot = null;
onMounted(() => {
  plot = new CanvasPlot(el.value, {
    xRange: props.xRange,
    yRange: props.yRange,
    xStep: props.xStep,
    yStep: props.yStep,
    xLabel: props.xLabel,
    yLabel: props.yLabel,
    grid: props.grid,
    legend: props.legend,
    series: props.series.map((s) => ({ ...s, stream: s.stream ? useStream(s.stream) : undefined })),
  });
  if (props.marker) plot.setMarker(props.marker[0], props.marker[1]);
});
watch(
  () => props.marker,
  (m) => plot && (m ? plot.setMarker(m[0], m[1]) : plot.setMarker(null)),
  { deep: true },
);
watch(
  () => [props.xRange, props.yRange],
  () => plot?.setRanges(props.xRange, props.yRange),
  { deep: true },
);
watch(
  () => props.series.map((s) => s.points || s.xy),
  (all) => {
    if (!plot) return;
    all.forEach((data, i) => {
      if (!data) return;
      if (props.series[i].xy) plot.setXY(i, data);
      else plot.setSeries(i, data);
    });
  },
  { deep: true },
);
onBeforeUnmount(() => plot?.destroy());
</script>

<template>
  <div ref="el" style="width: 100%; height: 100%"></div>
</template>
