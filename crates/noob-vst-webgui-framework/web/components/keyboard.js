/**
 * Keyboard — an on-screen piano keyboard that sends note events to the
 * plugin and lights up keys the plugin reports (host MIDI).
 *
 *   new Keyboard(el, client, { low: 48, high: 84 });
 *
 * Mouse / touch: press and glide across keys. Computer keyboard: the usual
 * `a w s e d f t g y h u j k o l p ; '` row, `z` / `x` shift octaves.
 *
 * ## Data flow
 *
 * Outgoing: every press calls `client.noteOn(note, velocity, channel)` and
 * every release `client.noteOff(note, 0, channel)` — binary `Events` frames
 * (see `docs/WIRE.md`), which the plugin's audio thread drains. A note is
 * never sent twice while held; a QWERTY key, a pointer and a glide all share
 * one `_held` set, so overlapping inputs release cleanly.
 *
 * Incoming (`opts.remote`): the component listens to `client.on('event')`
 * and marks keys the *plugin* reports (host MIDI, a sequencer) with the
 * `.remote` class — `NOTE_ON` with velocity > 0 lights, `NOTE_OFF` or a
 * zero-velocity `NOTE_ON` clears. A key held locally shows `.on` instead.
 *
 * ## Layout
 *
 * Keys are absolutely positioned `<div>`s. White keys share the container
 * width equally; a black key is 60 % of a white key wide, 62 % as tall, and
 * starts 30 % of a white key left of the boundary it sits on. Layout is
 * redone on resize (`ResizeObserver`) and when the range changes. A "C"
 * label is printed on every C when `opts.labels` is on.
 *
 * ## Gestures
 *
 * * **Pointer**: primary button on a key starts the note; the velocity comes
 *   from how far down the key was hit (`0.2 … 1.0`, top to bottom, offset by
 *   0.3 so a mid-key press is ≈ 0.8). Moving to another key while held
 *   glides (release the old note, start the new one at `opts.velocity`).
 *   Releasing, cancelling, or leaving the keyboard without a button held
 *   releases the note. Pointer capture keeps the gesture alive across keys.
 * * **QWERTY** (`opts.qwerty`): `a w s e d f t g y h u j k o l p ; '` map to
 *   semitones 0..17 above `opts.low + 12 · octave`; `z` / `x` shift
 *   `octave` (clamped −4..4). Keys are ignored when the focus is in an
 *   input, select, textarea or contenteditable, when a modifier is held, and
 *   on auto-repeat. Window `blur` releases everything so no note hangs
 *   when the window loses focus.
 *
 * ## Styling
 *
 * CSS variables: `--noob-vst-webgui-framework-key-white`, `--noob-vst-webgui-framework-key-black`,
 * `--noob-vst-webgui-framework-key-border`, `--noob-vst-webgui-framework-accent` (locally held keys),
 * `--noob-vst-webgui-framework-key-remote` (keys the plugin reports). Classes: root
 * `.noob-vst-webgui-framework-kbd`, `.key.white` / `.key.black`, state `.on` / `.remote`,
 * `.label`.
 */
import { EventKind, injectStyle } from '../noob-vst-webgui-framework.js';

const CSS = `
.noob-vst-webgui-framework-kbd{position:relative;width:100%;height:100%;user-select:none;-webkit-user-select:none;touch-action:none;
  font:10px system-ui,sans-serif}
.noob-vst-webgui-framework-kbd .key{position:absolute;box-sizing:border-box;border-radius:0 0 4px 4px;cursor:pointer;transition:background .05s}
.noob-vst-webgui-framework-kbd .white{top:0;bottom:0;background:var(--noob-vst-webgui-framework-key-white,#e2e8f0);border:1px solid var(--noob-vst-webgui-framework-key-border,#0f172a);z-index:1}
.noob-vst-webgui-framework-kbd .black{top:0;height:62%;background:var(--noob-vst-webgui-framework-key-black,#1e293b);border:1px solid #000;z-index:2}
.noob-vst-webgui-framework-kbd .white.on{background:var(--noob-vst-webgui-framework-accent,#5ac8fa)}
.noob-vst-webgui-framework-kbd .black.on{background:var(--noob-vst-webgui-framework-accent,#5ac8fa)}
.noob-vst-webgui-framework-kbd .white.remote{background:var(--noob-vst-webgui-framework-key-remote,#ffd166)}
.noob-vst-webgui-framework-kbd .black.remote{background:var(--noob-vst-webgui-framework-key-remote,#ffd166)}
.noob-vst-webgui-framework-kbd .label{position:absolute;bottom:3px;left:0;right:0;text-align:center;color:#334155;pointer-events:none}
.noob-vst-webgui-framework-kbd .hint{position:absolute;top:3px;left:0;right:0;text-align:center;color:#64748b;pointer-events:none;font-size:9px}
`;

/** Pitch classes (note % 12) that are black keys. */
const BLACK = new Set([1, 3, 6, 8, 10]);
/** Computer keyboard → semitone offset from the current QWERTY base note. */
const QWERTY = { a: 0, w: 1, s: 2, e: 3, d: 4, f: 5, t: 6, g: 7, y: 8, h: 9, u: 10, j: 11, k: 12, o: 13, l: 14, p: 15, ';': 16, "'": 17 };
/** Pitch-class names used by `Keyboard.noteName`. */
const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

/**
 * On-screen piano keyboard bound to a client.
 *
 * Public fields: `el` (root `<div>`), `client`, `opts`, and the `octave`
 * accessor. Notes are MIDI numbers (60 = C4 = middle C); velocities are
 * 0..1 floats as on the wire.
 */
export class Keyboard {
  /**
   * @param {HTMLElement} container Element the keyboard is appended to; decides the size.
   * @param {import('../noob-vst-webgui-framework.js').NoobVstWebguiFrameworkClient} client Client whose `noteOn` / `noteOff` are called.
   * @param {object} [opts]
   * @param {number} [opts.low=48] Lowest MIDI note shown (C3).
   * @param {number} [opts.high=84] Highest MIDI note shown (C6).
   * @param {number} [opts.velocity=0.8] Velocity for QWERTY notes and glides (0..1); pointer presses derive theirs from the hit position.
   * @param {number} [opts.channel=0] MIDI channel sent with every event.
   * @param {boolean} [opts.qwerty=true] Listen to the computer keyboard (window-level listeners).
   * @param {boolean} [opts.labels=true] Print `C3`, `C4`, … on the C keys.
   * @param {boolean} [opts.remote=true] Light keys from plugin-reported note events.
   * @param {(note:number, on:boolean, velocity:number)=>void} [opts.onNote] Called after every locally started (`on = true`) or released note, for a page's own display.
   * @example
   * const kbd = new Keyboard(document.querySelector('#keys'), client, { low: 36, high: 96 });
   * octaveDown.onclick = () => (kbd.octave -= 1);
   */
  constructor(container, client, opts = {}) {
    injectStyle('noob-vst-webgui-framework-kbd-css', CSS);
    this.client = client;
    this.opts = { low: 48, high: 84, velocity: 0.8, channel: 0, qwerty: true, labels: true, remote: true, onNote: null, ...opts };
    this.el = document.createElement('div');
    this.el.className = 'noob-vst-webgui-framework-kbd';
    container.appendChild(this.el);
    this._keys = new Map();
    this._held = new Set();
    this._remote = new Set();
    this._qwertyHeld = new Map();
    this._octave = 0;
    this._pointerNote = null;
    this._build();
    this.el.addEventListener('pointerdown', this._onDown);
    this.el.addEventListener('pointermove', this._onMove);
    this.el.addEventListener('pointerup', this._onUp);
    this.el.addEventListener('pointercancel', this._onUp);
    this.el.addEventListener('pointerleave', this._onUp);
    if (this.opts.qwerty) {
      window.addEventListener('keydown', this._onKeyDown);
      window.addEventListener('keyup', this._onKeyUp);
      window.addEventListener('blur', this._releaseAll);
    }
    if (this.opts.remote) {
      this._offEvent = client.on('event', (e) => {
        if (e.kind === EventKind.NOTE_ON && e.value > 0) this._remote.add(e.a);
        else if (e.kind === EventKind.NOTE_OFF || e.kind === EventKind.NOTE_ON) this._remote.delete(e.a);
        else return;
        this._paint(e.a);
      });
    }
    this._ro = new ResizeObserver(() => this._layout());
    this._ro.observe(this.el);
  }

  /**
   * Show a different range of keys (rebuilds the DOM; held notes keep
   * sounding but lose their highlight if they scroll out of range).
   * @param {number} low Lowest MIDI note.
   * @param {number} high Highest MIDI note.
   */
  setRange(low, high) {
    this.opts.low = low;
    this.opts.high = high;
    this._build();
  }

  /**
   * Octave shift applied to QWERTY input (`z` / `x` change it), clamped to
   * −4..4. The base note is `opts.low + 12 · octave`.
   * @type {number}
   */
  get octave() {
    return this._octave;
  }
  set octave(o) {
    this._octave = Math.max(-4, Math.min(4, o));
  }

  /** Create one `<div class="key">` per note in range (plus C labels) and lay them out. */
  _build() {
    this.el.textContent = '';
    this._keys.clear();
    for (let n = this.opts.low; n <= this.opts.high; n++) {
      const k = document.createElement('div');
      const black = BLACK.has(n % 12);
      k.className = `key ${black ? 'black' : 'white'}`;
      k.dataset.note = String(n);
      if (!black && this.opts.labels && n % 12 === 0) {
        const l = document.createElement('div');
        l.className = 'label';
        l.textContent = `C${Math.floor(n / 12) - 1}`;
        k.appendChild(l);
      }
      this.el.appendChild(k);
      this._keys.set(n, k);
    }
    this._layout();
  }

  /** Position keys: white keys share the width equally, black keys straddle the boundary before them. */
  _layout() {
    const w = this.el.clientWidth;
    let whites = 0;
    for (let n = this.opts.low; n <= this.opts.high; n++) if (!BLACK.has(n % 12)) whites++;
    const ww = w / Math.max(1, whites);
    let x = 0;
    for (let n = this.opts.low; n <= this.opts.high; n++) {
      const k = this._keys.get(n);
      if (BLACK.has(n % 12)) {
        k.style.left = `${x - ww * 0.3}px`;
        k.style.width = `${ww * 0.6}px`;
      } else {
        k.style.left = `${x}px`;
        k.style.width = `${ww}px`;
        x += ww;
      }
    }
  }

  /** Refresh one key's state classes: `.on` when held locally, else `.remote` when the plugin reports it. */
  _paint(n) {
    const k = this._keys.get(n);
    if (!k) return;
    k.classList.toggle('on', this._held.has(n));
    k.classList.toggle('remote', !this._held.has(n) && this._remote.has(n));
  }

  /** Start a note once (ignored while already held): send, notify, repaint. */
  _noteOn(n, v = this.opts.velocity) {
    if (this._held.has(n)) return;
    this._held.add(n);
    this.client.noteOn(n, v, this.opts.channel);
    this.opts.onNote?.(n, true, v);
    this._paint(n);
  }
  /** Release a note if held: send, notify, repaint. */
  _noteOff(n) {
    if (!this._held.delete(n)) return;
    this.client.noteOff(n, 0, this.opts.channel);
    this.opts.onNote?.(n, false, 0);
    this._paint(n);
  }
  /** Release everything (window blur, destroy) so nothing hangs. */
  _releaseAll = () => {
    for (const n of [...this._held]) this._noteOff(n);
    this._qwertyHeld.clear();
    this._pointerNote = null;
  };

  /**
   * MIDI note under a pointer event, via hit testing (black keys overlap
   * white ones, so the top-most element decides).
   * @returns {number|null}
   */
  _noteAt(e) {
    const t = document.elementFromPoint(e.clientX, e.clientY);
    const k = t && t.closest ? t.closest('.key') : null;
    return k && this.el.contains(k) ? Number(k.dataset.note) : null;
  }
  /** Primary button: velocity from the vertical hit position, then note on. */
  _onDown = (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    this.el.setPointerCapture(e.pointerId);
    const n = this._noteAt(e);
    if (n == null) return;
    const rect = this._keys.get(n).getBoundingClientRect();
    const v = Math.max(0.2, Math.min(1, (e.clientY - rect.top) / rect.height + 0.3));
    this._pointerNote = n;
    this._noteOn(n, v);
  };
  /** Glide: crossing into another key releases the old note and starts the new one. */
  _onMove = (e) => {
    if (this._pointerNote == null) return;
    const n = this._noteAt(e);
    if (n == null || n === this._pointerNote) return;
    this._noteOff(this._pointerNote);
    this._pointerNote = n;
    this._noteOn(n);
  };
  /** Release on up / cancel, or on leave when no button is held (a captured drag may leave and come back). */
  _onUp = (e) => {
    if (this._pointerNote == null) return;
    if (e.type === 'pointerleave' && e.buttons) return;
    this._noteOff(this._pointerNote);
    this._pointerNote = null;
  };
  /** QWERTY: `z`/`x` shift the octave; mapped keys start a note unless a text field has focus. */
  _onKeyDown = (e) => {
    const t = e.target;
    if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    const key = e.key.toLowerCase();
    if (key === 'z') return void (this.octave = this._octave - 1);
    if (key === 'x') return void (this.octave = this._octave + 1);
    const semi = QWERTY[key];
    if (semi == null) return;
    const n = this.opts.low + this._octave * 12 + semi;
    if (this._qwertyHeld.has(key)) return;
    this._qwertyHeld.set(key, n);
    this._noteOn(n);
    e.preventDefault();
  };
  /** QWERTY release: the note remembered at key-down is released even if the octave changed since. */
  _onKeyUp = (e) => {
    const key = e.key.toLowerCase();
    const n = this._qwertyHeld.get(key);
    if (n == null) return;
    this._qwertyHeld.delete(key);
    this._noteOff(n);
  };

  /**
   * Scientific pitch name of a MIDI note (`60 → "C4"`, `61 → "C#4"`).
   * @param {number} n MIDI note number.
   * @returns {string}
   */
  static noteName(n) {
    return NAMES[n % 12] + (Math.floor(n / 12) - 1);
  }

  /** Release held notes, unsubscribe from client events and window keys, remove the element. */
  destroy() {
    this._releaseAll();
    this._offEvent?.();
    this._ro.disconnect();
    window.removeEventListener('keydown', this._onKeyDown);
    window.removeEventListener('keyup', this._onKeyUp);
    window.removeEventListener('blur', this._releaseAll);
    this.el.remove();
  }
}

export default Keyboard;
