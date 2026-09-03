/**
 * Scope — canvas oscilloscope / waveform view bound to a noob-vst-webgui-framework Stream of
 * time-domain samples (interleaved if `channels > 1`).
 *
 * ## Data model
 *
 * A frame is `frames × channels` samples in −1..1, interleaved
 * (`[l0, r0, l1, r1, …]` for stereo). The scope keeps only the latest frame
 * (latest wins, like the stream itself) and draws it once per animation
 * frame when a new one has arrived. The plugin decides what a frame means:
 * a fixed window, a zero-crossing-aligned cycle, a decimated history.
 *
 * ## Mapping
 *
 * Time runs left to right over the full width; amplitude is centred on the
 * middle line with `gain` as a linear vertical scale (`±1 → ±(h/2 − 1)·gain`
 * px). With more than two samples per pixel column the trace is drawn as a
 * **min / max envelope** per column (a vertical segment from the column's
 * maximum to its minimum), which keeps transients visible and costs `w`
 * segments regardless of the frame length; otherwise it is a plain
 * polyline through every sample.
 *
 * ## Drawing
 *
 * Canvas sized to the container × `devicePixelRatio`, context scaled to CSS
 * pixels. The optional grid is the centre line plus lines at 25 % and 75 %
 * of the height (±0.5 / `gain`). Channels cycle through `colors`; `fill`
 * closes each trace to the centre line at 20 % alpha.
 */

/**
 * Waveform view for one Stream.
 *
 * Public fields: `canvas`, `stream`, `opts`.
 */
export class Scope {
  /**
   * @param {HTMLElement} container Element the canvas is appended to; decides the size.
   * @param {import('../noob-vst-webgui-framework.js').Stream} stream Stream of interleaved time-domain samples in −1..1.
   * @param {object} [opts]
   * @param {number} [opts.channels] Interleaved channels in a frame; defaults to the stream's channel count.
   * @param {number} [opts.gain=1] Vertical scale (2 doubles the trace height).
   * @param {string[]} [opts.colors] One CSS colour per channel, cycled; default sky / orange / green / yellow.
   * @param {number} [opts.lineWidth=1.5] Trace width in px.
   * @param {boolean} [opts.fill=false] Fill each trace to the centre line (20 % alpha).
   * @param {boolean} [opts.grid=true] Draw the centre and ±50 % lines.
   * @param {string} [opts.gridColor='rgba(255,255,255,0.08)']
   * @example
   * const scope = new Scope(document.querySelector('#scope'), client.stream('scope'), { fill: true });
   */
  constructor(container, stream, opts = {}) {
    this.stream = stream;
    this.opts = {
      channels: stream.channels,
      gain: 1,
      colors: ['#5ac8fa', '#ff7a59', '#06d6a0', '#ffd166'],
      lineWidth: 1.5,
      fill: false,
      grid: true,
      gridColor: 'rgba(255,255,255,0.08)',
      ...opts,
    };
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
    this._data = null;
    this._dirty = false;
    this._off = stream.on((d) => {
      this._data = d;
      this._dirty = true;
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
  }

  /** Animation loop: redraw only when a new frame (or a resize) marked the view dirty. */
  _tick = () => {
    if (!this._running) return;
    if (this._dirty) {
      this._dirty = false;
      this._draw();
    }
    this._raf = requestAnimationFrame(this._tick);
  };

  /** Grid, then each channel as a min/max envelope (dense frames) or a polyline. */
  _draw() {
    const ctx = this._ctx;
    const w = this._w;
    const h = this._h;
    ctx.clearRect(0, 0, w, h);
    const mid = h / 2;
    if (this.opts.grid) {
      ctx.strokeStyle = this.opts.gridColor;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(0, Math.round(mid) + 0.5);
      ctx.lineTo(w, Math.round(mid) + 0.5);
      for (const f of [0.25, 0.75]) {
        ctx.moveTo(0, Math.round(h * f) + 0.5);
        ctx.lineTo(w, Math.round(h * f) + 0.5);
      }
      ctx.stroke();
    }
    const d = this._data;
    if (!d || d.length === 0) return;
    const ch = this.opts.channels;
    const frames = Math.floor(d.length / ch);
    if (frames < 2) return;
    const scale = (mid - 1) * this.opts.gain;
    ctx.lineWidth = this.opts.lineWidth;
    ctx.lineJoin = 'round';
    for (let c = 0; c < ch; c++) {
      ctx.beginPath();
      // Min/max per pixel column when there are more samples than pixels.
      if (frames > w * 2) {
        const per = frames / w;
        for (let x = 0; x < w; x++) {
          const s0 = Math.floor(x * per);
          const s1 = Math.min(frames, Math.floor((x + 1) * per));
          let mn = Infinity;
          let mx = -Infinity;
          for (let s = s0; s < s1; s++) {
            const v = d[s * ch + c];
            if (v < mn) mn = v;
            if (v > mx) mx = v;
          }
          const y0 = mid - mx * scale;
          const y1 = mid - mn * scale;
          if (x === 0) ctx.moveTo(x, y0);
          ctx.lineTo(x, y0);
          ctx.lineTo(x, y1);
        }
      } else {
        for (let s = 0; s < frames; s++) {
          const x = (s / (frames - 1)) * w;
          const y = mid - d[s * ch + c] * scale;
          if (s === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        }
      }
      const color = this.opts.colors[c % this.opts.colors.length];
      if (this.opts.fill) {
        ctx.save();
        ctx.lineTo(w, mid);
        ctx.lineTo(0, mid);
        ctx.closePath();
        ctx.globalAlpha = 0.2;
        ctx.fillStyle = color;
        ctx.fill();
        ctx.restore();
      }
      ctx.strokeStyle = color;
      ctx.stroke();
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

export default Scope;
