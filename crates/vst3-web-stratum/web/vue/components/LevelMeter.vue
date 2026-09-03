<script setup>
/**
 * Thin wrapper over the framework-agnostic canvas meter.
 *
 * Usage:
 *
 *   <div class="w-3 h-40"><LevelMeter stream="meter_out" :min-db="-60" :max-db="6" /></div>
 *
 * Props:
 * - `stream` (string, required): id of a meter stream in the manifest. One
 *   value per channel per frame (peak or RMS in dBFS, as the plug-in
 *   publishes it); the bar count follows the stream's `channels`.
 * - `minDb` (number, default -60): bottom of the scale.
 * - `maxDb` (number, default 6): top of the scale.
 * - `orientation` ('vertical' | 'horizontal', default 'vertical').
 *
 * Emits: nothing.
 *
 * Exposes: `resetClip()` clears the clip indicator (the meter latches when a
 * value reaches 0 dBFS).
 *
 * Sizing: the root `<div>` fills its parent (`width: 100%; height: 100%`),
 * so the parent must have a size. The meter subscribes on mount and
 * destroys itself on unmount. Must be mounted once `ready` is true, as it
 * looks the stream up by id. See `crates/vst3-web-stratum/web/components/README.md` for the meter's
 * own options (gap, background, hold time) and CSS variables.
 */
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { Meter } from '../../components/meter.js';
import { useStream } from '../useVst3WebStratum.js';

const props = defineProps({
  stream: { type: String, required: true },
  minDb: { type: Number, default: -60 },
  maxDb: { type: Number, default: 6 },
  orientation: { type: String, default: 'vertical' },
});

const el = ref(null);
let meter = null;
onMounted(() => {
  meter = new Meter(el.value, useStream(props.stream), {
    minDb: props.minDb,
    maxDb: props.maxDb,
    orientation: props.orientation,
    gap: 2,
    background: 'rgba(255,255,255,0.05)',
  });
});
onBeforeUnmount(() => meter?.destroy());
defineExpose({ resetClip: () => meter?.resetClip() });
</script>

<template>
  <div ref="el" style="width: 100%; height: 100%"></div>
</template>
