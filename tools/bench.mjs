#!/usr/bin/env node
/**
 * bench.mjs — headless latency / throughput benchmark for a running noob-vst-webgui-framework
 * server (Node 22+, uses the built-in WebSocket).
 *
 *   node tools/bench.mjs <port> [--pings 2000] [--edits 2000] [--seconds 3]
 *
 * Measures, from the client's point of view:
 *   rtt        ping -> pong (socket task only, no pump involved)
 *   edit echo  ParamEdit -> echoed ParamValues (decode, apply, host queue,
 *              fan-out through the pump thread, encode, send)
 *   streams    frames/s, kbit/s and inter-arrival jitter per stream
 * All while the demo's telemetry streams are flowing, i.e. under load.
 */

const args = process.argv.slice(2);
const port = Number(args[0]);
if (!port) {
  console.error('usage: node tools/bench.mjs <port> [--pings N] [--edits N] [--seconds S]');
  process.exit(2);
}
const opt = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? Number(args[i + 1]) : def;
};
const N_PINGS = opt('pings', 2000);
const N_EDITS = opt('edits', 2000);
const SECONDS = opt('seconds', 3);

const K = { HELLO: 0x01, PARAM_VALUES: 0x10, PARAM_EDIT: 0x11, STREAM_F32: 0x20, PING: 0x30, PONG: 0x31, SUBSCRIBE: 0x40 };
const FLAG_ECHO = 1;

const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
ws.binaryType = 'arraybuffer';

let manifest = null;
let resolveReady;
const ready = new Promise((r) => (resolveReady = r));
let pongResolve = null;
let echoWaiters = new Map(); // f32 bits -> resolve
const streamStats = new Map(); // index -> { frames, bytes, last, gaps: [] }
let countStreams = false;

ws.onopen = () => {};
ws.onerror = (e) => {
  console.error('websocket error', e.message || e);
  process.exit(1);
};
ws.onclose = () => {
  if (!manifest) {
    console.error('closed before manifest');
    process.exit(1);
  }
};
ws.onmessage = (ev) => {
  if (typeof ev.data === 'string') {
    const m = JSON.parse(ev.data);
    if (m.t === 'manifest') {
      manifest = m;
      resolveReady();
    }
    return;
  }
  const buf = ev.data;
  const dv = new DataView(buf);
  const kind = dv.getUint8(0);
  const arg = dv.getUint16(2, true);
  if (kind === K.PONG && pongResolve) {
    const r = pongResolve;
    pongResolve = null;
    r(performance.now() - dv.getFloat64(4, true));
  } else if (kind === K.PARAM_VALUES) {
    let o = 4;
    for (let i = 0; i < arg; i++, o += 8) {
      const flags = dv.getUint16(o + 2, true);
      if (flags & FLAG_ECHO) {
        const bits = dv.getUint32(o + 4, true);
        const r = echoWaiters.get(bits);
        if (r) {
          echoWaiters.delete(bits);
          r(performance.now());
        }
      }
    }
  } else if (kind === K.STREAM_F32 && countStreams) {
    const now = performance.now();
    let s = streamStats.get(arg);
    if (!s) {
      s = { frames: 0, bytes: 0, last: 0, gaps: [] };
      streamStats.set(arg, s);
    }
    s.frames++;
    s.bytes += buf.byteLength;
    if (s.last) s.gaps.push(now - s.last);
    s.last = now;
  }
};

function sendPing() {
  const b = new ArrayBuffer(12);
  const v = new DataView(b);
  v.setUint8(0, K.PING);
  v.setUint16(2, 0, true);
  v.setFloat64(4, performance.now(), true);
  ws.send(b);
}

function sendEdit(index, phase, value) {
  const b = new ArrayBuffer(12);
  const v = new DataView(b);
  v.setUint8(0, K.PARAM_EDIT);
  v.setUint8(1, 0);
  v.setUint16(2, 1, true);
  v.setUint16(4, index, true);
  v.setUint8(6, phase);
  v.setUint8(7, 0);
  v.setFloat32(8, value, true);
  ws.send(b);
  return new DataView(b).getUint32(8, true);
}

function pct(sorted, p) {
  if (!sorted.length) return NaN;
  const i = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[i];
}
const us = (ms) => `${(ms * 1000).toFixed(0).padStart(6)} µs`;
function summary(label, samples) {
  const s = [...samples].sort((a, b) => a - b);
  const mean = s.reduce((a, b) => a + b, 0) / s.length;
  console.log(
    `${label.padEnd(12)} n=${String(s.length).padStart(5)}  p50 ${us(pct(s, 50))}  p90 ${us(pct(s, 90))}  p99 ${us(pct(s, 99))}  max ${us(s[s.length - 1])}  mean ${us(mean)}`,
  );
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await ready;
console.log(`connected to "${manifest.name}": ${manifest.params.length} params, ${manifest.streams.length} streams`);
await sleep(300); // let the streams settle

// 1. RTT
const rtts = [];
for (let i = 0; i < N_PINGS; i++) {
  const p = new Promise((r) => (pongResolve = r));
  sendPing();
  rtts.push(await p);
}
summary('ping rtt', rtts);

// 2. Edit -> echo. Use the first automatable parameter.
const target = manifest.params.find((p) => p.automatable) || manifest.params[0];
const original = null;
const echoes = [];
sendEdit(target.index, 0, 0.5);
for (let i = 0; i < N_EDITS; i++) {
  // Unique value per edit so each echo is unambiguous.
  const value = Math.fround(0.05 + 0.9 * ((i * 7919) % N_EDITS) / N_EDITS);
  const t0 = performance.now();
  const done = new Promise((r) => echoWaiters.set(new DataView(new Float32Array([value]).buffer).getUint32(0, true), r));
  sendEdit(target.index, 1, value);
  const t1 = await Promise.race([done, sleep(1000).then(() => NaN)]);
  if (Number.isNaN(t1)) {
    console.error(`edit ${i}: no echo within 1 s`);
    break;
  }
  echoes.push(t1 - t0);
}
sendEdit(target.index, 2, target.default_norm);
sendEdit(target.index, 0, target.default_norm);
sendEdit(target.index, 1, target.default_norm);
sendEdit(target.index, 2, target.default_norm);
summary('edit echo', echoes);
void original;

// 3. Streams
countStreams = true;
await sleep(SECONDS * 1000);
countStreams = false;
console.log(`\nstreams over ${SECONDS}s:`);
for (const [idx, s] of [...streamStats.entries()].sort((a, b) => a[0] - b[0])) {
  const spec = manifest.streams[idx];
  const gaps = [...s.gaps].sort((a, b) => a - b);
  const fps = s.frames / SECONDS;
  const kbps = (s.bytes * 8) / SECONDS / 1000;
  console.log(
    `  ${spec.id.padEnd(10)} ${fps.toFixed(1).padStart(7)} frames/s  ${kbps.toFixed(0).padStart(7)} kbit/s  ` +
      `gap p50 ${pct(gaps, 50).toFixed(2)} ms  p99 ${pct(gaps, 99).toFixed(2)} ms  max ${(gaps[gaps.length - 1] || 0).toFixed(2)} ms`,
  );
}

ws.close();
process.exit(0);
