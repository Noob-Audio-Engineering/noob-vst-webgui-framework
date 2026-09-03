# Performance

The headline requirement of vst3-web-stratum is control latency: a knob drag in the
page must reach the audio thread, and the echo must come back, fast enough
that nothing feels like a web page. This document gives the numbers, how
they were measured, where the time goes, and the knobs that trade latency,
bandwidth and CPU against each other.

## Numbers

Measured with `node tools/bench.mjs 4242` against the Noob-Q standalone
built in release mode, on Windows 11, loopback, with 386 parameters and eight
streams live (two spectra at 93 frames/s, meters and band telemetry at block
rate). One run, 2000 samples each:

| Measurement | p50 | p90 | p99 | max | mean |
|---|---|---|---|---|---|
| Ping round trip (`Ping` → `Pong`) | 42 µs | 61 µs | 143 µs | 516 µs | 49 µs |
| Edit echo (`ParamEdit` → `ParamValues` back) | 51 µs | 77 µs | 138 µs | 798 µs | 58 µs |

Stream delivery in the same run:

| Stream | Rate | Bandwidth | Gap p50 | Gap p99 |
|---|---|---|---|---|
| `spectrum_pre`, `spectrum_post` (1025 bins) | 93.7 frames/s | 3.1 Mbit/s each | 10.6 ms | 11.3 ms |
| `meter_in`, `meter_out` (4 values) | 187.7 frames/s | 54 kbit/s | 5.5 ms | 5.8 ms |
| `band_dyn` (24 values) | 187.7 frames/s | 174 kbit/s | 5.5 ms | 5.8 ms |
| `band_level` (24 values) | 47 frames/s | 44 kbit/s | 21.3 ms | 21.8 ms |
| `curve` (sticky, on change) | 0.3 frames/s | 3 kbit/s | n/a | n/a |

The gap percentiles track the audio block period (256 samples at 48 kHz is
5.33 ms) with under half a millisecond of jitter, which means frames are
delivered as they are produced, not batched.

The synth (`noob-wave`) measures the same way; its edit echo was 90 µs at the
median in a debug build.

For comparison, the same edit over an HTTP request per change on the same
machine costs a few milliseconds, and a WebSocket with Nagle enabled costs up
to 40 ms per small frame.

## Methodology

`tools/bench.mjs` (see [TOOLS.md](TOOLS.md)) is a plain Node WebSocket
client that:

1. connects and reads the manifest;
2. sends 2000 `Ping` frames one at a time, timing `Pong` with
   `performance.now()`;
3. sends 2000 `ParamEdit` frames on the first automatable parameter, one at
   a time, timing the `ParamValues` echo (`echo_edits` is on by default in
   the standalones, and the client sets the echo flag);
4. listens to every stream for three seconds and reports rate, bandwidth and
   the distribution of inter-frame gaps.

Numbers are wall-clock in the client, so they include the Node event loop.
The plug-in side adds nothing measurable: decode, atomic store and re-encode
are well under a microsecond each. Run the bench a few times and look at the
median; the maxima are dominated by the operating system's scheduler.

To measure in a real plug-in window, the page's own `client.stats.echoAvgMs`
and `rttAvgMs` show the same two figures, as the example UIs do in their
top-right corner.

## Where the time goes

For one edit and its echo:

| Step | Cost | Notes |
|---|---|---|
| Page: pointer event to `ws.send` | ~5 µs | 12-byte frame written into a preallocated `ArrayBuffer`; no JSON |
| Loopback TCP, one direction | 10 to 30 µs | `TCP_NODELAY` set on accept; the kernel does the rest |
| Net thread: decode, apply, queue | < 1 µs | atomic store into the parameter store; `try_send` to the host queue |
| Pump wake and coalesce | 5 to 20 µs | `unpark` from the net thread; one frame per client |
| Loopback TCP back | 10 to 30 µs | |
| Page: decode `ParamValues` | ~2 µs | header plus 8 bytes per entry |

The audio thread is not on this path at all. It reads the new value with an
atomic load on its next block; that delay is the host's block size and
belongs to the host, not to vst3-web-stratum.

## Tuning

### Server (`ServerConfig`)

| Option | Default | Effect |
|---|---|---|
| `wake(WakeMode::Unpark)` | Unpark | The audio thread unparks the pump after publishing. Lowest latency. `WakeMode::Poll` never wakes from the audio thread; the pump runs on `poll_interval` only. Use it if your real-time policy forbids any syscall from the audio callback. |
| `poll_interval` | 1 ms | Upper bound on how long a change waits when nothing wakes the pump. Lower it under `Poll`; leave it under `Unpark`. |
| `echo_edits` | on in the examples | Echo a client's own edits back to it (needed for the latency display and for a page that shows the value the plug-in actually stored). Costs one small frame per edit; turn it off for many-client setups. |
| `send_queue` | 64 frames | Per-client outbound queue. Larger tolerates a slower client before a resync is forced; smaller bounds memory and staleness. |
| `max_message_size` | 1 MiB | Largest inbound frame accepted. Only the store can approach it. |

### Bridge (`Vst3WebStratumBuilder`)

| Option | Effect |
|---|---|
| `ui_queue(n)` | Parameter changes waiting for the pump. Overflow is counted in `dropped_ui_changes` and repaired by `sync_all_params`. Raise it if a preset load changes hundreds of parameters at once from a thread other than the pump. |
| `host_queue(n)` | Edits waiting for the host. Overflow means edits are lost; raise it if the host drains rarely. |
| `StreamSpec::new(id, capacity)` | Capacity is the maximum frame length. Memory is three frames per stream (the triple buffer) plus the sticky copy. |

### Streams

* Publish at the rate the display needs, not the rate you have. Meters at
  block rate are fine (tens of bytes). Spectra every second block halve
  bandwidth with no visible difference at 60 Hz displays; the analyzer's own
  averaging hides the rest.
* Publish state-like data (curves, tables, envelopes) **on change** and mark
  the stream `sticky()`, so late clients still get it and idle instances send
  nothing.
* Let pages subscribe with an interval (`stream.subscribe({ intervalMs })`)
  or disable streams they do not show. Throttling is per client and costs
  the audio thread nothing.
* Keep frames as `f32`. Converting to `u8` (`StreamU8`) saves bandwidth for
  images and histograms only.

### Page

* Draw from `requestAnimationFrame`, not from every frame event; keep the
  latest frame and render it once per display refresh. The components in
  `@elyerinfox/vst3-web-stratum/components` do this.
* Do not copy stream data. The `Float32Array` you receive is a view over the
  socket buffer for the duration of the callback; read from it or keep a
  reference to the whole frame if you need it later.
* Coalesce gestures: send `perform` on every pointer move (they are 12
  bytes), but avoid sending a `begin`/`end` pair per move. `Param.set` does
  the right thing.
* Batch preset loads with `client.setMany` (one frame with many entries),
  not a loop of single edits.

### Plug-in side

* Publish streams **after** processing the block, once, then let the single
  `wake` inside `publish` do its job. Do not call `sync_all_params` from the
  audio thread; it is for the host side after a state restore.
* The pump coalesces parameter changes per parameter, so a thousand host
  automation points in one block cost one frame.

## What not to do

* JSON per frame. Encoding a 1025-bin spectrum as text costs more than the
  DSP that produced it and allocates on both ends.
* A fixed port with several instances. See
  [MULTI-INSTANCE.md](MULTI-INSTANCE.md).
* Sleeping in the audio callback to "give the UI time". vst3-web-stratum never needs
  it; every hand-off is non-blocking.
* Waiting for an echo before drawing the knob. Draw the local value at once;
  the echo is for statistics and for reconciling with the host.

## Reproducing

```sh
cargo build --release -p noob-q --bin noob-q-standalone
./target/release/noob-q-standalone     # port 4242
node tools/bench.mjs 4242
```

Vary the load with a second bench process, a browser tab with the analyzer
open, or by setting `analyzer_resolution` to Maximum with
`node tools/setparam.mjs 4242 analyzer_resolution 1`.
