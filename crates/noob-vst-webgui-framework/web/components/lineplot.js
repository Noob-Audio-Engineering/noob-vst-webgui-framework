/**
 * LinePlot — a small XY chart for curves a plug-in publishes or a page
 * computes: a compressor's transfer curve, a filter response, a lookup
 * table, an envelope. One or more series over a shared `xRange` and
 * `yRange`, with a grid, axis labels and an optional marker for the current
 * operating point.
 *
 * ## Data model
 *
 * A series is either `points` (an array or typed array of `y` values spread
 * uniformly across `xRange`, which is how a sticky curve stream arrives) or
 * `xy` (an array of `[x, y]` pairs). Series can be bound to a Stream: each
 * frame replaces the series' `points`. `setSeries(i, ys)` and
 * `setXY(i, pairs)` update by hand; `setMarker(x, y)` moves the marker
 * (`null` hides it).
 *
 * ## Drawing
 *
 * The plot redraws when its data changes or the container resizes (it does
 * not run an animation loop; a marker driven at frame rate simply calls
 * `setMarker` every frame). Axes are linear; pass values already in dB or
 * log units. Colours default to the `--noob-vst-webgui-framework-*` variables
 * (`grid`, `text-dim`) when set on the container.
 *
 * @example
 * const plot = new LinePlot(el, {
 *   xRange: [-60, 0], yRange: [-60, 0], xLabel: 'in dB', yLabel: 'out dB',
 *   series: [{ stream: client.stream('transfer'), color: '#ffb547' }, { xy: [[-60, -60], [0, 0]], color: 'rgba(255,255,255,0.2)', dash: [4, 4] }],
 * });
 * client.stream('meter').on((d) => plot.setMarker(inDb(d), outDb(d)));
 */

/**
 * @typedef {object} LinePlotSeries
 * @property {ArrayLike<number>} [points] `y` values spread uniformly over `xRange`.
 * @property {Array<[number, number]>} [xy] Explicit `[x, y]` pairs.
 * @property {import('../noob-vst-webgui-framework.js').Stream} [stream] Stream whose frames replace `points`.
 * @property {string} [color='#58c4ff'] Line colour.
 * @property {number} [width=1.5] Line width in px.
 * @property {number[]} [dash] Canvas dash pattern.
 * @property {boolean} [fill=false] Fill down to the bottom of `yRange`.
 * @property {string} [label] Legend text.
 */

function cssVar(el, name, dflt) {
  const v = getComputedStyle(el).getPropertyValue(name).trim();
  return v || dflt;
}

/**
 * XY line chart.
 *
 * Public fields: `canvas`, `opts`, `series`.
 */
export class LinePlot {
  /**
   * @param {HTMLElement} container Element the canvas is appended to; decides the size.
   * @param {object} [opts]
   * @param {LinePlotSeries[]} [opts.series=[]]
   * @param {[number, number]} [opts.xRange=[0, 1]]
   * @param {[number, number]} [opts.yRange=[0, 1]]
   * @param {number} [opts.xStep] Grid spacing along x (default: a fifth of the range).
   * @param {number} [opts.yStep] Grid spacing along y (default: a fifth of the range).
   * @param {string} [opts.xLabel=''] Axis caption at the bottom right.
   * @param {string} [opts.yLabel=''] Axis caption at the top left.
   * @param {boolean} [opts.grid=true]
   * @param {boolean} [opts.legend=true]
   * @param {string} [opts.markerColor='#ffffff']
   * @param {string} [opts.gridColor] Defaults to `--noob-vst-webgui-framework-grid` or `rgba(255,255,255,0.08)`.
   * @param {string} [opts.textColor] Defaults to `--noob-vst-webgui-framework-text-dim` or `rgba(255,255,255,0.45)`.
   * @param {number} [opts.padding=18] Space kept for labels, in px.
   */
  constructor(container, opts = {}) {
    this.opts = {
      series: [],
      xRange: [0, 1],
      yRange: [0, 1],
      xLabel: '',
      yLabel: '',
      grid: true,
      legend: true,
      markerColor: '#ffffff',
      padding: 18,
      ...opts,
    };
    this._container = container;
    const c = (this.canvas = document.createElement('canvas'));
    c.style.display = 'block';
    c.style.width = '100%';
    c.style.height = '100%';
    container.appendChild(c);
    this._ctx = c.getContext('2d');
    this.series = this.opts.series.map((s) => ({ color: '#58c4ff', width: 1.5, fill: false, ...s, _off: null }));
    this.series.forEach((s, i) => {
      if (s.stream) s._off = s.stream.on((d) => this.setSeries(i, d));
    });
    this._marker = null;
    this._ro = new ResizeObserver(() => this._resize());
    this._ro.observe(container);
    this._resize();
  }

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
    this._draw();
  }

  /**
   * Replace a series' uniformly spaced `y` values.
   * @param {number} i
   * @param {ArrayLike<number>} ys
   */
  setSeries(i, ys) {
    const s = this.series[i];
    if (!s) return;
    s.points = ys.length !== undefined && !(ys instanceof Float32Array) ? Float32Array.from(ys) : Float32Array.from(ys);
    s.xy = undefined;
    this._draw();
  }

  /**
   * Replace a series' explicit points.
   * @param {number} i
   * @param {Array<[number, number]>} pairs
   */
  setXY(i, pairs) {
    const s = this.series[i];
    if (!s) return;
    s.xy = pairs;
    s.points = undefined;
    this._draw();
  }

  /**
   * Move (or hide, with `null`) the operating-point marker.
   * @param {number|null} x
   * @param {number} [y]
   */
  setMarker(x, y) {
    this._marker = x == null ? null : [x, y];
    this._draw();
  }

  /** Change the axis ranges and redraw. */
  setRanges(xRange, yRange) {
    if (xRange) this.opts.xRange = xRange;
    if (yRange) this.opts.yRange = yRange;
    this._draw();
  }

  /** Unsubscribe and remove the canvas. */
  destroy() {
    for (const s of this.series) s._off?.();
    this._ro.disconnect();
    this.canvas.remove();
  }

  _palette() {
    if (this._colors) return this._colors;
    const el = this._container;
    this._colors = {
      grid: this.opts.gridColor || cssVar(el, '--noob-vst-webgui-framework-grid', 'rgba(255,255,255,0.08)'),
      text: this.opts.textColor || cssVar(el, '--noob-vst-webgui-framework-text-dim', 'rgba(255,255,255,0.45)'),
    };
    return this._colors;
  }

  /** x value → px. */
  xFor(x) {
    const [lo, hi] = this.opts.xRange;
    const p = this.opts.padding;
    return p + ((x - lo) / (hi - lo)) * (this._w - p * 1.5);
  }

  /** y value → px. */
  yFor(y) {
    const [lo, hi] = this.opts.yRange;
    const p = this.opts.padding;
    return this._h - p - ((y - lo) / (hi - lo)) * (this._h - p * 1.5);
  }

  _draw() {
    const ctx = this._ctx;
    const w = this._w;
    const h = this._h;
    const col = this._palette();
    const { xRange, yRange, padding: p } = this.opts;
    ctx.clearRect(0, 0, w, h);

    // Grid
    if (this.opts.grid) {
      const xs = this.opts.xStep || (xRange[1] - xRange[0]) / 5;
      const ys = this.opts.yStep || (yRange[1] - yRange[0]) / 5;
      ctx.strokeStyle = col.grid;
      ctx.fillStyle = col.text;
      ctx.lineWidth = 1;
      ctx.font = '9px system-ui, sans-serif';
      for (let x = Math.ceil(xRange[0] / xs) * xs; x <= xRange[1] + 1e-9; x += xs) {
        const px = Math.round(this.xFor(x)) + 0.5;
        ctx.beginPath();
        ctx.moveTo(px, p * 0.5);
        ctx.lineTo(px, h - p);
        ctx.stroke();
        ctx.textAlign = 'center';
        ctx.textBaseline = 'top';
        ctx.fillText(String(Math.round(x * 100) / 100), px, h - p + 3);
      }
      for (let y = Math.ceil(yRange[0] / ys) * ys; y <= yRange[1] + 1e-9; y += ys) {
        const py = Math.round(this.yFor(y)) + 0.5;
        ctx.beginPath();
        ctx.moveTo(p, py);
        ctx.lineTo(w - p * 0.5, py);
        ctx.stroke();
        ctx.textAlign = 'right';
        ctx.textBaseline = 'middle';
        ctx.fillText(String(Math.round(y * 100) / 100), p - 3, py);
      }
    }
    // Axis captions
    ctx.fillStyle = col.text;
    ctx.font = '10px system-ui, sans-serif';
    if (this.opts.xLabel) {
      ctx.textAlign = 'right';
      ctx.textBaseline = 'bottom';
      ctx.fillText(this.opts.xLabel, w - p * 0.5, h - p - 2);
    }
    if (this.opts.yLabel) {
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      ctx.fillText(this.opts.yLabel, p + 3, p * 0.5 + 2);
    }

    // Series
    for (const s of this.series) {
      const pts = [];
      if (s.points && s.points.length) {
        const n = s.points.length;
        for (let i = 0; i < n; i++) pts.push([xRange[0] + ((xRange[1] - xRange[0]) * i) / Math.max(1, n - 1), s.points[i]]);
      } else if (s.xy && s.xy.length) pts.push(...s.xy);
      if (pts.length < 2) continue;
      ctx.beginPath();
      pts.forEach(([x, y], i) => {
        const px = this.xFor(x);
        const py = this.yFor(Math.max(yRange[0] - 1e9, y));
        if (i) ctx.lineTo(px, py);
        else ctx.moveTo(px, py);
      });
      if (s.fill) {
        const base = this.yFor(yRange[0]);
        ctx.lineTo(this.xFor(pts[pts.length - 1][0]), base);
        ctx.lineTo(this.xFor(pts[0][0]), base);
        ctx.closePath();
        ctx.fillStyle = s.color;
        ctx.globalAlpha = 0.15;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.beginPath();
        pts.forEach(([x, y], i) => (i ? ctx.lineTo(this.xFor(x), this.yFor(y)) : ctx.moveTo(this.xFor(x), this.yFor(y))));
      }
      ctx.strokeStyle = s.color;
      ctx.lineWidth = s.width;
      ctx.setLineDash(s.dash || []);
      ctx.lineJoin = 'round';
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // Marker
    if (this._marker) {
      const [x, y] = this._marker;
      const px = this.xFor(Math.max(xRange[0], Math.min(xRange[1], x)));
      const py = this.yFor(Math.max(yRange[0], Math.min(yRange[1], y)));
      ctx.fillStyle = this.opts.markerColor;
      ctx.beginPath();
      ctx.arc(px, py, 4, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = this.opts.markerColor;
      ctx.globalAlpha = 0.35;
      ctx.beginPath();
      ctx.moveTo(px, py);
      ctx.lineTo(px, this.yFor(yRange[0]));
      ctx.moveTo(px, py);
      ctx.lineTo(this.xFor(xRange[0]), py);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }

    // Legend
    if (this.opts.legend) {
      ctx.font = '10px system-ui, sans-serif';
      ctx.textBaseline = 'top';
      ctx.textAlign = 'right';
      let x = w - p * 0.5;
      for (const s of [...this.series].reverse()) {
        if (!s.label) continue;
        ctx.fillStyle = s.color;
        ctx.fillText(s.label, x, p * 0.5 + 2);
        x -= ctx.measureText(s.label).width + 12;
      }
    }
  }
}
