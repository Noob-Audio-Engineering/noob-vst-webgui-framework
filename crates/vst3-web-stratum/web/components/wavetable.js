/**
 * WavetableView — draws a table of single-cycle frames as a stack and the
 * frame the plugin is currently playing in front, at the morph position.
 *
 *   const view = new WavetableView(el, { position: client.param('wt_pos') });
 *   client.stream('wavetable').on((d, s) => view.setTable(d, s.meta.frames));
 *   client.stream('wt_position').on((d) => view.setLivePosition(d[0]));
 *
 * Vertical drag sets the position Param (with begin / end gestures).
 *
 * ## Data model
 *
 * A table is `frames` consecutive single cycles of `size` samples each
 * (`data.length = frames · size`), values in −1..1. Plugins publish it on a
 * **sticky** stream so a late window still gets it (the server replays the
 * last frame on connect). Two positions are drawn:
 *
 * * `opts.position` — the *parameter*, 0..1 over the table (a Param, a
 *   number, or a getter as accepted by `plainOf`); it decides which stack
 *   frame is highlighted and is what dragging edits.
 * * `setLivePosition(p)` — the position the plugin is *actually* playing
 *   after modulation (an LFO, an envelope). When set it drives the front
 *   frame; otherwise the parameter does.
 *
 * ## Projection
 *
 * Frames are drawn back to front as a pseudo-3D stack: frame at depth
 * `t ∈ 0..1` (0 = front) is offset by `t · depthX · width` to the right and
 * `−t · depthY · height` upward, and spans `(1 − depthX) · width`
 * horizontally with an amplitude of `0.45 · (1 − depthY) · height`. The
 * table is subsampled to at most `maxFrames` in the stack and at most 128
 * points per cycle. The front frame sits at a fractional depth and is
 * linearly interpolated between the two nearest table frames (`_sample`),
 * so a morph sweeps smoothly rather than stepping.
 *
 * ## Gestures (when `draggable` and `position` is a Param)
 *
 * Vertical drag covers the full 0..1 range over 80 % of the height (Shift:
 * ×0.1); wheel moves ±0.03 per notch (Shift ±0.005) inside one gesture that
 * closes 150 ms after the last notch. Both use begin / set / end on the
 * Param.
 *
 * ## Styling
 *
 * Colours are options only: `color` (front frame, with a glow), `stackColor`
 * (back frames), `nearColor` (the stack frame nearest the parameter
 * position). Root class `.vst3-web-stratum-wt` (cursor `ns-resize`).
 */
import { injectStyle, plainOf } from '../vst3-web-stratum.js';

const CSS = `
.vst3-web-stratum-wt{position:relative;width:100%;height:100%;user-select:none;-webkit-user-select:none;touch-action:none;cursor:ns-resize}
.vst3-web-stratum-wt canvas{display:block;width:100%;height:100%}
`;

/** Duck-typed test for a vst3-web-stratum Param (anything with an `on` subscription). */
const isParam = (v) => v && typeof v === 'object' && typeof v.on === 'function';

/**
 * Wavetable stack view.
 *
 * Public fields: `el` (root `<div>`), `canvas`, `opts`, and the `position`
 * getter. Feed it with `setTable` and, optionally, `setLivePosition`.
 */
export class WavetableView {
  /**
   * @param {HTMLElement} container Element the view is appended to; decides the size.
   * @param {object} [opts]
   * @param {object|number|(() => number)} [opts.position=0] Morph position 0..1: a Param (editable), a number, or a getter.
   * @param {number} [opts.depthX=0.45] Horizontal offset of the back frame, as a fraction of width.
   * @param {number} [opts.depthY=0.55] Vertical offset of the back frame, as a fraction of height.
   * @param {number} [opts.maxFrames=32] Frames drawn in the stack (the table is subsampled evenly).
   * @param {string} [opts.color='#5ac8fa'] Front (live) frame colour.
   * @param {string} [opts.stackColor='rgba(148,163,184,0.35)'] Back frame colour.
   * @param {string} [opts.nearColor='rgba(90,200,250,0.5)'] Colour of the stack frame nearest the parameter position.
   * @param {boolean} [opts.draggable=true] Let a vertical drag / wheel edit `position` (only when it is a Param).
   * @example
   * const view = new WavetableView(el, { position: client.param('wt_position'), maxFrames: 24 });
   * client.stream('wavetable').on((d, s) => view.setTable(d, s.meta.frames));
   */
  constructor(container, opts = {}) {
    injectStyle('vst3-web-stratum-wt-css', CSS);
    this.opts = {
      position: 0,
      depthX: 0.45,
      depthY: 0.55,
      maxFrames: 32,
      color: '#5ac8fa',
      stackColor: 'rgba(148,163,184,0.35)',
      nearColor: 'rgba(90,200,250,0.5)',
      draggable: true,
      ...opts,
    };
    this.el = document.createElement('div');
    this.el.className = 'vst3-web-stratum-wt';
    this.canvas = document.createElement('canvas');
    this.el.appendChild(this.canvas);
    container.appendChild(this.el);
    this._ctx = this.canvas.getContext('2d');
    this._table = null;
    this._frames = 0;
    this._size = 0;
    this._live = null;
    this._dirty = true;
    this._ro = new ResizeObserver(() => {
      this._resize();
      this._dirty = true;
    });
    this._ro.observe(this.el);
    this._resize();
    if (isParam(this.opts.position)) this._off = this.opts.position.on(() => (this._dirty = true));
    if (this.opts.draggable) {
      this.el.addEventListener('pointerdown', this._onDown);
      this.el.addEventListener('pointermove', this._onMove);
      this.el.addEventListener('pointerup', this._onUp);
      this.el.addEventListener('pointercancel', this._onUp);
      this.el.addEventListener('wheel', this._onWheel, { passive: false });
    }
    this._running = true;
    this._raf = requestAnimationFrame(this._tick);
  }

  /**
   * Load a table: `data` holds `frames` consecutive single cycles, so the
   * cycle length is `data.length / frames`. The array is kept by reference
   * (stream frames are never reused, so that is safe).
   * @param {Float32Array|number[]} data Samples in −1..1.
   * @param {number} frames Number of cycles in `data` (≥ 1).
   */
  setTable(data, frames) {
    frames = Math.max(1, frames | 0);
    this._table = data;
    this._frames = frames;
    this._size = Math.floor(data.length / frames);
    this._dirty = true;
  }

  /**
   * The position the plugin is actually playing (after modulation), 0..1.
   * Drives the front frame; pass `null` to fall back to the parameter.
   * @param {number|null} p
   */
  setLivePosition(p) {
    this._live = p;
    this._dirty = true;
  }

  /**
   * The parameter position (0..1, clamped), resolved from a Param, number
   * or getter.
   * @type {number}
   */
  get position() {
    return Math.max(0, Math.min(1, plainOf(this.opts.position)));
  }

  /** Match the backing store to the element × devicePixelRatio; draw in CSS px. */
  _resize() {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, this.el.clientWidth);
    const h = Math.max(1, this.el.clientHeight);
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this._w = w;
    this._h = h;
    this._ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  /** Animation loop: redraw only when a table, position or resize marked the view dirty. */
  _tick = () => {
    if (!this._running) return;
    if (this._dirty) {
      this._dirty = false;
      this._draw();
    }
    this._raf = requestAnimationFrame(this._tick);
  };

  /**
   * Sample of frame `f` (fractional: interpolates linearly between the two
   * neighbouring frames) at index `i`.
   * @param {number} f Frame position, 0 … frames − 1.
   * @param {number} i Sample index within the cycle.
   * @returns {number}
   */
  _sample(f, i) {
    const t = this._table;
    const n = this._size;
    const f0 = Math.floor(f);
    const f1 = Math.min(this._frames - 1, f0 + 1);
    const k = f - f0;
    const a = t[f0 * n + i];
    const b = t[f1 * n + i];
    return a + (b - a) * k;
  }

  /** Stack (back to front, nearest-to-position frame highlighted), then the glowing front frame at the live depth. */
  _draw() {
    const ctx = this._ctx;
    const w = this._w;
    const h = this._h;
    ctx.clearRect(0, 0, w, h);
    if (!this._table || !this._size) return;
    const frames = this._frames;
    const n = this._size;
    const step = Math.max(1, Math.ceil(n / 128));
    const drawn = Math.min(frames, this.opts.maxFrames);
    const dx = w * this.opts.depthX;
    const dy = h * this.opts.depthY;
    const fw = w - dx;
    const fh = (h - dy) * 0.9;
    const live = this._live != null ? this._live : this.position;
    const pos = Math.max(0, Math.min(1, this.position));

    const path = (f, ox, oy, amp) => {
      ctx.beginPath();
      for (let i = 0, k = 0; i <= n; i += step, k++) {
        const v = this._sample(f, Math.min(i, n - 1));
        const x = ox + (i / n) * fw;
        const y = oy - v * amp;
        if (k === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
    };

    // Back to front.
    for (let k = drawn - 1; k >= 0; k--) {
      const t = drawn === 1 ? 0 : k / (drawn - 1);
      const f = t * (frames - 1);
      const ox = t * dx;
      const oy = h - dy * (1 - t) - fh * 0.5 - (h - dy) * 0.05;
      const near = Math.abs(t - pos) < 1 / Math.max(2, drawn);
      ctx.strokeStyle = near ? this.opts.nearColor : this.opts.stackColor;
      ctx.lineWidth = near ? 1.4 : 1;
      path(f, ox, oy, fh * 0.5);
      ctx.stroke();
    }
    // Live frame in front, at its own depth.
    const lt = Math.max(0, Math.min(1, live));
    const lox = lt * dx;
    const loy = h - dy * (1 - lt) - fh * 0.5 - (h - dy) * 0.05;
    ctx.strokeStyle = this.opts.color;
    ctx.lineWidth = 2.2;
    ctx.shadowColor = this.opts.color;
    ctx.shadowBlur = 8;
    path(lt * (frames - 1), lox, loy, fh * 0.5);
    ctx.stroke();
    ctx.shadowBlur = 0;
  }

  /** Primary button starts a position drag (only when `position` is a Param): capture, open the gesture. */
  _onDown = (e) => {
    if (e.button !== 0 || !isParam(this.opts.position)) return;
    e.preventDefault();
    this.el.setPointerCapture(e.pointerId);
    this._drag = { id: e.pointerId, y: e.clientY, n: this.opts.position.value };
    this.opts.position.beginEdit();
  };
  /** Vertical motion: 80 % of the height = the full range (Shift ×0.1). */
  _onMove = (e) => {
    if (!this._drag || e.pointerId !== this._drag.id) return;
    const dy = this._drag.y - e.clientY;
    this._drag.y = e.clientY;
    this._drag.n = Math.max(0, Math.min(1, this._drag.n + (dy / (this._h * 0.8)) * (e.shiftKey ? 0.1 : 1)));
    this.opts.position.set(this._drag.n);
  };
  /** End of the drag: close the gesture. */
  _onUp = (e) => {
    if (!this._drag || e.pointerId !== this._drag.id) return;
    this._drag = null;
    this.opts.position.endEdit();
  };
  /** Wheel: ±0.03 per notch (Shift ±0.005), one gesture closed 150 ms after the last notch. */
  _onWheel = (e) => {
    if (!isParam(this.opts.position)) return;
    e.preventDefault();
    const p = this.opts.position;
    if (!this._wheelTimer) p.beginEdit();
    clearTimeout(this._wheelTimer);
    p.set(p.value + (e.deltaY < 0 ? 1 : -1) * (e.shiftKey ? 0.005 : 0.03));
    this._wheelTimer = setTimeout(() => {
      this._wheelTimer = null;
      p.endEdit();
    }, 150);
  };

  /** Stop the animation loop, unsubscribe from the position Param and remove the element. */
  destroy() {
    this._running = false;
    cancelAnimationFrame(this._raf);
    this._off?.();
    this._ro.disconnect();
    this.el.remove();
  }
}

export default WavetableView;
