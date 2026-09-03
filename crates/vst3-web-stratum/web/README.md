# `@elyerinfox/vst3-web-stratum`

The browser side of [vst3-web-stratum](../README.md): a dependency-free ES
module that talks to a vst3-web-stratum-bridged plug-in over its local WebSocket, plus
canvas components and an optional Vue 3 layer.

| entry point                   | what it is                                                                    | docs |
|-------------------------------|-------------------------------------------------------------------------------|------|
| `@elyerinfox/vst3-web-stratum`                | `Vst3WebStratumClient`, `Param`, `Stream`, `Store`, `History`, helpers               | this file |
| `@elyerinfox/vst3-web-stratum/components`     | Canvas components: `Knob`, `Meter`, `Spectrum`, `EqCurve`, `Scope`, `Keyboard`, `WavetableView`, `Envelope`, and the filter math behind `EqCurve` | [components/README.md](components/README.md) |
| `@elyerinfox/vst3-web-stratum/vue`            | Composables (`useVst3WebStratum`, `useParam`, `useStore`, ...) and Vue components (`Knob`, `Popover`, `ContextMenu`, `LevelMeter`) | this file, [Vue layer](#the-vue-layer-elyerinfoxvst3-web-stratumvue) |

Every file is plain JavaScript with JSDoc, so an editor with TypeScript
language services shows types and documentation inline. The wire protocol is
specified in [docs/WIRE.md](../../../docs/WIRE.md).

---

## Contents

- [Install and link](#install-and-link)
- [Vite settings](#vite-settings)
- [Five-minute tour](#five-minute-tour)
- [`Vst3WebStratumClient`](#vst3webstratumclient)
- [`Param`](#param)
- [`Stream`](#stream)
- [`Store`](#store)
- [`History`](#history)
- [Events, messages and helpers](#events-messages-and-helpers)
- [The Vue layer (`@elyerinfox/vst3-web-stratum/vue`)](#the-vue-layer-elyerinfoxvst3-web-stratumvue)
- [Theming](#theming)
- [Loading without a bundler](#loading-without-a-bundler)
- [Design notes](#design-notes)

---

## Install and link

The package is not published; the example apps link it from the repository:

```jsonc
// crates/<your-plugin>/web/package.json
{
  "dependencies": {
    "@elyerinfox/vst3-web-stratum": "file:../../../crates/vst3-web-stratum/web",
    "vue": "^3.5"                       // only for @elyerinfox/vst3-web-stratum/vue
  }
}
```

`npm install` creates `node_modules/@elyerinfox/vst3-web-stratum` as a symlink to `crates/vst3-web-stratum/web/`.
`vue` is an optional peer dependency: the core client and the canvas
components never import it.

Exports (from `package.json`):

```
.               -> vst3-web-stratum.js
./components    -> components/index.js
./components/*  -> components/*.js
./vue           -> vue/index.js
./vue/*         -> vue/*
```

## Vite settings

Because the package is a symlink into the repo, and its Vue layer must use
the *app's* copy of `vue`, four settings are needed. The example
`vite.config.js` files carry them:

```js
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  base: './',                         // the plug-in serves the page from an arbitrary origin
  resolve: {
    preserveSymlinks: true,           // keep node_modules/@elyerinfox/vst3-web-stratum as a link into web/
    dedupe: ['vue'],                  // one Vue runtime, even for files under crates/vst3-web-stratum/web/vue
  },
  server: {
    fs: { allow: [repoRoot] },        // let the dev server read files outside the app folder
    proxy: {
      '/ws': { target: `ws://127.0.0.1:${port}`, ws: true },   // hot reload against a running standalone
      '/instance': { target: `http://127.0.0.1:${port}` },      // also matches /instances
    },
  },
});
```

Tailwind must scan the Vue components in `crates/vst3-web-stratum/web/vue` for utility classes, or
the classes they use are purged:

```css
/* src/style.css */
@import 'tailwindcss';
@source '../../../../crates/vst3-web-stratum/web/vue';
```

Scoped styles that use `@apply` need `@reference '../style.css';` at the top
of the `<style scoped>` block (Tailwind v4).

## Five-minute tour

```js
import { Vst3WebStratumClient } from '@elyerinfox/vst3-web-stratum';

// 1. Connect. Resolves once the manifest (parameter + stream list) is in.
const client = await Vst3WebStratumClient.connect();          // ws://<page host>/ws

// 2. Parameters: normalized 0..1 on the wire, plain values through the taper.
const cutoff = client.param('cutoff');
console.log(cutoff.plain, cutoff.format());            // 1000, "1.00 kHz"
cutoff.on((norm, info) => draw(cutoff.plain));         // host, plug-in, other windows

// 3. Gestures: begin once, set while dragging, end once (hosts record automation).
cutoff.beginEdit();
cutoff.set(0.5);
cutoff.endEdit();
cutoff.setPlain(2000);                                 // one-shot edit, begin+perform+end in one frame

// 4. Telemetry: Float32Array views, zero-copy, latest wins.
client.stream('spectrum').on((bins) => plot(bins));
client.stream('spectrum').subscribe({ maxHz: 30 });    // throttle server-side
client.stream('spectrum').subscribe({ enabled: false }); // hidden panel: stop it

// 5. Events to the audio thread, and from it.
client.noteOn(60, 0.8);
client.on('event', (e) => e.kind === 1 && light(e.a));

// 6. Ad-hoc messages.
client.send('resize', { width: 1180, height: 720 });
client.on('message', (topic, data) => topic === 'status' && show(data));

// 7. Page state that should travel with the plug-in.
client.store.set('presets.user', list);
client.store.on('presets.user', (k, v) => render(v));

// 8. Undo / redo / A-B over the whole parameter set.
import { History } from '@elyerinfox/vst3-web-stratum';
const history = new History(client);
history.undo();  history.toggleAB();
```

For a complete page with no framework and no build step see
[`examples/vanilla/index.html`](examples/vanilla/index.html).

---

## `Vst3WebStratumClient`

One instance per page, one WebSocket to one plug-in instance.

### Construction

```js
new Vst3WebStratumClient(url?, opts?)
Vst3WebStratumClient.connect(url?, opts?) -> Promise<Vst3WebStratumClient>
```

| argument / option        | default                         | meaning |
|--------------------------|---------------------------------|---------|
| `url`                    | `/ws` on the page's host, or `127.0.0.1:<port>` when the page URL has `?port=` | WebSocket URL (`ws://` or `wss://`) |
| `opts.autoReconnect`     | `true`                          | reconnect with backoff (250 ms doubling to 2 s) after a close |
| `opts.pingIntervalMs`    | `1000`                          | period of latency probes; `0` disables them |
| `opts.timeoutMs`         | none                            | `connect()` only: reject (and close) if no manifest arrives in time |

The constructor connects immediately. `connect()` resolves on the first
`'manifest'` event. On reconnect the manifest is applied onto the **same**
`Param` and `Stream` objects, so handles held by the page stay valid, stream
throttles are re-sent, and the store is re-hydrated.

### Properties

| property     | type               | meaning |
|--------------|--------------------|---------|
| `url`        | `string`           | the URL in use |
| `opts`       | `object`           | effective options |
| `connected`  | `boolean`          | socket open |
| `ready`      | `boolean`          | manifest received; `params` / `streams` valid |
| `manifest`   | `object \| null`   | the last manifest (`name`, `protocol`, `meta`, `params`, `streams`) |
| `params`     | `Param[]`          | by wire index |
| `streams`    | `Stream[]`         | by wire index |
| `clientId`   | `number`           | id assigned by the server (from `Hello`) |
| `store`      | `Store`            | the plug-in-persisted UI store |
| `stats`      | `object`           | `rttMs`, `rttAvgMs`, `echoMs`, `echoAvgMs`, `framesIn`, `bytesIn`, `fps`, `kbps` |

### Methods

| method | returns | notes |
|--------|---------|-------|
| `on(event, fn)` | unsubscribe fn | see [Events](#events) |
| `param(id)` | `Param` | throws for an unknown id |
| `stream(id)` | `Stream` | throws for an unknown id |
| `hasParam(id)` / `hasStream(id)` | `boolean` | feature-detect a plug-in build |
| `send(topic, data = null)` | — | ad-hoc JSON `{ t: 'msg', topic, data }`; dropped while not open; `store.*` topics are reserved |
| `sendEvents(events)` / `sendEvent(e)` | — | binary events to the audio thread; each `{ kind, channel, a, b, value, offset }` |
| `noteOn(note, velocity = 1, channel = 0)` | — | `EventKind.NOTE_ON` |
| `noteOff(note, velocity = 0, channel = 0)` | — | `EventKind.NOTE_OFF` |
| `control(number, value, channel = 0)` | — | `EventKind.CONTROL`, value 0..1 |
| `ping()` | — | one latency probe; result in `stats.rttMs` |
| `snapshot()` | `Float32Array` | every parameter's normalized value by index |
| `setMany(values, { emit = true })` | changed count | one frame of begin/perform/end for every parameter that changes; `values` is a snapshot or an iterable of `[Param \| id \| index, norm]`; `emit: false` suppresses the `'edit'` event |
| `applySnapshot(snapshot, opts)` | changed count | alias of `setMany` |
| `close()` | — | close and stop reconnecting (not reopenable) |

### Events

```js
const off = client.on('manifest', (m, client) => { ... });
off();
```

| event        | listener arguments   | fires |
|--------------|----------------------|-------|
| `'open'`     | `(client)`           | socket connected, before the manifest |
| `'close'`    | `(client)`           | socket closed; a reconnect may follow |
| `'manifest'` | `(manifest, client)` | manifest applied (also after every reconnect) |
| `'message'`  | `(topic, data)`      | a JSON message from the plug-in (store topics are routed to `store` instead) |
| `'error'`    | `(errorOrEvent)`     | socket error, or the constructor threw |
| `'stats'`    | `(stats)`            | once per second |
| `'edit'`     | `(param \| null)`    | a local gesture completed, a bare `set()`, or a `setMany()` batch (`null`) |
| `'event'`    | `(uiEvent)`          | each event the plug-in sends (host notes lighting a keyboard, for example) |

`EventKind`: `NOTE_ON 1`, `NOTE_OFF 2`, `CONTROL 3`, `PITCH_BEND 4`,
`AFTERTOUCH 5`, `PROGRAM 6`, `CUSTOM 0x80` (values `>= 0x80` are
plug-in-defined).

---

## `Param`

One parameter, created from the manifest. Never construct one yourself; ask
`client.param(id)`.

### Value spaces

* **Normalized** (`norm`, alias `value`): 0..1, what the wire and the host
  use. Discrete parameters snap to `spec.steps` positions.
* **Plain** (`plain`): the parameter's own units, through the taper:

  | `spec.taper` | plain from normalized `n` |
  |--------------|---------------------------|
  | `linear`     | `min + (max - min) · n` |
  | `log`        | `min · (max / min)^n` (geometric; `min` floored at the smallest positive f32) |
  | `skew`       | `min + (max - min) · n^(1 / skew)` |
  | `table`      | piecewise-linear over `spec.table`, 65 samples from the plug-in (how nih-plug ranges are mirrored) |

  `toPlain(n)` and `toNorm(p)` convert both ways; `toNorm` inverts the table
  by binary search.

### Properties

| property | meaning |
|----------|---------|
| `id`, `name`, `unit`, `group`, `index`, `spec` | from the manifest |
| `norm`, `value` | normalized value |
| `plain` | plain value |
| `min`, `max` | plain range |
| `editing` | inside a gesture (incoming values are ignored meanwhile) |
| `isDiscrete` | `steps > 1` |
| `isToggle` | `steps === 2` |
| `isBipolar` | range crosses zero (controls draw from the centre) |

### Methods

| method | notes |
|--------|-------|
| `on(fn)` | `fn(norm, { local, host, echo, param })`; returns an unsubscribe fn. `local` for changes made through this object on this page; `host` for automation / preset loads. Not called for echoes of this client's own edits, nor while a local gesture is in progress. |
| `beginEdit()` | start a gesture; sends `begin` with the current value |
| `set(norm)` | clamp, snap, apply locally, send. Inside a gesture: `perform`. Outside: `begin` + `perform` + `end` in one frame and the client's `'edit'` event fires. |
| `setPlain(plain)` | `set(toNorm(plain))` |
| `endEdit()` | send `end`, resume incoming values, fire `'edit'` |
| `reset()` | `set(spec.default_norm)` |
| `toPlain(n)`, `toNorm(p)` | conversions |
| `format(plain = this.plain)` | label for enumerations; `On` / `Off` for toggles; integer for other discrete; `k` prefix at 1000+ with a unit (`2.50 kHz`); otherwise 0–2 decimals by magnitude, then the unit |

A knob drag is: `beginEdit()` on pointer down, `set()` on every move,
`endEdit()` on pointer up. That is what lets a host record one automation
gesture and what keeps the host's echo from fighting the pointer.

---

## `Stream`

One telemetry stream, created from the manifest; ask `client.stream(id)`.

The audio thread publishes into a wait-free triple buffer; the server
forwards the newest frame to each client, at most as often as that client's
throttle allows. A slow page drops frames and never builds a backlog.

| property | meaning |
|----------|---------|
| `id`, `name`, `kind`, `capacity`, `channels`, `meta`, `index`, `spec` | from the manifest (`kind` is a hint such as `spectrum`, `meter`, `curve`, `scope`; `meta` carries plug-in data such as `sample_rate`, `fft_size`, `db`) |
| `data` | the latest frame: a `Float32Array` (or `Uint8Array`) view into the socket message; every message has its own buffer, so keeping it is safe, but the next frame is a different array |
| `seq` | sequence number; gaps show dropped frames |
| `ts` | plug-in-side timestamp in ms (monotonic since the bridge was created) |
| `fps` | measured incoming frame rate |

| method | notes |
|--------|-------|
| `on(fn)` | `fn(data, stream)` per frame; returns an unsubscribe fn |
| `subscribe({ maxHz = 0, enabled = true })` | server-side throttle (`0` = every frame) or disable; per client, remembered and re-sent after a reconnect |

Sticky streams (declared by the plug-in) have their last frame replayed to a
client that connects late, so state-like data such as a response curve or a
wavetable is present immediately.

---

## `Store`

`client.store`: a small JSON key-value object that lives in the plug-in, is
saved with the plug-in's state (by the nih-plug adapter) or in a file (by a
standalone), and is shared by every window of the instance. Use it for
presets, favourites and view settings that should travel with the plug-in
rather than with the browser profile.

| member | notes |
|--------|-------|
| `ready` | `true` once the first `store.all` has arrived |
| `get(key, dflt?)` | from the local cache; `dflt` before hydration or for a missing key |
| `has(key)`, `keys()` | inspection |
| `set(key, value)` | optimistic: updates the cache and local listeners at once, then sends `store.set`. `null` / `undefined` removes the key. Values are anything JSON can carry. |
| `on(key \| '*', fn)` | `fn(key, value)` on local sets, on `store.changed` from other windows, and (with `'*'` only) on hydration with `key === null`; returns an unsubscribe fn |

Limits enforced by the server: 256 KiB per value, 1 MiB per store. A refused
write is logged as a warning (`store.error`) and stays only in this page's
cache. The sender does not receive an echo of its own writes.

Vue users: `useStore()` and `useStoredRef(key, dflt)` below.

---

## `History`

```js
const history = new History(client, { limit: 200 });
history.undo(); history.redo(); history.toggleAB(); history.copyToOther();
history.on((h) => update(h.canUndo, h.canRedo, h.ab));
history.destroy();
```

Undo / redo / A-B over whole-parameter snapshots (`client.snapshot()`),
recorded from the client's `'edit'` event, so every control built on `Param`
gets history for free.

* **A step** is one completed gesture, one bare `set()`, or one `setMany()`
  batch (a preset load is one step). Identical snapshots are skipped. Host
  automation is not recorded, but the next local step captures whatever the
  host changed.
* `undo()` / `redo()` return `false` when there is nothing to do. They apply
  snapshots with `emit: false`, so they do not record themselves.
* **A/B**: `toggleAB()` stores the current state in the slot being left and
  applies the other slot (the first ever toggle re-applies the current
  state); `copyToOther()` copies the active state into the inactive slot.
  `ab` is `'A'` or `'B'`.
* `limit` (default 200) caps the undo depth.

---

## Events, messages and helpers

* **Constants**: `PROTOCOL_VERSION`, `FLAG_ECHO`, `FLAG_HOST`, `EventKind`.
* `injectStyle(id, css)`: add a `<style>` once (used by the canvas components).
* `plainOf(v)`: resolve a `Param`, a getter or a number to a plain number
  (lets component options accept any of the three).
* Default export: `Vst3WebStratumClient`.

Reserved message topics handled by the client: `store.all`, `store.changed`,
`store.error`. Topics handled by the nih-plug adapter: `resize`
(`{ width, height }`). The examples use `status`, `sample_rate`, `reset`.

---

## The Vue layer (`@elyerinfox/vst3-web-stratum/vue`)

```js
import { useVst3WebStratum, useParam, useStore, Knob, Popover, ContextMenu, LevelMeter } from '@elyerinfox/vst3-web-stratum/vue';
```

### Composables

| function | returns | when to call |
|----------|---------|--------------|
| `configureClient(opts)` | — | before the first `useVst3WebStratum()`; merges `{ url, autoReconnect, pingIntervalMs }` over the defaults (`pingIntervalMs: 500`) |
| `getClient()` | the page's `Vst3WebStratumClient` (created on first use) | any time |
| `useVst3WebStratum()` | `{ client, history, historyState, ready, connected, manifest, status, stats, modified }` | any time; render parameter UI behind `v-if="ready"` |
| `useParam(id)` | a reactive **handle** (below) | once `ready`; throws for unknown ids |
| `hasParam(id)`, `hasStream(id)` | `boolean` | once `ready` |
| `useStream(id)` | the client-side `Stream` (not reactive; draw from `stream.on()`) | once `ready` |
| `send(topic, data)` | — | any time (dropped while offline) |
| `useStore()` | `{ ready, data, get, set }` with `data` a reactive object of every key | any time; fills in on hydration |
| `useStoredRef(key, dflt)` | writable computed bound to one key (`v-model` works) | any time |
| `stateToJson({ skip })` | `{ id: plain }` for every parameter | once `ready` |
| `loadState(values, { reset = true, skip })` | — | once `ready`; one `setMany` frame, one history step; missing ids reset to defaults unless `reset: false`; clears `modified` |

`useVst3WebStratum()` refs: `ready` (manifest in), `connected` (socket open),
`manifest` (shallow ref; `meta.sample_rate` is patched when the plug-in
sends `sample_rate`), `status` (the last `status` message), `stats` (copy of
`client.stats`, refreshed each second), `modified` (set by any local edit;
reset by `loadState`), `historyState` (`{ canUndo, canRedo, ab }`).

### Parameter handles

`useParam(id)` returns one shared `reactive()` object per id:

| field | kind | meaning |
|-------|------|---------|
| `id`, `name`, `unit`, `spec`, `labels`, `min`, `max`, `dflt` | static | from the manifest (`dflt` is the default *plain* value) |
| `param` | static | the underlying `Param` |
| `isToggle`, `isDiscrete`, `isBipolar` | static | shape hints |
| `norm` | reactive | normalized value |
| `plain` | computed | plain value |
| `text` | computed | `param.format()` |
| `on` | computed | `norm >= 0.5` (toggles) |
| `index` | computed | step index (discrete) |
| `label` | computed | label of the current step, `''` without labels |
| `set(norm)`, `setPlain(plain)`, `setIndex(i)`, `setOn(bool)`, `toggle()` | methods | write (one-shot edits outside a gesture) |
| `begin()`, `end()` | methods | gesture brackets around a series of `set()` |
| `reset()` | method | back to the default |
| `toNorm(plain)`, `toPlain(norm)`, `format(plain)` | methods | conversions |

Reactivity: the handle's `norm` is updated synchronously by the `Param`'s
listener, inside the WebSocket message task; Vue batches the re-render as
usual. Local `set()` updates `norm` before returning, so a knob never lags
its own drag.

### Components

All four live in `vue/components/` and are documented at the top of their
`<script setup>`.

**`Knob`** — rotary control for a handle.

| prop | default | meaning |
|------|---------|---------|
| `p` | required | the handle |
| `ring` | `null` | second handle drawn as a bipolar arc outside the track |
| `size` | `52` | px |
| `label` | `null` → `p.name` | caption |
| `bipolar` | `null` → `p.isBipolar` | arc from the centre |
| `color` | `null` → `--vst3-web-stratum-accent` | arc colour |
| `ringColor` | `'#ff5c5c'` | ring arc colour |
| `showValue` | `true` | formatted value under the knob |
| `sensitivity` | `180` | px of drag per full sweep (Shift ÷ 10) |
| `disabled` | `false` | ignore input, dim |

Pointer: drag (gesture-bracketed), Alt + drag also moves `ring`, Ctrl/Cmd +
click resets, wheel (coalesced into one gesture), double-click for text
entry. Keys: arrows (1 %, Shift 10 %, one step when discrete), Home / End,
Backspace / Delete reset, Enter opens the field; Enter commits, Escape
cancels. Typed values go through `parseValue()`; enumeration labels match by
prefix. Emits nothing.

**`Popover`** — panel anchored to an element, teleported to `body`.

| prop | default | meaning |
|------|---------|---------|
| `open` | `false` | |
| `anchor` | `null` | element to attach to |
| `placement` | `'top'` | `'top'` or `'bottom'`, 6 px away |
| `align` | `'start'` | `'start'`, `'center'`, `'end'` |
| `width` | `0` | fixed width in px; 0 = content |
| `title` | `''` | small heading |

Emits `close` on outside pointerdown or Escape. Default slot. Clamped to the
viewport, repositioned on open and resize.

**`ContextMenu`** — menu at a screen position, teleported to `body`.

| prop | default | meaning |
|------|---------|---------|
| `open` | `false` | |
| `x`, `y` | `0` | viewport px, clamped on screen |
| `items` | `[]` | `{ divider: true }` or `{ label, action?, checked?, disabled?, hint?, color? }` |

Runs `action()` then emits `close`; also closes on outside pointerdown and
Escape.

**`LevelMeter`** — Vue wrapper over the canvas `Meter`.

| prop | default | meaning |
|------|---------|---------|
| `stream` | required | meter stream id |
| `minDb` | `-60` | |
| `maxDb` | `6` | |
| `orientation` | `'vertical'` | or `'horizontal'` |

Fills its parent (which must have a size); exposes `resetClip()`.

### Value helpers (`@elyerinfox/vst3-web-stratum/vue`)

| function | notes |
|----------|-------|
| `freqToNote(hz)` | `{ name: 'A4', cents: 13, midi: 69 }`; middle C is C4 |
| `midiToFreq(midi)` | equal temperament, A4 = 440 |
| `noteName(midi)` | `60` → `'C4'` |
| `noteToFreq(text)` | `'A4'`, `'C#3+13'`, `'D#5 -7'` → Hz, or `NaN` |
| `noteLabel(hz)` | `'A4 +13'` |
| `parseValue(text, handle)` | typed text → plain value: `50%` (of range), `1k` / `1.5kHz` and note names for Hz, `2x` for dB, `250ms` / `0.5s` for time, plain numbers (decimal comma accepted); `NaN` when unusable |

---

## Theming

Everything visual reads CSS custom properties with a fallback, so a page can
restyle the library from one `:root` block. Set them in your stylesheet (the
example apps do it inside Tailwind's `@theme`):

| variable | used by | fallback |
|----------|---------|----------|
| `--vst3-web-stratum-accent` | knob arc and focus ring, menu ticks, spectrum accents | `#5ac8fa` |
| `--vst3-web-stratum-text` | values, pointer line, panel text | `#e2e8f0` |
| `--vst3-web-stratum-text-dim` | labels, hints, titles | `#64748b` |
| `--vst3-web-stratum-bg` | knob text field background | `#0d1016` |
| `--vst3-web-stratum-panel` | popover and menu background | `rgba(24, 29, 39, 0.96)` |
| `--vst3-web-stratum-border` | popover and menu border, dividers | `rgba(255, 255, 255, 0.1)` |
| `--vst3-web-stratum-track` | knob and meter tracks (canvas components) | component-specific |
| `--vst3-web-stratum-grid`, `--vst3-web-stratum-grid-strong` | spectrum / curve grids | component-specific |
| `--vst3-web-stratum-curve`, `--vst3-web-stratum-knob-body`, `--vst3-web-stratum-key-white`, `--vst3-web-stratum-key-black`, `--vst3-web-stratum-key-border`, `--vst3-web-stratum-key-remote` | canvas components | see [components/README.md](components/README.md) |

The Vue components use scoped, plain CSS (no Tailwind classes), so they work
in any app; only the example apps' own components rely on Tailwind.

## Loading without a bundler

Every vst3-web-stratum server also serves this library at `/vst3-web-stratum/`, so a page
embedded in a plug-in can import it with no build step:

```html
<script type="module">
  import { Vst3WebStratumClient } from '/vst3-web-stratum/vst3-web-stratum.js';
  import { Knob, Spectrum } from '/vst3-web-stratum/components/index.js';
</script>
```

The Vue layer needs a bundler (it ships `.vue` single-file components).

## Design notes

* **No product knowledge.** Parameter ids, ranges, tapers and labels come
  from the manifest; formatting is generic. Product-specific rules (band
  colours, Pro-Q style displays) belong in the app.
* **Gestures, not values.** `beginEdit` / `set` / `endEdit` map onto the
  host's begin / perform / end, so automation is recorded correctly and the
  host's echo cannot fight a drag. Echoes of the client's own edits are
  swallowed and only measured (`stats.echoMs`).
* **Zero-copy telemetry.** Stream frames are typed-array views over the
  socket message at byte 20; edit and ping frames reuse pre-allocated
  buffers.
* **Stable identity.** `Param` and `Stream` objects survive reconnects, so a
  page subscribes once and never re-wires.
* **Latest wins.** Streams never queue; parameter values and events are
  never dropped silently (the server resyncs a client that fell behind).
