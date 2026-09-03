/**
 * Meter — canvas level meter bound to a noob-vst-webgui-framework Stream.
 *
 * Frame layout: `[peak_0 .. peak_{ch-1}, rms_0 .. rms_{ch-1}?]`, linear
 * amplitude. Ballistics (decay, peak hold) run client-side at display rate,
 * so the plugin only needs to send the block peak.
 *
 * ## Data model
 *
 * Each stream frame carries one linear peak per channel (`|x|` over the
 * plugin's block, 1.0 = 0 dBFS) and, optionally, one RMS value per channel
 * after the peaks. The meter decides per frame whether RMS is present from
 * the length (`≥ 2·channels`). Values convert to dB with `20·log10(x)`; a
 * zero maps to −200 dB so the bar collapses instead of producing `-Infinity`.
 *
 * ## Ballistics (per channel, updated every animation frame)
 *
 * * **Attack** is instant: a frame above the shown level jumps the bar up.
 * * **Decay** is linear in dB at `decayDbPerSec`, applied with the real
 *   elapsed time (capped at 100 ms per frame so a background tab does not
 *   snap when it comes back).
 * * **Peak hold** keeps the highest level for `holdMs`, then falls at twice
 *   the decay rate until it meets the bar.
 * * **Clip** latches when a frame reaches 0 dBFS or more, until `resetClip()`.
 * * RMS, when present, is drawn as an opaque inner bar and the peak bar
 *   becomes translucent behind it.
 *
 * ## Drawing
 *
 * The canvas is sized to the container in CSS pixels times
 * `devicePixelRatio` (a `ResizeObserver` keeps it in sync) and the context
 * is scaled so all drawing happens in CSS pixels. Bars are laid out along
 * the orientation with `gap` px between channels. The fill is a gradient
 * with three hard-edged zones: `colors[0]` up to −12 dB, `colors[1]` up to
 * 0 dB, `colors[2]` above; the hold line is white, or `colors[3]` at or
 * above 0 dB; the clip marker is a 3 px block in `colors[3]` at the hot end.
 * The animation loop runs continuously (ballistics move without frames).
 */

/**
 * Level meter for one Stream.
 *
 * Public fields: `canvas` (the element), `stream`, `opts`.
 */
export class Meter {
  /**
   * @param {HTMLElement} container Element the canvas is appended to; decides the size.
   * @param {import('../noob-vst-webgui-framework.js').Stream} stream Stream of `[peak…, rms…?]` linear amplitudes.
   * @param {object} [opts]
   * @param {number} [opts.channels] Bars to draw; defaults to the stream's channel count.
   * @param {number} [opts.minDb=-60] Bottom of the scale.
   * @param {number} [opts.maxDb=6] Top of the scale.
   * @param {'vertical'|'horizontal'} [opts.orientation='vertical'] Bars grow up, or to the right.
   * @param {number} [opts.decayDbPerSec=24] Fall rate of the bar after a peak.
   * @param {number} [opts.holdMs=1200] How long the peak-hold line stays before falling.
   * @param {number} [opts.gap=3] Gap between channel bars in px.
   * @param {string[]} [opts.colors] `[low, mid, hot, clip]` CSS colours; zones split at −12 dB and 0 dB.
   * @param {string} [opts.background='rgba(255,255,255,0.06)'] Fill behind each bar.
   * @example
   * const meter = new Meter(document.querySelector('#out'), client.stream('meter_out'), { minDb: -48 });
   * clipButton.onclick = () => meter.resetClip();
   */
  constructor(container, stream, opts = {}) {
    this.stream = stream;
    this.opts = {
      channels: stream.channels,
      minDb: -60,
      maxDb: 6,
      orientation: 'vertical',
      decayDbPerSec: 24,
      holdMs: 1200,
      gap: 3,
      colors: ['#3ddc84', '#f5c542', '#ff5c5c', '#ff2d2d'],
      background: 'rgba(255,255,255,0.06)',
      ...opts,
    };
    const ch = this.opts.channels;
    this._target = new Float32Array(ch).fill(this.opts.minDb);
    this._show = new Float32Array(ch).fill(this.opts.minDb);
    this._hold = new Float32Array(ch).fill(this.opts.minDb);
    this._holdT = new Float64Array(ch);
    this._rms = new Float32Array(ch).fill(this.opts.minDb);
    this._clip = new Uint8Array(ch);
    this._hasRms = false;

    const c = (this.canvas = document.createElement('canvas'));
    c.style.display = 'block';
    c.style.width = '100%';
    c.style.height = '100%';
    container.appendChild(c);
    this._ctx = c.getContext('2d');
    this._ro = new ResizeObserver(() => this._resize());
    this._ro.observe(container);
    this._container = container;
    this._resize();

    this._off = stream.on((d) => this._onFrame(d));
    this._last = performance.now();
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

  /**
   * New frame: convert to dB, jump the bar up if louder (instant attack),
   * refresh the hold value and its timestamp, latch clip at ≥ 0 dBFS, and
   * read RMS if the frame is long enough.
   * @param {Float32Array} d
   */
  _onFrame(d) {
    const ch = this.opts.channels;
    const now = performance.now();
    this._hasRms = d.length >= ch * 2;
    for (let i = 0; i < ch; i++) {
      const lin = d[i] || 0;
      const db = lin > 0 ? 20 * Math.log10(lin) : -200;
      this._target[i] = db;
      if (db > this._show[i]) this._show[i] = db;
      if (db >= this._hold[i]) {
        this._hold[i] = db;
        this._holdT[i] = now;
      }
      if (db >= 0) this._clip[i] = 1;
      if (this._hasRms) {
        const r = d[ch + i] || 0;
        this._rms[i] = r > 0 ? 20 * Math.log10(r) : -200;
      }
    }
  }

  /** Clear the latched clip indicators on every channel. */
  resetClip() {
    this._clip.fill(0);
  }

  /** Animation loop: apply decay and hold release for the elapsed time, then draw. */
  _tick = () => {
    if (!this._running) return;
    const now = performance.now();
    const dt = Math.min(0.1, (now - this._last) / 1000);
    this._last = now;
    const decay = this.opts.decayDbPerSec * dt;
    for (let i = 0; i < this.opts.channels; i++) {
      if (this._show[i] > this._target[i]) this._show[i] = Math.max(this._target[i], this._show[i] - decay);
      if (now - this._holdT[i] > this.opts.holdMs) this._hold[i] = Math.max(this._show[i], this._hold[i] - decay * 2);
    }
    this._draw();
    this._raf = requestAnimationFrame(this._tick);
  };

  /**
   * dB → 0..1 along the bar (linear in dB, clamped).
   * @param {number} db
   * @returns {number}
   */
  _pos(db) {
    const { minDb, maxDb } = this.opts;
    return Math.max(0, Math.min(1, (db - minDb) / (maxDb - minDb)));
  }

  /** Background, zoned gradient bar (peak, and RMS if present), hold line, clip block, per channel. */
  _draw() {
    const ctx = this._ctx;
    const { channels: ch, gap, colors, orientation } = this.opts;
    const w = this._w;
    const h = this._h;
    ctx.clearRect(0, 0, w, h);
    const vertical = orientation === 'vertical';
    const len = vertical ? h : w;
    const thick = ((vertical ? w : h) - gap * (ch - 1)) / ch;
    const p12 = this._pos(-12);
    const p0 = this._pos(0);

    for (let i = 0; i < ch; i++) {
      const off = i * (thick + gap);
      // background
      ctx.fillStyle = this.opts.background;
      if (vertical) ctx.fillRect(off, 0, thick, h);
      else ctx.fillRect(0, off, w, thick);

      // gradient along the bar
      const g = vertical ? ctx.createLinearGradient(0, h, 0, 0) : ctx.createLinearGradient(0, 0, w, 0);
      g.addColorStop(0, colors[0]);
      g.addColorStop(p12, colors[0]);
      g.addColorStop(Math.min(1, p12 + 0.001), colors[1]);
      g.addColorStop(p0, colors[1]);
      g.addColorStop(Math.min(1, p0 + 0.001), colors[2]);
      g.addColorStop(1, colors[2]);

      const drawBar = (pos, alpha) => {
        ctx.globalAlpha = alpha;
        ctx.fillStyle = g;
        const l = pos * len;
        if (vertical) ctx.fillRect(off, h - l, thick, l);
        else ctx.fillRect(0, off, l, thick);
        ctx.globalAlpha = 1;
      };
      drawBar(this._pos(this._show[i]), this._hasRms ? 0.45 : 1);
      if (this._hasRms) drawBar(this._pos(this._rms[i]), 1);

      // peak hold line
      const hp = this._pos(this._hold[i]);
      if (hp > 0) {
        ctx.fillStyle = this._hold[i] >= 0 ? colors[3] : '#fff';
        if (vertical) ctx.fillRect(off, h - hp * h - 1, thick, 2);
        else ctx.fillRect(hp * w - 1, off, 2, thick);
      }
      // clip indicator
      if (this._clip[i]) {
        ctx.fillStyle = colors[3];
        if (vertical) ctx.fillRect(off, 0, thick, 3);
        else ctx.fillRect(w - 3, off, 3, thick);
      }
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

export default Meter;
