//! Linear-phase processing: a symmetric FIR designed from the target
//! magnitude response, applied with uniformly partitioned overlap-save FFT
//! convolution.
//!
//! ## FIR design ([`FirDesigner`])
//!
//! Frequency sampling: the desired magnitude is evaluated on the `N/2 + 1`
//! bins of an `N`-point spectrum with zero phase, mirrored to a real
//! spectrum, and inverse-transformed. The resulting zero-phase impulse is
//! rotated by `N/2` so it becomes causal and symmetric, then multiplied by
//! a Blackman window to tame the ripple that sampling a steep response
//! produces. A symmetric FIR has exactly linear phase with a group delay of
//! `N/2` samples, which is the "linear phase" the mode promises and the
//! reason it costs latency.
//!
//! ## Convolution ([`Convolver`])
//!
//! Uniformly partitioned overlap-save: the impulse is cut into partitions of
//! [`PARTITION`] samples, each transformed once with a `2 × PARTITION` FFT.
//! Input arrives in hops of `PARTITION` samples; every hop is transformed
//! once and pushed into a frequency-domain delay line. The output hop is the
//! inverse FFT of `Σⱼ FDL[j] · H[j]`, keeping only the second half
//! (overlap-save discards the circular wrap-around). Cost per hop is one
//! FFT, one inverse FFT and `n_parts` complex multiply-accumulates of
//! `2 × PARTITION` points, so a 65536-tap filter costs 256 partitions of
//! 512 points per 256 samples of audio, regardless of where the energy in
//! the impulse sits. Swapping the impulse keeps the delay line, so an EQ
//! change does not click.
//!
//! ## Latency
//!
//! Block buffering adds [`PARTITION`] samples and the FIR itself `taps / 2`:
//! `PARTITION + taps / 2` per stage, which the engine reports to the host.
//! With both L/R-specific and M/S-specific bands enabled the engine runs
//! two stages in series and the latency doubles.
//!
//! ## Real-time
//!
//! Everything is allocated in `new`; `process`, `set_impulse` and
//! `FirDesigner::design` do not allocate for the five quality sizes (the
//! FFT plans and scratch space are made up front). A redesign at *Maximum*
//! quality is a 65536-point inverse FFT plus 256 partition FFTs, a few
//! hundred microseconds on a desktop core, which is why the engine does it
//! at most every other block while dynamics move, and why dynamics are off
//! at the two highest qualities ([`QUALITY_DYNAMICS_LIMIT`]).

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Input is convolved in hops of this many samples; also the block latency
/// each convolver adds. 256 keeps the per-hop FFT small (512 points) at the
/// cost of more partitions for long filters.
pub const PARTITION: usize = 256;

/// Labels for the `lp_quality` parameter, indexed like [`QUALITY_TAPS`].
pub const QUALITY_NAMES: [&str; 5] = ["Low", "Medium", "High", "Very High", "Maximum"];
/// FIR lengths for the linear-phase quality settings. Longer filters follow
/// narrow, low-frequency shapes more accurately (a 4096-tap FIR at 48 kHz
/// cannot resolve much below ~25 Hz). Single-stage latency is
/// `PARTITION + taps / 2`: 2304 / 4352 / 8448 / 16640 / 33024 samples,
/// 48 ms … 688 ms at 48 kHz.
pub const QUALITY_TAPS: [usize; 5] = [4096, 8192, 16384, 32768, 65536];
/// The longest FIR (and therefore the buffer sizes allocated up front).
pub const MAX_TAPS: usize = 65536;
/// Qualities at or above this index are static-only (no dynamic EQ), as the
/// FIR redesign would be too costly per block.
pub const QUALITY_DYNAMICS_LIMIT: usize = 3;

/// Designs symmetric (zero-phase, then delayed) FIRs by frequency sampling.
/// Owns one inverse-FFT plan per quality size plus the buffers, so a design
/// on the audio thread allocates nothing.
pub struct FirDesigner {
    planner: FftPlanner<f32>,
    /// `(length, plan)` for every size designed so far; the five quality
    /// sizes are planned in `new`.
    ffts: Vec<(usize, Arc<dyn Fft<f32>>)>,
    /// Spectrum in, impulse out (`MAX_TAPS` complex values).
    buf: Vec<Complex<f32>>,
    /// rustfft scratch, sized for the largest plan.
    scratch: Vec<Complex<f32>>,
    /// Blackman window of the last designed length (rebuilt on a size change).
    window: Vec<f32>,
}

impl FirDesigner {
    /// Plan the inverse FFTs for every entry of [`QUALITY_TAPS`] and
    /// allocate the buffers.
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let ffts: Vec<_> = QUALITY_TAPS
            .iter()
            .map(|&n| (n, planner.plan_fft_inverse(n)))
            .collect();
        let scratch_len = ffts
            .iter()
            .map(|(_, f)| f.get_inplace_scratch_len())
            .max()
            .unwrap_or(0);
        FirDesigner {
            planner,
            ffts,
            buf: vec![Complex::default(); MAX_TAPS],
            scratch: vec![Complex::default(); scratch_len],
            window: Vec::new(),
        }
    }

    /// Fill `out` (length = one of [`QUALITY_TAPS`]) with a linear-phase FIR
    /// whose magnitude follows `db_at(freq_hz)`, sampled at `k · sr / N` for
    /// `k = 0 ..= N/2` (`db_at` is never asked below 1 Hz). The impulse is
    /// centred at `N/2` and Blackman-windowed. Cost: one inverse FFT of
    /// `out.len()` plus `out.len() / 2 + 1` calls of `db_at`. Any other
    /// length works too but plans (allocates) its FFT on first use.
    pub fn design(&mut self, sr: f32, out: &mut [f32], mut db_at: impl FnMut(f32) -> f32) {
        let n = out.len();
        let fft = match self.ffts.iter().find(|(len, _)| *len == n) {
            Some((_, f)) => f.clone(),
            None => {
                let f = self.planner.plan_fft_inverse(n);
                self.ffts.push((n, f.clone()));
                f
            }
        };
        let half = n / 2;
        let buf = &mut self.buf[..n];
        for k in 0..=half {
            let f = k as f32 * sr / n as f32;
            let mag = 10f32.powf(db_at(f.max(1.0)) / 20.0);
            buf[k] = Complex::new(mag, 0.0);
            if k > 0 && k < half {
                buf[n - k] = Complex::new(mag, 0.0);
            }
        }
        fft.process_with_scratch(buf, &mut self.scratch);
        if self.window.len() != n {
            // Blackman window: good stop-band rejection for a brickwall demo.
            self.window = (0..n)
                .map(|i| {
                    let x = 2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32;
                    0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
                })
                .collect();
        }
        let scale = 1.0 / n as f32;
        for i in 0..n {
            // Rotate the zero-phase response so its centre sits at n/2.
            let src = (i + half) % n;
            out[i] = buf[src].re * scale * self.window[i];
        }
    }
}

impl Default for FirDesigner {
    fn default() -> Self {
        FirDesigner::new()
    }
}

/// Uniformly partitioned overlap-save convolver for one channel (see the
/// module docs for the algorithm). Sample-in, sample-out interface with an
/// internal hop of [`PARTITION`] samples.
pub struct Convolver {
    /// Forward plan, `2 × PARTITION` points.
    fft: Arc<dyn Fft<f32>>,
    /// Inverse plan, `2 × PARTITION` points.
    ifft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    /// The impulse partitions, already in the frequency domain (`H[j]`).
    parts: Vec<Vec<Complex<f32>>>,
    /// Frequency-domain delay line: the spectra of the last `n_parts` input
    /// hops, as a ring.
    fdl: Vec<Vec<Complex<f32>>>,
    /// Ring position of the newest input spectrum.
    fdl_pos: usize,
    /// Partitions in use for the current impulse (`ceil(taps / PARTITION)`).
    n_parts: usize,
    /// The last two input hops (overlap-save needs `2 × PARTITION` samples).
    in_buf: Vec<f32>,
    /// The input hop being collected.
    in_q: Vec<f32>,
    /// The output hop being drained.
    out_q: Vec<f32>,
    /// Samples collected into `in_q` so far.
    fill: usize,
    /// `Σ FDL[j] · H[j]`, then its inverse FFT.
    acc: Vec<Complex<f32>>,
    /// Scratch for the forward transform of the input.
    tmp: Vec<Complex<f32>>,
    /// Length of the loaded impulse, samples.
    taps: usize,
}

impl Convolver {
    /// Allocate for impulses up to `max_taps` samples: `ceil(max_taps /
    /// PARTITION)` partitions for the impulse and as many for the delay
    /// line, `2 × PARTITION` complex values each.
    pub fn new(max_taps: usize) -> Self {
        let p = PARTITION;
        let len = 2 * p;
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(len);
        let ifft = planner.plan_fft_inverse(len);
        let scratch_len = fft
            .get_inplace_scratch_len()
            .max(ifft.get_inplace_scratch_len());
        let max_parts = max_taps.div_ceil(p);
        Convolver {
            fft,
            ifft,
            scratch: vec![Complex::default(); scratch_len],
            parts: (0..max_parts)
                .map(|_| vec![Complex::default(); len])
                .collect(),
            fdl: (0..max_parts)
                .map(|_| vec![Complex::default(); len])
                .collect(),
            fdl_pos: 0,
            n_parts: 0,
            in_buf: vec![0.0; len],
            in_q: vec![0.0; p],
            out_q: vec![0.0; p],
            fill: 0,
            acc: vec![Complex::default(); len],
            tmp: vec![Complex::default(); len],
            taps: 0,
        }
    }

    /// Load a new impulse response (any length up to `max_taps`): each
    /// partition is zero-padded to `2 × PARTITION` and transformed. The
    /// frequency-domain delay line is kept when the partition count is
    /// unchanged, so updating the filter while audio runs does not click; a
    /// different count (quality change) clears it.
    pub fn set_impulse(&mut self, h: &[f32]) {
        let p = PARTITION;
        let len = 2 * p;
        let n_parts = h.len().div_ceil(p).min(self.parts.len());
        for (j, part) in self.parts.iter_mut().enumerate().take(n_parts) {
            let start = j * p;
            let end = (start + p).min(h.len());
            for (i, c) in part.iter_mut().enumerate() {
                *c = if start + i < end {
                    Complex::new(h[start + i], 0.0)
                } else {
                    Complex::default()
                };
            }
            self.fft.process_with_scratch(part, &mut self.scratch);
        }
        if n_parts != self.n_parts {
            for f in self.fdl.iter_mut().take(n_parts) {
                f.iter_mut().for_each(|c| *c = Complex::default());
            }
            self.fdl_pos = 0;
        }
        self.n_parts = n_parts;
        self.taps = h.len();
        let _ = len;
    }

    /// Length of the loaded impulse, samples (0 before `set_impulse`).
    pub fn taps(&self) -> usize {
        self.taps
    }

    /// Latency added by block buffering (the FIR's own delay is `taps / 2`).
    pub fn latency(&self) -> usize {
        PARTITION
    }

    /// Clear all signal state (input hops, output hop, delay line) but keep
    /// the impulse. Used on sample-rate, mode and staging changes.
    pub fn reset(&mut self) {
        self.in_buf.iter_mut().for_each(|v| *v = 0.0);
        self.in_q.iter_mut().for_each(|v| *v = 0.0);
        self.out_q.iter_mut().for_each(|v| *v = 0.0);
        for f in &mut self.fdl {
            f.iter_mut().for_each(|c| *c = Complex::default());
        }
        self.fill = 0;
        self.fdl_pos = 0;
    }

    /// Output is the input convolved with the impulse, delayed by
    /// [`PARTITION`] samples. Cheap for all but every 256th call, which
    /// runs a whole hop (`run_block`); hosts with small buffers therefore
    /// see the cost land in one block out of several, which is normal for
    /// FFT convolution.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.out_q[self.fill];
        self.in_q[self.fill] = x;
        self.fill += 1;
        if self.fill == PARTITION {
            self.run_block();
            self.fill = 0;
        }
        y
    }

    /// One hop of overlap-save: shift the previous hop down, append the new
    /// one, transform, store in the delay line, multiply-accumulate against
    /// every impulse partition (partition `j` against the input hop from `j`
    /// hops ago), inverse-transform, and keep the second half.
    fn run_block(&mut self) {
        let p = PARTITION;
        let len = 2 * p;
        if self.n_parts == 0 {
            // No filter loaded: pass through with the block delay.
            self.out_q.copy_from_slice(&self.in_q);
            return;
        }
        // Overlap-save input block: [previous hop | new hop].
        self.in_buf.copy_within(p..len, 0);
        self.in_buf[p..].copy_from_slice(&self.in_q);
        for (t, v) in self.tmp.iter_mut().zip(&self.in_buf) {
            *t = Complex::new(*v, 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.tmp, &mut self.scratch);
        self.fdl[self.fdl_pos].copy_from_slice(&self.tmp);
        self.acc.iter_mut().for_each(|c| *c = Complex::default());
        for j in 0..self.n_parts {
            // The j-th partition of the impulse pairs with the input hop that
            // arrived j hops ago (ring index walks backwards from the newest).
            let idx = (self.fdl_pos + self.n_parts - j) % self.n_parts;
            let x = &self.fdl[idx];
            let h = &self.parts[j];
            for ((a, xk), hk) in self.acc.iter_mut().zip(x).zip(h) {
                *a += *xk * *hk;
            }
        }
        self.ifft
            .process_with_scratch(&mut self.acc, &mut self.scratch);
        // rustfft is unnormalized: scale by 1/len. The first half of the
        // result is circular garbage; the second half is the valid output.
        let scale = 1.0 / len as f32;
        for (o, c) in self.out_q.iter_mut().zip(&self.acc[p..]) {
            *o = c.re * scale;
        }
        self.fdl_pos = (self.fdl_pos + 1) % self.n_parts;
    }
}

/// A plain delay line, used to keep bypassed / unused paths time-aligned
/// with the convolvers (bypass in linear-phase mode must keep the mode's
/// latency, or toggling it would shift the audio in time).
pub struct Delay {
    buf: Vec<f32>,
    pos: usize,
    len: usize,
}

impl Delay {
    /// Allocate for delays up to `max − 1` samples.
    pub fn new(max: usize) -> Self {
        Delay {
            buf: vec![0.0; max.max(1)],
            pos: 0,
            len: 0,
        }
    }
    /// Set the delay in samples (clamped to the capacity). Changing it
    /// clears the line, so it is meant for mode changes, not modulation.
    pub fn set_delay(&mut self, samples: usize) {
        let s = samples.min(self.buf.len() - 1);
        if s != self.len {
            self.len = s;
            self.buf.iter_mut().for_each(|v| *v = 0.0);
            self.pos = 0;
        }
    }
    /// Delay one sample; a zero delay passes through.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.len == 0 {
            return x;
        }
        let n = self.buf.len();
        let read = (self.pos + n - self.len) % n;
        let y = self.buf[read];
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) % n;
        y
    }
    /// Clear the line (keeps the delay length).
    pub fn reset(&mut self) {
        self.buf.iter_mut().for_each(|v| *v = 0.0);
        self.pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convolver_matches_direct_convolution_with_block_delay() {
        let taps = 700;
        let h: Vec<f32> = (0..taps)
            .map(|i| ((i * 7919) % 97) as f32 / 97.0 - 0.5)
            .collect();
        let x: Vec<f32> = (0..3000)
            .map(|i| ((i * 104729) % 89) as f32 / 89.0 - 0.5)
            .collect();
        let mut c = Convolver::new(MAX_TAPS);
        c.set_impulse(&h);
        let y: Vec<f32> = x.iter().map(|&v| c.process(v)).collect();
        // y[n] should equal (h * x)[n - PARTITION].
        for n in PARTITION..x.len() {
            let m = n - PARTITION;
            let mut direct = 0.0f32;
            for k in 0..taps.min(m + 1) {
                direct += h[k] * x[m - k];
            }
            assert!((y[n] - direct).abs() < 1e-3, "n={n}: {} vs {direct}", y[n]);
        }
    }

    #[test]
    fn designed_fir_has_the_requested_magnitude() {
        let mut d = FirDesigner::new();
        let mut h = vec![0.0f32; 4096];
        // +6 dB above 2 kHz, flat below (a high shelf).
        d.design(48000.0, &mut h, |f| if f > 2000.0 { 6.0 } else { 0.0 });
        // Symmetric about n/2 => linear phase with a delay of exactly n/2.
        let half = h.len() / 2;
        for k in 1..half {
            assert!((h[half + k] - h[half - k]).abs() < 1e-5, "k={k}");
        }
        let peak =
            h.iter().enumerate().fold(
                (0, 0.0f32),
                |m, (i, &v)| if v.abs() > m.1 { (i, v.abs()) } else { m },
            );
        assert_eq!(peak.0, half);
        // Check magnitude by DFT at two frequencies.
        let mag = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, &v) in h.iter().enumerate() {
                let w = 2.0 * std::f32::consts::PI * f * i as f32 / 48000.0;
                re += v * w.cos();
                im -= v * w.sin();
            }
            20.0 * (re * re + im * im).sqrt().log10()
        };
        assert!(mag(500.0).abs() < 0.3, "{}", mag(500.0));
        assert!((mag(8000.0) - 6.0).abs() < 0.3, "{}", mag(8000.0));
    }

    #[test]
    fn delay_line_delays() {
        let mut d = Delay::new(64);
        d.set_delay(3);
        let out: Vec<f32> = [1.0, 2.0, 3.0, 4.0, 5.0]
            .iter()
            .map(|&x| d.process(x))
            .collect();
        assert_eq!(out, vec![0.0, 0.0, 0.0, 1.0, 2.0]);
    }
}
