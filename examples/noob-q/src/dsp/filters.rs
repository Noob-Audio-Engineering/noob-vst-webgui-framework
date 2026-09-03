//! Filter shapes and slopes: the second-order sections one EQ band is made
//! of, and how a band is designed from `(kind, freq, gain, q, slope)`.
//!
//! * Bell, notch, band-pass, all-pass and single shelves are *RBJ Audio EQ
//!   Cookbook* biquads ([`Coefs::rbj`]), the standard closed-form designs
//!   from the bilinear transform of analog prototypes.
//! * Steeper shelves are `order / 2` identical shelves in series, each with
//!   `gain / n`, so a 48 dB/oct shelf is four 12 dB/oct shelves whose gains
//!   add up to the requested one.
//! * Tilt shelves are a low shelf at `−g` and a high shelf at `+g` on the
//!   same corner (`g = gain / 2`), so the two ends differ by `gain` dB and
//!   the corner sits at 0 dB. *Flat Tilt* uses one such pair at a very low
//!   Q so the response is nearly a straight line on a log-frequency axis.
//! * Cuts are Butterworth cascades: `order / 2` second-order sections with
//!   the classic pole Qs ([`butterworth_q`]) plus one first-order section
//!   for odd orders ([`Coefs::one_pole_lp`] / [`Coefs::one_pole_hp`]). The
//!   band's Q scales the Q of the most resonant section, so it shapes the
//!   knee the way Pro-Q's resonant cuts do while the asymptotic slope stays
//!   the order's. "Brickwall" is a 32nd-order Butterworth (192 dB/oct).
//!
//! Coefficients are normalized (`a0 = 1`) and applied by [`Biquad`] in
//! transposed direct form II, two channels per section. The JavaScript twin
//! of this module, `crates/vst3-web-stratum/web/components/eqcurve.js`, uses the same formulas so
//! the curve the page draws is the curve the audio gets; keep the two in
//! step when changing anything here.

use std::f32::consts::{PI, SQRT_2};

/// Largest number of second-order sections one band can need
/// (brickwall cut = order 32 = 16 sections; tilt shelf at 96 dB/oct = 2 × 8).
pub const MAX_STAGES: usize = 16;

/// The shape of a band. Discriminants match [`KIND_NAMES`] and the
/// `b<n>_shape` parameter (0 = Bell … 9 = All Pass).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// Peaking EQ: `gain` at the centre, unity far away, Q sets the width.
    #[default]
    Bell,
    /// `gain` below the corner, unity above. Uses the slope control.
    LowShelf,
    /// High-pass Butterworth cascade of the slope's order; Q shapes the knee.
    LowCut,
    /// `gain` above the corner, unity below. Uses the slope control.
    HighShelf,
    /// Low-pass Butterworth cascade of the slope's order; Q shapes the knee.
    HighCut,
    /// Infinitely deep dip at the centre; Q sets the width. No gain.
    Notch,
    /// Only the region around the centre passes; Q sets the width. No gain.
    BandPass,
    /// Lows down and highs up (or the reverse) around the corner; `gain` is
    /// the difference between the two ends. Uses the slope control.
    TiltShelf,
    /// A tilt whose response is a straight line on a log-frequency axis.
    FlatTilt,
    /// Phase shift without a gain change (an alternative to polarity flip).
    AllPass,
}

/// UI labels, indexed like [`Kind`].
pub const KIND_NAMES: [&str; 10] = [
    "Bell",
    "Low Shelf",
    "Low Cut",
    "High Shelf",
    "High Cut",
    "Notch",
    "Band Pass",
    "Tilt Shelf",
    "Flat Tilt",
    "All Pass",
];

impl Kind {
    /// Inverse of `kind as usize`; out-of-range indices give `Bell`.
    pub fn from_index(i: usize) -> Kind {
        match i {
            1 => Kind::LowShelf,
            2 => Kind::LowCut,
            3 => Kind::HighShelf,
            4 => Kind::HighCut,
            5 => Kind::Notch,
            6 => Kind::BandPass,
            7 => Kind::TiltShelf,
            8 => Kind::FlatTilt,
            9 => Kind::AllPass,
            _ => Kind::Bell,
        }
    }
    /// The gain control applies (and so can dynamics).
    pub fn has_gain(self) -> bool {
        matches!(
            self,
            Kind::Bell | Kind::LowShelf | Kind::HighShelf | Kind::TiltShelf | Kind::FlatTilt
        )
    }
    /// Low cut or high cut (Butterworth cascades; the Q shapes the knee).
    pub fn is_cut(self) -> bool {
        matches!(self, Kind::LowCut | Kind::HighCut)
    }
    /// The slope control applies (cuts, shelves and the tilt shelf).
    pub fn uses_slope(self) -> bool {
        matches!(
            self,
            Kind::LowCut | Kind::HighCut | Kind::LowShelf | Kind::HighShelf | Kind::TiltShelf
        )
    }
}

/// Slope labels for the `b<n>_slope` parameter. Each maps to a filter order
/// in [`SLOPE_ORDERS`] (order = dB/oct ÷ 6; "Brickwall" is approximated by
/// a 32nd-order Butterworth, 192 dB/oct).
pub const SLOPE_NAMES: [&str; 10] = [
    "6 dB",
    "12 dB",
    "18 dB",
    "24 dB",
    "30 dB",
    "36 dB",
    "48 dB",
    "72 dB",
    "96 dB",
    "Brickwall",
];
/// Filter order per slope index. For shelves the order is halved and
/// clamped to 1..=8 cascaded shelves, so 6 dB and 12 dB both give one.
pub const SLOPE_ORDERS: [usize; 10] = [1, 2, 3, 4, 5, 6, 8, 12, 16, 32];

/// Normalized biquad coefficients (`a0 = 1`) of
/// `H(z) = (b0 + b1 z⁻¹ + b2 z⁻²) / (1 + a1 z⁻¹ + a2 z⁻²)`.
/// A first-order section is stored with `b2 = a2 = 0`.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Coefs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

/// The RBJ cookbook prototypes [`Coefs::rbj`] can design.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rbj {
    /// Peaking EQ with gain.
    Peak,
    /// Low shelf with gain.
    LowShelf,
    /// High shelf with gain.
    HighShelf,
    /// Second-order low-pass; the gain argument is ignored.
    LowPass,
    /// Second-order high-pass; the gain argument is ignored.
    HighPass,
    /// Notch; the gain argument is ignored.
    Notch,
    /// Constant-0-dB-peak band-pass; the gain argument is ignored.
    BandPass,
    /// Second-order all-pass; the gain argument is ignored.
    AllPass,
}

impl Coefs {
    /// A section that does nothing (`y = x`).
    pub const IDENTITY: Coefs = Coefs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// RBJ *Audio EQ Cookbook* design, normalized by `a0`.
    ///
    /// With `ω0 = 2π · freq / sr`, `A = 10^(gain_dB / 40)` and
    /// `α = sin ω0 / (2 Q)`, each prototype's `b`/`a` are the cookbook's
    /// closed forms. `freq` is clamped to `1 Hz ..= 0.499 · sr` and `q` to at
    /// least `1e-3`, so any parameter value yields a stable section. The
    /// shelves use the cookbook's "shelf slope" form with `Q` as the slope
    /// parameter, which gives the familiar resonant bump above `Q ≈ 0.7`.
    pub fn rbj(kind: Rbj, freq: f32, gain_db: f32, q: f32, sr: f32) -> Coefs {
        let w0 = 2.0 * PI * freq.clamp(1.0, sr * 0.499) / sr;
        let (sn, cs) = w0.sin_cos();
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sn / (2.0 * q.max(1e-3));
        let (b0, b1, b2, a0, a1, a2) = match kind {
            Rbj::LowShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * (a + 1.0 - (a - 1.0) * cs + sq),
                    2.0 * a * (a - 1.0 - (a + 1.0) * cs),
                    a * (a + 1.0 - (a - 1.0) * cs - sq),
                    a + 1.0 + (a - 1.0) * cs + sq,
                    -2.0 * (a - 1.0 + (a + 1.0) * cs),
                    a + 1.0 + (a - 1.0) * cs - sq,
                )
            }
            Rbj::HighShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * (a + 1.0 + (a - 1.0) * cs + sq),
                    -2.0 * a * (a - 1.0 + (a + 1.0) * cs),
                    a * (a + 1.0 + (a - 1.0) * cs - sq),
                    a + 1.0 - (a - 1.0) * cs + sq,
                    2.0 * (a - 1.0 - (a + 1.0) * cs),
                    a + 1.0 - (a - 1.0) * cs - sq,
                )
            }
            Rbj::LowPass => (
                (1.0 - cs) / 2.0,
                1.0 - cs,
                (1.0 - cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            Rbj::HighPass => (
                (1.0 + cs) / 2.0,
                -(1.0 + cs),
                (1.0 + cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            Rbj::Notch => (1.0, -2.0 * cs, 1.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            Rbj::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            Rbj::AllPass => (
                1.0 - alpha,
                -2.0 * cs,
                1.0 + alpha,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            Rbj::Peak => (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            ),
        };
        Coefs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// First-order low-pass, 6 dB/oct: the bilinear transform of
    /// `1 / (1 + s)` with `k = tan(π · freq / sr)`, giving
    /// `b0 = b1 = k / (1 + k)`, `a1 = (k − 1) / (1 + k)`. −3 dB at `freq`.
    pub fn one_pole_lp(freq: f32, sr: f32) -> Coefs {
        let k = (PI * freq.clamp(1.0, sr * 0.499) / sr).tan();
        let n = 1.0 / (1.0 + k);
        Coefs {
            b0: k * n,
            b1: k * n,
            b2: 0.0,
            a1: (k - 1.0) * n,
            a2: 0.0,
        }
    }

    /// First-order high-pass, 6 dB/oct: the complement of
    /// [`one_pole_lp`](Self::one_pole_lp), `b0 = −b1 = 1 / (1 + k)`.
    pub fn one_pole_hp(freq: f32, sr: f32) -> Coefs {
        let k = (PI * freq.clamp(1.0, sr * 0.499) / sr).tan();
        let n = 1.0 / (1.0 + k);
        Coefs {
            b0: n,
            b1: -n,
            b2: 0.0,
            a1: (k - 1.0) * n,
            a2: 0.0,
        }
    }

    /// `20 · log10 |H(e^{jω})|` at `freq`, evaluated directly from the
    /// coefficients (`|b0 + b1 e^{−jω} + b2 e^{−2jω}| / |1 + a1 e^{−jω} +
    /// a2 e^{−2jω}|`). Used for the response curve, auto gain and the
    /// linear-phase target; cheap enough to call thousands of times per
    /// redesign.
    pub fn magnitude_db(&self, freq: f32, sr: f32) -> f32 {
        let w = 2.0 * PI * freq / sr;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        10.0 * ((nr * nr + ni * ni).max(1e-30) / (dr * dr + di * di).max(1e-30)).log10()
    }
}

/// A second-order section with state for two channels (left/right or
/// mid/side, whichever domain the band runs in). Transposed direct form II:
/// two state variables per channel, good numerical behaviour at low
/// frequencies, and coefficients can be swapped between samples without a
/// click as long as the change is small (the engine redesigns per block and
/// the dynamics smooth their gain).
#[derive(Clone, Copy, Default, Debug)]
pub struct Biquad {
    /// The coefficients; the engine writes them directly on redesign.
    pub c: Coefs,
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    /// Filter one sample of channel `ch` (0 or 1).
    #[inline]
    pub fn process(&mut self, ch: usize, x: f32) -> f32 {
        let y = self.c.b0 * x + self.z1[ch];
        self.z1[ch] = self.c.b1 * x - self.c.a1 * y + self.z2[ch];
        self.z2[ch] = self.c.b2 * x - self.c.a2 * y;
        y
    }
    /// Clear the state of both channels (used when a section is added to a
    /// cascade or the sample rate changes, never on a plain coefficient
    /// update).
    pub fn reset(&mut self) {
        self.z1 = [0.0; 2];
        self.z2 = [0.0; 2];
    }
}

/// Q of the `k`-th (1-based) second-order section of a Butterworth filter of
/// the given order: `1 / (2 sin((2k − 1) π / 2N))`. The sections' pole pairs
/// sit evenly on the unit circle of the analog prototype, which is what
/// gives the maximally flat pass-band; cascading them in this order (least
/// resonant first) keeps intermediate signals small.
pub fn butterworth_q(order: usize, k: usize) -> f32 {
    1.0 / (2.0 * ((2 * k - 1) as f32 * PI / (2.0 * order as f32)).sin())
}

/// Design one band. Fills `out` and returns how many sections were used
/// (at most [`MAX_STAGES`]); the caller applies `out[..n]` in series.
///
/// * `kind` — the shape; see [`Kind`] for what each one does with `gain_db`,
///   `q` and `slope`.
/// * `freq` — centre / corner in Hz.
/// * `gain_db` — the band gain (ignored by shapes without gain).
/// * `q` — the band's Q; for cuts it scales the most resonant section so the
///   knee goes from soft (`q < 0.707`) to resonant (`q > 0.707`).
/// * `slope` — index into [`SLOPE_ORDERS`]; ignored by bells, notches,
///   band-passes, all-passes and the flat tilt.
///
/// Pure function of its inputs; the engine caches the inputs and only calls
/// it when one of them changed.
pub fn design_band(
    kind: Kind,
    freq: f32,
    gain_db: f32,
    q: f32,
    slope: usize,
    sr: f32,
    out: &mut [Coefs; MAX_STAGES],
) -> usize {
    let order = SLOPE_ORDERS[slope.min(SLOPE_ORDERS.len() - 1)];
    match kind {
        Kind::Bell => {
            out[0] = Coefs::rbj(Rbj::Peak, freq, gain_db, q, sr);
            1
        }
        Kind::Notch => {
            out[0] = Coefs::rbj(Rbj::Notch, freq, 0.0, q, sr);
            1
        }
        Kind::BandPass => {
            out[0] = Coefs::rbj(Rbj::BandPass, freq, 0.0, q, sr);
            1
        }
        Kind::AllPass => {
            out[0] = Coefs::rbj(Rbj::AllPass, freq, 0.0, q, sr);
            1
        }
        Kind::LowShelf | Kind::HighShelf => {
            // Steeper shelves are cascades of gentler ones.
            let n = (order / 2).clamp(1, 8);
            let rbj = if kind == Kind::LowShelf {
                Rbj::LowShelf
            } else {
                Rbj::HighShelf
            };
            let c = Coefs::rbj(rbj, freq, gain_db / n as f32, q, sr);
            for o in out.iter_mut().take(n) {
                *o = c;
            }
            n
        }
        Kind::TiltShelf | Kind::FlatTilt => {
            let (n, q) = if kind == Kind::FlatTilt {
                (1, 0.18)
            } else {
                ((order / 2).clamp(1, 8), q)
            };
            let g = gain_db / (2.0 * n as f32);
            let lo = Coefs::rbj(Rbj::LowShelf, freq, -g, q, sr);
            let hi = Coefs::rbj(Rbj::HighShelf, freq, g, q, sr);
            for i in 0..n {
                out[2 * i] = lo;
                out[2 * i + 1] = hi;
            }
            2 * n
        }
        Kind::LowCut | Kind::HighCut => {
            let n2 = order / 2;
            let odd = order % 2;
            let rbj = if kind == Kind::LowCut {
                Rbj::HighPass
            } else {
                Rbj::LowPass
            };
            for k in 1..=n2 {
                let mut qk = butterworth_q(order, k);
                if k == n2 {
                    // The band's Q shapes the knee via the most resonant section.
                    qk = (qk * q / (SQRT_2 / 2.0)).clamp(0.05, 40.0);
                }
                out[k - 1] = Coefs::rbj(rbj, freq, 0.0, qk, sr);
            }
            if odd == 1 {
                out[n2] = if kind == Kind::LowCut {
                    Coefs::one_pole_hp(freq, sr)
                } else {
                    Coefs::one_pole_lp(freq, sr)
                };
            }
            n2 + odd
        }
    }
}

/// Magnitude of a designed band at `freq`, in dB: the sum of its sections'
/// [`Coefs::magnitude_db`] (magnitudes multiply, so dB add).
pub fn band_magnitude_db(stages: &[Coefs], freq: f32, sr: f32) -> f32 {
    stages.iter().map(|c| c.magnitude_db(freq, sr)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn design(kind: Kind, f: f32, g: f32, q: f32, slope: usize) -> Vec<Coefs> {
        let mut out = [Coefs::IDENTITY; MAX_STAGES];
        let n = design_band(kind, f, g, q, slope, 48000.0, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn every_slope_is_butterworth_at_the_corner() {
        for (i, order) in SLOPE_ORDERS.iter().enumerate() {
            let s = design(Kind::LowCut, 1000.0, 0.0, SQRT_2 / 2.0, i);
            assert_eq!(s.len(), order / 2 + order % 2);
            let corner = band_magnitude_db(&s, 1000.0, 48000.0);
            assert!((corner + 3.0).abs() < 0.15, "order {order}: {corner}");
            let per_oct =
                band_magnitude_db(&s, 100.0, 48000.0) - band_magnitude_db(&s, 50.0, 48000.0);
            assert!(
                (per_oct - 6.0 * *order as f32).abs() < 0.6,
                "order {order}: {per_oct}"
            );
        }
    }

    #[test]
    fn one_pole_is_six_db_per_octave() {
        let c = Coefs::one_pole_lp(1000.0, 48000.0);
        // Well above the corner but away from Nyquist, where the bilinear
        // transform's zero steepens the digital response.
        let d = c.magnitude_db(4000.0, 48000.0) - c.magnitude_db(8000.0, 48000.0);
        assert!(d > 5.5 && d < 7.0, "{d}");
        assert!((c.magnitude_db(1000.0, 48000.0) + 3.0).abs() < 0.1);
        let h = Coefs::one_pole_hp(1000.0, 48000.0);
        let d = h.magnitude_db(250.0, 48000.0) - h.magnitude_db(125.0, 48000.0);
        assert!(d > 5.5 && d < 6.5, "{d}");
    }

    #[test]
    fn tilt_shelf_is_antisymmetric() {
        let s = design(Kind::TiltShelf, 1000.0, 6.0, 0.7, 1);
        let lo = band_magnitude_db(&s, 50.0, 48000.0);
        let hi = band_magnitude_db(&s, 15000.0, 48000.0);
        assert!((lo + 3.0).abs() < 0.3, "{lo}");
        assert!((hi - 3.0).abs() < 0.3, "{hi}");
        assert!(band_magnitude_db(&s, 1000.0, 48000.0).abs() < 0.3);
    }

    #[test]
    fn steep_shelf_reaches_full_gain() {
        let s = design(Kind::HighShelf, 2000.0, 9.0, 0.7, 8);
        assert_eq!(s.len(), 8);
        assert!((band_magnitude_db(&s, 15000.0, 48000.0) - 9.0).abs() < 0.3);
        assert!(band_magnitude_db(&s, 100.0, 48000.0).abs() < 0.3);
    }
}
