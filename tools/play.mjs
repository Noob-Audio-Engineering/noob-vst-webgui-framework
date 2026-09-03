#!/usr/bin/env node
/**
 * play.mjs — send notes to a running noob-vst-webgui-framework synth and check that it makes
 * sound (peak of the `meter_out` stream), headlessly.
 *
 *   node tools/play.mjs <port> [note=60] [ms=400]
 */
const [port, noteArg, msArg] = process.argv.slice(2);
if (!port) {
  console.error('usage: node tools/play.mjs <port> [note] [ms]');
  process.exit(2);
}
const note = Number(noteArg || 60);
const hold = Number(msArg || 400);
const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
ws.binaryType = 'arraybuffer';
let manifest = null;
let meterIndex = -1;
let peakDuring = 0;
let peakAfter = 0;
let phase = 'idle';
let notesSeen = 0;

function event(kind, a, value) {
  const b = new ArrayBuffer(16);
  const v = new DataView(b);
  v.setUint8(0, 0x12);
  v.setUint16(2, 1, true);
  v.setUint8(4, kind);
  v.setUint8(5, 0);
  v.setUint8(6, a);
  v.setUint8(7, 0);
  v.setFloat32(8, value, true);
  v.setUint32(12, 0, true);
  ws.send(b);
}

ws.onmessage = (ev) => {
  if (typeof ev.data === 'string') {
    const m = JSON.parse(ev.data);
    if (m.t !== 'manifest') return;
    manifest = m;
    meterIndex = m.streams.findIndex((s) => s.id === 'meter_out');
    if (meterIndex < 0) {
      console.error('no meter_out stream');
      process.exit(1);
    }
    setTimeout(() => {
      phase = 'during';
      event(1, note, 0.9);
      setTimeout(() => {
        event(2, note, 0);
        setTimeout(() => (phase = 'after'), 600);
        setTimeout(finish, 900);
      }, hold);
    }, 200);
    return;
  }
  const dv = new DataView(ev.data);
  const kind = dv.getUint8(0);
  const arg = dv.getUint16(2, true);
  if (kind === 0x13) notesSeen += arg;
  if (kind !== 0x20 || arg !== meterIndex) return;
  const len = dv.getUint32(16, true);
  if (len < 2) return;
  const peak = Math.max(dv.getFloat32(20, true), dv.getFloat32(24, true));
  if (phase === 'during') peakDuring = Math.max(peakDuring, peak);
  if (phase === 'after') peakAfter = Math.max(peakAfter, peak);
};
function finish() {
  const db = (p) => (p > 0 ? `${(20 * Math.log10(p)).toFixed(1)} dBFS` : '-inf');
  console.log(`synth "${manifest.name}": note ${note} held ${hold} ms`);
  console.log(`  peak while held : ${db(peakDuring)}`);
  console.log(`  peak after off  : ${db(peakAfter)}`);
  const ok = peakDuring > 0.01 && peakAfter < peakDuring * 0.5;
  console.log(ok ? '  OK: it sounds and releases' : '  FAIL');
  ws.close();
  process.exit(ok ? 0 : 1);
}
ws.onerror = () => {
  console.error('connection failed');
  process.exit(1);
};
