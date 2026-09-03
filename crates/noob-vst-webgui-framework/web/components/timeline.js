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
 * `devicePixelRatio`. Colours default to the `--noob-vst-webgui-framework-*`
 * variables (`grid`, `text`) when set on the container.
 *
 * ## Peaks
 *
 * A series may mark the moments it peaked, so the chart says where the worst
 * of them were and, on hover, how deep they went. Set `peaks` on the series
 * (see `TimelinePeaks`); it is off by default and costs nothing when off.
 *
 * Peaks are found as samples arrive rather than by scanning at draw time: a
 * candidate is held while the value keeps going the right way and confirmed
 * once the value retreats by `hysteresis`, so a peak is a genuine local
 * extreme rather than whichever sample happened to be lowest. Two are kept
 * at least `minGapMs` apart, the stronger winning, and only the `max` most
 * significant inside the window are marked. Each belongs to a moment, so its
 * dot scrolls left and leaves the chart with it.
 *
 * Each marked peak is a small dot with its value in a callout box beside it,
 * a pointer on the box aiming back at the dot it belongs to. The whole set
 * is drawn faintly at `dimOpacity`, so a chart watched from across the room
 * is not shouted at, and comes to full strength while the pointer is
 * anywhere over the chart: reading the numbers is a deliberate act, and
 * pointing at the chart is what makes it one.
 *
 * The dot, the box and its text take the series' own colour and the caller's
 * `format`, so the component decides nothing about how the value reads; the
 * box sits on `--noob-vst-webgui-framework-panel` so the text stays legible
 * over the traces.
 *
 * @example
 * new Timeline(el, {
 *   seconds: 8,
 *   series: [
 *     { stream: client.stream('meter'), index: 2, unit: 'linear', range: [-60, 6], color: '#58c4ff', label: 'out' },
 *     {
 *       stream: client.stream('meter'), index: 4, unit: 'db', range: [-24, 0],
 *       color: '#ffb547', label: 'GR', fill: true, fillTo: 0,
 *       peaks: { direction: 'min', threshold: -3, format: (v) => `${v.toFixed(1)} dB` },
 *     },
 *   ],
 * });
 */

/**
 * @typedef {object} TimelinePeaks
 * @property {'max'|'min'} [direction='max'] Which extreme is a peak: `'min'` for a value that falls, such as a gain reduction.
 * @property {number} [threshold] Ignore peaks that never get past this value, in the series' unit. Default: mark them all.
 * @property {number} [hysteresis=1] How far the value must come back from a candidate, in the series' unit, before it counts as a peak.
 * @property {number} [minGapMs=350] Closest two peaks may sit in time; a peak inside that window replaces the weaker one.
 * @property {number} [max=4] Most peaks marked at once, the most significant first.
 * @property {number} [dimOpacity=0.4] How faint the peaks are while the pointer is off the chart. `1` keeps them at full strength always.
 * @property {(value:number)=>string} [format] Label text for the value. Default: one decimal place.
 */

/**
 * @typedef {object} TimelineSeries
 * @property {import('../noob-vst-webgui-framework.js').Stream} [stream] Source stream; omit to feed with `push()`.
 * @property {number} [index=0] Element of the frame to read.
 * @property {'linear'|'db'|'raw'} [unit='raw'] Conversion of the raw value.
 * @property {[number, number]} [range=[-60, 6]] Bottom and top of this series' scale.
 * @property {string} [color='#58c4ff'] Line colour.
 * @property {number} [width=1.5] Line width in px.
 * @property {boolean} [fill=false] Fill towards `fillTo`.
 * @property {number} [fillTo] Baseline of the fill in the series' unit (default: bottom of `range`).
 * @property {string} [label] Legend text.
 * @property {TimelinePeaks} [peaks] Name this series' peaks on the chart. Off by default.
 */

/** The face the grid, the legend and the peak labels all share. */
const LABEL_FONT = '10px system-ui, sans-serif';

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
   * @param {boolean} [opts.timeGrid=false] One vertical line per second across the whole height, instead of the short ticks.
   * @param {boolean} [opts.legend=true] Draw series labels in the top-left corner.
   * @param {string} [opts.gridColor] Defaults to `--noob-vst-webgui-framework-grid` or `rgba(255,255,255,0.08)`.
   * @param {string} [opts.textColor] Defaults to `--noob-vst-webgui-framework-text-dim` or `rgba(255,255,255,0.45)`.
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
      timeGrid: false,
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
    for (const s of this.series) this._initPeaks(s);
    // Pointing at the chart brings the peaks to full strength, so a chart
    // with no peaks listens to nothing.
    this._ptrOn = false;
    this._onPointerOver = null;
    this._onPointerOut = null;
    if (this.series.some((s) => s._pk)) {
      this._onPointerOver = () => {
        this._ptrOn = true;
      };
      this._onPointerOut = () => {
        this._ptrOn = false;
      };
      // `pointermove` as well as `pointerenter`, so a pointer already resting
      // on the chart when it appears is noticed.
      c.addEventListener('pointerenter', this._onPointerOver);
      c.addEventListener('pointermove', this._onPointerOver);
      c.addEventListener('pointerleave', this._onPointerOut);
      c.addEventListener('pointercancel', this._onPointerOut);
    }
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
    if (s._pk) this._trackPeak(s, v, t);
  }

  /**
   * Resolve a series' `peaks` option and allocate everything the detector and
   * the labels will need, so neither allocates later. Leaves `_pk` null when
   * the series does not want labels, which is what makes the feature free.
   * @param {object} s A resolved series.
   */
  _initPeaks(s) {
    const p = s.peaks;
    s._pk = null;
    if (!p) return;
    const minGapMs = p.minGapMs ?? 350;
    const max = Math.max(1, Math.round(p.max ?? 4));
    // Room for a full window of peaks at the closest spacing they may sit,
    // so the ring never drops one that is still on screen.
    const cap = Math.max(16, Math.ceil((this.opts.seconds * 1000) / minGapMs) + 4);
    s._pk = {
      // +1 labels the highest values, -1 the lowest; every comparison below
      // multiplies by this, so one detector serves both directions.
      sign: p.direction === 'min' ? -1 : 1,
      hasThreshold: Number.isFinite(p.threshold),
      threshold: p.threshold ?? 0,
      hysteresis: Math.abs(p.hysteresis ?? 1),
      minGapMs,
      max,
      // A value that never comes back (a sustained reduction) would hold a
      // candidate for ever, so commit one that has been held a whole window.
      holdMs: this.opts.seconds * 1000,
      dimOpacity: Math.max(0, Math.min(1, p.dimOpacity ?? 0.4)),
      format: p.format || ((v) => v.toFixed(1)),
    };
    s._pkT = new Float64Array(cap);
    s._pkV = new Float32Array(cap);
    s._pkHead = 0;
    s._pkN = 0;
    s._candOn = false;
    s._candV = 0;
    s._candT = 0;
    s._pkPick = new Int32Array(max);
  }

  /**
   * One sample through the peak detector: hold the best value seen while the
   * series keeps going the right way, and confirm it once the series comes
   * back by `hysteresis` (or leaves the threshold, or has held a whole
   * window). Allocation-free.
   * @param {object} s
   * @param {number} v Converted value.
   * @param {number} t Timestamp.
   */
  _trackPeak(s, v, t) {
    const p = s._pk;
    const sg = p.sign;
    if (!p.hasThreshold || sg * v >= sg * p.threshold) {
      if (!s._candOn || sg * v > sg * s._candV) {
        s._candV = v;
        s._candT = t;
        s._candOn = true;
        return;
      }
      if (sg * s._candV - sg * v >= p.hysteresis || t - s._candT >= p.holdMs) {
        this._commitPeak(s, s._candV, s._candT);
        s._candOn = false;
      }
      return;
    }
    if (s._candOn) {
      this._commitPeak(s, s._candV, s._candT);
      s._candOn = false;
    }
  }

  /** Store a confirmed peak, merging into the previous one when they are closer than `minGapMs`. */
  _commitPeak(s, v, t) {
    const p = s._pk;
    const cap = s._pkT.length;
    if (s._pkN > 0) {
      const last = (s._pkHead - 1 + cap) % cap;
      if (t - s._pkT[last] < p.minGapMs) {
        if (p.sign * v > p.sign * s._pkV[last]) {
          s._pkV[last] = v;
          s._pkT[last] = t;
        }
        return;
      }
    }
    s._pkT[s._pkHead] = t;
    s._pkV[s._pkHead] = v;
    s._pkHead = (s._pkHead + 1) % cap;
    if (s._pkN < cap) s._pkN++;
  }

  /** Stop the animation, unsubscribe and remove the canvas. */
  destroy() {
    this._running = false;
    cancelAnimationFrame(this._raf);
    for (const s of this.series) s._off?.();
    if (this._onPointerOver) {
      this.canvas.removeEventListener('pointerenter', this._onPointerOver);
      this.canvas.removeEventListener('pointermove', this._onPointerOver);
      this.canvas.removeEventListener('pointerleave', this._onPointerOut);
      this.canvas.removeEventListener('pointercancel', this._onPointerOut);
    }
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
      grid: this.opts.gridColor || cssVar(el, '--noob-vst-webgui-framework-grid', 'rgba(255,255,255,0.08)'),
      text: this.opts.textColor || cssVar(el, '--noob-vst-webgui-framework-text-dim', 'rgba(255,255,255,0.45)'),
      // Behind a peak's callout, so its value stays legible over the traces.
      panel: cssVar(el, '--noob-vst-webgui-framework-panel', 'rgba(18,18,18,0.92)'),
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

  /** Grid, time ticks, each series (fill then line), peak labels, legend. */
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
      ctx.font = LABEL_FONT;
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
    // One mark per second: a line across the chart, or a stub at the bottom.
    // The line supersedes the stub, since they would sit on the same x.
    if (this.opts.timeGrid || this.opts.timeTicks) {
      const full = this.opts.timeGrid;
      ctx.strokeStyle = col.grid;
      ctx.lineWidth = 1;
      for (let s = 1; s < this.opts.seconds; s++) {
        const x = Math.round(w - (s * 1000 * w) / span) + 0.5;
        ctx.beginPath();
        ctx.moveTo(x, full ? 0 : h - 6);
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

    this._drawPeaks(ctx, now, span, w, h);

    // Legend
    if (this.opts.legend) {
      ctx.font = LABEL_FONT;
      ctx.textBaseline = 'top';
      ctx.textAlign = 'right';
      let x = w - 6;
      // Right to left, so the first series ends up leftmost. Indexed rather
      // than reversing a copy, which would allocate two arrays every frame.
      for (let i = this.series.length - 1; i >= 0; i--) {
        const s = this.series[i];
        if (!s.label) continue;
        ctx.fillStyle = s.color;
        ctx.fillText(s.label, x, 4);
        x -= ctx.measureText(s.label).width + 12;
      }
    }
  }

  /**
   * Mark the most significant peaks still on screen and name their values,
   * for every series that asked for it. Each rides at its peak's own moment,
   * so it scrolls left and leaves with it, and each is drawn faintly until
   * the pointer comes near, which brings it to full strength. Returns
   * immediately for a chart where no series wants peaks, and allocates
   * nothing: the winners are chosen by insertion into the series' own fixed
   * pick list, and the pointer is tracked in scalars.
   */
  _drawPeaks(ctx, now, span, w, h) {
    const col = this._palette();
    for (const s of this.series) {
      const p = s._pk;
      if (!p || s._pkN === 0) continue;
      const cap = s._pkT.length;
      const start = (s._pkHead - s._pkN + cap) % cap;
      const sg = p.sign;

      // Keep the `max` most significant peaks inside the window, ordered.
      let picked = 0;
      for (let k = 0; k < s._pkN; k++) {
        const j = (start + k) % cap;
        if (now - s._pkT[j] > span) continue;
        const v = s._pkV[j];
        let pos = picked;
        while (pos > 0 && sg * s._pkV[s._pkPick[pos - 1]] < sg * v) pos--;
        if (pos >= p.max) continue;
        for (let q = Math.min(picked, p.max - 1); q > pos; q--) s._pkPick[q] = s._pkPick[q - 1];
        s._pkPick[pos] = j;
        if (picked < p.max) picked++;
      }
      if (picked === 0) continue;

      // A falling series carries its fill above the line, so its callouts sit
      // below the point, and the other way round for a rising one.
      const below = sg < 0;
      ctx.font = LABEL_FONT;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.lineWidth = 1;
      ctx.globalAlpha = this._ptrOn ? 1 : p.dimOpacity;
      for (let q = 0; q < picked; q++) {
        const j = s._pkPick[q];
        const v = s._pkV[j];
        const x = w - ((now - s._pkT[j]) * w) / span;
        const y = this._y(s, v);
        ctx.fillStyle = s.color;
        ctx.beginPath();
        ctx.arc(x, y, 2.25, 0, Math.PI * 2);
        ctx.fill();
        this._callout(ctx, col, s, p.format(v), x, y, below, w, h);
      }
      ctx.globalAlpha = 1;
    }
  }

  /**
   * A peak's value in a small box whose pointer aims back at its dot. Drawn
   * as one path so the outline has no seam where the pointer meets the box,
   * and flipped to the other side of the dot when there is no room.
   */
  _callout(ctx, col, s, text, x, y, below, w, h) {
    const padX = 5;
    const bh = 15;
    const r = 3;
    const tail = 5; // how far the pointer reaches towards the dot
    const tw = 3.5; // half the pointer's width where it meets the box
    const bw = ctx.measureText(text).width + padX * 2;

    // Below the dot by default; flip if the box would leave the chart.
    let side = below;
    if (side && y + tail + bh > h - 1) side = false;
    else if (!side && y - tail - bh < 1) side = true;

    const bx = Math.max(1, Math.min(w - bw - 1, x - bw / 2));
    const by = side ? y + tail : y - tail - bh;
    // The pointer follows the dot along the box's edge, but stays on the
    // straight part of it rather than running into a rounded corner.
    const tailX = Math.max(bx + r + tw, Math.min(bx + bw - r - tw, x));
    const edge = side ? by : by + bh;

    ctx.beginPath();
    if (side) {
      ctx.moveTo(bx + r, by);
      ctx.lineTo(tailX - tw, edge);
      ctx.lineTo(x, y);
      ctx.lineTo(tailX + tw, edge);
      ctx.lineTo(bx + bw - r, by);
    } else {
      ctx.moveTo(bx + r, by);
      ctx.lineTo(bx + bw - r, by);
    }
    ctx.quadraticCurveTo(bx + bw, by, bx + bw, by + r);
    ctx.lineTo(bx + bw, by + bh - r);
    ctx.quadraticCurveTo(bx + bw, by + bh, bx + bw - r, by + bh);
    if (!side) {
      ctx.lineTo(tailX + tw, edge);
      ctx.lineTo(x, y);
      ctx.lineTo(tailX - tw, edge);
    }
    ctx.lineTo(bx + r, by + bh);
    ctx.quadraticCurveTo(bx, by + bh, bx, by + bh - r);
    ctx.lineTo(bx, by + r);
    ctx.quadraticCurveTo(bx, by, bx + r, by);
    ctx.closePath();

    ctx.fillStyle = col.panel;
    ctx.fill();
    ctx.strokeStyle = s.color;
    ctx.stroke();
    ctx.fillStyle = s.color;
    ctx.fillText(text, bx + bw / 2, by + bh / 2 + 0.5);
  }
}
