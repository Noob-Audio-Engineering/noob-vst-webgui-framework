/**
 * Spectrum — canvas spectrum analyser bound to a vst3-web-stratum Stream of per-bin
 * magnitudes (dB by default). Log frequency axis, peak-picking per pixel so
 * narrow peaks never vanish, time-based smoothing so frame rate does not
 * change the look. Supports zoom, freeze / peak hold, tilt, and peak
 * detection for "spectrum grab" style interactions.
 *
 * ## Data model
 *
 * A frame is one value per FFT bin, `bins = fftSize / 2 + 1` of them, bin
 * `k` centred on `k · sampleRate / fftSize` Hz (bin 0 is DC). Values are
 * dB unless the stream's meta says `db: false` (then linear magnitudes are
 * converted with `20·log10`). `fftSize` comes from `opts.fftSize`, then the
 * stream meta `fft_size`, else it is inferred as `(bins − 1) · 2`; the
 * sample rate from `opts.sampleRate`, then meta `sample_rate`, else 48 kHz.
 * A frame with a different length than the last one resets the smoothing
 * state (the plugin changed its resolution).
 *
 * ## Smoothing
 *
 * Every bin is a one-pole smoother **in dB**, stepped per frame with the
 * plugin's frame timestamps: coefficient `1 − exp(−dt / τ)` with
 * `τ = attackMs` when the new value is higher (0 → instant) and
 * `τ = releaseMs` when lower. Using `dt` from the timestamps (clamped to
 * 0..200 ms, 16 ms if unknown) makes the decay look the same at 30 and 190
 * frames per second. Frozen views use release 0 (nothing decays) and keep
 * a separate peak-hold array that only ever rises.
 *
 * ## Coordinates
 *
 * `x = (ln f − ln minHz) / (ln maxHz − ln minHz) · width` (log frequency),
 * `y = height − (dB − minDb) / (maxDb − minDb) · height` (linear dB). The
 * inverse functions are public (`freqForX`, `dbForY`) so a page can place
 * overlays and read the pointer. Tilt (`slopeDbPerOct`) is added at display
 * time only: `+slope · log2(f / 1000)`, i.e. a pink-noise reference.
 *
 * ## Drawing
 *
 * One column per CSS pixel. Where a column covers several bins the column
 * takes the **maximum** (so a narrow peak at high frequencies never
 * disappears between pixels); where bins are wider than a pixel (low
 * frequencies) the value is linearly interpolated between the two bins;
 * below bin 1 the trace holds bin 1 so it reaches the left edge instead of
 * falling into DC. The result is a polyline plus an optional fill to the
 * bottom. Grid: vertical lines at 20/50/100…20k Hz with labels, horizontal
 * lines every 6 dB (12 dB when the range is over 60 dB). `dbScale` draws a
 * separate labelled dB column on the left or right edge (step 6/12/24 dB by
 * range) for pages that overlay an EQ scale on top. Canvas is sized to the
 * container × `devicePixelRatio`; redraws happen at most once per animation
 * frame and only when a frame, option or resize marked the view dirty.
 */

/** Frequencies that get a grid line and label, when inside the visible range. */
const DEFAULT_GRID_HZ = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];

/**
 * Spectrum analyser for one Stream.
 *
 * Public fields: `canvas`, `stream`, `opts`. Most options can be changed
 * live through the setters below; colours and `grid` / `fill` / `dbScale`
 * can be poked on `opts` directly (the next frame picks them up).
 */
export class Spectrum {
  /**
   * @param {HTMLElement} container Element the canvas is appended to; decides the size.
   * @param {import('../vst3-web-stratum.js').Stream} stream Stream of per-bin magnitudes.
   * @param {object} [opts]
   * @param {number} [opts.sampleRate] From stream meta `sample_rate`, else 48000.
   * @param {number} [opts.fftSize] From stream meta `fft_size`, else inferred from the frame length.
   * @param {boolean} [opts.isDb=true] Values are already dB (else linear magnitude). Default follows meta `db`.
   * @param {number} [opts.minHz=20] Left edge.
   * @param {number} [opts.maxHz] Right edge; defaults to sampleRate/2.
   * @param {number} [opts.minDb=-90] Bottom edge.
   * @param {number} [opts.maxDb=0] Top edge.
   * @param {number} [opts.releaseMs=120] Smoothing release time constant (the analyser "speed").
   * @param {number} [opts.attackMs=0] Smoothing attack time constant (0 = instant).
   * @param {boolean} [opts.grid=true] Draw the frequency / dB grid.
   * @param {boolean} [opts.fill=true] Fill under the trace.
   * @param {string} [opts.color='#5ac8fa'] Trace colour.
   * @param {string} [opts.fillColor='rgba(90,200,250,0.18)']
   * @param {string} [opts.gridColor='rgba(255,255,255,0.08)']
   * @param {string} [opts.textColor='rgba(255,255,255,0.35)'] Grid and scale labels.
   * @param {number} [opts.lineWidth=1.5]
   * @param {number} [opts.slopeDbPerOct=0] Tilt applied for display, pivoting at 1 kHz.
   * @param {'left'|'right'|'none'} [opts.dbScale='none'] Draw a labelled dB scale on one edge.
   * @example
   * const an = new Spectrum(el, client.stream('spectrum_post'), { minDb: -90, maxDb: 0, slopeDbPerOct: 4.5 });
   * an.setReleaseMs(300);                 // slower
   * freezeButton.onclick = () => an.setFrozen(!an.frozen);
   * el.onpointermove = (e) => label.textContent = `${an.freqForX(e.offsetX).toFixed(0)} Hz`;
   */
  constructor(container, stream, opts = {}) {
    this.stream = stream;
    const meta = stream.meta || {};
    const sr = opts.sampleRate || meta.sample_rate || 48000;
    this.opts = {
      sampleRate: sr,
      fftSize: opts.fftSize || meta.fft_size || 0,
      isDb: meta.db !== false,
      minHz: 20,
      maxHz: sr / 2,
      minDb: -90,
      maxDb: 0,
      releaseMs: 120,
      attackMs: 0,
      grid: true,
      fill: true,
      color: '#5ac8fa',
      fillColor: 'rgba(90,200,250,0.18)',
      gridColor: 'rgba(255,255,255,0.08)',
      textColor: 'rgba(255,255,255,0.35)',
      lineWidth: 1.5,
      slopeDbPerOct: 0,
      /** Draw the analyzer's dB scale on the 'left' or 'right' edge, or 'none'. */
      dbScale: 'none',
      ...opts,
    };
    this._smooth = null;
    this._hold = null;
    this._lastTs = 0;
    this._dirty = false;
    this._frozen = false;
    this._bins = 0;

    const c = (this.canvas = document.createElement('canvas'));
    c.style.display = 'block';
    c.style.width = '100%';
    c.style.height = '100%';
    container.appendChild(c);
    this._ctx = c.getContext('2d');
    this._container = container;
    this._ro = new ResizeObserver(() => {
      this._resize();
      this._dirty = true;
    });
    this._ro.observe(container);
    this._resize();

    this._off = stream.on((d, s) => this._onFrame(d, s));
    this._running = true;
    this._raf = requestAnimationFrame(this._tick);
  }

  /** Size the backing store to the container × devicePixelRatio and cache the log-axis constants. */
  _resize() {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, this._container.clientWidth);
    const h = Math.max(1, this._container.clientHeight);
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this._w = w;
    this._h = h;
    this._ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this._logMin = Math.log(this.opts.minHz);
    this._logSpan = Math.log(this.opts.maxHz) - this._logMin;
  }

  /**
   * Change the visible frequency range (zoom). `minHz` is clamped to ≥ 1 Hz
   * and `maxHz` to at least 1.5 × `minHz`.
   * @param {number} minHz
   * @param {number} maxHz
   */
  setRange(minHz, maxHz) {
    this.opts.minHz = Math.max(1, minHz);
    this.opts.maxHz = Math.max(this.opts.minHz * 1.5, maxHz);
    this._resize();
    this._dirty = true;
  }
  /**
   * Change the vertical dB range.
   * @param {number} minDb Bottom edge.
   * @param {number} maxDb Top edge.
   */
  setDbRange(minDb, maxDb) {
    this.opts.minDb = minDb;
    this.opts.maxDb = maxDb;
    this._dirty = true;
  }
  /**
   * Change the smoothing release time constant (the analyser "speed").
   * @param {number} ms Larger = slower decay; 0 = no smoothing.
   */
  setReleaseMs(ms) {
    this.opts.releaseMs = ms;
  }
  /**
   * Change the display tilt, pivoting at 1 kHz (4.5 dB/oct makes pink noise
   * read flat-ish; 0 shows the raw spectrum).
   * @param {number} dbPerOct
   */
  setTilt(dbPerOct) {
    this.opts.slopeDbPerOct = dbPerOct;
    this._dirty = true;
  }
  /**
   * Freeze: stop decaying and keep the peak-hold maximum. Frames keep
   * arriving and can only raise the held levels; unfreezing drops the hold
   * and resumes normal smoothing from the current state.
   * @param {boolean} on
   */
  setFrozen(on) {
    this._frozen = on;
    if (on && this._smooth) this._hold = Float32Array.from(this._smooth);
    if (!on) this._hold = null;
    this._dirty = true;
  }
  /** @returns {boolean} Whether the view is frozen. */
  get frozen() {
    return this._frozen;
  }

  /**
   * x pixel (CSS px, 0 = left edge) for a frequency on the log axis.
   * @param {number} f Hz
   * @returns {number}
   */
  xForFreq(f) {
    return ((Math.log(Math.max(f, 1e-6)) - this._logMin) / this._logSpan) * this._w;
  }
  /**
   * Frequency for an x pixel (inverse of `xForFreq`; not clamped).
   * @param {number} x CSS px
   * @returns {number} Hz
   */
  freqForX(x) {
    return Math.exp(this._logMin + (x / this._w) * this._logSpan);
  }
  /**
   * y pixel (0 = top) for a dB value on the linear dB axis.
   * @param {number} db
   * @returns {number}
   */
  yForDb(db) {
    const { minDb, maxDb } = this.opts;
    return this._h - ((db - minDb) / (maxDb - minDb)) * this._h;
  }
  /**
   * dB value for a y pixel (inverse of `yForDb`; not clamped).
   * @param {number} y CSS px
   * @returns {number}
   */
  dbForY(y) {
    const { minDb, maxDb } = this.opts;
    return minDb + (1 - y / this._h) * (maxDb - minDb);
  }

  /** Width of one FFT bin in Hz: `sampleRate / fftSize`, inferring `fftSize` from the frame length when unknown. */
  _binHz() {
    const bins = this._bins || (this._smooth ? this._smooth.length : 0);
    const fftSize = this.opts.fftSize || (bins - 1) * 2;
    return this.opts.sampleRate / Math.max(2, fftSize);
  }

  /**
   * Displayed (smoothed, tilted; held when frozen) level at `freq`,
   * linearly interpolated between the two neighbouring bins.
   * @param {number} freq Hz
   * @returns {number} dB, or `NaN` before the first frame or outside the bin range.
   */
  valueAt(freq) {
    const sm = this._frozen && this._hold ? this._hold : this._smooth;
    if (!sm) return NaN;
    const kf = freq / this._binHz();
    const ka = Math.floor(kf);
    if (ka < 0 || ka >= sm.length - 1) return NaN;
    const t = kf - ka;
    let v = sm[ka] * (1 - t) + sm[ka + 1] * t;
    if (this.opts.slopeDbPerOct) v += this.opts.slopeDbPerOct * Math.log2(freq / 1000);
    return v;
  }

  /**
   * Local maxima of the displayed spectrum (smoothed, tilted, held when
   * frozen), for "spectrum grab" style interactions: hover a peak, click to
   * create an EQ band there.
   *
   * A bin is a peak when it is strictly above its left neighbour and at
   * least its right neighbour; bins 0, 1 and the last two are skipped, as are
   * frequencies outside the visible range and levels below `minDb`. Peaks are
   * sorted loudest first and then thinned: a peak within `minDistanceOct`
   * octaves of a louder kept peak is dropped, until `max` remain.
   *
   * @param {object} [opts]
   * @param {number} [opts.minDistanceOct=0.15] Minimum spacing between kept peaks, in octaves.
   * @param {number} [opts.minDb=-70] Ignore peaks below this displayed level.
   * @param {number} [opts.max=40] Maximum number of peaks returned.
   * @returns {{freq:number, db:number, x:number, y:number}[]} Loudest first; `x`/`y` are canvas CSS px.
   */
  peaks({ minDistanceOct = 0.15, minDb = -70, max = 40 } = {}) {
    const sm = this._frozen && this._hold ? this._hold : this._smooth;
    if (!sm) return [];
    const binHz = this._binHz();
    const out = [];
    for (let k = 2; k < sm.length - 2; k++) {
      const f = k * binHz;
      if (f < this.opts.minHz || f > this.opts.maxHz) continue;
      const v = sm[k] + (this.opts.slopeDbPerOct ? this.opts.slopeDbPerOct * Math.log2(f / 1000) : 0);
      if (v < minDb) continue;
      const l = sm[k - 1] + (this.opts.slopeDbPerOct ? this.opts.slopeDbPerOct * Math.log2(((k - 1) * binHz) / 1000) : 0);
      const r = sm[k + 1] + (this.opts.slopeDbPerOct ? this.opts.slopeDbPerOct * Math.log2(((k + 1) * binHz) / 1000) : 0);
      if (v > l && v >= r) out.push({ freq: f, db: v });
    }
    out.sort((a, b) => b.db - a.db);
    const kept = [];
    for (const p of out) {
      if (kept.every((q) => Math.abs(Math.log2(q.freq / p.freq)) > minDistanceOct)) kept.push(p);
      if (kept.length >= max) break;
    }
    for (const p of kept) {
      p.x = this.xForFreq(p.freq);
      p.y = this.yForDb(p.db);
    }
    return kept;
  }

  /**
   * New frame: (re)allocate the smoother if the bin count changed, derive
   * `dt` from the stream timestamps, then run the per-bin attack / release
   * one-pole in dB. While frozen the hold array tracks the maximum.
   * @param {Float32Array} d Per-bin values.
   * @param {import('../vst3-web-stratum.js').Stream} s The stream (for `ts`).
   */
  _onFrame(d, s) {
    const n = d.length;
    this._bins = n;
    if (!this._smooth || this._smooth.length !== n) {
      this._smooth = new Float32Array(n);
      this._smooth.fill(this.opts.minDb - 30);
      this._lastTs = s.ts;
      this._hold = null;
    }
    const dt = Math.max(0, Math.min(200, s.ts - this._lastTs)) || 16;
    this._lastTs = s.ts;
    const rel = this._frozen ? 0 : this.opts.releaseMs > 0 ? 1 - Math.exp(-dt / this.opts.releaseMs) : 1;
    const att = this.opts.attackMs > 0 ? 1 - Math.exp(-dt / this.opts.attackMs) : 1;
    const sm = this._smooth;
    const isDb = this.opts.isDb;
    for (let i = 0; i < n; i++) {
      let v = d[i];
      if (!isDb) v = v > 0 ? 20 * Math.log10(v) : -200;
      const cur = sm[i];
      sm[i] = v > cur ? cur + (v - cur) * att : cur + (v - cur) * rel;
    }
    if (this._frozen && this._hold) {
      const h = this._hold;
      for (let i = 0; i < n; i++) if (sm[i] > h[i]) h[i] = sm[i];
    }
    this._dirty = true;
  }

  /** Animation loop: redraw only when a frame, setter or resize marked the view dirty. */
  _tick = () => {
    if (!this._running) return;
    if (this._dirty) {
      this._dirty = false;
      this._draw();
    }
    this._raf = requestAnimationFrame(this._tick);
  };

  /**
   * Grid, dB scale, then the trace: one y per pixel column, computed as the
   * max over the bins the column covers, or interpolated when a bin spans
   * several columns (see the file header), plus the display tilt.
   */
  _draw() {
    const ctx = this._ctx;
    const w = this._w;
    const h = this._h;
    ctx.clearRect(0, 0, w, h);
    if (this.opts.grid) this._drawGrid(ctx, w, h);
    if (this.opts.dbScale !== 'none') this._drawDbScale(ctx, w, h);
    const sm = this._frozen && this._hold ? this._hold : this._smooth;
    if (!sm) return;
    const bins = sm.length;
    const binHz = this._binHz();
    const slope = this.opts.slopeDbPerOct;

    const cols = w + 1;
    if (!this._ys || this._ys.length !== cols) this._ys = new Float32Array(cols);
    const ys = this._ys;
    let count = 0;
    const start = 0;
    for (let x = 0; x <= w; x++) {
      const kf = this.freqForX(x) / binHz;
      if (kf >= bins) break;
      const k0 = Math.ceil(this.freqForX(x - 0.5) / binHz);
      const k1 = Math.floor(this.freqForX(x + 0.5) / binHz);
      let v;
      if (kf < 1) {
        // Below the first real bin the FFT has no data (bin 0 is DC):
        // hold the first bin's level so the trace reaches the left edge.
        v = sm[Math.min(1, bins - 1)];
      } else if (k1 >= k0 && k0 < bins) {
        v = -1e9;
        const end = Math.min(k1, bins - 1);
        for (let k = Math.max(k0, 0); k <= end; k++) if (sm[k] > v) v = sm[k];
      } else {
        const ka = Math.floor(kf);
        const kb = Math.min(ka + 1, bins - 1);
        const t = kf - ka;
        v = sm[Math.max(0, ka)] * (1 - t) + sm[kb] * t;
      }
      if (slope) v += slope * Math.log2(this.freqForX(x) / 1000);
      ys[x] = this.yForDb(v);
      count = x + 1;
    }
    if (start < 0 || count - start < 2) return;

    if (this.opts.fill) {
      ctx.beginPath();
      ctx.moveTo(start, ys[start]);
      for (let x = start + 1; x < count; x++) ctx.lineTo(x, ys[x]);
      ctx.lineTo(count - 1, h + 2);
      ctx.lineTo(start, h + 2);
      ctx.closePath();
      ctx.fillStyle = this.opts.fillColor;
      ctx.fill();
    }
    ctx.beginPath();
    ctx.moveTo(start, ys[start]);
    for (let x = start + 1; x < count; x++) ctx.lineTo(x, ys[x]);
    ctx.strokeStyle = this.opts.color;
    ctx.lineWidth = this.opts.lineWidth;
    ctx.lineJoin = 'round';
    ctx.stroke();
  }

  /**
   * The analyser's own dB scale on the `dbScale` edge (separate from any EQ
   * scale drawn on top): labels every 6, 12 or 24 dB depending on the range,
   * skipping the ones that would clip at the top or bottom.
   */
  _drawDbScale(ctx, w, h) {
    const { minDb, maxDb } = this.opts;
    const span = maxDb - minDb;
    const step = span > 96 ? 24 : span > 48 ? 12 : 6;
    ctx.fillStyle = this.opts.textColor;
    ctx.font = '10px system-ui, sans-serif';
    ctx.textBaseline = 'middle';
    ctx.textAlign = this.opts.dbScale === 'right' ? 'right' : 'left';
    const x = this.opts.dbScale === 'right' ? w - 4 : 4;
    for (let db = Math.ceil(minDb / step) * step; db <= maxDb; db += step) {
      const y = this.yForDb(db);
      if (y < 8 || y > h - 8) continue;
      ctx.fillText(`${db}`, x, y);
    }
    ctx.textAlign = 'left';
  }

  /** Vertical lines at the decade frequencies with labels, horizontal lines every 6 or 12 dB. */
  _drawGrid(ctx, w, h) {
    ctx.strokeStyle = this.opts.gridColor;
    ctx.fillStyle = this.opts.textColor;
    ctx.lineWidth = 1;
    ctx.font = '10px system-ui, sans-serif';
    ctx.textBaseline = 'bottom';
    for (const f of DEFAULT_GRID_HZ) {
      if (f < this.opts.minHz || f > this.opts.maxHz) continue;
      const x = Math.round(this.xForFreq(f)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
      ctx.fillText(f >= 1000 ? `${f / 1000}k` : String(f), x + 3, h - 2);
    }
    const { minDb, maxDb } = this.opts;
    const step = maxDb - minDb > 60 ? 12 : 6;
    ctx.textBaseline = 'top';
    for (let db = Math.ceil(minDb / step) * step; db <= maxDb; db += step) {
      const y = Math.round(this.yForDb(db)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
      if (db !== maxDb) ctx.fillText(`${db}`, 3, y + 1);
    }
  }

  /** Stop the animation loop, unsubscribe from the stream and remove the canvas. */
  destroy() {
    this._running = false;
    cancelAnimationFrame(this._raf);
    this._off();
    this._ro.disconnect();
    this.canvas.remove();
  }
}

export default Spectrum;
