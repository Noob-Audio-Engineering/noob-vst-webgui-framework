/**
 * Vue bridge over the framework-agnostic vst3-web-stratum client.
 *
 * One client per page. `useParam(id)` returns a *reactive* handle whose
 * `norm` / `plain` / `text` / `on` / `index` / `label` fields follow the
 * plugin (host automation, other UIs) and whose `set()` / `begin()` /
 * `end()` send gestures back. Handles are cached, so every component that
 * asks for the same id shares one subscription.
 *
 *   import { useVst3WebStratum, useParam } from '@elyerinfox/vst3-web-stratum/vue';
 *   const { ready } = useVst3WebStratum();
 *   const cutoff = useParam('cutoff');     // once ready
 *   cutoff.plain, cutoff.text, cutoff.set(0.5)
 *
 * ## How it fits together
 *
 * * A single `Vst3WebStratumClient` is created lazily by `getClient()` (or the
 *   first composable that needs it) using the options given to
 *   `configureClient()`. Module-level refs (`ready`, `connected`,
 *   `manifest`, `status`, `stats`, `modified`) mirror the client's events,
 *   so any component can `useVst3WebStratum()` and render from them.
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
 * `useVst3WebStratum()` and `getClient()` may be called at any time. `useParam()`,
 * `useStream()`, `stateToJson()` and `loadState()` need the manifest: call
 * them once `ready` is true (render the parameter UI behind
 * `v-if="ready"`), or they throw for unknown ids. `useStore()` and
 * `useStoredRef()` may be called early; they fill in when the store is
 * hydrated.
 */
import { computed, reactive, ref, shallowRef } from 'vue';
import { History, Vst3WebStratumClient } from '../vst3-web-stratum.js';

/**
 * @typedef {object} ParamHandle
 * The reactive object returned by {@link useParam}. Read fields in
 * templates; call the methods from event handlers.
 * @property {string} id            Parameter id.
 * @property {import('../vst3-web-stratum.js').Param} param The underlying client-side parameter (not reactive).
 * @property {string} name          Display name.
 * @property {string} unit          Unit suffix (`'Hz'`, `''`).
 * @property {import('../vst3-web-stratum.js').ParamSpec} spec The manifest entry.
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
 * Call before the first `useVst3WebStratum()` to change the connection options.
 *
 * Merged over the defaults (`pingIntervalMs: 500`). Accepts every
 * `Vst3WebStratumClient` option plus `url` for the WebSocket URL. Calls made after
 * the client exists have no effect.
 * @param {{ url?: string, autoReconnect?: boolean, pingIntervalMs?: number }} opts
 * @returns {void}
 */
export function configureClient(opts) {
  clientOpts = { ...clientOpts, ...opts };
}

/**
 * The page's single `Vst3WebStratumClient`, created on first use.
 *
 * Wires the module refs to the client's events: `'open'` / `'close'` drive
 * `connected`; `'manifest'` sets `manifest`, `ready` and clears `modified`;
 * `'stats'` refreshes `stats`; `'message'` with topic `status` fills
 * `status`, and `sample_rate` patches `manifest.meta.sample_rate` so
 * analyzers follow the host's rate; `'edit'` sets `modified`. Also creates
 * the `History`.
 * @returns {Vst3WebStratumClient}
 */
export function getClient() {
  if (!client) {
    client = new Vst3WebStratumClient(clientOpts.url || null, clientOpts);
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
 *   client: Vst3WebStratumClient,
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
export function useVst3WebStratum() {
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
 * @returns {import('../vst3-web-stratum.js').Stream}
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
