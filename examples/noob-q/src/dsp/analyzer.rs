//! Spectrum analyzer with switchable resolution, block meters, and the demo
//! signal sources used by the standalone binary.
//!
//! The analyzer is deliberately simple on the Rust side: it keeps a ring of
//! recent samples and, on demand, Hann-windows the last `N` of them and
//! returns the magnitude spectrum in dBFS, scaled so a full-scale sine reads
//! 0 dB. Everything a user perceives as "the analyzer" (range, speed /
//! averaging, tilt, freeze, peak hold, the frequency axis, spectrum grab)
//! happens in the page, in `crates/vst3-web-stratum/web/components/spectrum.js`, on those raw bins.
//! That keeps the audio thread cheap (one FFT every other block) and lets
//! several windows display the same data differently. The `analyzer_range`,
//! `analyzer_speed`, `analyzer_tilt`, `analyzer_freeze` and `display_range`
//! parameters exist only so those settings are saved with the plug-in
//! state; the DSP never reads them.
//!
//! Both hosts run three analyzers (input, output, side-chain) and two
//! [`Meter`]s, and publish spectra on every second block, meters on every
//! block.

use std::f32::consts::PI;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Labels for the `analyzer_resolution` parameter, indexed like
/// [`RESOLUTION_FFT`].
pub const RESOLUTION_NAMES: [&str; 4] = ["Low", "Medium", "High", "Maximum"];
/// FFT size per resolution: 47 / 23 / 12 / 6 Hz per bin at 48 kHz, with the
/// time response getting slower as the size grows.
pub const RESOLUTION_FFT: [usize; 4] = [1024, 2048, 4096, 8192];
/// The largest FFT (sizes the ring and the work buffer).
pub const MAX_FFT: usize = 8192;
/// Stream capacity: bins of the largest FFT. A frame at a lower resolution
/// is shorter; the page uses the frame length to place bins.
pub const MAX_BINS: usize = MAX_FFT / 2 + 1;

/// A mono spectrum analyzer: push samples continuously, call
/// [`compute`](Self::compute) whenever a frame should be published.
pub struct Analyzer {
    /// One forward plan per resolution.
    ffts: Vec<Arc<dyn Fft<f32>>>,
    /// Current resolution index.
    size: usize,
    /// Work buffer, `MAX_FFT` complex values.
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// One Hann window per resolution.
    windows: Vec<Vec<f32>>,
    /// The last `2 × MAX_FFT` input samples.
    ring: Vec<f32>,
    /// Write position in `ring`.
    pos: usize,
}

impl Analyzer {
    /// Plan every resolution's FFT, build the windows and allocate the
    /// buffers. Starts at *Medium* resolution.
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let ffts: Vec<_> = RESOLUTION_FFT
            .iter()
            .map(|&n| planner.plan_fft_forward(n))
            .collect();
        let scratch_len = ffts
            .iter()
            .map(|f| f.get_inplace_scratch_len())
            .max()
            .unwrap_or(0);
        let windows = RESOLUTION_FFT
            .iter()
            .map(|&n| {
                (0..n)
                    .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
                    .collect()
            })
            .collect();
        Analyzer {
            ffts,
            size: 1,
            buf: vec![Complex::default(); MAX_FFT],
            scratch: vec![Complex::default(); scratch_len],
            windows,
            ring: vec![0.0; MAX_FFT * 2],
            pos: 0,
        }
    }

    /// Select a resolution (index into [`RESOLUTION_FFT`]).
    pub fn set_resolution(&mut self, index: usize) {
        self.size = index.min(RESOLUTION_FFT.len() - 1);
    }

    /// The FFT size of the current resolution.
    pub fn fft_size(&self) -> usize {
        RESOLUTION_FFT[self.size]
    }

    /// Number of bins the next `compute` will write (`fft_size / 2 + 1`).
    pub fn bins(&self) -> usize {
        self.fft_size() / 2 + 1
    }

    /// Append one input sample (the hosts push the mono sum `(L + R) / 2`).
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.ring[self.pos] = x;
        self.pos = (self.pos + 1) % self.ring.len();
    }

    /// Magnitude spectrum of the most recent window, in dBFS, written to
    /// `out[..bins]`. Returns the number of bins written. The scale is
    /// `4 / N`: `2 / N` for a one-sided spectrum of a real signal and
    /// another `2` for the Hann window's coherent gain of 0.5, so a
    /// full-scale sine reads 0 dB in its bin. Bin 0 is DC; the hosts
    /// publish it as-is and the page decides what to draw below 20 Hz.
    pub fn compute(&mut self, out: &mut [f32]) -> usize {
        let n = self.fft_size();
        let bins = n / 2 + 1;
        let ring_len = self.ring.len();
        let start = (self.pos + ring_len - n) % ring_len;
        let window = &self.windows[self.size];
        let buf = &mut self.buf[..n];
        for (k, c) in buf.iter_mut().enumerate() {
            *c = Complex::new(self.ring[(start + k) % ring_len] * window[k], 0.0);
        }
        self.ffts[self.size].process_with_scratch(buf, &mut self.scratch);
        let gain = 4.0 / n as f32;
        for (k, o) in out.iter_mut().enumerate().take(bins) {
            let mag = buf[k].norm() * gain;
            *o = 20.0 * mag.max(1e-9).log10();
        }
        bins
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Analyzer::new()
    }
}

/// Block peak + RMS, stereo. Feed every sample, [`take`](Self::take) once
/// per block; the page keeps its own ballistics (peak hold, fall time) from
/// the per-block values.
#[derive(Default)]
pub struct Meter {
    peak: [f32; 2],
    sum_sq: [f32; 2],
    n: u32,
}

impl Meter {
    /// Accumulate one stereo sample.
    #[inline]
    pub fn feed(&mut self, l: f32, r: f32) {
        self.peak[0] = self.peak[0].max(l.abs());
        self.peak[1] = self.peak[1].max(r.abs());
        self.sum_sq[0] += l * l;
        self.sum_sq[1] += r * r;
        self.n += 1;
    }

    /// `[peak_l, peak_r, rms_l, rms_r]` for the block (linear, 1.0 = 0 dBFS),
    /// then reset for the next block. This is the layout of the `meter_in` /
    /// `meter_out` streams.
    pub fn take(&mut self) -> [f32; 4] {
        let n = self.n.max(1) as f32;
        let out = [
            self.peak[0],
            self.peak[1],
            (self.sum_sq[0] / n).sqrt(),
            (self.sum_sq[1] / n).sqrt(),
        ];
        *self = Meter::default();
        out
    }
}

/// Labels for the standalone's `src_kind` / `sc_kind` parameters, indexed
/// like the `kind` argument of [`Source::next`].
pub const SOURCE_NAMES: [&str; 6] = [
    "Pink Noise",
    "White Noise",
    "Saw",
    "Sine",
    "Drum Loop",
    "Silence",
];

/// Demo signal source for the standalone binary, so the analyzer and the
/// dynamics have something to show without a DAW. Deterministic per seed.
pub struct Source {
    /// Oscillator phase, 0..1 (saw / sine).
    phase: f32,
    /// Pink-noise filter state.
    pink: [f32; 7],
    /// xorshift32 state.
    rng: u32,
    /// Sample counter for the drum loop's sequencer.
    drum_t: u32,
    /// Kick amplitude envelope (also drives its pitch drop).
    drum_env: f32,
    /// Kick oscillator phase.
    drum_phase: f32,
    /// Snare (pink burst) envelope.
    snare_env: f32,
    /// Hi-hat (white burst) envelope.
    hat_env: f32,
}

impl Source {
    /// A source with its own noise sequence; two sources with different
    /// seeds are uncorrelated (the standalone uses one for the input and one
    /// for the side-chain).
    pub fn new(seed: u32) -> Self {
        Source {
            phase: 0.0,
            pink: [0.0; 7],
            rng: seed | 1,
            drum_t: 0,
            drum_env: 0.0,
            drum_phase: 0.0,
            snare_env: 0.0,
            hat_env: 0.0,
        }
    }

    /// Uniform white noise in −1..1 from a xorshift32 generator.
    #[inline]
    fn white(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Pink noise (−3 dB/oct): Paul Kellet's "refined" seven-pole
    /// approximation applied to the white generator, scaled to roughly the
    /// same RMS as the white output.
    #[inline]
    fn pink(&mut self) -> f32 {
        // Paul Kellet's refined pink noise filter.
        let w = self.white();
        let b = &mut self.pink;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.153_852;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;
        pink * 0.11
    }

    /// One sample of the selected source at unity level. `kind` indexes
    /// [`SOURCE_NAMES`]: 0 pink noise, 1 white noise, 2 naive saw at `freq`,
    /// 3 sine at `freq`, 4 a synthesized 120 BPM drum loop (kick on beats 1
    /// and 3 with a pitch drop, pink-noise snare on 2 and 4, white-noise
    /// hats on eighths), anything else silence. The saw is not band-limited
    /// on purpose: its aliasing is visible on the analyzer.
    #[inline]
    pub fn next(&mut self, kind: usize, freq: f32, sr: f32) -> f32 {
        match kind {
            0 => self.pink(),
            1 => self.white(),
            2 | 3 => {
                self.phase += freq / sr;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                if kind == 2 {
                    2.0 * self.phase - 1.0
                } else {
                    (2.0 * PI * self.phase).sin()
                }
            }
            4 => {
                // A crude synthesized beat: kick on 1 and 3, snare on 2 and
                // 4, hats on eighths, at 120 BPM.
                let step = (sr * 0.25) as u32; // one eighth note
                if self.drum_t.is_multiple_of(step) {
                    let eighth = (self.drum_t / step) % 8;
                    if eighth.is_multiple_of(4) {
                        self.drum_env = 1.0;
                        self.drum_phase = 0.0;
                    }
                    if eighth % 4 == 2 {
                        self.snare_env = 1.0;
                    }
                    self.hat_env = 0.6;
                }
                self.drum_t = self.drum_t.wrapping_add(1);
                let kick_f = 45.0 + 120.0 * self.drum_env * self.drum_env;
                self.drum_phase += kick_f / sr;
                let kick = (2.0 * PI * self.drum_phase).sin() * self.drum_env;
                self.drum_env *= 1.0 - 8.0 / sr;
                let snare = self.pink() * 3.0 * self.snare_env;
                self.snare_env *= 1.0 - 25.0 / sr;
                let hat = self.white() * 0.25 * self.hat_env;
                self.hat_env *= 1.0 - 60.0 / sr;
                (kick * 0.9 + snare + hat).clamp(-1.0, 1.0)
            }
            _ => 0.0,
        }
    }
}

impl Default for Source {
    fn default() -> Self {
        Source::new(0x9E37_79B9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_finds_a_sine_at_every_resolution() {
        for res in 0..RESOLUTION_FFT.len() {
            let mut a = Analyzer::new();
            a.set_resolution(res);
            for i in 0..MAX_FFT * 2 {
                a.push((2.0 * PI * 1000.0 * i as f32 / 48000.0).sin());
            }
            let mut out = vec![0.0; MAX_BINS];
            let bins = a.compute(&mut out);
            assert_eq!(bins, RESOLUTION_FFT[res] / 2 + 1);
            let bin_hz = 48000.0 / RESOLUTION_FFT[res] as f32;
            let expect = (1000.0 / bin_hz).round() as usize;
            let (peak_bin, peak) = out[..bins]
                .iter()
                .enumerate()
                .fold((0, f32::MIN), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
            assert!((peak_bin as i32 - expect as i32).abs() <= 1);
            assert!(peak > -1.5 && peak < 0.5, "res {res}: peak {peak} dBFS");
        }
    }
}
