#!/usr/bin/env node
/**
 * setparam.mjs — set a parameter on a running noob-vst-webgui-framework server from the shell.
 *
 *   node tools/setparam.mjs <port> <id> <normalized 0..1>
 *   node tools/setparam.mjs 4242 display_range 1        # ±30 dB (last label)
 *   node tools/setparam.mjs 4242 b1_freq 0.5
 *
 * Sends begin / perform / end in one frame, then exits.
 */
const [port, id, value] = process.argv.slice(2);
if (!port || !id || value == null) {
  console.error('usage: node tools/setparam.mjs <port> <id> <normalized>');
  process.exit(2);
}
const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
ws.binaryType = 'arraybuffer';
ws.onmessage = (ev) => {
  if (typeof ev.data !== 'string') return;
  const m = JSON.parse(ev.data);
  if (m.t !== 'manifest') return;
  const p = m.params.find((x) => x.id === id);
  if (!p) {
    console.error(`unknown param "${id}"`);
    process.exit(1);
  }
  const n = Math.max(0, Math.min(1, Number(value)));
  const b = new ArrayBuffer(4 + 8 * 3);
  const v = new DataView(b);
  v.setUint8(0, 0x11);
  v.setUint16(2, 3, true);
  [0, 1, 2].forEach((phase, i) => {
    const o = 4 + i * 8;
    v.setUint16(o, p.index, true);
    v.setUint8(o + 2, phase);
    v.setFloat32(o + 4, n, true);
  });
  ws.send(b);
  console.log(`${id} (#${p.index}) <- ${n}`);
  setTimeout(() => {
    ws.close();
    process.exit(0);
  }, 50);
};
ws.onerror = () => {
  console.error('connection failed');
  process.exit(1);
};
