#!/usr/bin/env node
// List the noob-vst-webgui-framework instances running on this machine.
//
//   node tools/instances.mjs                 # scan the discovery directory, probe each
//   node tools/instances.mjs --name noob-q   # only instances of one plug-in
//   node tools/instances.mjs 4242            # ask a running server: instances of ITS plug-in
//   node tools/instances.mjs 4242 --all      # ... or every noob-vst-webgui-framework instance it can see
//   node tools/instances.mjs --json          # machine-readable
//
// Discovery files live in %LOCALAPPDATA%\noob-vst-webgui-framework\instances (Windows),
// ~/Library/Application Support/noob-vst-webgui-framework/instances (macOS) or
// $XDG_RUNTIME_DIR/noob-vst-webgui-framework/instances (Linux). Each server writes one on start
// and removes it on a clean stop; stale files (crashes) are dropped here when
// the probe fails.

import { readdir, readFile, unlink } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const json = args.includes('--json');
const all = args.includes('--all');
const port = args.find((a) => /^\d+$/.test(a));
const nameIx = args.indexOf('--name');
const onlyName = nameIx >= 0 ? args[nameIx + 1] : null;

function discoveryDir() {
  if (process.platform === 'win32') {
    return join(process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local'), 'noob-vst-webgui-framework', 'instances');
  }
  if (process.platform === 'darwin') {
    return join(homedir(), 'Library', 'Application Support', 'noob-vst-webgui-framework', 'instances');
  }
  const base = process.env.XDG_RUNTIME_DIR || join(homedir(), '.local', 'state');
  return join(base, 'noob-vst-webgui-framework', 'instances');
}

async function probe(p, ms = 500) {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), ms);
  try {
    const r = await fetch(`http://127.0.0.1:${p}/instance`, { signal: ctl.signal });
    if (!r.ok) return null;
    return await r.json();
  } catch {
    return null;
  } finally {
    clearTimeout(t);
  }
}

async function scan() {
  const dir = discoveryDir();
  let files = [];
  try {
    files = (await readdir(dir)).filter((f) => f.endsWith('.json'));
  } catch {
    return { dir, live: [] };
  }
  const live = [];
  await Promise.all(
    files.map(async (f) => {
      const path = join(dir, f);
      let rec;
      try {
        rec = JSON.parse(await readFile(path, 'utf8'));
      } catch {
        return;
      }
      const seen = await probe(rec.port);
      if (seen && seen.pid === rec.pid) live.push(seen);
      else await unlink(path).catch(() => {});
    }),
  );
  live.sort((a, b) => a.port - b.port);
  return { dir, live };
}

let result;
if (port) {
  // A server lists only instances of its own plug-in unless asked for all.
  const r = await fetch(`http://127.0.0.1:${port}/instances${all ? '?all=1' : ''}`);
  result = { via: `127.0.0.1:${port}`, live: await r.json() };
} else {
  result = await scan();
}
if (onlyName) result.live = result.live.filter((i) => i.name === onlyName);

if (json) {
  console.log(JSON.stringify(result, null, 2));
} else {
  const { live } = result;
  if (result.dir) console.log(`discovery: ${result.dir}`);
  if (result.via) console.log(`via: ${result.via}`);
  if (!live.length) {
    console.log('no live instances');
  } else {
    const w = Math.max(...live.map((i) => i.name.length), 4);
    console.log(`${'name'.padEnd(w)}  pid     port   url`);
    for (const i of live) {
      console.log(`${i.name.padEnd(w)}  ${String(i.pid).padEnd(7)} ${String(i.port).padEnd(6)} ${i.url}`);
    }
  }
}
