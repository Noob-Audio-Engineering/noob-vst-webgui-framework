/**
 * Envelope — an ADSR display with draggable handles bound to four Params
 * (attack, decay, release in seconds; sustain 0..1).
 *
 *   new Envelope(el, { attack: p('a_att'), decay: p('a_dec'), sustain: p('a_sus'), release: p('a_rel') });
 *
 * Drag the attack, decay/sustain and release handles; gestures use
 * begin / end so hosts record them. Times use a square-root scale so short
 * values stay reachable.
 *
 * ## Data model
 *
 * Four inputs, each a Param, a number or a getter (anything `plainOf`
 * accepts): `attack`, `decay`, `release` in **seconds** (plain values, so a
 * Param whose plain unit is seconds works directly) and `sustain` as a
 * **level 0..1**. A Param whose plain range is 0..100 % must be wrapped in a
 * getter / adapter; the component does not rescale. Only Params are
 * editable; numbers and getters make a read-only display.
 *
 * ## Layout
 *
 * The width is split into three equal time budgets (attack, decay, release)
 * of `stageWidth = width · (1 − sustainWidth) / 3` px each plus a sustain
 * plateau of `sustainWidth · width`. A time maps to pixels with a square
 * root: `px = sqrt(min(t, maxTime) / maxTime) · stageWidth`, so 1 ms is
 * still a visible sliver next to 4 s and the handle for a short stage is
 * grabbable. Levels map linearly with an 8 px pad top and bottom. Segments
 * are quadratic Béziers whose control point sits early in the segment,
 * which reads as the exponential curve a real envelope has.
 *
 * ## Handles and gestures
 *
 * Three handles: **attack** (end of the attack ramp, horizontal only),
 * **decay** (end of the decay: horizontal edits decay time, vertical edits
 * sustain level, both in one gesture) and **release** (end of the release,
 * horizontal only). Shift makes motion fine (×0.2). Each drag opens
 * `beginEdit` on the Params it touches and closes them on release.
 *
 * Labels `A … / D … / S … % / R …` sit in four fixed columns along the
 * bottom (fixed so that short stages do not overlap).
 *
 * `opts.onHover` and `opts.stageIndicator` are accepted for forward
 * compatibility but not yet used: hover is not reported and no playing-
 * stage marker is drawn. The `grid` group is likewise reserved.
 *
 * ## Styling
 *
 * `opts.color` fills the area (15 % alpha), strokes the line and fills the
 * handles. CSS variables: `--vst3-web-stratum-grid`, `--vst3-web-stratum-text-dim` (labels).
 * Classes: root `.vst3-web-stratum-env`, `.fill`, `.line`, `.handle`, `.hit`.
 */
import { injectStyle, plainOf } from '../vst3-web-stratum.js';

const CSS = `
.vst3-web-stratum-env{position:relative;width:100%;height:100%;user-select:none;-webkit-user-select:none;touch-action:none}
.vst3-web-stratum-env svg{display:block;width:100%;height:100%;overflow:visible}
.vst3-web-stratum-env .fill{opacity:.15}
.vst3-web-stratum-env .line{fill:none;stroke-width:2;stroke-linejoin:round}
.vst3-web-stratum-env .grid{stroke:var(--vst3-web-stratum-grid,rgba(255,255,255,.07))}
.vst3-web-stratum-env .handle{stroke:#fff;stroke-width:1.5;cursor:move}
.vst3-web-stratum-env .hit{fill:transparent;cursor:move}
.vst3-web-stratum-env text{font:10px system-ui,sans-serif;fill:var(--vst3-web-stratum-text-dim,rgba(255,255,255,.45));pointer-events:none}
`;

const SVG = 'http://www.w3.org/2000/svg';
/** Duck-typed test for a vst3-web-stratum Param (anything with an `on` subscription). */
const isParam = (v) => v && typeof v === 'object' && typeof v.on === 'function';

/**
 * ADSR editor.
 *
 * Public fields: `el` (root `<div>`), `svg`, `opts`. Values are read live
 * from the inputs on every render; there is no setter API because the
 * Params are the state.
 */
export class Envelope {
  /**
   * @param {HTMLElement} container Element the editor is appended to; decides the size.
   * @param {object} opts
   * @param {object|number|(() => number)} opts.attack   Attack time in seconds (Param, number or getter).
   * @param {object|number|(() => number)} opts.decay    Decay time in seconds.
   * @param {object|number|(() => number)} opts.sustain  Sustain level 0..1.
   * @param {object|number|(() => number)} opts.release  Release time in seconds.
   * @param {number} [opts.maxTime=4] Seconds each time stage may occupy on screen (longer values clamp visually).
   * @param {number} [opts.sustainWidth=0.22] Fraction of the width for the sustain plateau.
   * @param {string} [opts.color='#5ac8fa'] Line, fill and handle colour.
   * @param {(stage:string)=>void} [opts.onHover] Reserved; not called yet.
   * @param {number|null} [opts.stageIndicator] Reserved; 0..3 stage currently playing, not drawn yet.
   * @example
   * const p = (id) => client.param(id);
   * new Envelope(el, { attack: p('amp_attack'), decay: p('amp_decay'), sustain: p('amp_sustain'), release: p('amp_release') });
   */
  constructor(container, opts) {
    injectStyle('vst3-web-stratum-env-css', CSS);
    this.opts = { maxTime: 4, sustainWidth: 0.22, color: '#5ac8fa', ...opts };
    this.el = document.createElement('div');
    this.el.className = 'vst3-web-stratum-env';
    const svg = (this.svg = document.createElementNS(SVG, 'svg'));
    this.el.appendChild(svg);
    container.appendChild(this.el);
    this._grid = document.createElementNS(SVG, 'g');
    this._fill = document.createElementNS(SVG, 'path');
    this._fill.setAttribute('class', 'fill');
    this._fill.setAttribute('fill', this.opts.color);
    this._line = document.createElementNS(SVG, 'path');
    this._line.setAttribute('class', 'line');
    this._line.setAttribute('stroke', this.opts.color);
    this._labels = document.createElementNS(SVG, 'g');
    svg.append(this._grid, this._fill, this._line, this._labels);
    this._handles = ['attack', 'decay', 'release'].map((stage) => {
      const g = document.createElementNS(SVG, 'g');
      const hit = document.createElementNS(SVG, 'circle');
      hit.setAttribute('class', 'hit');
      hit.setAttribute('r', 14);
      const c = document.createElementNS(SVG, 'circle');
      c.setAttribute('class', 'handle');
      c.setAttribute('r', 5.5);
      c.setAttribute('fill', this.opts.color);
      g.append(hit, c);
      g.addEventListener('pointerdown', (e) => this._onDown(e, stage));
      g.addEventListener('pointermove', (e) => this._onMove(e, stage));
      g.addEventListener('pointerup', (e) => this._onUp(e, stage));
      g.addEventListener('pointercancel', (e) => this._onUp(e, stage));
      svg.appendChild(g);
      return { stage, g };
    });
    this._offs = [];
    for (const k of ['attack', 'decay', 'sustain', 'release']) {
      if (isParam(this.opts[k])) this._offs.push(this.opts[k].on(() => this._schedule()));
    }
    this._ro = new ResizeObserver(() => {
      this._resize();
      this._schedule();
    });
    this._ro.observe(this.el);
    this._resize();
    this._render();
  }

  /** Keep the SVG user space equal to CSS pixels (`viewBox = 0 0 w h`). */
  _resize() {
    this._w = Math.max(1, this.el.clientWidth);
    this._h = Math.max(1, this.el.clientHeight);
    this.svg.setAttribute('viewBox', `0 0 ${this._w} ${this._h}`);
  }

  /**
   * Resolve the four inputs to numbers: times clamped to ≥ 0, sustain to 0..1.
   * @returns {{attack:number, decay:number, sustain:number, release:number}}
   */
  _values() {
    return {
      attack: Math.max(0, plainOf(this.opts.attack)),
      decay: Math.max(0, plainOf(this.opts.decay)),
      sustain: Math.max(0, Math.min(1, plainOf(this.opts.sustain))),
      release: Math.max(0, plainOf(this.opts.release)),
    };
  }

  // Each time stage gets the same horizontal budget; sqrt keeps 1 ms visible.
  /** Pixels available to each of the three time stages. */
  _stageWidth() {
    return (this._w * (1 - this.opts.sustainWidth)) / 3;
  }
  /** Seconds → px within a stage: `sqrt(min(t, maxTime) / maxTime) · stageWidth`. */
  _timeToPx(t) {
    return Math.sqrt(Math.min(t, this.opts.maxTime) / this.opts.maxTime) * this._stageWidth();
  }
  /** px within a stage → seconds (inverse of `_timeToPx`, clamped to 0..maxTime). */
  _pxToTime(px) {
    const r = Math.max(0, Math.min(1, px / this._stageWidth()));
    return r * r * this.opts.maxTime;
  }
  /** Level 0..1 → y px, with an 8 px pad top and bottom. */
  _yFor(level) {
    const pad = 8;
    return this._h - pad - level * (this._h - 2 * pad);
  }
  /** y px → level 0..1 (inverse of `_yFor`, clamped). */
  _levelFor(y) {
    const pad = 8;
    return Math.max(0, Math.min(1, (this._h - pad - y) / (this._h - 2 * pad)));
  }

  /** Coalesce redraws: at most one `_render` per animation frame. */
  _schedule() {
    if (this._dirty) return;
    this._dirty = true;
    requestAnimationFrame(() => {
      this._dirty = false;
      this._render();
    });
  }

  /**
   * Compute the four breakpoints (attack end, decay end, sustain end,
   * release end), build the line and fill paths from quadratic segments,
   * move the handles onto their breakpoints and rewrite the labels.
   */
  _render() {
    const v = this._values();
    const x0 = 0;
    const xa = x0 + this._timeToPx(v.attack);
    const xd = xa + this._timeToPx(v.decay);
    const xs = xd + this._w * this.opts.sustainWidth;
    const xr = xs + this._timeToPx(v.release);
    const y0 = this._yFor(0);
    const yPeak = this._yFor(1);
    const ySus = this._yFor(v.sustain);
    // Exponential-looking segments via quadratic curves.
    const d =
      `M${x0} ${y0} Q${x0 + (xa - x0) * 0.35} ${yPeak} ${xa} ${yPeak} ` +
      `Q${xa + (xd - xa) * 0.3} ${ySus} ${xd} ${ySus} L${xs} ${ySus} ` +
      `Q${xs + (xr - xs) * 0.3} ${y0} ${xr} ${y0}`;
    this._line.setAttribute('d', d);
    this._fill.setAttribute('d', `${d} L${xr} ${y0} L${x0} ${y0} Z`);
    const pos = [
      [xa, yPeak],
      [xd, ySus],
      [xr, y0],
    ];
    this._handles.forEach((h, i) => h.g.setAttribute('transform', `translate(${pos[i][0].toFixed(1)} ${pos[i][1].toFixed(1)})`));
    this._labels.textContent = '';
    const fmt = (t) => (t < 1 ? `${Math.round(t * 1000)} ms` : `${t.toFixed(2)} s`);
    // Labels in four fixed columns; segment centres collide for short stages.
    const texts = ['A ' + fmt(v.attack), 'D ' + fmt(v.decay), 'S ' + Math.round(v.sustain * 100) + '%', 'R ' + fmt(v.release)];
    texts.forEach((s, i) => {
      const t = document.createElementNS(SVG, 'text');
      t.setAttribute('x', ((i + 0.5) / 4) * this._w);
      t.setAttribute('y', this._h - 1);
      t.setAttribute('text-anchor', 'middle');
      t.textContent = s;
      this._labels.appendChild(t);
    });
  }

  /** Pointer position in SVG user space (CSS px from the top-left of the svg). */
  _local(e) {
    const r = this.svg.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }
  /** Primary button on a handle: capture, snapshot the values, open the gesture(s). */
  _onDown(e, stage) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    const v = this._values();
    this._drag = { stage, id: e.pointerId, start: this._local(e), v };
    const p = this.opts[stage];
    if (isParam(p)) p.beginEdit();
    if (stage === 'decay' && isParam(this.opts.sustain)) this.opts.sustain.beginEdit();
  }
  /**
   * Drag: horizontal delta (Shift ×0.2) is added in *pixel* space to the
   * stage's start position and converted back through the sqrt scale, so
   * the handle follows the pointer exactly; the decay handle also maps its
   * vertical delta to the sustain level.
   */
  _onMove(e, stage) {
    const d = this._drag;
    if (!d || d.stage !== stage || e.pointerId !== d.id) return;
    const [x, y] = this._local(e);
    const fine = e.shiftKey ? 0.2 : 1;
    const dx = (x - d.start[0]) * fine;
    const p = this.opts[stage];
    if (stage === 'attack') {
      if (isParam(p)) p.setPlain(this._pxToTime(this._timeToPx(d.v.attack) + dx));
    } else if (stage === 'decay') {
      if (isParam(p)) p.setPlain(this._pxToTime(this._timeToPx(d.v.decay) + dx));
      if (isParam(this.opts.sustain)) this.opts.sustain.setPlain(this._levelFor(this._yFor(d.v.sustain) + (y - d.start[1]) * fine));
    } else if (stage === 'release') {
      if (isParam(p)) p.setPlain(this._pxToTime(this._timeToPx(d.v.release) + dx));
    }
    this._schedule();
  }
  /** End of the drag: close the gesture(s) opened in `_onDown`. */
  _onUp(e, stage) {
    const d = this._drag;
    if (!d || d.stage !== stage) return;
    this._drag = null;
    const p = this.opts[stage];
    if (isParam(p)) p.endEdit();
    if (stage === 'decay' && isParam(this.opts.sustain)) this.opts.sustain.endEdit();
  }

  /** Unsubscribe from the Params and remove the element. */
  destroy() {
    this._offs.forEach((f) => f());
    this._ro.disconnect();
    this.el.remove();
  }
}

export default Envelope;
