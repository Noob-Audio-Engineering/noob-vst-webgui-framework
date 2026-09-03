# Troubleshooting

Symptoms, likely causes, and what to do. When in doubt, run
`node tools/instances.mjs` to see what is running and on which port, and
open the instance's URL in a normal browser: if the page works there, the
problem is in the web view or the host, not in vst3-web-stratum.

## The editor window is blank

* **No `web/dist`.** The plug-in embeds `web/dist` at compile time. If the
  page 404s, rebuild the SPA (`npm run build` in the example's `web/`) and
  rebuild the plug-in. The standalone prints "web/dist not found" with the
  commands to run.
* **WebView2 runtime missing (Windows 10).** The adapter logs "embedded web
  view unavailable" and opens the page in the system browser instead. Install
  the Evergreen WebView2 runtime.
* **WebKitGTK missing (Linux).** Install `libwebkit2gtk-4.1` (and the `-dev`
  package to build). Same fallback as above.
* **Unsupported parent window.** Hosts that hand the editor an unusual
  window handle get the system-browser fallback and a log line.

## The page loads but says "disconnected" or reconnects forever

* The page's origin and the server differ. The client connects to
  `location.host` by default; a page served by Vite must proxy `/ws` (the
  example configs do, using `VST3_WEB_STRATUM_PORT`). If you open `dist/index.html`
  from the file system there is no server behind it.
* The server died. The standalone prints its URL on start; a plug-in logs
  through nih-plug's `nih_log!`. Reopen the editor to restart the server (it
  starts lazily on first `spawn`).
* A firewall or endpoint-security product blocks loopback WebSockets. Rare,
  but seen with some corporate agents; allow the plug-in host binary.

## "Address already in use" or the standalone panics on start

* You passed `--port N` and `N` is taken. Drop the flag to let it probe, or
  pick another port.
* The previous copy is still running: `node tools/instances.mjs` shows it;
  kill it or use it.

## Values on the page do not match the host

* The page shows normalized values converted with the manifest table; the
  host shows its own formatting. Small rounding differences in the last digit
  are expected. Large differences mean the id maps to a different parameter:
  check the manifest (`/ws` handshake, or `client.manifest.params` in the
  devtools console).
* Automation from the host is not visible: the adapter pushes host changes
  from the `Editor` callbacks; if the host only calls `param_values_changed`
  the adapter resyncs everything, which is correct but coarse. If nothing
  arrives at all, the editor is not open (the callbacks stop when the window
  closes; a detached browser tab still sees UI-originated changes).

## Edits from the page do not reach the host

* Inside a plug-in, edits are forwarded from the UI timer. If the host has no
  usable timer (only Windows has one today) the adapter forwards them from
  the net thread. If neither happens, the log shows why.
* `drain_edits` is never called in a standalone: the host loop must call it
  (the examples do every 5 ms).

## Streams are choppy or late

* The client is throttled: check `stream.subscribe` calls in your page; a
  hidden panel may have set an interval or disabled the stream.
* The pump is polling only (`WakeMode::Poll`) with a long `poll_interval`.
* The page draws on every frame instead of on `requestAnimationFrame`, and
  drawing is slower than the frame rate. Draw the latest frame per refresh.
* A slow client forced resyncs: `client.stats` shows frames per second; the
  server logs a resync per client when it happens repeatedly.

## The wavetable, curve, or another on-change display is empty until something changes

The stream is not sticky. Add `.sticky()` to its `StreamSpec`; the server
then replays the last frame to late clients.

## Presets or favourites disappeared

* They live in the UI store since the multi-instance work, not in
  `localStorage`. A plug-in persists them in its state: reload the session.
  A standalone keeps them in `<data dir>/vst3-web-stratum/<name>.store.json`.
* A state saved before the store existed has no store; that is expected.
* A value over 256 KiB or a store over 1 MiB is rejected; the page logs a
  `store.error` warning in the console.

## Two instances show each other's state

They cannot, unless they share a port, which the probe policy prevents.
Check `node tools/instances.mjs`: two records with the same port means one
of them was started with `--port` while the other held it, or a stale record
survived (it is deleted on the next scan).

## `cargo build` fails with "cannot open output file" on Windows

The standalone you are rebuilding is running. Kill it:
`taskkill /F /IM noob-q-standalone.exe`.

## `include_dir!` panics during `cargo build --features plugin`

`web/dist` does not exist. Build the SPA first.

## No sound from noob-wave

* The standalone printed "running silently": no default output device, or
  cpal could not open it. Plug something in, or accept silence (the UI still
  works).
* The device is exclusive to another application (ASIO drivers do this).
* The output meter moves but you hear nothing: the OS routed the default
  device somewhere else (a monitor's HDMI audio is a common one, and is the
  device the example picked in testing).
* `node tools/play.mjs 4243` reports the peak level; if it is silent there
  too, the synth is not receiving events: check that the page is connected
  to the right port.

## The analyzer looks wrong (cut off, wrong scaling)

The frequency scale measures its own width; a container with zero width at
mount time (a hidden tab) draws nothing until it is shown. The display range
selector (`display_range`) changes the dB scale of the curve, not the
analyzer, which has its own range in the analyzer panel.

## Something else

* Turn on logging: the standalones use `env_logger` (`RUST_LOG=debug`); the
  plug-ins log through nih-plug (`NIH_LOG=stderr` or a file path).
* Open the page in a browser with devtools: `client.stats`,
  `client.manifest`, `client.store`, and the `store.error` / decode warnings
  are all visible in the console.
* Read [WIRE.md](WIRE.md) and watch the frames with a small script (see
  [TOOLS.md](TOOLS.md#writing-your-own)).
