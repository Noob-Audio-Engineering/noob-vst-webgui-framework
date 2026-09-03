/**
 * Knob — an SVG rotary control bound to a noob-vst-webgui-framework Param.
 *
 *   new Knob(container, client.param('cutoff'), { size: 80 });
 *
 * Drag vertically (Shift = fine), scroll, double-click to reset, arrow keys.
 * Emits begin/perform/end gestures so the host records automation properly.
 *
 * ## Data model
 *
 * The knob works in the Param's **normalized** domain (0..1) and never
 * converts to plain values itself: the arc and indicator follow
 * `param.value`, the text follows `param.format()` (or `opts.format(plain)`).
 * Discrete Params (`param.isDiscrete`, i.e. `spec.steps > 1`) snap the drag
 * to steps and only send when the step changes, so a host sees one edit per
 * detent, and wheel / arrow keys move exactly one step.
 *
 * ## Geometry
 *
 * The SVG uses a fixed `viewBox` of 100×100 scaled to `opts.size` px. Angles
 * are measured clockwise from 12 o'clock: the track spans
 * `-sweep/2 … +sweep/2` (270° by default, so it starts at 7:30 and ends at
 * 4:30). The value arc runs from the track start (unipolar) or from 12
 * o'clock (bipolar) to the current angle `-sweep/2 + sweep·value`. Track and
 * value arcs are 8 units wide with round caps; the indicator line is 4 wide.
 *
 * ## Rendering
 *
 * Param changes set a dirty flag and redraw once per animation frame
 * (`_schedule`), so a burst of host automation costs one DOM update.
 * `_render` touches four things: the value arc path, the indicator rotation,
 * the value text and the ARIA attributes.
 *
 * ## Gestures
 *
 * | input                       | effect                                             |
 * |-----------------------------|----------------------------------------------------|
 * | drag vertically             | `sensitivity` px = the full range (Shift: ×0.1)    |
 * | wheel                       | ±0.02 (Shift: ±0.002), one step when discrete; the gesture stays open for 150 ms after the last notch |
 * | double-click                | `param.reset()` (host default)                     |
 * | ↑ → / ↓ ←                   | ±0.01 (Shift: ±0.1), one step when discrete        |
 * | Home / End                  | 0 / 1                                              |
 * | Backspace / Delete          | default value                                      |
 *
 * Pointer capture keeps the drag alive when the pointer leaves the knob.
 * The element is focusable (`tabIndex = 0`) and exposes `role="slider"`
 * with `aria-valuemin/max/now/text`, so screen readers and keyboard users
 * get the same control.
 *
 * ## Styling
 *
 * CSS variables: `--noob-vst-webgui-framework-text` (indicator, value text), `--noob-vst-webgui-framework-accent`
 * (value arc, focus ring; `opts.color` overrides it per knob),
 * `--noob-vst-webgui-framework-track` (background arc), `--noob-vst-webgui-framework-knob-body` (disc fill).
 * Classes on the root `.noob-vst-webgui-framework-knob`: `svg`, `.track`, `.value`, `.ind`,
 * `.body`, `.val` (value text), `.lbl` (label).
 */
import { injectStyle } from '../noob-vst-webgui-framework.js';

const CSS = `
.noob-vst-webgui-framework-knob{display:inline-flex;flex-direction:column;align-items:center;gap:2px;
  user-select:none;-webkit-user-select:none;touch-action:none;cursor:ns-resize;outline:none;
  font:12px/1.2 system-ui,-apple-system,Segoe UI,sans-serif;color:var(--noob-vst-webgui-framework-text,#e6e6e6)}
.noob-vst-webgui-framework-knob:focus-visible svg{filter:drop-shadow(0 0 3px var(--noob-vst-webgui-framework-accent,#5ac8fa))}
.noob-vst-webgui-framework-knob svg{display:block;overflow:visible}
.noob-vst-webgui-framework-knob .track{fill:none;stroke:var(--noob-vst-webgui-framework-track,rgba(255,255,255,.14));stroke-linecap:round}
.noob-vst-webgui-framework-knob .value{fill:none;stroke:var(--noob-vst-webgui-framework-accent,#5ac8fa);stroke-linecap:round}
.noob-vst-webgui-framework-knob .ind{stroke:var(--noob-vst-webgui-framework-text,#e6e6e6);stroke-linecap:round}
.noob-vst-webgui-framework-knob .body{fill:var(--noob-vst-webgui-framework-knob-body,rgba(255,255,255,.06))}
.noob-vst-webgui-framework-knob .val{font-variant-numeric:tabular-nums;opacity:.95}
.noob-vst-webgui-framework-knob .lbl{opacity:.6;font-size:11px;letter-spacing:.02em;text-transform:uppercase}
`;

/**
 * Point on a circle. `deg` is measured clockwise from 12 o'clock (SVG's y
 * axis points down, hence the `-90°` shift from the usual convention).
 * @param {number} cx
 * @param {number} cy
 * @param {number} r
 * @param {number} deg
 * @returns {[number, number]} `[x, y]`
 */
function polar(cx, cy, r, deg) {
  const a = ((deg - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}

/**
 * SVG path data for a clockwise arc from angle `a0` to `a1` (degrees, see
 * `polar`). The order of the angles does not matter; an arc shorter than
 * 0.01° yields an empty string so a zero value draws nothing rather than a
 * dot. The large-arc flag is set past 180°.
 * @param {number} cx
 * @param {number} cy
 * @param {number} r
 * @param {number} a0
 * @param {number} a1
 * @returns {string}
 */
function arcPath(cx, cy, r, a0, a1) {
  if (a1 < a0) [a0, a1] = [a1, a0];
  if (a1 - a0 < 0.01) return '';
  const [x0, y0] = polar(cx, cy, r, a0);
  const [x1, y1] = polar(cx, cy, r, a1);
  const large = a1 - a0 > 180 ? 1 : 0;
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}

const SVG = 'http://www.w3.org/2000/svg';

/**
 * Rotary control for one Param.
 *
 * Public fields after construction: `el` (the root `<div>`, focusable),
 * `param` (the bound Param) and `opts` (the resolved options; `size` and
 * `sweep` are read at construction only, the rest may be changed live).
 */
export class Knob {
  /**
   * @param {HTMLElement} container Element the knob is appended to.
   * @param {import('../noob-vst-webgui-framework.js').Param} param The parameter to show and edit.
   * @param {object} [opts]
   * @param {number} [opts.size=72] Diameter in px (the SVG's rendered size).
   * @param {string} [opts.label] Text under the knob; defaults to `param.name`. Pass `''` for none.
   * @param {boolean} [opts.showValue=true] Show the formatted value under the arc.
   * @param {boolean} [opts.bipolar] Draw the value arc from 12 o'clock instead of the track start; defaults to `param.isBipolar` (min < 0 < max).
   * @param {number} [opts.sensitivity=200] Pixels of vertical drag for the full 0..1 range.
   * @param {number} [opts.sweep=270] Track extent in degrees, centred on 12 o'clock.
   * @param {string} [opts.color] Accent colour for this knob only (sets `--noob-vst-webgui-framework-accent` on the root).
   * @param {(plain:number)=>string} [opts.format] Custom value formatter; receives the plain value, defaults to `param.format()`.
   * @example
   * const knob = new Knob(document.querySelector('#cutoff'), client.param('cutoff'), {
   *   size: 64,
   *   format: (hz) => (hz >= 1000 ? `${(hz / 1000).toFixed(2)} kHz` : `${hz.toFixed(0)} Hz`),
   * });
   * // later
   * knob.destroy();
   */
  constructor(container, param, opts = {}) {
    injectStyle('noob-vst-webgui-framework-knob-css', CSS);
    this.param = param;
    this.opts = {
      size: 72,
      label: param.name,
      showValue: true,
      bipolar: param.isBipolar,
      sensitivity: 200,
      sweep: 270,
      color: null,
      format: null,
      ...opts,
    };
    const size = this.opts.size;
    const el = (this.el = document.createElement('div'));
    el.className = 'noob-vst-webgui-framework-knob';
    el.tabIndex = 0;
    el.setAttribute('role', 'slider');
    el.setAttribute('aria-label', this.opts.label);
    el.setAttribute('aria-valuemin', '0');
    el.setAttribute('aria-valuemax', '1');
    if (this.opts.color) el.style.setProperty('--noob-vst-webgui-framework-accent', this.opts.color);

    const svg = document.createElementNS(SVG, 'svg');
    svg.setAttribute('width', size);
    svg.setAttribute('height', size);
    svg.setAttribute('viewBox', '0 0 100 100');
    const stroke = 8;
    const r = 50 - stroke / 2 - 2;
    this._r = r;
    const body = document.createElementNS(SVG, 'circle');
    body.setAttribute('class', 'body');
    body.setAttribute('cx', 50);
    body.setAttribute('cy', 50);
    body.setAttribute('r', r - stroke / 2 - 4);
    const track = document.createElementNS(SVG, 'path');
    track.setAttribute('class', 'track');
    track.setAttribute('stroke-width', stroke);
    track.setAttribute('d', arcPath(50, 50, r, -this.opts.sweep / 2, this.opts.sweep / 2));
    const value = (this._value = document.createElementNS(SVG, 'path'));
    value.setAttribute('class', 'value');
    value.setAttribute('stroke-width', stroke);
    const ind = (this._ind = document.createElementNS(SVG, 'line'));
    ind.setAttribute('class', 'ind');
    ind.setAttribute('stroke-width', 4);
    ind.setAttribute('x1', 50);
    ind.setAttribute('y1', 50 - (r - stroke / 2 - 6));
    ind.setAttribute('x2', 50);
    ind.setAttribute('y2', 50 - (r - stroke / 2 - 18));
    svg.append(body, track, value, ind);
    el.appendChild(svg);

    if (this.opts.showValue) {
      this._val = document.createElement('div');
      this._val.className = 'val';
      el.appendChild(this._val);
    }
    if (this.opts.label) {
      const lbl = document.createElement('div');
      lbl.className = 'lbl';
      lbl.textContent = this.opts.label;
      el.appendChild(lbl);
    }
    container.appendChild(el);

    this._dirty = false;
    this._raf = null;
    this._off = param.on(() => this._schedule());

    el.addEventListener('pointerdown', this._onDown);
    el.addEventListener('pointermove', this._onMove);
    el.addEventListener('pointerup', this._onUp);
    el.addEventListener('pointercancel', this._onUp);
    el.addEventListener('dblclick', this._onDbl);
    el.addEventListener('wheel', this._onWheel, { passive: false });
    el.addEventListener('keydown', this._onKey);
    this._render();
  }

  /** Coalesce redraws: at most one `_render` per animation frame. */
  _schedule() {
    if (this._dirty) return;
    this._dirty = true;
    this._raf = requestAnimationFrame(() => {
      this._dirty = false;
      this._render();
    });
  }

  /**
   * Draw the current `param.value`: value arc (from the track start, or from
   * 12 o'clock when bipolar), indicator rotation, value text and ARIA state.
   */
  _render() {
    const n = this.param.value;
    const half = this.opts.sweep / 2;
    const a = -half + this.opts.sweep * n;
    const d = this.opts.bipolar ? arcPath(50, 50, this._r, 0, a) : arcPath(50, 50, this._r, -half, a);
    this._value.setAttribute('d', d);
    this._ind.setAttribute('transform', `rotate(${a.toFixed(2)} 50 50)`);
    if (this._val) {
      this._val.textContent = this.opts.format ? this.opts.format(this.param.plain) : this.param.format();
    }
    this.el.setAttribute('aria-valuenow', n.toFixed(3));
    this.el.setAttribute('aria-valuetext', this.param.format());
  }

  /**
   * Primary button starts a drag: capture the pointer, remember the start
   * value and open the edit gesture. The drag state carries the unclamped
   * accumulated position so fine / coarse switches mid-drag stay smooth.
   */
  _onDown = (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    this.el.focus();
    this.el.setPointerCapture(e.pointerId);
    this._drag = { y: e.clientY, n: this.param.value, id: e.pointerId };
    this.param.beginEdit();
  };

  /** Vertical motion → normalized delta (`sensitivity` px per full range, Shift = ×0.1). */
  _onMove = (e) => {
    if (!this._drag || e.pointerId !== this._drag.id) return;
    const fine = e.shiftKey ? 0.1 : 1;
    const dy = this._drag.y - e.clientY;
    this._drag.y = e.clientY;
    this._drag.n += (dy / this.opts.sensitivity) * fine;
    this._drag.n = Math.max(0, Math.min(1, this._drag.n));
    if (this.param.isDiscrete) {
      // Only send when the snapped step changes.
      const last = this.param.spec.steps - 1;
      const snapped = Math.round(this._drag.n * last) / last;
      if (snapped !== this.param.value) this.param.set(snapped);
    } else {
      this.param.set(this._drag.n);
    }
    this._schedule();
  };

  /** End of the drag (or a cancelled pointer): close the edit gesture. */
  _onUp = (e) => {
    if (!this._drag || e.pointerId !== this._drag.id) return;
    this._drag = null;
    this.param.endEdit();
  };

  /** Double-click resets to the host default (`Param.reset` wraps its own gesture). */
  _onDbl = (e) => {
    e.preventDefault();
    this.param.reset();
    this._schedule();
  };

  /**
   * Wheel: one step per notch for discrete Params, else ±0.02 (Shift ±0.002).
   * Consecutive notches share one begin/end gesture, closed 150 ms after
   * the last one, so a host records a single automation ramp.
   */
  _onWheel = (e) => {
    e.preventDefault();
    const step = this.param.isDiscrete ? 1 / (this.param.spec.steps - 1) : e.shiftKey ? 0.002 : 0.02;
    const dir = e.deltaY < 0 ? 1 : -1;
    if (!this._wheelTimer) this.param.beginEdit();
    clearTimeout(this._wheelTimer);
    this.param.set(this.param.value + dir * step);
    this._wheelTimer = setTimeout(() => {
      this._wheelTimer = null;
      this.param.endEdit();
    }, 150);
    this._schedule();
  };

  /**
   * Keyboard: arrows ±0.01 (Shift ±0.1) or one discrete step, Home / End to
   * the ends, Backspace / Delete to the default. Keys send a bare `set`
   * without begin / end; hosts treat that as an instantaneous edit.
   */
  _onKey = (e) => {
    const p = this.param;
    const step = p.isDiscrete ? 1 / (p.spec.steps - 1) : e.shiftKey ? 0.1 : 0.01;
    let n = null;
    switch (e.key) {
      case 'ArrowUp':
      case 'ArrowRight':
        n = p.value + step;
        break;
      case 'ArrowDown':
      case 'ArrowLeft':
        n = p.value - step;
        break;
      case 'Home':
        n = 0;
        break;
      case 'End':
        n = 1;
        break;
      case 'Backspace':
      case 'Delete':
        n = p.spec.default_norm;
        break;
      default:
        return;
    }
    e.preventDefault();
    p.set(n);
    this._schedule();
  };

  /** Unsubscribe from the Param, cancel any pending frame and remove the element. */
  destroy() {
    this._off();
    if (this._raf) cancelAnimationFrame(this._raf);
    this.el.remove();
  }
}

export default Knob;
