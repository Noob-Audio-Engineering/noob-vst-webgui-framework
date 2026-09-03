# Tools

Four Node scripts in `tools/` talk to a running vst3-web-stratum server over its
WebSocket or HTTP endpoints. They need Node 20 or newer (they use the global
`WebSocket` and `fetch`) and nothing else; there is no `npm install` step.

All of them take the port of the instance as the first argument. Find ports
with `instances.mjs` or read them from a standalone's start-up banner.

## `instances.mjs`: list running instances

```sh
node tools/instances.mjs            # scan the discovery directory and probe each record
node tools/instances.mjs 4242       # ask the server on port 4242 for its view (/instances)
node tools/instances.mjs --json     # machine-readable
```

```
discovery: C:\Users\you\AppData\Local\vst3-web-stratum\instances
name       pid     port   url
noob-q     34080   4242   http://127.0.0.1:4242/
noob-wave  39288   4243   http://127.0.0.1:4243/
```

Without a port it reads every `<pid>-<port>.json` in the discovery
directory, fetches `/instance` from that port with a 500 ms timeout, keeps
the record if the answer carries the same pid, and deletes the file
otherwise (a crashed instance). With a port it prints what that server's
`/instances` returns, which is the same scan done server-side. See
[MULTI-INSTANCE.md](MULTI-INSTANCE.md#discovery).

## `bench.mjs`: latency and throughput

```sh
node tools/bench.mjs 4242
```

```
connected to "noob-q": 386 params, 8 streams
ping rtt     n= 2000  p50     42 µs  p90     61 µs  p99    143 µs  max    516 µs  mean     49 µs
edit echo    n= 2000  p50     51 µs  p90     77 µs  p99    138 µs  max    798 µs  mean     58 µs

streams over 3s:
  spectrum_pre    93.7 frames/s     3087 kbit/s  gap p50 10.59 ms  p99 11.26 ms  max 11.36 ms
  ...
```

* **ping rtt**: a `Ping` frame answered by `Pong` on the net thread. The
  floor of the transport.
* **edit echo**: a `ParamEdit` (perform phase, echo flag set) on the first
  automatable parameter, timed until the `ParamValues` frame that carries
  the value back. Requires `echo_edits` on the server, which the standalones
  and the adapter enable by default.
* **streams**: for every stream the client is subscribed to, frames per
  second, bandwidth, and the distribution of gaps between consecutive
  frames. Gaps that track the block period mean frames are delivered as
  produced.

The bench edits a real parameter (it restores nothing), so run it against a
standalone, not against a plug-in in a session you care about.
[PERFORMANCE.md](PERFORMANCE.md) explains how to read the numbers.

## `setparam.mjs`: set one parameter

```sh
node tools/setparam.mjs <port> <id> <normalized>
node tools/setparam.mjs 4242 b1_freq 0.35
node tools/setparam.mjs 4243 filter_cutoff 0.8
```

Connects, waits for the manifest, resolves the id to an index, sends a full
gesture (begin, perform, end) with the normalized value, prints the plain
value the server reports back, and exits. Useful for scripting states and
for checking that an id exists. Use `0..1` values; convert from plain with
the manifest table if you need to (or use the page).

## `play.mjs`: play a note headlessly

```sh
node tools/play.mjs <port> [note=60] [hold_ms=300]
node tools/play.mjs 4243 60 400
```

```
synth "noob-wave": note 60 held 400 ms
  peak while held : -11.6 dBFS
  peak after off  : -inf
  OK: it sounds and releases
```

Sends a note-on event, subscribes to the output meter stream, holds the note,
sends note-off, and reports the peak level while held and after release. The
exit code is non-zero if the synth was silent or did not release, which makes
it usable as a smoke test in CI-like scripts. Only meaningful for an
instrument that consumes `Events` frames (Noob-Wave does; Noob-Q ignores
them).

## Writing your own

The scripts are short and self-contained; copy one. The essentials:

```js
const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`);
ws.binaryType = 'arraybuffer';
ws.onmessage = (ev) => {
  if (typeof ev.data === 'string') {
    const m = JSON.parse(ev.data);          // manifest or { t: 'msg', topic, data }
  } else {
    const view = new DataView(ev.data);     // [kind u8][flags u8][arg u16] ...
  }
};
```

Frame layouts are in [WIRE.md](WIRE.md). For anything beyond a few lines,
import `@elyerinfox/vst3-web-stratum` (`crates/vst3-web-stratum/web/vst3-web-stratum.js`) instead: it runs unchanged in Node
20+ and gives you `Param`, `Stream` and `Store` handles.
