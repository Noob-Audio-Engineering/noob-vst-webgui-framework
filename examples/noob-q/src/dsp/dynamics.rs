//! Per-band dynamics (Pro-Q's "dynamic EQ"): a level detector on the band's
//! own frequency region drives the band's gain within a signed range above
//! a manual or automatic threshold.
//!
//! The detector input is the band's region isolated by a band-pass (or a
//! low- / high-pass for shelves and cuts) that the engine designs from the
//! band's frequency and Q, fed either from the plug-in input (`(L + R) / 2`)
//! or from the external side-chain. [`Dynamics::feed`] runs a peak envelope
//! follower per sample with separate attack and release coefficients;
//! [`Dynamics::update_block`] turns the envelope into a gain once per block:
//!
//! ```text
//! over   = env_dB − threshold_dB
//! amount = clamp(over / 12 dB, 0, 1)      // soft knee, full range 12 dB above threshold
//! target = range_dB × amount              // signed: negative range cuts, positive boosts
//! gain  += (target − gain) × 0.5          // block-rate smoothing, click-free
//! ```
//!
//! With *auto threshold* the threshold follows the region's running average
//! level (a one-second one-pole average of the envelope) plus 3 dB, so the
//! band reacts to peaks above the recent norm rather than to an absolute
//! level and needs no tuning when the source changes. The engine redesigns
//! the band's filter with `static gain + dynamic gain`, which is what the
//! page draws as the moving part of the band's dynamic range indicator and
//! publishes on the `band_dyn` stream.
//!
//! Dynamics only apply to shapes with gain (bells, shelves, tilts) and are
//! disabled at the two highest linear-phase qualities, where redesigning a
//! 32768- or 65536-tap FIR every block would be too costly.

/// One band's dynamics settings (the `b<n>_dyn_*` parameters).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DynSettings {
    /// `b<n>_dyn_on`. Off keeps the dynamic gain at 0 dB.
    pub on: bool,
    /// `b<n>_dyn_range`, signed, dB. Positive = boost when the region gets
    /// loud (upward), negative = cut when it gets loud (downward, like a
    /// compressor on that region only).
    pub range_db: f32,
    /// `b<n>_dyn_thr`: manual threshold in dBFS of the band-filtered
    /// detector signal. Ignored while `auto_threshold` is on.
    pub threshold_db: f32,
    /// `b<n>_dyn_auto`: follow the region's own average level instead of
    /// `threshold_db`.
    pub auto_threshold: bool,
    /// `b<n>_dyn_attack`: envelope attack time constant, ms (0.1..500).
    pub attack_ms: f32,
    /// `b<n>_dyn_release`: envelope release time constant, ms (1..2000).
    pub release_ms: f32,
    /// `b<n>_dyn_sc`: detect on the external side-chain input instead of
    /// the band's own input. Falls back to the input when the host provides
    /// no side-chain.
    pub external: bool,
}

impl Default for DynSettings {
    fn default() -> Self {
        DynSettings {
            on: false,
            range_db: 0.0,
            threshold_db: -24.0,
            auto_threshold: true,
            attack_ms: 10.0,
            release_ms: 120.0,
            external: false,
        }
    }
}

/// dB above threshold at which the full range is reached (soft knee).
const KNEE_DB: f32 = 12.0;
/// Auto threshold sits this far above the region's running average.
const AUTO_OFFSET_DB: f32 = 3.0;

/// The per-band detector state. One per band, owned by the engine; the
/// band-pass that isolates the region lives in the engine's `Band`, this
/// only sees the filtered samples.
pub struct Dynamics {
    /// Peak envelope of the detector signal, linear.
    env: f32,
    /// Running average of the envelope in dB, for the auto threshold.
    avg_db: f32,
    /// The smoothed dynamic gain currently applied, dB.
    gain_db: f32,
    /// Per-sample attack coefficient (`1 − e^{−1 / (t · sr)}`).
    att: f32,
    /// Per-sample release coefficient.
    rel: f32,
    /// Sample rate the coefficients were computed for.
    sr: f32,
}

impl Default for Dynamics {
    fn default() -> Self {
        Dynamics {
            env: 0.0,
            avg_db: -90.0,
            gain_db: 0.0,
            att: 0.0,
            rel: 0.0,
            sr: 48_000.0,
        }
    }
}

impl Dynamics {
    /// Recompute the attack / release coefficients for `s` at `sr`:
    /// `1 − exp(−1 / (t · sr))`, the per-sample step of a one-pole with time
    /// constant `t` seconds. Attack is clamped to at least 0.05 ms and
    /// release to at least 1 ms.
    pub fn set(&mut self, s: &DynSettings, sr: f32) {
        self.sr = sr;
        self.att = 1.0 - (-1.0 / (s.attack_ms.max(0.05) * 1e-3 * sr)).exp();
        self.rel = 1.0 - (-1.0 / (s.release_ms.max(1.0) * 1e-3 * sr)).exp();
    }

    /// Feed one detector sample (already band-filtered). Peak follower:
    /// the envelope moves toward `|x|` with the attack coefficient when
    /// rising and the release coefficient when falling.
    #[inline]
    pub fn feed(&mut self, x: f32) {
        let a = x.abs();
        let k = if a > self.env { self.att } else { self.rel };
        self.env += (a - self.env) * k;
    }

    /// Update once per block after feeding the block's samples; returns the
    /// dynamic gain in dB (see the module docs for the formula). `block_len`
    /// sets the step of the one-second running average behind the auto
    /// threshold. Off, it returns 0 and clears the gain.
    pub fn update_block(&mut self, s: &DynSettings, block_len: usize) -> f32 {
        if !s.on {
            self.gain_db = 0.0;
            return 0.0;
        }
        let env_db = 20.0 * (self.env + 1e-7).log10();
        // Running average over roughly one second, for the auto threshold.
        let k = (block_len as f32 / self.sr).min(1.0);
        self.avg_db += (env_db - self.avg_db) * k;
        let thr = if s.auto_threshold {
            self.avg_db + AUTO_OFFSET_DB
        } else {
            s.threshold_db
        };
        let over = env_db - thr;
        let amount = (over / KNEE_DB).clamp(0.0, 1.0);
        let target = s.range_db * amount;
        // Block-rate smoothing keeps coefficient updates click-free.
        self.gain_db += (target - self.gain_db) * 0.5;
        if self.gain_db.abs() < 1e-3 {
            self.gain_db = 0.0;
        }
        self.gain_db
    }

    /// The dynamic gain returned by the last `update_block`, dB.
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// The current detector envelope in dBFS (what the `band_level` stream
    /// publishes so the page can show the trigger level).
    pub fn level_db(&self) -> f32 {
        20.0 * (self.env + 1e-7).log10()
    }

    /// Clear the envelope and gain (the running average is kept, so the
    /// auto threshold does not jump).
    pub fn reset(&mut self) {
        self.env = 0.0;
        self.gain_db = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loud_region_reaches_range_and_releases() {
        let s = DynSettings {
            on: true,
            range_db: -6.0,
            threshold_db: -20.0,
            auto_threshold: false,
            attack_ms: 1.0,
            release_ms: 20.0,
            external: false,
        };
        let mut d = Dynamics::default();
        d.set(&s, 48000.0);
        // 0 dBFS square-ish input, well above threshold + knee.
        for _ in 0..4800 {
            d.feed(1.0);
        }
        let mut g = 0.0;
        for _ in 0..20 {
            g = d.update_block(&s, 256);
        }
        assert!((g + 6.0).abs() < 0.05, "{g}");
        // Silence: gain returns to zero.
        for _ in 0..48000 {
            d.feed(0.0);
        }
        for _ in 0..40 {
            g = d.update_block(&s, 256);
        }
        assert!(g.abs() < 0.05, "{g}");
    }
}
