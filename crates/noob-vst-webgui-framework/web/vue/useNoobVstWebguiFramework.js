/**
 * Vue bridge over the framework-agnostic noob-vst-webgui-framework client.
 *
 * One client per page. `useParam(id)` returns a *reactive* handle whose
 * `norm` / `plain` / `text` / `on` / `index` / `label` fields follow the
 * plugin (host automation, other UIs) and whose `set()` / `begin()` /
 * `end()` send gestures back. Handles are cached, so every component that
 * asks for the same id shares one subscription.
 *
 *   import { useNoobVstWebguiFramework, useParam } from '@noob-audio-engineering/noob-vst-webgui-framework/vue';
 *   const { ready } = useNoobVstWebguiFramework();
 *   const cutoff = useParam('cutoff');     // once ready
 *   cutoff.plain, cutoff.text, cutoff.set(0.5)
 *
 * ## How it fits together
 *
 * * A single `NoobVstWebguiFrameworkClient` is created lazily by `getClient()` (or the
 *   first composable that needs it) using the options given to
 *   `configureClient()`. Module-level refs (`ready`, `connected`,
 *   `manifest`, `status`, `stats`, `modified`) mirror the client's events,
 *   so any component can `useNoobVstWebguiFramework()` and render from them.
 * * `useParam(id)` wraps `client.param(id)` in a `reactive()` object with
 *   computed views of the value. The underlying `Param` survives
 *   reconnects, and so does the handle.
 * * A `History` is attached to the client, with a reactive `historyState`
 *   for toolbar buttons.
 * * `useStore()` mirrors `client.store` into a reactive object, and
 *   `useStoredRef(key, dflt)` binds one key to a writable computed.
 * * `stateToJson()` / `loadState()` turn the whole parameter set into a
 *   `{ id: plain }` object and back: the building block for presets.
 *
 * ## Reactivity guarantees
 *
 * Every field on a handle is driven by the `Param`'s own listener, which
 * runs synchronously inside the WebSocket message task. Vue batches the
 * resulting re-render into the next microtask / animation frame as usual.
 * Setting through a handle updates the handle's `norm` synchronously (the
 * `Param` emits before returning), so a knob never lags its own drag.
 *
 * ## Ordering
 *
 * `useNoobVstWebguiFramework()` and `getClient()` may be called at any time. `useParam()`,
 * `useStream()`, `stateToJson()` and `loadState()` need the manifest: call
 * them once `ready` is true (render the parameter UI behind
 * `v-if="ready"`), or they throw for unknown ids. `useStore()` and
 * `useStoredRef()` may be called early; they fill in when the store is
 * hydrated.
 */
import { computed, getCurrentScope, onScopeDispose, reactive, ref, shallowRef } from 'vue';
import { History, NoobVstWebguiFrameworkClient } from '../noob-vst-webgui-framework.js';
import { NeedleModel } from '../components/needle.js';

/**
 * @typedef {object} ParamHandle
 * The reactive object returned by {@link useParam}. Read fields in
 * templates; call the methods from event handlers.
 * @property {string} id            Parameter id.
 * @property {import('../noob-vst-webgui-framework.js').Param} param The underlying client-side parameter (not reactive).
 * @property {string} name          Display name.
 * @property {string} unit          Unit suffix (`'Hz'`, `''`).
 * @property {import('../noob-vst-webgui-framework.js').ParamSpec} spec The manifest entry.
 * @property {string[]} labels      Enumeration labels, empty when none.
 * @property {boolean} isToggle     Two positions.
 * @property {boolean} isDiscrete   Finite positions.
 * @property {boolean} isBipolar    Plain range crosses zero.
 * @property {number} min           Plain minimum.
 * @property {number} max           Plain maximum.
 * @property {number} dflt          Default plain value.
 * @property {number} norm          Normalized value 0..1 (reactive ref, unwrapped).
 * @property {number} plain         Plain value (computed).
 * @property {string} text          Formatted value with unit (computed).
 * @property {boolean} on           `norm >= 0.5`, for toggles (computed).
 * @property {number} index         Step index for discrete parameters (computed).
 * @property {string} label         Label of the current step, `''` without labels (computed).
 * @property {(norm: number) => void} set        Set the normalized value (a one-shot edit outside a gesture).
 * @property {(plain: number) => void} setPlain  Set a plain value.
 * @property {(i: number) => void} setIndex      Choose a step by index (clamped).
 * @property {(on: boolean) => void} setOn       Set a toggle.
 * @property {() => void} toggle                 Flip a toggle.
 * @property {() => void} begin                  Start a gesture (`beginEdit`).
 * @property {() => void} end                    End a gesture (`endEdit`).
 * @property {() => void} reset                  Back to the default.
 * @property {(plain: number) => number} toNorm  Convert plain -> normalized.
 * @property {(norm: number) => number} toPlain  Convert normalized -> plain.
 * @property {(plain: number) => string} format  Format a plain value like `text` does.
 */

let client = null;
let history = null;
/** Options for the client; see {@link configureClient}. */
let clientOpts = { pingIntervalMs: 500 };
/** True once the manifest has arrived (parameter handles may be created). */
const ready = ref(false);
/** True while the socket is open. */
const connected = ref(false);
/** The latest manifest (`shallowRef`; replace, do not mutate). */
const manifest = shallowRef(null);
/** The plug-in's last `status` message, if it sends one (the examples send one per second). */
const status = shallowRef(null);
/** A copy of `client.stats`, refreshed once per second. */
const stats = shallowRef({ rttMs: NaN, rttAvgMs: NaN, echoMs: NaN, echoAvgMs: NaN, fps: 0, kbps: 0 });
/** Reactive mirror of the `History` flags for toolbar buttons. */
const historyState = reactive({ canUndo: false, canRedo: false, ab: 'A' });
/** Set when any local gesture completes; reset it after loading a preset. */
const modified = ref(false);

/**
 * Call before the first `useNoobVstWebguiFramework()` to change the connection options.
 *
 * Merged over the defaults (`pingIntervalMs: 500`). Accepts every
 * `NoobVstWebguiFrameworkClient` option plus `url` for the WebSocket URL. Calls made after
 * the client exists have no effect.
 * @param {{ url?: string, autoReconnect?: boolean, pingIntervalMs?: number }} opts
 * @returns {void}
 */
export function configureClient(opts) {
  clientOpts = { ...clientOpts, ...opts };
}

/**
 * The page's single `NoobVstWebguiFrameworkClient`, created on first use.
 *
 * Wires the module refs to the client's events: `'open'` / `'close'` drive
 * `connected`; `'manifest'` sets `manifest`, `ready` and clears `modified`;
 * `'stats'` refreshes `stats`; `'message'` with topic `status` fills
 * `status`, and `sample_rate` patches `manifest.meta.sample_rate` so
 * analyzers follow the host's rate; `'edit'` sets `modified`. Also creates
 * the `History`.
 * @returns {NoobVstWebguiFrameworkClient}
 */
export function getClient() {
  if (!client) {
    client = new NoobVstWebguiFrameworkClient(clientOpts.url || null, clientOpts);
    client.on('open', () => (connected.value = true));
    client.on('close', () => (connected.value = false));
    client.on('manifest', (m) => {
      manifest.value = m;
      ready.value = true;
      modified.value = false;
    });
    client.on('stats', (s) => (stats.value = { ...s }));
    client.on('message', (topic, data) => {
      if (topic === 'status') status.value = data;
      if (topic === 'sample_rate' && manifest.value) {
        manifest.value = { ...manifest.value, meta: { ...manifest.value.meta, sample_rate: data.sample_rate } };
      }
    });
    client.on('edit', () => (modified.value = true));
    history = new History(client);
    history.on(() => {
      historyState.canUndo = history.canUndo;
      historyState.canRedo = history.canRedo;
      historyState.ab = history.ab;
    });
  }
  return client;
}

/**
 * Everything a component needs to render connection state and toolbars.
 *
 * Safe to call at any time, from any component; the same objects are
 * returned every time.
 * @returns {{
 *   client: NoobVstWebguiFrameworkClient,
 *   history: History,
 *   historyState: { canUndo: boolean, canRedo: boolean, ab: 'A'|'B' },
 *   ready: import('vue').Ref<boolean>,
 *   connected: import('vue').Ref<boolean>,
 *   manifest: import('vue').ShallowRef<object|null>,
 *   status: import('vue').ShallowRef<object|null>,
 *   stats: import('vue').ShallowRef<object>,
 *   modified: import('vue').Ref<boolean>,
 * }}
 */
export function useNoobVstWebguiFramework() {
  getClient();
  return { client, history, historyState, ready, connected, manifest, status, stats, modified };
}

/** Handle cache by parameter id, so every component shares one subscription. */
const handles = new Map();

/**
 * Reactive handle for one parameter. Call only once `ready` is true.
 *
 * Returns the same handle for the same id. The handle's `norm` follows the
 * `Param` through a subscription that is never removed (handles live as
 * long as the page). See {@link ParamHandle} for the fields.
 * @param {string} id
 * @returns {ParamHandle}
 * @throws {Error} For an id that is not in the manifest.
 */
export function useParam(id) {
  let h = handles.get(id);
  if (h) return h;
  const p = getClient().param(id);
  const norm = ref(p.value);
  p.on((v) => (norm.value = v));
  const last = Math.max(1, p.spec.steps - 1);
  h = reactive({
    id,
    param: p,
    name: p.name,
    unit: p.unit,
    spec: p.spec,
    labels: p.spec.labels || [],
    isToggle: p.isToggle,
    isDiscrete: p.isDiscrete,
    isBipolar: p.isBipolar,
    min: p.spec.min,
    max: p.spec.max,
    dflt: p.spec.default,
    norm,
    plain: computed(() => p.toPlain(norm.value)),
    text: computed(() => p.format(p.toPlain(norm.value))),
    on: computed(() => norm.value >= 0.5),
    index: computed(() => Math.round(norm.value * last)),
    label: computed(() => (p.spec.labels && p.spec.labels.length ? p.spec.labels[Math.round(norm.value * last)] : '')),
    set: (n) => p.set(n),
    setPlain: (v) => p.setPlain(v),
    setIndex: (i) => p.set(Math.max(0, Math.min(last, i)) / last),
    setOn: (b) => p.set(b ? 1 : 0),
    toggle: () => p.set(p.value >= 0.5 ? 0 : 1),
    begin: () => p.beginEdit(),
    end: () => p.endEdit(),
    reset: () => p.reset(),
    toNorm: (v) => p.toNorm(v),
    toPlain: (n) => p.toPlain(n),
    format: (v) => p.format(v),
  });
  handles.set(id, h);
  return h;
}

/**
 * Whether the manifest declares this parameter (valid once `ready`).
 * @param {string} id
 * @returns {boolean}
 */
export function hasParam(id) {
  return getClient().hasParam(id);
}

/**
 * The client-side `Stream` for an id (not reactive: subscribe with
 * `stream.on()` and draw on a canvas). Valid once `ready`.
 * @param {string} id
 * @returns {import('../noob-vst-webgui-framework.js').Stream}
 * @throws {Error} For an id that is not in the manifest.
 */
export function useStream(id) {
  return getClient().stream(id);
}

/**
 * Whether the manifest declares this stream (valid once `ready`).
 * @param {string} id
 * @returns {boolean}
 */
export function hasStream(id) {
  return getClient().hasStream(id);
}

/**
 * Send an ad-hoc JSON message to the plugin.
 * @param {string} topic
 * @param {any} [data]
 * @returns {void}
 */
export function send(topic, data) {
  getClient().send(topic, data);
}

// ---------------------------------------------------------------------------
// UI store (persisted by the plug-in)
// ---------------------------------------------------------------------------

let storeState = null;

/**
 * Reactive view of `client.store`: `ready`, `data` (a reactive object of
 * every key) and `set(key, value)`. Rendering from `data` re-renders when
 * the plug-in restores state or another client changes a key.
 *
 * Created once and shared. `data` is rebuilt on hydration and patched on
 * every change, so `computed`s over it stay correct. `get(key, dflt)` reads
 * with a default; `set` writes through to the store (and thus to the
 * plug-in and every other window). May be called before the connection is
 * up.
 * @returns {{
 *   ready: import('vue').Ref<boolean>,
 *   data: Record<string, any>,
 *   set: (key: string, value: any) => void,
 *   get: (key: string, dflt?: any) => any,
 * }}
 */
export function useStore() {
  if (storeState) return storeState;
  const c = getClient();
  const data = reactive({});
  const ready = ref(c.store.ready);
  const sync = () => {
    for (const k of Object.keys(data)) if (!c.store.has(k)) delete data[k];
    for (const k of c.store.keys()) data[k] = c.store.get(k);
    ready.value = c.store.ready;
  };
  sync();
  c.store.on('*', (k, v) => {
    if (k == null) sync();
    else if (v == null) delete data[k];
    else data[k] = v;
  });
  storeState = { ready, data, set: (k, v) => c.store.set(k, v), get: (k, d) => (k in data ? data[k] : d) };
  return storeState;
}

/**
 * A writable computed bound to one store key, with a default.
 *
 *   const zoom = useStoredRef('view.zoom', 1);   // v-model="zoom" works
 *
 * Reads give the stored value or `dflt` while the key is absent; writes go
 * through `useStore().set`, so they reach the plug-in and other windows.
 * Write `null` to remove the key.
 * @template T
 * @param {string} key
 * @param {T} dflt
 * @returns {import('vue').WritableComputedRef<T>}
 */
export function useStoredRef(key, dflt) {
  const s = useStore();
  return computed({
    get: () => (key in s.data ? s.data[key] : dflt),
    set: (v) => s.set(key, v),
  });
}

/**
 * `{ id: plain }` for every parameter (the whole plug-in state).
 *
 * Plain values, so a saved preset is readable and independent of tapers.
 * Valid once `ready`.
 * @param {{ skip?: (id: string) => boolean }} [opts]
 * @param {(id: string) => boolean} [opts.skip] Return true to leave a parameter out (UI-only settings).
 * @returns {Record<string, number>}
 */
export function stateToJson({ skip = null } = {}) {
  const out = {};
  for (const p of getClient().params) if (!skip || !skip(p.id)) out[p.id] = p.plain;
  return out;
}

/**
 * Load `{ id: plain }` in one frame. Ids not present are reset to their
 * defaults when `reset` is true; `skip(id)` excludes parameters (UI-only
 * settings, demo sources) from both loading and resetting.
 *
 * Goes through `client.setMany`, so it is one history step and only
 * parameters that actually change are sent. Unknown ids in `values` are
 * ignored. Clears `modified`.
 * @param {Record<string, number>} values
 * @param {{ reset?: boolean, skip?: (id: string) => boolean }} [opts]
 * @param {boolean} [opts.reset=true] Reset parameters missing from `values` to their defaults.
 * @param {(id: string) => boolean} [opts.skip] Parameters to leave untouched.
 * @returns {void}
 */
export function loadState(values, { reset = true, skip = null } = {}) {
  const edits = [];
  for (const p of getClient().params) {
    if (skip && skip(p.id)) continue;
    if (values[p.id] != null) edits.push([p, p.toNorm(values[p.id])]);
    else if (reset) edits.push([p, p.spec.default_norm]);
  }
  getClient().setMany(edits);
  modified.value = false;
}

// ---------------------------------------------------------------------------
// Reactive stream values
// ---------------------------------------------------------------------------

/**
 * A `ref` that follows one element of a stream's frames, updated at most
 * once per animation frame (so a block-rate stream does not trigger a
 * render per block). `unit` converts like the meters do: `'linear'`
 * amplitude to dBFS, `'db'` / `'raw'` as is. Call once `ready` is true.
 *
 *   const gr = useStreamValue('meter', { index: 4, unit: 'db' });   // gr.value
 *
 * @param {string} id Stream id.
 * @param {{ index?: number, unit?: 'raw'|'linear'|'db', initial?: number }} [opts]
 * @returns {import('vue').Ref<number>}
 */
export function useStreamValue(id, { index = 0, unit = 'raw', initial = 0 } = {}) {
  const value = ref(initial);
  const s = getClient().stream(id);
  let latest = initial;
  let pending = false;
  const off = s.on((d) => {
    const raw = d[index] ?? 0;
    latest = unit === 'linear' ? (raw > 0 ? 20 * Math.log10(raw) : -200) : raw;
    if (!pending) {
      pending = true;
      requestAnimationFrame(() => {
        pending = false;
        value.value = latest;
      });
    }
  });
  if (getCurrentScope()) onScopeDispose(off);
  return value;
}

/**
 * A `shallowRef` holding the latest frame (`Float32Array`) of a stream,
 * updated at most once per animation frame. The array is the frame the
 * client received; copy it if you keep it. Call once `ready` is true.
 *
 * @param {string} id Stream id.
 * @returns {import('vue').ShallowRef<Float32Array|null>}
 */
export function useStreamFrame(id) {
  const frame = shallowRef(null);
  const s = getClient().stream(id);
  let latest = null;
  let pending = false;
  const off = s.on((d) => {
    latest = d;
    if (!pending) {
      pending = true;
      requestAnimationFrame(() => {
        pending = false;
        frame.value = latest;
      });
    }
  });
  if (getCurrentScope()) onScopeDispose(off);
  return frame;
}

/**
 * A needle meter's behaviour as reactive state, for a page that draws its
 * own meter face. Feeds a `NeedleModel` from one element of a stream (or
 * from `set(value)` when `id` is `null`) and exposes the animated position
 * once per frame: `frac` (0..1 along the scale), `angle` (degrees, 0 up),
 * `position` (scale units) and `target`. `marks(values)` gives positions
 * for scale marks. Options are the `NeedleModel` options plus `index`,
 * `sweep` (degrees, default 90) and `autoStart` (default true). Call once
 * `ready` is true; stops when the component unmounts.
 *
 *   const gr = useNeedle('meter', { index: 4, unit: 'db', mode: 'reduction' });
 *   <line :transform="`rotate(${gr.angle})`" />
 *
 * @param {string|null} id
 * @param {object} [opts]
 * @returns {{ frac: import('vue').Ref<number>, angle: import('vue').Ref<number>, position: import('vue').Ref<number>, target: import('vue').Ref<number>, model: import('../components/needle.js').NeedleModel, set: (v: number) => void, marks: (values: number[]) => { value: number, frac: number, angle: number }[], stop: () => void }}
 */
export function useNeedle(id, opts = {}) {
  const { index = 0, sweep = 90, autoStart = true, ...modelOpts } = opts;
  const model = new NeedleModel(modelOpts);
  const frac = ref(model.frac());
  const angle = ref((model.angle(undefined, sweep) * 180) / Math.PI);
  const position = ref(model.position);
  const target = ref(model.target);
  let off = null;
  if (id) off = getClient().stream(id).on((d) => (target.value = model.set(d[index] ?? 0)));
  const publish = () => {
    frac.value = model.frac();
    angle.value = (model.angle(undefined, sweep) * 180) / Math.PI;
    position.value = model.position;
  };
  if (autoStart) model.start(publish);
  const stop = () => {
    model.stop();
    off?.();
    off = null;
  };
  if (getCurrentScope()) onScopeDispose(stop);
  return {
    frac,
    angle,
    position,
    target,
    model,
    set: (v) => (target.value = model.set(v)),
    marks: (values) => model.marks(values, sweep).map((m) => ({ ...m, angle: (m.angle * 180) / Math.PI })),
    stop,
  };
}

/**
 * Knob behaviour without a knob: pointer drag (vertical, Shift for fine),
 * wheel (one gesture per burst of notches), double-click to reset, arrow /
 * Home / End keys, all wrapped in begin / set / end gestures on a handle.
 * Spread the returned `handlers` on any element and draw the control
 * yourself from `p.norm`; `dragging` is a ref for hover / active styling.
 *
 *   const { handlers, dragging } = useKnobGesture(input, { sensitivity: 200 });
 *   <svg v-on="handlers" tabindex="0" :class="{ active: dragging }">…</svg>
 *
 * A dial whose printed scale is not linear in the parameter (an attenuator
 * marked in dB, a compressor's Input knob) should turn at a constant rate
 * under the pointer, not the value: pass `rotation` with `toRotation(norm)`
 * and `fromRotation(rot)` (both 0..1) and the drag, wheel and keys move in
 * rotation space, converting back to the parameter through your mapping.
 *
 * @param {object} p A `useParam` handle.
 * @param {{ sensitivity?: number, fine?: number, wheelStep?: number, discrete?: boolean, rotation?: { toRotation: (norm: number) => number, fromRotation: (rot: number) => number } }} [opts]
 *   `sensitivity`: pixels for a full sweep (default 200); `fine`: Shift
 *   multiplier (default 0.2); `wheelStep`: change per notch in rotation
 *   space (default 0.02, or one step for discrete handles); `discrete`: snap
 *   drags to the handle's steps (default: the handle's `isDiscrete`);
 *   `rotation`: the dial's own taper, see above (default: identity).
 * @returns {{ handlers: Record<string, (e: Event) => void>, dragging: import('vue').Ref<boolean> }}
 */
export function useKnobGesture(p, opts = {}) {
  const sensitivity = opts.sensitivity ?? 200;
  const fine = opts.fine ?? 0.2;
  const discrete = opts.discrete ?? p.isDiscrete;
  const last = Math.max(1, (p.spec?.steps || 2) - 1);
  const wheelStep = opts.wheelStep ?? (discrete ? 1 / last : 0.02);
  const toRot = opts.rotation?.toRotation ?? ((n) => n);
  const fromRot = opts.rotation?.fromRotation ?? ((r) => r);
  const dragging = ref(false);
  const clamp = (n) => Math.max(0, Math.min(1, n));
  const snap = (n) => (discrete ? Math.round(n * last) / last : n);
  /** Apply a change expressed in rotation space. */
  const apply = (rot) => p.set(snap(clamp(fromRot(clamp(rot)))));
  let start = null;
  let wheelTimer = 0;
  let wheelOpen = false;
  const move = (e) => {
    if (!start || e.pointerId !== start.id) return;
    const dy = start.y - e.clientY;
    const k = e.shiftKey ? fine : 1;
    apply(start.rot + (dy / sensitivity) * k);
  };
  const up = (e) => {
    if (!start || e.pointerId !== start.id) return;
    start = null;
    dragging.value = false;
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', up);
    p.end();
  };
  const handlers = {
    pointerdown(e) {
      if (e.button !== 0) return;
      e.preventDefault();
      start = { id: e.pointerId, y: e.clientY, rot: toRot(p.norm) };
      dragging.value = true;
      p.begin();
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', up);
      window.addEventListener('pointercancel', up);
    },
    dblclick(e) {
      e.preventDefault();
      p.begin();
      p.reset();
      p.end();
    },
    wheel(e) {
      e.preventDefault();
      if (!wheelOpen) {
        wheelOpen = true;
        p.begin();
      }
      const dir = e.deltaY < 0 ? 1 : -1;
      const step = wheelStep * (e.shiftKey && !discrete ? fine : 1);
      apply(toRot(p.norm) + dir * step);
      clearTimeout(wheelTimer);
      wheelTimer = setTimeout(() => {
        wheelOpen = false;
        p.end();
      }, 180);
    },
    keydown(e) {
      const step = discrete ? 1 / last : e.shiftKey ? 0.001 : 0.01;
      const rot = toRot(p.norm);
      let n = null;
      if (e.key === 'ArrowUp' || e.key === 'ArrowRight') n = rot + step;
      else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') n = rot - step;
      else if (e.key === 'Home') n = 0;
      else if (e.key === 'End') n = 1;
      if (n === null) return;
      e.preventDefault();
      p.begin();
      apply(n);
      p.end();
    },
  };
  return { handlers, dragging };
}

// ---------------------------------------------------------------------------
// Window size
// ---------------------------------------------------------------------------

/**
 * Live window sizing for a page inside a plug-in window. The host cannot
 * resize a nih-plug editor on its own, so the page asks: `request(w, h)`
 * sends a `resize` message (coalesced to one per animation frame, clamped
 * to `min` / `max`, height derived from `aspect` when given) and the
 * adapter resizes the host window and the web view to match; the adapter
 * also remembers the size in the UI store under `storeKey` so the editor
 * reopens at it. `gripHandlers` turn any element into a drag grip
 * (`v-on="gripHandlers"`), sending sizes while dragging. `width` and
 * `height` follow the viewport. `enabled` is false in a browser tab (the
 * manifest says `standalone`), where the page simply follows the tab.
 *
 *   const { enabled, dragging, gripHandlers, request } = useWindowSize({ min: [900, 520], aspect: 1100 / 620 });
 *
 * @param {{ min?: [number, number], max?: [number, number], aspect?: number|null, storeKey?: string, enabled?: boolean|null }} [opts]
 * @returns {{ width: import('vue').Ref<number>, height: import('vue').Ref<number>, enabled: import('vue').ComputedRef<boolean>, dragging: import('vue').Ref<boolean>, request: (w: number, h: number) => [number, number], gripHandlers: Record<string, (e: PointerEvent) => void> }}
 */
export function useWindowSize({ min = [480, 320], max = [7680, 4320], aspect = null, storeKey = 'window', enabled = null } = {}) {
  const c = getClient();
  const width = ref(window.innerWidth);
  const height = ref(window.innerHeight);
  const onViewport = () => {
    width.value = window.innerWidth;
    height.value = window.innerHeight;
  };
  window.addEventListener('resize', onViewport);
  if (getCurrentScope()) onScopeDispose(() => window.removeEventListener('resize', onViewport));
  const isEnabled = computed(() => (enabled != null ? enabled : !manifest.value?.meta?.standalone));
  const clamp = (w, h) => {
    w = Math.round(Math.max(min[0], Math.min(max[0], w)));
    if (aspect) h = w / aspect;
    h = Math.round(Math.max(min[1], Math.min(max[1], h)));
    return [w, h];
  };
  let next = null;
  let pending = false;
  const request = (w, h) => {
    const size = clamp(w, h);
    if (!isEnabled.value) return size;
    next = size;
    if (!pending) {
      pending = true;
      requestAnimationFrame(() => {
        pending = false;
        c.send('resize', { width: next[0], height: next[1] });
      });
    }
    return size;
  };
  const dragging = ref(false);
  let start = null;
  const move = (e) => {
    if (!start || e.pointerId !== start.id) return;
    request(start.w + (e.clientX - start.x), start.h + (e.clientY - start.y));
  };
  const up = (e) => {
    if (!start || e.pointerId !== start.id) return;
    const [w, h] = clamp(start.w + (e.clientX - start.x), start.h + (e.clientY - start.y));
    start = null;
    dragging.value = false;
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', up);
    request(w, h);
    c.store.set(storeKey, { width: w, height: h });
  };
  const gripHandlers = {
    pointerdown(e) {
      if (e.button !== 0 || !isEnabled.value) return;
      e.preventDefault();
      start = { id: e.pointerId, x: e.clientX, y: e.clientY, w: window.innerWidth, h: window.innerHeight };
      dragging.value = true;
      window.addEventListener('pointermove', move);
      window.addEventListener('pointerup', up);
      window.addEventListener('pointercancel', up);
    },
  };
  // Fullscreen intent. In a host window the adapter resizes the editor to
  // the monitor's work area and restores the previous size on the way back
  // (the page's screen size is sent as a fallback); in a browser tab the
  // Fullscreen API does the same for the tab.
  const fullscreen = ref(false);
  const onFsChange = () => {
    if (!isEnabled.value) fullscreen.value = !!document.fullscreenElement;
  };
  document.addEventListener('fullscreenchange', onFsChange);
  if (getCurrentScope()) onScopeDispose(() => document.removeEventListener('fullscreenchange', onFsChange));
  const setFullscreen = (on) => {
    if (isEnabled.value) {
      c.send('fullscreen', { on: !!on, width: window.screen.availWidth, height: window.screen.availHeight });
      fullscreen.value = !!on;
    } else if (on) {
      document.documentElement.requestFullscreen?.().catch(() => {});
    } else {
      document.exitFullscreen?.().catch(() => {});
    }
  };
  const toggleFullscreen = () => setFullscreen(!fullscreen.value);
  return { width, height, enabled: isEnabled, dragging, request, gripHandlers, fullscreen, setFullscreen, toggleFullscreen };
}
