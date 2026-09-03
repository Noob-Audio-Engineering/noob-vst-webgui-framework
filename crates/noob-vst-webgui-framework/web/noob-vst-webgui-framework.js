/**
 * noob-vst-webgui-framework.js — browser client for the noob-vst-webgui-framework bridge.
 *
 * ES module, zero dependencies. Runs in any browser, WebView2 or WKWebView.
 *
 *   import { NoobVstWebguiFrameworkClient } from '/noob-vst-webgui-framework/noob-vst-webgui-framework.js';
 *   const client = await NoobVstWebguiFrameworkClient.connect();          // ws://<host>/ws
 *   const cutoff = client.param('cutoff');
 *   cutoff.on(v => console.log('cutoff', cutoff.plain));   // host / other UI changes
 *   cutoff.beginEdit(); cutoff.set(0.5); cutoff.endEdit(); // user gesture
 *   client.stream('spectrum').on(bins => draw(bins));      // Float32Array, zero-copy
 *
 * Wire format: docs/WIRE.md. Everything hot is binary; the manifest and
 * ad-hoc messages are JSON text frames.
 *
 * ## What the client does
 *
 * One `NoobVstWebguiFrameworkClient` owns one WebSocket to one plug-in instance. Through it
 * the page gets:
 *
 * * **Parameters** (`Param`): every plug-in parameter as an object with a
 *   normalized value (0..1) that follows the host, the plug-in and every
 *   other window of the instance, plus `set` / `beginEdit` / `endEdit` to
 *   send gestures back. The plug-in's parameter list, ranges and tapers come
 *   from the manifest, so this file has no product knowledge.
 * * **Streams** (`Stream`): telemetry the audio thread publishes (spectra,
 *   meters, curves, waveforms). Frames arrive as `Float32Array` views over
 *   the socket message, no copy. Latest wins: a slow page drops frames and
 *   never builds a backlog.
 * * **Events**: note on / off, controllers and plug-in-defined events in
 *   both directions, as compact binary frames aimed at the audio thread.
 * * **Messages**: ad-hoc JSON `{ topic, data }` in both directions for
 *   things that are neither parameters nor telemetry (resize requests,
 *   status, preset names).
 * * **The UI store** (`Store`): a small JSON object that lives in the
 *   plug-in and is saved with its state, for presets, favourites and view
 *   settings that should travel with the plug-in instead of the browser.
 * * **History**: undo / redo / A-B over whole-parameter snapshots, recorded
 *   from completed gestures.
 * * **Stats**: round-trip time from pings, edit-to-echo latency, incoming
 *   frame and bit rates.
 *
 * ## Connection lifecycle
 *
 * 1. `new NoobVstWebguiFrameworkClient(url, opts)` opens the socket at once. `url`
 *    defaults to `/ws` on the page's own origin, or `127.0.0.1:<port>` when
 *    the page was opened with `?port=`.
 * 2. `'open'` fires when the socket is up. Pings start if enabled.
 * 3. The server sends `Hello` (protocol version, client id), then the
 *    manifest (JSON), then a `ParamValues` snapshot of every parameter, the
 *    last frame of every sticky stream, and the whole UI store.
 * 4. `'manifest'` fires; `ready` becomes true; `params` / `streams` are
 *    populated. `NoobVstWebguiFrameworkClient.connect()` resolves here.
 * 5. On close, `'close'` fires and, unless `autoReconnect` is off, the
 *    client retries with exponential backoff (250 ms doubling to a 2 s cap).
 *    On reconnect the manifest is re-applied **onto the same `Param` and
 *    `Stream` objects**, so handles and subscriptions held by the page stay
 *    valid, stream throttles are re-sent, and the store is re-hydrated.
 * 6. `close()` stops everything and disables reconnecting.
 *
 * ## Binary frames the client decodes
 *
 * Every binary message starts with a 4-byte header `[kind u8][flags u8][arg u16]`:
 *
 * | kind | name         | decoded into                                          |
 * |------|--------------|-------------------------------------------------------|
 * | 0x01 | Hello        | protocol check, `clientId`                            |
 * | 0x10 | ParamValues  | `Param._receive()` per entry (index, flags, f32 value)|
 * | 0x13 | EventsOut    | `'event'` listeners, one `UiEvent` per 12-byte entry  |
 * | 0x20 | StreamF32    | `Stream._receive()` with a `Float32Array` view         |
 * | 0x21 | StreamU8     | `Stream._receive()` with a `Uint8Array` view           |
 * | 0x31 | Pong         | `stats.rttMs` / `stats.rttAvgMs`                       |
 *
 * And encodes: 0x11 ParamEdit (gestures), 0x12 Events (to the audio
 * thread), 0x30 Ping, 0x40 Subscribe (per-stream throttle / enable).
 *
 * ## Threading and zero-copy
 *
 * Everything here runs on the page's main thread inside WebSocket `message`
 * events; nothing is deferred, so a parameter change reaches your listener
 * in the same task the bytes arrived in. Stream frames are `Float32Array`
 * views into the message's own `ArrayBuffer` (offset 20), which the browser
 * allocates per message and never reuses, so holding a frame is safe and
 * copying is never needed. Edit and ping frames are written into small
 * pre-allocated buffers, so a gesture allocates nothing.
 */

/** Wire protocol version this client speaks; a mismatch logs a warning. */
export const PROTOCOL_VERSION = 1;
/** `ParamValues` entry flag: this value is the echo of an edit made by *this* client. */
export const FLAG_ECHO = 0x0001;
/** `ParamValues` entry flag: this value came from the host (automation, preset load). */
export const FLAG_HOST = 0x0002;

/**
 * Event kinds (mirrors `noob_vst_webgui_framework::wire::event_kind`). `>= 0x80` is plugin-defined.
 *
 * Used in the `kind` field of a {@link UiEvent}. `NOTE_ON` with `value`
 * (velocity) 0 is treated as a note off by the examples, as in MIDI.
 *
 * @readonly
 * @enum {number}
 */
export const EventKind = {
  NOTE_ON: 1,
  NOTE_OFF: 2,
  CONTROL: 3,
  PITCH_BEND: 4,
  AFTERTOUCH: 5,
  PROGRAM: 6,
  CUSTOM: 0x80,
};

/**
 * @typedef {object} UiEvent
 * One event in either direction, 12 bytes on the wire.
 * @property {number} kind    One of {@link EventKind} or a plug-in-defined value >= 0x80.
 * @property {number} [channel=0] 0..255, MIDI channel for note / controller events.
 * @property {number} [a=0]   0..255; note number or controller number.
 * @property {number} [b=0]   0..255; spare byte for plug-in-defined events.
 * @property {number} [value=0] f32; velocity 0..1, controller value 0..1, pitch bend -1..1.
 * @property {number} [offset=0] u32; sample offset within the block for sample-accurate scheduling (0 = now).
 */

/**
 * @typedef {object} ParamSpec
 * One entry of the manifest's `params` array, as sent by the plug-in.
 * @property {number} index         Position in the value tables and on the wire.
 * @property {string} id            Stable identifier (`'cutoff'`, `'b3_gain'`).
 * @property {string} name          Display name.
 * @property {string} unit          Unit suffix without a leading space (`'Hz'`, `'dB'`, `''`).
 * @property {string} group         Grouping hint for UIs (`'global'`, `'Band 3'`).
 * @property {number} min           Plain-value minimum.
 * @property {number} max           Plain-value maximum.
 * @property {number} default       Default plain value.
 * @property {number} default_norm  Default normalized value.
 * @property {'linear'|'log'|'skew'|'table'} taper How normalized maps to plain.
 * @property {number} [skew]        Exponent for the `skew` taper (`plain = min + span * n^(1/skew)`).
 * @property {number} steps         0 for continuous, otherwise the number of discrete positions (2 = toggle).
 * @property {string[]} labels      One label per step for enumerations, else empty.
 * @property {boolean} automatable  False for UI-only parameters the host should not see.
 * @property {number[]} table       65 samples of normalized -> plain, always present; used when the taper is `table` or unknown.
 */

/**
 * @typedef {object} StreamSpec
 * One entry of the manifest's `streams` array.
 * @property {number} index     Stream index on the wire.
 * @property {string} id        Stable identifier (`'spectrum_post'`).
 * @property {string} name      Display name.
 * @property {string} kind      Free-form hint: `'spectrum'`, `'meter'`, `'curve'`, `'scope'`, `'raw'`, ...
 * @property {number} capacity  Maximum number of f32 values per frame.
 * @property {number} channels  Interleave factor (a stereo meter has 2).
 * @property {object} meta      Plug-in-provided metadata such as `sample_rate`, `fft_size`, `db`.
 * @property {boolean} [sticky] The server replays the last frame to late clients.
 */

/**
 * @typedef {object} Manifest
 * The JSON text frame sent once per connection after `Hello`.
 * @property {'manifest'} t
 * @property {string} name          Plug-in / instance name.
 * @property {number} protocol      Wire protocol version.
 * @property {object} meta          Plug-in-defined metadata (`sample_rate`, `vendor`, `version`, `standalone`, ...).
 * @property {ParamSpec[]} params
 * @property {StreamSpec[]} streams
 */

/**
 * @typedef {object} ParamChangeInfo
 * Second argument to a {@link Param#on} listener.
 * @property {boolean} local  True when the change was made through this `Param` on this page.
 * @property {boolean} echo   Always false: echoes of local edits are swallowed and only measured.
 * @property {boolean} host   True when the value came from the host (automation, preset load).
 * @property {Param} param    The parameter itself, for shared listeners.
 */

/**
 * @typedef {object} ClientStats
 * Live statistics, updated in place and emitted through the `'stats'` event once per second.
 * @property {number} rttMs     Latest ping round trip in milliseconds (`NaN` until the first pong).
 * @property {number} rttAvgMs  Exponential moving average of `rttMs` (weight 0.2).
 * @property {number} echoMs    Latest edit-to-echo latency: from `set()` to the server's echo of that value.
 * @property {number} echoAvgMs Exponential moving average of `echoMs`.
 * @property {number} framesIn  Binary frames received since construction.
 * @property {number} bytesIn   Bytes received since construction.
 * @property {number} fps       Binary frames per second over the last window.
 * @property {number} kbps      Kilobits per second over the last window.
 */

/**
 * @typedef {object} ClientOptions
 * @property {boolean} [autoReconnect=true] Reconnect with backoff after a close.
 * @property {OfflineOptions} [offline] Design-time fallback: if no manifest
 *   has arrived after `timeoutMs` (or at once with `immediate`), run from a
 *   local manifest with local parameter values and synthetic frames until
 *   the real plug-in connects. See {@link mockManifest}.
 * @property {number} [pingIntervalMs=1000] Period of latency probes; `0` disables them.
 * @property {number} [timeoutMs]           `NoobVstWebguiFrameworkClient.connect()` only: reject if no manifest arrives in time.
 */

/**
 * @callback Unsubscribe
 * Returned by every `on()`; call it to remove the listener.
 * @returns {void}
 */

/**
 * Binary frame kinds, first header byte. See docs/WIRE.md.
 * @private
 */
const Kind = {
  HELLO: 0x01,
  PARAM_VALUES: 0x10,
  PARAM_EDIT: 0x11,
  EVENTS: 0x12,
  EVENTS_OUT: 0x13,
  STREAM_F32: 0x20,
  STREAM_U8: 0x21,
  PING: 0x30,
  PONG: 0x31,
  SUBSCRIBE: 0x40,
};

/** Byte offset of the sample data in a stream frame: 4 header + 4 seq + 8 ts + 4 len. @private */
const STREAM_DATA_OFFSET = 20;
/** Smallest positive f32; the floor used by the `log` taper so `min = 0` does not divide by zero. @private */
const F32_MIN_POSITIVE = 1.1754944e-38;

/** Clamp to 0..1. @private */
const clamp01 = (n) => (n < 0 ? 0 : n > 1 ? 1 : n);

/**
 * Minimal synchronous event emitter. Listeners run in insertion order; one
 * throwing listener is logged and does not stop the others.
 * @private
 * @returns {{ on: (fn: Function) => Unsubscribe, emit: (a?: any, b?: any) => void, size: number }}
 */
function makeEmitter() {
  const fns = new Set();
  return {
    on(fn) {
      fns.add(fn);
      return () => fns.delete(fn);
    },
    emit(a, b) {
      for (const f of fns) {
        try {
          f(a, b);
        } catch (e) {
          console.error(e);
        }
      }
    },
    get size() {
      return fns.size;
    },
  };
}

/**
 * The WebSocket URL to use when none is given: `/ws` on the page's host,
 * or `127.0.0.1:<port>` when the page URL carries `?port=` (handy for a
 * page served by Vite that talks to a standalone on another port).
 * @private
 * @returns {string}
 */
function defaultUrl() {
  const q = new URLSearchParams(location.search);
  const port = q.get('port');
  const host = port ? `127.0.0.1:${port}` : location.host;
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${proto}://${host}/ws`;
}

// ---------------------------------------------------------------------------
// Param
// ---------------------------------------------------------------------------

/**
 * One plugin parameter. Values on the wire are normalized 0..1.
 *
 * Two value spaces:
 *
 * * **normalized** (`norm`, `value`): 0..1, what the wire and the host use.
 *   Discrete parameters snap to `steps` evenly spaced positions.
 * * **plain** (`plain`): the parameter's own units (Hz, dB, an enum index),
 *   derived through the taper from the manifest: `linear`, `log`
 *   (geometric between `min` and `max`), `skew` (`min + span * n^(1/skew)`),
 *   or `table` (piecewise-linear over 65 samples, which is how nih-plug
 *   ranges are mirrored). `toPlain()` / `toNorm()` convert both ways.
 *
 * Changes from the host, the plug-in or another window arrive through
 * `on()`. Changes from this page go out as gestures: `beginEdit()` once,
 * `set()` as often as the pointer moves, `endEdit()` once, so a host can
 * record automation correctly. A bare `set()` outside a gesture sends
 * begin / perform / end in one frame.
 *
 * Objects are created from the manifest and reused across reconnects, so it
 * is safe to keep references.
 */
export class Param {
  /**
   * Created by the client from the manifest; not meant to be constructed by
   * pages.
   * @param {NoobVstWebguiFrameworkClient} client
   * @param {ParamSpec} spec
   */
  constructor(client, spec) {
    /** @private */
    this._client = client;
    /** @private */
    this._ev = makeEmitter();
    /** @private */
    this._editing = false;
    /** Values sent recently, for edit-to-echo timing. @private */
    this._sent = [];
    this._applySpec(spec);
    /** Normalized value. */
    this.norm = spec.default_norm;
  }

  /**
   * Adopt a (possibly updated) spec; called on every manifest so the same
   * object survives a reconnect.
   * @private
   * @param {ParamSpec} spec
   */
  _applySpec(spec) {
    /** @type {ParamSpec} The manifest entry this parameter was built from. */
    this.spec = spec;
    /** @type {number} Wire index. */
    this.index = spec.index;
    /** @type {string} Stable id (`'cutoff'`). */
    this.id = spec.id;
    /** @type {string} Display name. */
    this.name = spec.name;
    /** @type {string} Unit suffix, `''` when none. */
    this.unit = spec.unit || '';
    /** @type {string} Group hint from the plug-in. */
    this.group = spec.group || '';
  }

  /** Normalized value, 0..1. @type {number} */
  get value() {
    return this.norm;
  }
  /** Plain value in the parameter's own units. @type {number} */
  get plain() {
    return this.toPlain(this.norm);
  }
  /** True between `beginEdit()` and `endEdit()`; incoming values are ignored meanwhile. @type {boolean} */
  get editing() {
    return this._editing;
  }
  /** True when the parameter has a finite number of positions (`steps > 1`). @type {boolean} */
  get isDiscrete() {
    return this.spec.steps > 1;
  }
  /** True for two-position parameters (`steps === 2`), formatted as On / Off. @type {boolean} */
  get isToggle() {
    return this.spec.steps === 2;
  }
  /** True when the plain range crosses zero (a gain, a pan): controls draw from the centre. @type {boolean} */
  get isBipolar() {
    return this.spec.min < 0 && this.spec.max > 0;
  }
  /** Plain minimum. @type {number} */
  get min() {
    return this.spec.min;
  }
  /** Plain maximum. @type {number} */
  get max() {
    return this.spec.max;
  }

  /**
   * Clamp to 0..1 and, for discrete parameters, snap to the nearest step.
   * @private
   * @param {number} n
   * @returns {number}
   */
  _snap(n) {
    n = clamp01(n);
    if (this.spec.steps > 1) {
      const last = this.spec.steps - 1;
      n = Math.round(n * last) / last;
    }
    return n;
  }

  /**
   * Normalized -> plain, using the taper when known, else the manifest table.
   * The input is snapped first, so a discrete parameter always yields one of
   * its positions.
   * @param {number} n Normalized value, 0..1.
   * @returns {number} Plain value.
   */
  toPlain(n) {
    const s = this.spec;
    n = this._snap(n);
    const span = s.max - s.min;
    switch (s.taper) {
      case 'linear':
        return s.min + span * n;
      case 'log': {
        const lo = Math.max(s.min, F32_MIN_POSITIVE);
        return lo * Math.pow(s.max / lo, n);
      }
      case 'skew':
        return s.min + span * Math.pow(n, 1 / s.skew);
      default: {
        const t = s.table;
        if (!t || t.length < 2) return s.min + span * n;
        const x = n * (t.length - 1);
        const i = Math.min(Math.floor(x), t.length - 2);
        return t[i] + (t[i + 1] - t[i]) * (x - i);
      }
    }
  }

  /**
   * Plain -> normalized. Inverse of {@link Param#toPlain}; for the `table`
   * taper this is a binary search over the (monotonic) table with linear
   * interpolation between samples. The result is clamped to 0..1.
   * @param {number} p Plain value.
   * @returns {number} Normalized value.
   */
  toNorm(p) {
    const s = this.spec;
    const span = s.max - s.min;
    if (span === 0) return 0;
    let n;
    switch (s.taper) {
      case 'linear':
        n = (p - s.min) / span;
        break;
      case 'log': {
        const lo = Math.max(s.min, F32_MIN_POSITIVE);
        n = Math.log(Math.max(p, lo) / lo) / Math.log(s.max / lo);
        break;
      }
      case 'skew':
        n = Math.pow(Math.max(0, (p - s.min) / span), s.skew);
        break;
      default: {
        const t = s.table;
        if (!t || t.length < 2) {
          n = (p - s.min) / span;
          break;
        }
        const asc = t[t.length - 1] >= t[0];
        let lo = 0;
        let hi = t.length - 1;
        while (hi - lo > 1) {
          const mid = (lo + hi) >> 1;
          if ((t[mid] <= p) === asc) lo = mid;
          else hi = mid;
        }
        const d = t[hi] - t[lo];
        const f = d === 0 ? 0 : (p - t[lo]) / d;
        n = (lo + clamp01(f)) / (t.length - 1);
      }
    }
    return clamp01(n);
  }

  /**
   * Human-readable value with unit.
   *
   * Rules, in order: enumerations return their label; toggles return
   * `'On'` / `'Off'`; other discrete parameters return the integer; values
   * of 1000 or more with a unit use a `k` prefix (`'2.50 kHz'`,
   * `'12.0 kHz'`); otherwise 0, 1 or 2 decimals depending on magnitude, then
   * the unit. Product-specific formatting belongs in the page, not here.
   * @param {number} [plain=this.plain] Plain value to format.
   * @returns {string}
   */
  format(plain = this.plain) {
    const s = this.spec;
    if (s.labels && s.labels.length) {
      const i = Math.round(this.toNorm(plain) * (s.steps - 1));
      return s.labels[Math.max(0, Math.min(s.labels.length - 1, i))];
    }
    if (s.steps === 2) return plain >= 0.5 ? 'On' : 'Off';
    const a = Math.abs(plain);
    let txt;
    if (s.steps > 1) txt = String(Math.round(plain));
    else if (a >= 1000 && s.unit) return `${(plain / 1000).toFixed(a >= 10000 ? 1 : 2)} k${s.unit}`;
    else if (a >= 1000) txt = plain.toFixed(0);
    else if (a >= 100) txt = plain.toFixed(0);
    else if (a >= 10) txt = plain.toFixed(1);
    else txt = plain.toFixed(2);
    return s.unit ? `${txt} ${s.unit}` : txt;
  }

  /**
   * Subscribe to value changes. `fn(norm, info)` where `info.local` is true
   * for changes made through this Param, `info.host` for host automation.
   * Returns an unsubscribe function.
   *
   * Fires for local `set()` calls (synchronously, before the frame is sent)
   * and for incoming values from the host, the plug-in and other windows.
   * It does not fire for the echo of this client's own edits (those only
   * feed the latency statistics) nor while a local gesture is in progress.
   * @param {(norm: number, info: ParamChangeInfo) => void} fn
   * @returns {Unsubscribe}
   */
  on(fn) {
    return this._ev.on(fn);
  }

  /**
   * Start a gesture (mirrors VST3 beginEdit).
   *
   * Sends a `begin` edit with the current value and suppresses incoming
   * values until `endEdit()`, so a host echo cannot fight the pointer.
   * Calling it while already editing is a no-op.
   * @returns {void}
   */
  beginEdit() {
    if (this._editing) return;
    this._editing = true;
    this._client._sendEdit(this.index, 0, this.norm);
  }

  /**
   * Set the normalized value. Inside a gesture this is a `performEdit`;
   * outside one it sends begin/perform/end as a single batched frame.
   *
   * The value is clamped and snapped, applied locally at once, and sent.
   * Listeners see `{ local: true }`. Outside a gesture the client's `'edit'`
   * event fires too (which is what `History` records); inside one it fires
   * from `endEdit()`.
   * @param {number} norm Normalized value, 0..1.
   * @returns {void}
   */
  set(norm) {
    norm = this._snap(norm);
    this.norm = norm;
    this._sent.push({ v: Math.fround(norm), t: performance.now() });
    if (this._sent.length > 64) this._sent.shift();
    if (this._editing) this._client._sendEdit(this.index, 1, norm);
    else this._client._sendEdits([[this.index, 0, norm], [this.index, 1, norm], [this.index, 2, norm]]);
    this._ev.emit(norm, { local: true, echo: false, host: false, param: this });
    if (!this._editing) this._client._ev.edit.emit(this);
  }

  /**
   * `set()` with a plain value (converted through {@link Param#toNorm}).
   * @param {number} plain
   * @returns {void}
   */
  setPlain(plain) {
    this.set(this.toNorm(plain));
  }

  /**
   * End a gesture (mirrors VST3 endEdit).
   *
   * Sends the `end` edit with the final value, re-enables incoming values,
   * and fires the client's `'edit'` event. A no-op when not editing.
   * @returns {void}
   */
  endEdit() {
    if (!this._editing) return;
    this._editing = false;
    this._client._sendEdit(this.index, 2, this.norm);
    this._client._ev.edit.emit(this);
  }

  /**
   * Set the manifest default (`spec.default_norm`) as a one-shot edit.
   * @returns {void}
   */
  reset() {
    this.set(this.spec.default_norm);
  }

  /**
   * Called by the client for every `ParamValues` entry addressed to this
   * parameter. Echoes of local edits (`FLAG_ECHO`) are matched against the
   * recently sent values to measure latency and are otherwise dropped;
   * anything else updates `norm` and notifies listeners, unless a local
   * gesture is in progress.
   * @private
   * @param {number} norm  Normalized value from the wire (f32).
   * @param {number} flags `FLAG_ECHO` / `FLAG_HOST` bits.
   */
  _receive(norm, flags) {
    if (flags & FLAG_ECHO) {
      const v = Math.fround(norm);
      const i = this._sent.findIndex((s) => s.v === v);
      if (i >= 0) {
        const t = this._sent[i].t;
        this._sent.splice(0, i + 1);
        this._client._recordEcho(performance.now() - t);
      }
      return;
    }
    if (this._editing) return;
    this.norm = norm;
    this._ev.emit(norm, { local: false, echo: false, host: (flags & FLAG_HOST) !== 0, param: this });
  }
}

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

/**
 * One telemetry stream: a Float32Array per frame, latest wins.
 *
 * The plug-in's audio thread publishes frames into a wait-free triple
 * buffer; the server forwards the newest one to each client at most as fast
 * as that client's `subscribe()` throttle allows. Each frame carries a
 * sequence number (gaps show dropped frames) and a plug-in-side timestamp.
 *
 * Listeners get the frame as a `Float32Array` (or `Uint8Array` for byte
 * streams) that is a view into the socket message's buffer. Every message
 * has its own buffer, so you may keep the array; but the next frame is a
 * different array, so re-read `stream.data` rather than caching the first.
 *
 * Streams are created from the manifest and reused across reconnects.
 */
export class Stream {
  /**
   * Created by the client from the manifest; not meant to be constructed by
   * pages.
   * @param {NoobVstWebguiFrameworkClient} client
   * @param {StreamSpec} spec
   */
  constructor(client, spec) {
    /** @private */
    this._client = client;
    /** @private */
    this._ev = makeEmitter();
    this._applySpec(spec);
    /** Most recent frame (a view into that message's buffer; never reused). @type {Float32Array|Uint8Array} */
    this.data = new Float32Array(0);
    /** Sequence number of the latest frame; increments per published frame, so gaps mean drops. @type {number} */
    this.seq = 0;
    /** Plugin-side timestamp of the latest frame, milliseconds. */
    this.ts = 0;
    /** Measured incoming frame rate. */
    this.fps = 0;
    /** @private */
    this._frames = 0;
    /** @private */
    this._fpsT = performance.now();
  }

  /**
   * Adopt a (possibly updated) spec; called on every manifest.
   * @private
   * @param {StreamSpec} spec
   */
  _applySpec(spec) {
    /** @type {StreamSpec} The manifest entry. */
    this.spec = spec;
    /** @type {number} Wire index. */
    this.index = spec.index;
    /** @type {string} Stable id (`'spectrum_post'`). */
    this.id = spec.id;
    /** @type {string} Display name. */
    this.name = spec.name;
    /** @type {string} Kind hint (`'spectrum'`, `'meter'`, `'curve'`, ...). */
    this.kind = spec.kind;
    /** @type {number} Maximum values per frame. */
    this.capacity = spec.capacity;
    /** @type {number} Interleave factor (channels per sample). */
    this.channels = spec.channels || 1;
    /** @type {object} Plug-in metadata (`sample_rate`, `fft_size`, `db`, ...). */
    this.meta = spec.meta || {};
  }

  /**
   * `fn(data, stream)` on every frame. Returns an unsubscribe function.
   * @param {(data: Float32Array|Uint8Array, stream: Stream) => void} fn
   * @returns {Unsubscribe}
   */
  on(fn) {
    return this._ev.on(fn);
  }

  /**
   * Ask the server to throttle this stream. `maxHz: 0` means every frame.
   * `enabled: false` stops it entirely (a hidden panel should do this).
   *
   * The setting is per client and per stream, remembered by the client and
   * re-sent after a reconnect. Throttling happens server-side, so a
   * throttled stream costs no bandwidth.
   * @param {{ maxHz?: number, enabled?: boolean }} [opts]
   * @param {number} [opts.maxHz=0]     Maximum frames per second; `0` = unlimited.
   * @param {boolean} [opts.enabled=true] `false` unsubscribes until the next call.
   * @returns {void}
   */
  subscribe({ maxHz = 0, enabled = true } = {}) {
    const us = maxHz > 0 ? Math.round(1e6 / maxHz) : 0;
    this._sub = { us, enabled };
    this._client._sendSubscribe(this.index, us, enabled);
  }

  /**
   * Called by the client for each decoded frame: stores it, updates the
   * measured frame rate once a second, and notifies listeners.
   * @private
   * @param {Float32Array|Uint8Array} data
   * @param {number} seq
   * @param {number} ts Milliseconds on the plug-in's clock.
   */
  _receive(data, seq, ts) {
    this.data = data;
    this.seq = seq;
    this.ts = ts;
    this._frames++;
    const now = performance.now();
    if (now - this._fpsT >= 1000) {
      this.fps = (this._frames * 1000) / (now - this._fpsT);
      this._frames = 0;
      this._fpsT = now;
    }
    this._ev.emit(data, this);
  }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/**
 * The connection to one plug-in instance. See the file header for the
 * lifecycle. Construct it directly (it connects immediately and fires
 * `'manifest'` when usable) or await {@link NoobVstWebguiFrameworkClient.connect}.
 *
 * Events (subscribe with {@link NoobVstWebguiFrameworkClient#on}):
 *
 * | event        | listener arguments                     | when                                                        |
 * |--------------|----------------------------------------|-------------------------------------------------------------|
 * | `'open'`     | `(client)`                             | socket connected (before the manifest)                      |
 * | `'close'`    | `(client)`                             | socket closed; a reconnect may follow                       |
 * | `'manifest'` | `(manifest, client)`                   | manifest applied; `params` / `streams` usable; also on reconnect |
 * | `'message'`  | `(topic, data)`                        | an ad-hoc JSON message from the plug-in (store topics excluded) |
 * | `'error'`    | `(errorOrEvent)`                       | socket error or failed construction                         |
 * | `'stats'`    | `(stats)`                              | once per second with {@link ClientStats}                    |
 * | `'edit'`     | `(param \| null)`                      | a local gesture completed, a one-shot `set()`, or `setMany()` (null) |
 * | `'event'`    | `(uiEvent)`                            | each {@link UiEvent} the plug-in sends (host notes, for example) |
 */
export class NoobVstWebguiFrameworkClient {
  /**
   * @param {string} [url] WebSocket URL. Defaults to `/ws` on the page's host,
   *   or `127.0.0.1:<port>` if the page was opened with `?port=`.
   * @param {object} [opts]
   * @param {boolean} [opts.autoReconnect=true]
   * @param {number} [opts.pingIntervalMs=1000] 0 disables latency probes.
   */
  constructor(url, opts = {}) {
    /** @type {string} The WebSocket URL in use. */
    this.url = url || defaultUrl();
    /** @type {ClientOptions} Effective options. */
    this.opts = { autoReconnect: true, pingIntervalMs: 1000, ...opts };
    /** @type {Manifest|null} The last manifest received, `null` before the first. */
    this.manifest = null;
    /** @type {Param[]} */
    this.params = [];
    /** @type {Stream[]} */
    this.streams = [];
    /** @private */
    this._byId = new Map();
    /** @private */
    this._sById = new Map();
    /** @type {number} Id the server assigned to this connection (from `Hello`); 0 before that. */
    this.clientId = 0;
    /** @type {boolean} Socket is open. */
    this.connected = false;
    /** @type {boolean} Manifest received; `params` and `streams` are valid. */
    this.ready = false;
    /** @type {ClientStats} */
    this.stats = {
      rttMs: NaN,
      rttAvgMs: NaN,
      echoMs: NaN,
      echoAvgMs: NaN,
      framesIn: 0,
      bytesIn: 0,
      fps: 0,
      kbps: 0,
    };
    /** @private */
    this._ev = {
      open: makeEmitter(),
      close: makeEmitter(),
      manifest: makeEmitter(),
      message: makeEmitter(),
      error: makeEmitter(),
      stats: makeEmitter(),
      /** Fired with the Param after a completed local gesture or one-shot set. */
      edit: makeEmitter(),
      /** Fired with `{kind, channel, a, b, value}` for each event the plugin sends. */
      event: makeEmitter(),
    };
    /** @private */
    this._ws = null;
    /** Current reconnect delay in ms. @private */
    this._retry = 250;
    /** Set by `close()`; stops reconnecting. @private */
    this._closed = false;
    /** The plug-in-persisted key-value store for UI state. */
    this.store = new Store(this);
    /** Reused buffer for edit frames of up to 8 entries. @private */
    this._editBuf = new ArrayBuffer(4 + 8 * 8);
    /** @private */
    this._editView = new DataView(this._editBuf);
    /** Reused buffer for ping and subscribe frames. @private */
    this._small = new ArrayBuffer(20);
    /** @private */
    this._smallView = new DataView(this._small);
    /** @private */
    this._statFrames = 0;
    /** @private */
    this._statBytes = 0;
    /** @private */
    this._statT = performance.now();
    /** @type {boolean} True while running from an offline manifest (no plug-in reached yet). */
    this.offline = false;
    /** @private */
    this._offlineTimer = 0;
    /** @private */
    this._offlineFrames = 0;
    if (this.opts.offline) {
      const o = this.opts.offline;
      this._offlineTimer = setTimeout(() => {
        if (!this.ready) this._goOffline();
      }, o.immediate ? 0 : (o.timeoutMs ?? 1200));
    }
    this._connect();
  }

  /**
   * Connect and resolve once the manifest has arrived.
   *
   * @param {string} [url] As for the constructor.
   * @param {ClientOptions} [opts] As for the constructor, plus `timeoutMs`:
   *   reject (and close the client) if no manifest arrives in that time.
   *   Without it the promise waits as long as reconnecting continues.
   * @returns {Promise<NoobVstWebguiFrameworkClient>}
   */
  static connect(url, opts = {}) {
    return new Promise((resolve, reject) => {
      const c = new NoobVstWebguiFrameworkClient(url, opts);
      let timer = null;
      const off = c.on('manifest', () => {
        off();
        if (timer) clearTimeout(timer);
        resolve(c);
      });
      if (opts.timeoutMs) {
        timer = setTimeout(() => {
          off();
          c.close();
          reject(new Error(`bridge: no manifest within ${opts.timeoutMs} ms`));
        }, opts.timeoutMs);
      }
    });
  }

  /**
   * Events: 'open', 'close', 'manifest', 'message' (topic, data), 'error', 'stats'.
   * Also 'edit' and 'event'; see the class description for the arguments.
   * @param {'open'|'close'|'manifest'|'message'|'error'|'stats'|'edit'|'event'} event
   * @param {Function} fn
   * @returns {Unsubscribe}
   * @throws {Error} For an unknown event name.
   */
  on(event, fn) {
    const e = this._ev[event];
    if (!e) throw new Error(`bridge: unknown event ${event}`);
    return e.on(fn);
  }

  /**
   * Look a parameter up by id. Valid once `ready`; the same object is
   * returned every time and survives reconnects.
   * @param {string} id
   * @returns {Param}
   * @throws {Error} When the id is not in the manifest.
   */
  param(id) {
    const p = this._byId.get(id);
    if (!p) throw new Error(`bridge: unknown param "${id}"`);
    return p;
  }

  /**
   * Look a stream up by id. Valid once `ready`.
   * @param {string} id
   * @returns {Stream}
   * @throws {Error} When the id is not in the manifest.
   */
  stream(id) {
    const s = this._sById.get(id);
    if (!s) throw new Error(`bridge: unknown stream "${id}"`);
    return s;
  }

  /**
   * Whether the manifest declares a parameter with this id (lets a page
   * adapt to a plug-in build that lacks a feature).
   * @param {string} id
   * @returns {boolean}
   */
  hasParam(id) {
    return this._byId.has(id);
  }
  /**
   * Whether the manifest declares a stream with this id.
   * @param {string} id
   * @returns {boolean}
   */
  hasStream(id) {
    return this._sById.has(id);
  }

  /**
   * Send an ad-hoc JSON message to the plugin.
   *
   * Goes out as `{ "t": "msg", "topic", "data" }`; the plug-in reads it from
   * its message queue (the nih-plug adapter handles `'resize'` itself).
   * Silently dropped while the socket is not open. Topics starting with
   * `store.` are reserved for the UI store.
   * @param {string} topic
   * @param {any} [data=null] Anything JSON-serialisable.
   * @returns {void}
   */
  send(topic, data = null) {
    if (this._ws && this._ws.readyState === 1) {
      this._ws.send(JSON.stringify({ t: 'msg', topic, data }));
    }
  }

  /**
   * Send events to the plugin's audio thread in one frame. Each event is
   * `{ kind, channel = 0, a = 0, b = 0, value = 0, offset = 0 }`.
   *
   * The audio thread drains these on its next block, so the latency is one
   * network hop plus at most one block. An empty array sends nothing.
   * @param {UiEvent[]} events
   * @returns {void}
   */
  sendEvents(events) {
    const n = events.length;
    if (!n) return;
    const buf = new ArrayBuffer(4 + 12 * n);
    const v = new DataView(buf);
    v.setUint8(0, Kind.EVENTS);
    v.setUint8(1, 0);
    v.setUint16(2, n, true);
    let o = 4;
    for (const e of events) {
      v.setUint8(o, e.kind);
      v.setUint8(o + 1, e.channel || 0);
      v.setUint8(o + 2, e.a || 0);
      v.setUint8(o + 3, e.b || 0);
      v.setFloat32(o + 4, e.value || 0, true);
      v.setUint32(o + 8, e.offset || 0, true);
      o += 12;
    }
    this._sendRaw(new Uint8Array(buf));
  }

  /**
   * Send one event; see {@link NoobVstWebguiFrameworkClient#sendEvents}.
   * @param {UiEvent} e
   * @returns {void}
   */
  sendEvent(e) {
    this.sendEvents([e]);
  }

  /**
   * Note on (MIDI note number, velocity 0..1).
   * @param {number} note     0..127.
   * @param {number} [velocity=1] 0..1.
   * @param {number} [channel=0]
   * @returns {void}
   */
  noteOn(note, velocity = 1, channel = 0) {
    this.sendEvent({ kind: EventKind.NOTE_ON, channel, a: note, value: velocity });
  }

  /**
   * Note off.
   * @param {number} note     0..127.
   * @param {number} [velocity=0] Release velocity, 0..1.
   * @param {number} [channel=0]
   * @returns {void}
   */
  noteOff(note, velocity = 0, channel = 0) {
    this.sendEvent({ kind: EventKind.NOTE_OFF, channel, a: note, value: velocity });
  }

  /**
   * Controller change (number, 0..1).
   * @param {number} number   Controller number, 0..127 (120 and 123 are "all notes off" in the examples).
   * @param {number} value    0..1.
   * @param {number} [channel=0]
   * @returns {void}
   */
  control(number, value, channel = 0) {
    this.sendEvent({ kind: EventKind.CONTROL, channel, a: number, value });
  }

  /**
   * Send a latency probe; the result lands in `stats.rttMs`.
   *
   * Sent automatically every `pingIntervalMs`; call it yourself when probes
   * are disabled. The frame carries `performance.now()` and the server
   * returns it unchanged, so no clock sync is needed.
   * @returns {void}
   */
  ping() {
    const v = this._smallView;
    v.setUint8(0, Kind.PING);
    v.setUint8(1, 0);
    v.setUint16(2, 0, true);
    v.setFloat64(4, performance.now(), true);
    this._sendRaw(new Uint8Array(this._small, 0, 12));
  }

  /**
   * Close the socket and stop reconnecting. The object cannot be reopened;
   * make a new client instead.
   * @returns {void}
   */
  close() {
    this._closed = true;
    clearTimeout(this._offlineTimer);
    this._stopOffline();
    if (this._pingTimer) clearInterval(this._pingTimer);
    if (this._ws) this._ws.close();
  }

  /**
   * Every parameter's normalized value, by index.
   *
   * The unit `History` and A/B work in; also handy for "copy state" features.
   * @returns {Float32Array} One entry per parameter index.
   */
  snapshot() {
    const out = new Float32Array(this.params.length);
    for (let i = 0; i < this.params.length; i++) out[i] = this.params[i] ? this.params[i].norm : 0;
    return out;
  }

  /**
   * Set many parameters in one frame (begin / perform / end for each), only
   * for those whose value actually changes. `values` is a Float32Array from
   * `snapshot()` or an iterable of `[Param|id|index, norm]` pairs.
   *
   * Values are snapped per parameter and compared as f32, so re-applying a
   * snapshot sends nothing. Each changed parameter notifies its listeners
   * with `{ local: true }`; then, if `emit` is true, one `'edit'` event
   * fires with `null` (a single history step for the whole batch).
   * @param {Float32Array|number[]|Iterable<[Param|string|number, number]>} values
   * @param {{ emit?: boolean }} [opts]
   * @param {boolean} [opts.emit=true] Fire the `'edit'` event (`History` passes false for undo / redo).
   * @returns {number} How many parameters changed.
   */
  setMany(values, { emit = true } = {}) {
    const edits = [];
    const touched = [];
    const push = (p, norm) => {
      if (!p) return;
      const n = p._snap(norm);
      if (Math.fround(n) === Math.fround(p.norm)) return;
      p.norm = n;
      edits.push([p.index, 0, n], [p.index, 1, n], [p.index, 2, n]);
      touched.push(p);
    };
    if (values instanceof Float32Array || Array.isArray(values) && typeof values[0] === 'number') {
      for (let i = 0; i < values.length; i++) push(this.params[i], values[i]);
    } else {
      for (const [k, norm] of values) {
        const p = typeof k === 'object' ? k : typeof k === 'number' ? this.params[k] : this._byId.get(k);
        push(p, norm);
      }
    }
    if (!edits.length) return 0;
    this._sendEdits(edits);
    for (const p of touched) p._ev.emit(p.norm, { local: true, echo: false, host: false, param: p });
    if (emit) this._ev.edit.emit(null);
    return touched.length;
  }

  /**
   * Alias of `setMany` for a full snapshot.
   * @param {Float32Array} snapshot From {@link NoobVstWebguiFrameworkClient#snapshot}.
   * @param {{ emit?: boolean }} [opts]
   * @returns {number} How many parameters changed.
   */
  applySnapshot(snapshot, opts) {
    return this.setMany(snapshot, opts);
  }

  // -- internals -----------------------------------------------------------

  /**
   * Open the socket and wire its callbacks. A construction failure (bad
   * URL) is reported as `'error'` and retried like a close.
   * @private
   */
  _connect() {
    if (this._closed) return;
    let ws;
    try {
      ws = new WebSocket(this.url);
    } catch (e) {
      this._ev.error.emit(e);
      this._scheduleReconnect();
      return;
    }
    ws.binaryType = 'arraybuffer';
    ws.onopen = () => {
      this.connected = true;
      this._retry = 250;
      this._ev.open.emit(this);
      if (this.opts.pingIntervalMs > 0) {
        this._pingTimer = setInterval(() => this.ping(), this.opts.pingIntervalMs);
        this.ping();
      }
    };
    ws.onmessage = (ev) => this._onMessage(ev);
    ws.onerror = (ev) => this._ev.error.emit(ev);
    ws.onclose = () => {
      this.connected = false;
      this.ready = false;
      if (this._pingTimer) clearInterval(this._pingTimer);
      this._pingTimer = null;
      this._ev.close.emit(this);
      this._scheduleReconnect();
    };
    this._ws = ws;
  }

  /**
   * Retry after the current backoff delay, then double it (cap 2 s).
   * @private
   */
  _scheduleReconnect() {
    if (this._closed || !this.opts.autoReconnect) return;
    setTimeout(() => this._connect(), this._retry);
    this._retry = Math.min(this._retry * 2, 2000);
  }

  /**
   * Socket `message` handler. Text frames are JSON: the manifest, the
   * reserved `store.*` topics (routed to `store`), or a `'message'` event.
   * Binary frames go to `_decode()` and feed the traffic statistics, which
   * are published once a second.
   * @private
   * @param {MessageEvent} ev
   */
  _onMessage(ev) {
    if (typeof ev.data === 'string') {
      let m;
      try {
        m = JSON.parse(ev.data);
      } catch (e) {
        console.warn('bridge: bad JSON', e);
        return;
      }
      if (m.t === 'manifest') this._applyManifest(m);
      else if (m.t === 'msg') {
        if (m.topic === 'store.all') this.store._hydrate(m.data && m.data.values);
        else if (m.topic === 'store.changed') this.store._changed(m.data.key, m.data.value);
        else if (m.topic === 'store.error') console.warn('bridge store:', m.data);
        else {
          // The manifest is built before a plug-in knows its sample rate, so
          // the rate in it is a placeholder until the host says otherwise.
          // Keep `meta.sample_rate` current, and still pass the message on:
          // a page that reads the rate once from the manifest would
          // otherwise put every spectrum peak at the wrong frequency.
          if (m.topic === 'sample_rate' && this.manifest && m.data && Number.isFinite(m.data.sample_rate)) {
            this.manifest.meta = { ...this.manifest.meta, sample_rate: m.data.sample_rate };
            this.meta = this.manifest.meta;
          }
          this._ev.message.emit(m.topic, m.data);
        }
      }
      return;
    }
    const buf = ev.data;
    this._statFrames++;
    this._statBytes += buf.byteLength;
    this.stats.framesIn++;
    this.stats.bytesIn += buf.byteLength;
    this._decode(buf);
    const now = performance.now();
    if (now - this._statT >= 1000) {
      const dt = (now - this._statT) / 1000;
      this.stats.fps = this._statFrames / dt;
      this.stats.kbps = (this._statBytes * 8) / dt / 1000;
      this._statFrames = 0;
      this._statBytes = 0;
      this._statT = now;
      this._ev.stats.emit(this.stats);
    }
  }

  /**
   * Build (or refresh, on reconnect) the `Param` and `Stream` tables from a
   * manifest. Existing objects are kept by id so page-held handles stay
   * valid; stream throttles set earlier are re-sent. Ends by marking the
   * client `ready` and firing `'manifest'`.
   * @private
   * @param {Manifest} m
   */
  _applyManifest(m) {
    if (this.offline && !this._applyingOffline) this._stopOffline();
    this.manifest = m;
    const params = [];
    const byId = new Map();
    for (const spec of m.params) {
      let p = this._byId.get(spec.id);
      if (p) p._applySpec(spec);
      else p = new Param(this, spec);
      params[spec.index] = p;
      byId.set(spec.id, p);
    }
    const streams = [];
    const sById = new Map();
    for (const spec of m.streams) {
      let s = this._sById.get(spec.id);
      if (s) s._applySpec(spec);
      else s = new Stream(this, spec);
      streams[spec.index] = s;
      sById.set(spec.id, s);
      if (s._sub) this._sendSubscribe(s.index, s._sub.us, s._sub.enabled);
    }
    this.params = params;
    this.streams = streams;
    this._byId = byId;
    this._sById = sById;
    this.ready = true;
    this._ev.manifest.emit(m, this);
  }

  /**
   * Enter offline (design) mode: apply the manifest from `opts.offline`
   * (given, or built with {@link mockManifest} from its `params` and
   * `streams`), hydrate an empty store, and start the synthetic frame
   * generators in `opts.offline.frames`. Reconnecting continues in the
   * background; the first real manifest ends offline mode.
   * @private
   */
  _goOffline() {
    const o = this.opts.offline;
    const m = o.manifest || mockManifest(o);
    this._applyingOffline = true;
    this._applyManifest(m);
    this._applyingOffline = false;
    this.offline = true;
    if (!this.store.ready) this.store._hydrate({});
    if (o.frames) {
      const t0 = performance.now();
      let seq = 0;
      this._offlineFrames = setInterval(() => {
        const t = (performance.now() - t0) / 1000;
        for (const [id, gen] of Object.entries(o.frames)) {
          const s = this._sById.get(id);
          if (!s) continue;
          const out = gen(t, s);
          if (!out) continue;
          s._receive(out instanceof Float32Array ? out : Float32Array.from(out), seq, Math.round(t * 1e9));
        }
        seq++;
      }, 1000 / (o.frameRate ?? 30));
    }
    this._ev.message.emit('offline', { name: m.name });
  }

  /** Leave offline mode: stop the frame generators. @private */
  _stopOffline() {
    if (!this.offline) return;
    this.offline = false;
    clearInterval(this._offlineFrames);
    this._offlineFrames = 0;
  }

  /**
   * Decode one binary frame (see the table in the file header). Malformed
   * or truncated frames are ignored; unknown kinds are skipped so a newer
   * server can add frame types without breaking older pages.
   * @private
   * @param {ArrayBuffer} buf
   */
  _decode(buf) {
    if (buf.byteLength < 4) return;
    const dv = new DataView(buf);
    const kind = dv.getUint8(0);
    const arg = dv.getUint16(2, true);
    switch (kind) {
      case Kind.HELLO: {
        const version = dv.getUint16(4, true);
        if (version !== PROTOCOL_VERSION) {
          console.warn(`bridge: protocol ${version} != ${PROTOCOL_VERSION}`);
        }
        this.clientId = dv.getUint16(10, true);
        break;
      }
      case Kind.PARAM_VALUES: {
        let o = 4;
        for (let i = 0; i < arg && o + 8 <= buf.byteLength; i++, o += 8) {
          const index = dv.getUint16(o, true);
          const flags = dv.getUint16(o + 2, true);
          const value = dv.getFloat32(o + 4, true);
          const p = this.params[index];
          if (p) p._receive(value, flags);
        }
        break;
      }
      case Kind.STREAM_F32: {
        const s = this.streams[arg];
        const seq = dv.getUint32(4, true);
        const ts = (dv.getUint32(8, true) + dv.getUint32(12, true) * 4294967296) / 1e6;
        const len = dv.getUint32(16, true);
        if (STREAM_DATA_OFFSET + len * 4 > buf.byteLength) return;
        const data = new Float32Array(buf, STREAM_DATA_OFFSET, len);
        if (s) s._receive(data, seq, ts);
        break;
      }
      case Kind.STREAM_U8: {
        const s = this.streams[arg];
        const seq = dv.getUint32(4, true);
        const ts = (dv.getUint32(8, true) + dv.getUint32(12, true) * 4294967296) / 1e6;
        const len = dv.getUint32(16, true);
        if (STREAM_DATA_OFFSET + len > buf.byteLength) return;
        const data = new Uint8Array(buf, STREAM_DATA_OFFSET, len);
        if (s) s._receive(data, seq, ts);
        break;
      }
      case Kind.EVENTS_OUT: {
        let o = 4;
        for (let i = 0; i < arg && o + 12 <= buf.byteLength; i++, o += 12) {
          this._ev.event.emit({
            kind: dv.getUint8(o),
            channel: dv.getUint8(o + 1),
            a: dv.getUint8(o + 2),
            b: dv.getUint8(o + 3),
            value: dv.getFloat32(o + 4, true),
            offset: dv.getUint32(o + 8, true),
          });
        }
        break;
      }
      case Kind.PONG: {
        const sent = dv.getFloat64(4, true);
        const rtt = performance.now() - sent;
        this.stats.rttMs = rtt;
        this.stats.rttAvgMs = Number.isNaN(this.stats.rttAvgMs) ? rtt : this.stats.rttAvgMs * 0.8 + rtt * 0.2;
        break;
      }
      default:
        break;
    }
  }

  /**
   * Send bytes if the socket is open; otherwise drop them (edits made while
   * offline are not queued: the next `ParamValues` snapshot wins).
   * @private
   * @param {Uint8Array} bytes
   */
  _sendRaw(bytes) {
    const ws = this._ws;
    if (ws && ws.readyState === 1) ws.send(bytes);
  }

  /**
   * One `ParamEdit` frame with a single entry, from the pre-allocated buffer.
   * @private
   * @param {number} index Parameter index.
   * @param {0|1|2} phase 0 = begin, 1 = perform, 2 = end.
   * @param {number} value Normalized value.
   */
  _sendEdit(index, phase, value) {
    const v = this._editView;
    v.setUint8(0, Kind.PARAM_EDIT);
    v.setUint8(1, 0);
    v.setUint16(2, 1, true);
    v.setUint16(4, index, true);
    v.setUint8(6, phase);
    v.setUint8(7, 0);
    v.setFloat32(8, value, true);
    this._sendRaw(new Uint8Array(this._editBuf, 0, 12));
  }

  /**
   * `edits`: array of `[index, phase, value]`.
   * Batched into one `ParamEdit` frame; the pre-allocated buffer is used
   * for up to 8 entries, a fresh one beyond that.
   * @private
   * @param {Array<[number, 0|1|2, number]>} edits
   */
  _sendEdits(edits) {
    const n = edits.length;
    const size = 4 + 8 * n;
    let buf = this._editBuf;
    let v = this._editView;
    if (size > buf.byteLength) {
      buf = new ArrayBuffer(size);
      v = new DataView(buf);
    }
    v.setUint8(0, Kind.PARAM_EDIT);
    v.setUint8(1, 0);
    v.setUint16(2, n, true);
    let o = 4;
    for (const [index, phase, value] of edits) {
      v.setUint16(o, index, true);
      v.setUint8(o + 2, phase);
      v.setUint8(o + 3, 0);
      v.setFloat32(o + 4, value, true);
      o += 8;
    }
    this._sendRaw(new Uint8Array(buf, 0, size));
  }

  /**
   * One `Subscribe` frame: minimum interval in microseconds (0 = every
   * frame) and an enable flag for one stream.
   * @private
   * @param {number} index
   * @param {number} us
   * @param {boolean} enabled
   */
  _sendSubscribe(index, us, enabled) {
    const v = this._smallView;
    v.setUint8(0, Kind.SUBSCRIBE);
    v.setUint8(1, 0);
    v.setUint16(2, index, true);
    v.setUint32(4, us, true);
    v.setUint8(8, enabled ? 1 : 0);
    v.setUint8(9, 0);
    v.setUint16(10, 0, true);
    this._sendRaw(new Uint8Array(this._small, 0, 12));
  }

  /**
   * Record one edit-to-echo measurement (called from `Param._receive`).
   * @private
   * @param {number} ms
   */
  _recordEcho(ms) {
    this.stats.echoMs = ms;
    this.stats.echoAvgMs = Number.isNaN(this.stats.echoAvgMs) ? ms : this.stats.echoAvgMs * 0.8 + ms * 0.2;
  }
}

/**
 * Undo / redo and A/B over whole-parameter snapshots. Records a snapshot
 * after every completed local gesture (the client's `edit` event), so any
 * control built on `Param` gets history for free.
 *
 *   const history = new History(client);
 *   history.undo(); history.redo(); history.toggleAB(); history.copyAtoB();
 *
 * What counts as a step: one completed gesture (`beginEdit` ... `endEdit`),
 * one bare `set()`, or one `setMany()` batch (a preset load is a single
 * step). Steps whose snapshot equals the previous one are skipped. Host
 * automation is not recorded, but the next local step captures whatever the
 * host changed in between. Undo / redo apply their snapshots with
 * `emit: false`, so they do not record themselves.
 *
 * A/B: two named slots. `toggleAB()` saves the current state into the slot
 * being left and applies the other slot (the very first toggle, with no
 * other state yet, re-applies the current state). `copyToOther()` copies the
 * active state into the inactive slot. Applying the other slot is one
 * undoable step.
 */
export class History {
  /**
   * @param {NoobVstWebguiFrameworkClient} client
   * @param {{ limit?: number }} [opts]
   * @param {number} [opts.limit=200] Maximum undo depth; the oldest entries are dropped.
   */
  constructor(client, { limit = 200 } = {}) {
    /** @type {NoobVstWebguiFrameworkClient} */
    this.client = client;
    /** @type {number} Maximum undo depth. */
    this.limit = limit;
    /** @private */
    this._undo = [];
    /** @private */
    this._redo = [];
    /** The snapshot that matches the current state (`null` until the manifest arrives). @private */
    this._current = client.ready ? client.snapshot() : null;
    /** @private */
    this._ab = 'A';
    /** Snapshot stored for the inactive A/B slot. @private */
    this._other = null;
    /** @private */
    this._ev = makeEmitter();
    /** @private */
    this._offEdit = client.on('edit', () => this.record());
    /** @private */
    this._offManifest = client.on('manifest', () => {
      if (!this._current) this._current = client.snapshot();
    });
    /** @private */
    this._offHost = null;
  }

  /**
   * Listen for changes to `canUndo` / `canRedo` / `ab`.
   * @param {(history: History) => void} fn
   * @returns {Unsubscribe}
   */
  on(fn) {
    return this._ev.on(fn);
  }
  /** @type {boolean} */
  get canUndo() {
    return this._undo.length > 0;
  }
  /** @type {boolean} */
  get canRedo() {
    return this._redo.length > 0;
  }
  /** The active slot, `'A'` or `'B'`. @type {'A'|'B'} */
  get ab() {
    return this._ab;
  }

  /**
   * Push the current state as a new history entry (called automatically).
   * Clears the redo stack; does nothing if the state has not changed.
   * @returns {void}
   */
  record() {
    const now = this.client.snapshot();
    if (this._current && sameSnapshot(now, this._current)) return;
    if (this._current) this._undo.push(this._current);
    if (this._undo.length > this.limit) this._undo.shift();
    this._redo.length = 0;
    this._current = now;
    this._ev.emit(this);
  }

  /**
   * Restore the previous snapshot.
   * @returns {boolean} False when there was nothing to undo.
   */
  undo() {
    if (!this._undo.length) return false;
    const prev = this._undo.pop();
    this._redo.push(this._current);
    this._current = prev;
    this.client.setMany(prev, { emit: false });
    this._ev.emit(this);
    return true;
  }

  /**
   * Re-apply the snapshot undone last.
   * @returns {boolean} False when there was nothing to redo.
   */
  redo() {
    if (!this._redo.length) return false;
    const next = this._redo.pop();
    this._undo.push(this._current);
    this._current = next;
    this.client.setMany(next, { emit: false });
    this._ev.emit(this);
    return true;
  }

  /**
   * Switch between the A and B states (the inactive one is stored).
   * @returns {void}
   */
  toggleAB() {
    const now = this.client.snapshot();
    const other = this._other || now;
    this._other = now;
    this._ab = this._ab === 'A' ? 'B' : 'A';
    this.client.setMany(other);
    this._ev.emit(this);
  }

  /**
   * Copy the active state into the inactive slot.
   * @returns {void}
   */
  copyToOther() {
    this._other = this.client.snapshot();
    this._ev.emit(this);
  }

  /**
   * Stop listening to the client. The stacks are kept but no longer grow.
   * @returns {void}
   */
  destroy() {
    this._offEdit();
    this._offManifest();
  }
}

/**
 * Compare two snapshots as f32, so a value that round-tripped through the
 * wire still compares equal.
 * @private
 * @param {Float32Array} a
 * @param {Float32Array} b
 * @returns {boolean}
 */
function sameSnapshot(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (Math.fround(a[i]) !== Math.fround(b[i])) return false;
  return true;
}

/**
 * Key-value store for the page's own state (presets, view settings). Lives
 * in the plug-in, is persisted with the plug-in's state by the adapter, and
 * is shared by every client of the instance. Hydrated on connect; use
 * `ready` / `on('*')` before trusting `get`.
 *
 *   client.store.set('presets', list);
 *   const list = client.store.get('presets', []);
 *   client.store.on('presets', (v) => render(v));
 *
 * Semantics:
 *
 * * **Hydration.** The server sends the whole store (`store.all`) right
 *   after the parameter snapshot on every connect, and again whenever the
 *   plug-in replaces it (a host restoring state). Until then `ready` is
 *   false and `get()` returns its default. Hydration replaces the local
 *   cache and fires `'*'` listeners with `key === null`.
 * * **Writes** are optimistic: `set()` updates the cache and notifies local
 *   listeners at once, then sends `store.set`. Other clients receive
 *   `store.changed`; the sender does not get an echo. A value the server
 *   refuses (over 256 KiB, or the store over 1 MiB) is logged as a warning
 *   (`store.error`) and stays only in this page's cache.
 * * **Values** are anything JSON can carry. `null` (or `undefined`) removes
 *   the key. Keys are plain strings; the examples namespace them
 *   (`'presets.user'`, `'eqmatch.references'`).
 * * **What belongs here:** state that should travel with the plug-in
 *   instance and be seen by every window of it. Per-machine conveniences
 *   (a collapsed panel) can stay in `localStorage`.
 */
export class Store {
  /**
   * Created by the client as `client.store`.
   * @param {NoobVstWebguiFrameworkClient} client
   */
  constructor(client) {
    /** @private */
    this._client = client;
    /** @private */
    this._cache = new Map();
    /** @private */
    this._ev = makeEmitter();
    /** @type {boolean} True once the first `store.all` has arrived. */
    this.ready = false;
  }

  /**
   * Read a key from the local cache.
   * @param {string} key
   * @param {any} [dflt=undefined] Returned when the key is absent (or before hydration).
   * @returns {any}
   */
  get(key, dflt = undefined) {
    return this._cache.has(key) ? this._cache.get(key) : dflt;
  }

  /**
   * Whether the key is present.
   * @param {string} key
   * @returns {boolean}
   */
  has(key) {
    return this._cache.has(key);
  }

  /**
   * Every key currently in the store.
   * @returns {string[]}
   */
  keys() {
    return [...this._cache.keys()];
  }

  /**
   * Set (or remove with `null`) a key; the plug-in and other clients follow.
   * @param {string} key
   * @param {any} value JSON-serialisable; `null` / `undefined` removes the key.
   * @returns {void}
   */
  set(key, value) {
    if (value == null) this._cache.delete(key);
    else this._cache.set(key, value);
    this._client.send('store.set', { key, value: value == null ? null : value });
    this._ev.emit(key, value);
  }

  /**
   * `fn(key, value)`; `key` filters, `'*'` fires for every change and on hydration (`key === null`).
   *
   * Fires for local `set()` calls, for `store.changed` from other clients,
   * and (with `'*'` only) for hydration. On a removal `value` is `null`.
   * @param {string|'*'} key
   * @param {(key: string|null, value: any) => void} fn
   * @returns {Unsubscribe}
   */
  on(key, fn) {
    return this._ev.on((k, v) => {
      if (key === '*' || k === key) fn(k, v);
    });
  }

  /**
   * Replace the cache with the server's copy (`store.all`).
   * @private
   * @param {Record<string, any>|null|undefined} values
   */
  _hydrate(values) {
    this._cache = new Map(Object.entries(values || {}));
    this.ready = true;
    this._ev.emit(null, null);
  }

  /**
   * Apply a change made by another client (`store.changed`).
   * @private
   * @param {string} key
   * @param {any} value `null` removes.
   */
  _changed(key, value) {
    if (value == null) this._cache.delete(key);
    else this._cache.set(key, value);
    this._ev.emit(key, value);
  }
}

/**
 * @typedef {object} OfflineOptions
 * @property {Manifest} [manifest] A complete manifest to use; otherwise one is built from `params` / `streams`.
 * @property {string} [name='offline'] Plug-in name for the built manifest.
 * @property {object} [meta] Manifest `meta` (e.g. `{ sample_rate: 48000 }`).
 * @property {Array<object>} [params] Minimal specs: `{ id, name?, unit?, group?, min = 0, max = 1, default = min, taper = 'linear'|'log'|'skew', skew?, steps?, labels?, toggle?, automatable? }`.
 * @property {Array<object>} [streams] Minimal specs: `{ id, name?, kind = 'raw', capacity = 1, channels = 1, meta?, sticky? }`.
 * @property {Record<string, (tSeconds: number, stream: Stream) => (ArrayLike<number>|null)>} [frames] Synthetic frame generators by stream id, called `frameRate` times a second.
 * @property {number} [frameRate=30]
 * @property {number} [timeoutMs=1200] How long to wait for a real manifest before falling back.
 * @property {boolean} [immediate=false] Fall back at once (and still connect for real in the background).
 */

/**
 * Build a manifest for offline (design-time) use from minimal parameter and
 * stream specs, so a page can be developed and styled before the plug-in
 * exists or without running it: `configureClient({ offline: { params, streams,
 * frames } })` in Vue, or `new NoobVstWebguiFrameworkClient(null, { offline })`.
 * Tapers are the same as the server's (`linear`, `log`, `skew`); the
 * 65-point `table` and `default_norm` are derived. Ids must match what the
 * plug-in will publish, or the page will not bind once it goes live.
 *
 * @param {OfflineOptions} [spec]
 * @returns {Manifest}
 */
export function mockManifest({ name = 'offline', meta = {}, params = [], streams = [] } = {}) {
  const clamp01 = (n) => Math.max(0, Math.min(1, n));
  const outParams = params.map((p, index) => {
    const min = p.min ?? 0;
    const taper = p.taper || 'linear';
    const skew = p.skew ?? 1;
    const steps = p.steps ?? (p.labels ? p.labels.length : p.toggle ? 2 : 0);
    // A stepped parameter (`labels`, `toggle`, or an explicit `steps`) with no
    // range of its own spans its step indices, which is what the plug-in
    // publishes for one. Without this a default past the first step would
    // normalise above 1 and clamp, so the page would open on the last step.
    const max = p.max ?? (steps > 1 ? min + steps - 1 : 1);
    const dflt = p.default ?? min;
    const lo = Math.max(min, 1e-9);
    const toPlain = (n) => (taper === 'log' ? lo * Math.pow(max / lo, n) : taper === 'skew' ? min + (max - min) * Math.pow(n, 1 / skew) : min + (max - min) * n);
    const toNorm = (v) => (taper === 'log' ? Math.log(v / lo) / Math.log(max / lo) : taper === 'skew' ? Math.pow((v - min) / (max - min), skew) : (v - min) / (max - min));
    return {
      index,
      id: p.id,
      name: p.name ?? p.id,
      unit: p.unit ?? '',
      group: p.group ?? '',
      min,
      max,
      default: dflt,
      default_norm: clamp01(toNorm(dflt)),
      taper,
      skew: taper === 'skew' ? skew : undefined,
      steps,
      labels: p.labels ?? [],
      automatable: p.automatable ?? true,
      table: Array.from({ length: 65 }, (_, i) => toPlain(i / 64)),
    };
  });
  const outStreams = streams.map((s, index) => ({
    index,
    id: s.id,
    name: s.name ?? s.id,
    kind: s.kind ?? 'raw',
    capacity: s.capacity ?? 1,
    channels: s.channels ?? 1,
    meta: s.meta ?? {},
    sticky: !!s.sticky,
  }));
  return { t: 'manifest', name, protocol: PROTOCOL_VERSION, meta, params: outParams, streams: outStreams };
}

/**
 * Inject a stylesheet once. Used by the components.
 *
 * A no-op outside a document (SSR, tests) and when an element with that id
 * already exists, so components can call it from every constructor.
 * @param {string} id  Element id used for de-duplication (`'noob-vst-webgui-framework-knob'`).
 * @param {string} css Stylesheet text.
 * @returns {void}
 */
export function injectStyle(id, css) {
  if (typeof document === 'undefined' || document.getElementById(id)) return;
  const el = document.createElement('style');
  el.id = id;
  el.textContent = css;
  document.head.appendChild(el);
}

/**
 * Resolve a value that may be a Param, a function or a number to a plain number.
 *
 * Lets a component option accept a live `Param` (read each frame), a getter,
 * or a constant. `null` / `undefined` resolve to 0.
 * @param {Param|(() => number)|number|{ plain: number }|null|undefined} v
 * @returns {number}
 */
export function plainOf(v) {
  if (v == null) return 0;
  if (typeof v === 'number') return v;
  if (typeof v === 'function') return v();
  if (typeof v === 'object' && 'plain' in v) return v.plain;
  return Number(v);
}

export default NoobVstWebguiFrameworkClient;
