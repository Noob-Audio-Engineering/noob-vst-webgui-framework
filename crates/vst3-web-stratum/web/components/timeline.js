/**
 * Timeline — a scrolling strip chart of one or more values over the last
 * few seconds: input level, output level, gain reduction, a modulation
 * source, anything a plug-in publishes at block rate.
 *
 * ## Data model
 *
 * Each series takes its samples from one element (`index`) of the frames of
 * a Stream, or from `push(series, value)` calls. Samples are timestamped
 * with `performance.now()` when they arrive and kept in a ring buffer that
 * covers `seconds` of history. `unit` converts raw values the way
 * `VuMeter` does: `'linear'` amplitude becomes dBFS, `'db'` is used as is.
 *
 * Every series maps its own `range` (`[bottom, top]`) onto the full height,
 * so a level in dBFS (`[-60, 6]`) and a gain reduction (`[-24, 0]`) can
 * share one chart; the grid and the labels belong to the first series
 * unless `gridSeries` says otherwise.
 *
 * ## Drawing
 *
 * Time runs left to right with "now" at the right edge. Each series is a
 * polyline (`width`) with an optional translucent `fill` down to its
 * baseline (`fillTo`, in the series' own unit; defaults to the bottom of
 * the range, or 0 for a gain-reduction series so the fill hangs from the
 * top). The chart redraws on every animation frame; the canvas follows the
 * container through a `ResizeObserver` and draws in CSS pixels at
 * `devicePixelRatio`. Colours default to the `--vst3-web-stratum-*`
 * variables (`grid`, `text`) when set on the container.
 *
 * @example
 * new Timeline(el, {
 *   seconds: 8,
 *   series: [
 *     { stream: client.stream('meter'), index: 2, unit: 'linear', range: [-60, 6], color: '#58c4ff', label: 'out' },
 *     { stream: client.stream('meter'), index: 4, unit: 'db', range: [-24, 0], color: '#ffb547', label: 'GR', fill: true, fillTo: 0 },
 *   ],
 * });
 */

/**
 * @typedef {object} TimelineSeries
 * @property {import('../vst3-web-stratum.js').Stream} [stream] Source stream; omit to feed with `push()`.
 * @property {number} [index=0] Element of the frame to read.
 * @property {'linear'|'db'|'raw'} [unit='raw'] Conversion of the raw value.
 * @property {[number, number]} [range=[-60, 6]] Bottom and top of this series' scale.
 * @property {string} [color='#58c4ff'] Line colour.
 * @property {number} [width=1.5] Line width in px.
 * @property {boolean} [fill=false] Fill towards `fillTo`.
 * @property {number} [fillTo] Baseline of the fill in the series' unit (default: bottom of `range`).
 * @property {string} [label] Legend text.
 */

function cssVar(el, name, dflt) {
  const v = getComputedStyle(el).getPropertyValue(name).trim();
  return v || dflt;
}

/**
 * Scrolling history chart.
 *
 * Public fields: `canvas`, `opts`, `series` (the resolved series options).
 */
export class Timeline {
  /**
   * @param {HTMLElement} container Element the canvas is appended to; decides the size.
   * @param {object} [opts]
   * @param {TimelineSeries[]} [opts.series=[]] What to draw.
   * @param {number} [opts.seconds=6] History shown.
   * @param {number} [opts.maxRate=240] Samples per second kept per series (frames above this rate are thinned).
   * @param {boolean} [opts.grid=true] Horizontal grid lines and labels for the grid series.
   * @param {number} [opts.gridSeries=0] Which series' range the grid follows.
   * @param {number} [opts.gridStep=12] Grid spacing in that series' unit.
   * @param {boolean} [opts.timeTicks=true] One tick per second along the bottom.
   * @param {boolean} [opts.legend=true] Draw series labels in the top-left corner.
   * @param {string} [opts.gridColor] Defaults to `--vst3-web-stratum-grid` or `rgba(255,255,255,0.08)`.
   * @param {string} [opts.textColor] Defaults to `--vst3-web-stratum-text-dim` or `rgba(255,255,255,0.45)`.
   * @param {string} [opts.background='transparent'] Fill behind the chart.
   */
  constructor(container, opts = {}) {
    this.opts = {
      series: [],
      seconds: 6,
      maxRate: 240,
      grid: true,
      gridSeries: 0,
      gridStep: 12,
      timeTicks: true,
      legend: true,
      background: 'transparent',
      ...opts,
    };
    this._container = container;
    const c = (this.canvas = document.createElement('canvas'));
    c.style.display = 'block';
    c.style.width = '100%';
    c.style.height = '100%';
    container.appendChild(c);
    this._ctx = c.getContext('2d');
    this._ro = new ResizeObserver(() => this._resize());
    this._ro.observe(container);
    this._resize();

    const cap = Math.ceil(this.opts.seconds * this.opts.maxRate) + 8;
    this.series = this.opts.series.map((s) => ({
      index: 0,
      unit: 'raw',
      range: [-60, 6],
      color: '#58c4ff',
      width: 1.5,
      fill: false,
      ...s,
      _t: new Float64Array(cap),
      _v: new Float32Array(cap),
      _n: 0,
      _head: 0,
      _lastT: -Infinity,
      _off: null,
    }));
    const minGap = 1000 / this.opts.maxRate;
    this.series.forEach((s, i) => {
      if (!s.stream) return;
      s._off = s.stream.on((d) => {
        const now = performance.now();
        if (now - s._lastT < minGap) return;
        this._push(i, d[s.index] ?? 0, now);
      });
    });
    this._running = true;
    this._raf = requestAnimationFrame(this._tick);
  }

  /** Match the backing store to the container × devicePixelRatio; draw in CSS px. */
  _resize() {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, this._container.clientWidth);
    const h = Math.max(1, this._container.clientHeight);
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this._w = w;
    this._h = h;
    this._ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this._colors = null;
  }

  /**
   * Add a sample to a series that has no stream (or in addition to it).
   * @param {number} series Index into `series`.
   * @param {number} value Raw value, converted with the series' `unit`.
   */
  push(series, value) {
    this._push(series, value, performance.now());
  }

  _push(i, raw, t) {
    const s = this.series[i];
    if (!s) return;
    let v = raw;
    if (s.unit === 'linear') v = raw > 0 ? 20 * Math.log10(raw) : -200;
    const cap = s._t.length;
    s._t[s._head] = t;
    s._v[s._head] = v;
    s._head = (s._head + 1) % cap;
    if (s._n < cap) s._n++;
    s._lastT = t;
  }

  /** Stop the animation, unsubscribe and remove the canvas. */
  destroy() {
    this._running = false;
    cancelAnimationFrame(this._raf);
    for (const s of this.series) s._off?.();
    this._ro.disconnect();
    this.canvas.remove();
  }

  _tick = () => {
    if (!this._running) return;
    this._draw();
    this._raf = requestAnimationFrame(this._tick);
  };

  _palette() {
    if (this._colors) return this._colors;
    const el = this._container;
    this._colors = {
      grid: this.opts.gridColor || cssVar(el, '--vst3-web-stratum-grid', 'rgba(255,255,255,0.08)'),
      text: this.opts.textColor || cssVar(el, '--vst3-web-stratum-text-dim', 'rgba(255,255,255,0.45)'),
    };
    return this._colors;
  }

  /**
   * Value → y in CSS px for a series.
   * @param {object} s
   * @param {number} v
   * @returns {number}
   */
  _y(s, v) {
    const [lo, hi] = s.range;
    const f = Math.max(-0.05, Math.min(1.05, (v - lo) / (hi - lo)));
    return this._h - f * this._h;
  }

  /** Grid, time ticks, each series (fill then line), legend. */
  _draw() {
    const ctx = this._ctx;
    const w = this._w;
    const h = this._h;
    const col = this._palette();
    const now = performance.now();
    const span = this.opts.seconds * 1000;
    ctx.clearRect(0, 0, w, h);
    if (this.opts.background !== 'transparent') {
      ctx.fillStyle = this.opts.background;
      ctx.fillRect(0, 0, w, h);
    }

    // Grid
    const gs = this.series[this.opts.gridSeries];
    if (this.opts.grid && gs) {
      ctx.strokeStyle = col.grid;
      ctx.fillStyle = col.text;
      ctx.lineWidth = 1;
      ctx.font = '10px system-ui, sans-serif';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'bottom';
      const [lo, hi] = gs.range;
      const step = this.opts.gridStep;
      for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) {
        const y = Math.round(this._y(gs, v)) + 0.5;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
        ctx.fillText(String(v), 3, y - 1);
      }
    }
    if (this.opts.timeTicks) {
      ctx.strokeStyle = col.grid;
      for (let s = 1; s < this.opts.seconds; s++) {
        const x = Math.round(w - (s * 1000 * w) / span) + 0.5;
        ctx.beginPath();
        ctx.moveTo(x, h - 6);
        ctx.lineTo(x, h);
        ctx.stroke();
      }
    }

    // Series
    for (const s of this.series) {
      if (s._n < 2) continue;
      const cap = s._t.length;
      const start = (s._head - s._n + cap) % cap;
      ctx.beginPath();
      let first = true;
      let firstX = 0;
      let lastX = 0;
      for (let k = 0; k < s._n; k++) {
        const j = (start + k) % cap;
        const x = w - ((now - s._t[j]) * w) / span;
        if (x < -2) continue;
        const y = this._y(s, s._v[j]);
        if (first) {
          ctx.moveTo(x, y);
          firstX = x;
          first = false;
        } else ctx.lineTo(x, y);
        lastX = x;
      }
      if (first) continue;
      if (s.fill) {
        const base = this._y(s, s.fillTo ?? s.range[0]);
        ctx.lineTo(lastX, base);
        ctx.lineTo(firstX, base);
        ctx.closePath();
        ctx.fillStyle = s.color;
        ctx.globalAlpha = 0.18;
        ctx.fill();
        ctx.globalAlpha = 1;
        // redraw the line on top
        ctx.beginPath();
        first = true;
        for (let k = 0; k < s._n; k++) {
          const j = (start + k) % cap;
          const x = w - ((now - s._t[j]) * w) / span;
          if (x < -2) continue;
          const y = this._y(s, s._v[j]);
          if (first) {
            ctx.moveTo(x, y);
            first = false;
          } else ctx.lineTo(x, y);
        }
      }
      ctx.strokeStyle = s.color;
      ctx.lineWidth = s.width;
      ctx.lineJoin = 'round';
      ctx.stroke();
    }

    // Legend
    if (this.opts.legend) {
      ctx.font = '10px system-ui, sans-serif';
      ctx.textBaseline = 'top';
      ctx.textAlign = 'right';
      let x = w - 6;
      for (const s of [...this.series].reverse()) {
        if (!s.label) continue;
        ctx.fillStyle = s.color;
        ctx.fillText(s.label, x, 4);
        x -= ctx.measureText(s.label).width + 12;
      }
    }
  }
}
