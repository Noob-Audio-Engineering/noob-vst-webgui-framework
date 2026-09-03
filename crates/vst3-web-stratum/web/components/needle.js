/**
 * NeedleModel — the behaviour of an analog needle meter without any
 * drawing: value conversion, scale mapping and needle ballistics. A page
 * draws whatever face it likes (SVG, canvas, CSS) from `frac` / `angle`.
 *
 * Several plug-ins want a needle that moves like a VU meter (or like a
 * lazier optical-compressor meter), and the maths is the same every time;
 * how the face looks is the plug-in's business, so nothing here paints.
 *
 * ## Values
 *
 * `set(raw)` accepts the plug-in's value in one of three units:
 *
 * * `'linear'`: linear amplitude (1.0 = 0 dBFS), converted with
 *   `20·log10(x) − reference` so a signal at `reference` dBFS reads 0.
 * * `'db'`: already in dB relative to the meter's zero.
 * * `'raw'`: any number on the scale's own axis (a percentage, a ratio).
 *
 * `mode` decides the resting value with no signal and the sign convention:
 * `'level'` rests at `min`; `'reduction'` rests at 0 and takes values as
 * `−|value|`, the way a gain-reduction meter swings left.
 *
 * ## Scale
 *
 * `frac(value)` maps a value to 0..1 along the scale. `scale: 'vu'` is
 * voltage-proportional (`10^(dB/20)` between `min` and `max`), which crowds
 * the marks towards the left exactly like a printed VU face; `'linear'`
 * spaces values evenly. `angle(value, sweep)` turns that into an angle in
 * radians centred on 0 (negative to the left) for a needle sweeping `sweep`
 * degrees. `marks(values)` returns `{ value, frac, angle }` for a list of
 * scale marks, ready to draw.
 *
 * ## Ballistics
 *
 * `step(dtSeconds)` advances the needle towards the current target as a
 * damped second-order system. A standard VU meter reaches 99 % of a step
 * in 300 ms with about 1 % overshoot (`riseMs: 300, damping: 0.62`); an
 * optical compressor's meter is lazier (`riseMs` around 500). The needle
 * position is `position` (in scale units) and `frac` / `angle` without
 * arguments read it. Call `step` from your own animation loop, or use
 * `start(onFrame)` which runs `requestAnimationFrame` for you and calls
 * `onFrame(model)` after each step.
 *
 * @example
 * const needle = new NeedleModel({ mode: 'reduction', unit: 'db', riseMs: 300 });
 * client.stream('meter').on((d) => needle.set(d[4]));
 * needle.start((m) => svgNeedle.setAttribute('transform', `rotate(${(m.angle() * 180) / Math.PI})`));
 */
export class NeedleModel {
  /**
   * @param {object} [opts]
   * @param {'linear'|'db'|'raw'} [opts.unit='db'] How `set()` values are read.
   * @param {'level'|'reduction'} [opts.mode='level'] Resting point and sign convention.
   * @param {number} [opts.reference=-18] dBFS that reads 0 for `'linear'` input.
   * @param {'vu'|'linear'} [opts.scale='vu'] Voltage-proportional or even spacing.
   * @param {number} [opts.min=-20] Left end of the scale, in scale units.
   * @param {number} [opts.max=3] Right end of the scale.
   * @param {number} [opts.riseMs=300] Time to reach 99 % of a step.
   * @param {number} [opts.damping=0.62] Damping ratio (lower overshoots more; 1 is critically damped).
   * @param {number} [opts.overshoot=1.5] How far past `max` (in scale units) the needle may travel.
   */
  constructor(opts = {}) {
    this.opts = { unit: 'db', mode: 'level', reference: -18, scale: 'vu', min: -20, max: 3, riseMs: 300, damping: 0.62, overshoot: 1.5, ...opts };
    const rest = this.opts.mode === 'reduction' ? 0 : this.opts.min;
    /** Current target in scale units. */
    this.target = rest;
    /** Needle position in scale units. */
    this.position = rest;
    this._vel = 0;
    this._raf = 0;
    this._last = 0;
  }

  /**
   * Feed a value in the configured `unit`.
   * @param {number} raw
   * @returns {number} The converted value in scale units.
   */
  set(raw) {
    let v = raw;
    if (this.opts.unit === 'linear') v = raw > 0 ? 20 * Math.log10(raw) - this.opts.reference : -200;
    if (this.opts.mode === 'reduction') v = -Math.abs(v);
    this.target = v;
    return v;
  }

  /**
   * Scale units → 0..1 along the scale (clamped a little past both ends).
   * @param {number} [value=this.position]
   * @returns {number}
   */
  frac(value = this.position) {
    const { min, max, scale } = this.opts;
    let f;
    if (scale === 'linear') f = (value - min) / (max - min);
    else {
      const lo = 10 ** (min / 20);
      const hi = 10 ** (max / 20);
      f = (10 ** (value / 20) - lo) / (hi - lo);
    }
    return Math.max(-0.08, Math.min(1.08, f));
  }

  /**
   * Scale units → needle angle in radians, 0 straight up, negative left.
   * @param {number} [value=this.position]
   * @param {number} [sweep=90] Total sweep in degrees.
   * @returns {number}
   */
  angle(value = this.position, sweep = 90) {
    const half = (sweep * Math.PI) / 360;
    return -half + this.frac(value) * 2 * half;
  }

  /**
   * Positions for a list of scale marks.
   * @param {number[]} values
   * @param {number} [sweep=90]
   * @returns {{ value: number, frac: number, angle: number }[]}
   */
  marks(values, sweep = 90) {
    return values.map((value) => ({ value, frac: this.frac(value), angle: this.angle(value, sweep) }));
  }

  /**
   * Advance the needle by `dt` seconds towards the target.
   * @param {number} dt Seconds since the last step (capped at 100 ms).
   * @returns {number} The new position.
   */
  step(dt) {
    dt = Math.min(0.1, Math.max(0, dt));
    const { min, max, riseMs, damping, overshoot } = this.opts;
    // 99 % settling time of a second-order system is about 4.6 / (ζ·ω).
    const omega = 4.6 / (damping * Math.max(0.001, riseMs / 1000));
    const target = Math.max(min - overshoot, Math.min(max + overshoot, this.target));
    const steps = Math.max(1, Math.ceil(dt / 0.004));
    const h = dt / steps;
    for (let i = 0; i < steps; i++) {
      const acc = omega * omega * (target - this.position) - 2 * damping * omega * this._vel;
      this._vel += acc * h;
      this.position += this._vel * h;
    }
    return this.position;
  }

  /**
   * Run `step` on every animation frame and call `onFrame(this)` after it.
   * @param {(m: NeedleModel) => void} onFrame
   */
  start(onFrame) {
    this.stop();
    this._last = performance.now();
    const tick = (now) => {
      this.step((now - this._last) / 1000);
      this._last = now;
      onFrame(this);
      this._raf = requestAnimationFrame(tick);
    };
    this._raf = requestAnimationFrame(tick);
  }

  /** Stop the animation started with `start`. */
  stop() {
    if (this._raf) cancelAnimationFrame(this._raf);
    this._raf = 0;
  }
}
