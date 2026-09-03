//! The processing engine: 24 bands with stereo placement, per-band dynamics
//! and solo, three processing modes, and the output stage. One [`Engine`]
//! per plug-in instance; the host calls [`Engine::configure`] with the
//! current settings and then [`Engine::process_block`] once per buffer.
//!
//! ## Block order
//!
//! 1. **Detectors.** For every enabled band with dynamics, filter the
//!    detector input (`(L + R) / 2` of the input, or of the external
//!    side-chain) through the band's detector biquad, feed the envelope
//!    follower, and take the block's dynamic gain. A change of more than
//!    0.01 dB marks the band for redesign.
//! 2. **Coefficients.** Every enabled band whose `(kind, freq, gain + dynamic
//!    gain, effective Q, slope)` changed is redesigned. In linear-phase mode
//!    the staging (one or two FIR stages, L/R or M/S domain) is re-evaluated.
//! 3. **Bypass** returns early, but in linear-phase mode it still delays the
//!    audio by the mode's latency so toggling bypass does not move it in time.
//! 4. **The EQ.** With any band soloed the output becomes the sum of the
//!    soloed bands' detector band-passes. Otherwise, zero latency / natural
//!    phase run the per-sample cascade (`Engine::process_pair`), switching
//!    between the L/R and M/S domains only where consecutive bands' placements
//!    differ; linear phase rebuilds the FIRs if they are stale (at most every
//!    other block while dynamics move) and runs one or two convolver stages.
//! 5. **Output stage.** `output_gain + auto_gain` in dB, a constant-power pan
//!    (cos/sin law) in L/R or M/S, polarity, then the *Character* saturation.
//!
//! ## Modes
//!
//! * *Zero Latency* — the biquad cascade; minimum phase, no latency.
//! * *Natural Phase* — in Pro-Q this matches an analog prototype's phase
//!   more closely with extra processing. This example approximates it with
//!   the same biquad path: the response is identical and only the label
//!   differs, so treat it as a placeholder rather than a distinct DSP.
//! * *Linear Phase* — the bands' current magnitude response (static plus
//!   dynamic gain) is summed per domain on a 1024-point log grid,
//!   interpolated onto the FFT grid, and turned into a symmetric FIR by the
//!   convolver module. Bands on both channels can run in either domain; only
//!   Left/Right-only bands force the L/R domain and Mid/Side-only bands the
//!   M/S domain. If both kinds exist the engine runs two stages in series
//!   (L/R first, then M/S) and the latency doubles ([`Engine::latency`]).
//!
//! ## Auto gain and the curve
//!
//! Auto gain is the negative mean of the static response over 64 log-spaced
//! points from 20 Hz to 20 kHz, recomputed whenever the static response
//! changes. [`Engine::response_db`] and [`Engine::curve`] include it, so the
//! curve the page draws is what the listener hears at unity output gain.
//! `configure` returns `true` when the static response changed, which is
//! the hosts' cue to republish the `curve` stream.
//!
//! ## Real-time behaviour
//!
//! No allocation after [`Engine::new`]. `configure` compares settings by
//! value and touches a few coefficients; a biquad redesign per band is a
//! handful of transcendental calls; the linear-phase redesign is the one
//! costly step (see the convolver module). A sample-rate change resets all
//! state and forces every redesign.

use super::convolver::{Convolver, Delay, FirDesigner, MAX_TAPS, PARTITION, QUALITY_TAPS};
use super::dynamics::{DynSettings, Dynamics};
use super::filters::{Biquad, Coefs, Kind, MAX_STAGES, Rbj, band_magnitude_db, design_band};

/// Number of bands (Pro-Q 4 has 24). Sizes the `band_dyn` / `band_level`
/// streams and the `b1_*` … `b24_*` parameter groups.
pub const BANDS: usize = 24;

/// Labels for the `b<n>_place` parameter, indexed like [`Placement`].
pub const PLACEMENT_NAMES: [&str; 5] = ["Stereo", "Left", "Right", "Mid", "Side"];

/// Which channels a band processes. Discriminants match [`PLACEMENT_NAMES`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Placement {
    /// Both channels, identical filters.
    #[default]
    Stereo,
    /// Left only.
    Left,
    /// Right only.
    Right,
    /// The mid signal `(L + R) / 2` only.
    Mid,
    /// The side signal `(L − R) / 2` only.
    Side,
}

impl Placement {
    /// Inverse of `placement as usize`; out-of-range indices give `Stereo`.
    pub fn from_index(i: usize) -> Placement {
        match i {
            1 => Placement::Left,
            2 => Placement::Right,
            3 => Placement::Mid,
            4 => Placement::Side,
            _ => Placement::Stereo,
        }
    }
    /// Mid or side: the band runs in the M/S domain.
    pub fn is_ms(self) -> bool {
        matches!(self, Placement::Mid | Placement::Side)
    }
}

/// Labels for the `processing_mode` parameter, indexed like [`Mode`].
pub const MODE_NAMES: [&str; 3] = ["Zero Latency", "Natural Phase", "Linear Phase"];

/// Processing mode; see the module docs for what each one does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// IIR biquad cascade, minimum phase, no latency.
    #[default]
    ZeroLatency,
    /// Approximated by the zero-latency IIR path in this example.
    NaturalPhase,
    /// FIR convolution; `PARTITION + taps / 2` samples of latency per stage.
    LinearPhase,
}

impl Mode {
    /// Inverse of `mode as usize`; out-of-range indices give `ZeroLatency`.
    pub fn from_index(i: usize) -> Mode {
        match i {
            1 => Mode::NaturalPhase,
            2 => Mode::LinearPhase,
            _ => Mode::ZeroLatency,
        }
    }
}

/// Everything the engine needs to know about one band; built from the
/// `b<n>_*` parameters by both hosts and compared by value in
/// [`Engine::configure`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BandSettings {
    /// `b<n>_on`. Off bands cost nothing.
    pub on: bool,
    /// `b<n>_shape`.
    pub kind: Kind,
    /// `b<n>_freq`, Hz.
    pub freq: f32,
    /// `b<n>_gain`, dB; ignored by shapes without gain.
    pub gain_db: f32,
    /// `b<n>_q`.
    pub q: f32,
    /// `b<n>_slope`, an index into `filters::SLOPE_ORDERS`.
    pub slope: usize,
    /// `b<n>_place`.
    pub placement: Placement,
    /// `b<n>_solo`; with any band soloed the output is the soloed regions.
    pub solo: bool,
    /// The `b<n>_dyn_*` parameters.
    pub dynamics: DynSettings,
}

impl Default for BandSettings {
    fn default() -> Self {
        BandSettings {
            on: false,
            kind: Kind::Bell,
            freq: 1000.0,
            gain_db: 0.0,
            q: 1.0,
            slope: 1,
            placement: Placement::Stereo,
            solo: false,
            dynamics: DynSettings::default(),
        }
    }
}

/// Labels for the `pan_mode` parameter (index 1 = pan between mid and side).
pub const PAN_MODE_NAMES: [&str; 2] = ["L/R", "M/S"];
/// Labels for the `character` parameter, indexed like the `mode` argument of
/// the saturation stage.
pub const CHARACTER_NAMES: [&str; 3] = ["Clean", "Subtle", "Warm"];

/// The global (non-band) settings, built from the `global` parameter group
/// by both hosts and compared by value in [`Engine::configure`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Globals {
    /// `bypass`: skip the EQ and the output stage. Linear phase keeps its
    /// latency while bypassed.
    pub bypass: bool,
    /// `output_gain`, dB.
    pub output_gain_db: f32,
    /// `gain_scale` as 0..2 (0 % .. 200 %); scales every band's static gain,
    /// so a whole curve can be exaggerated or tamed at once.
    pub gain_scale: f32,
    /// `auto_gain`: add the negative mean of the static response so the
    /// perceived level stays put while shaping.
    pub auto_gain: bool,
    /// `output_pan` as −1 (left / mid) .. 1 (right / side).
    pub pan: f32,
    /// `pan_mode`: pan between mid and side instead of left and right.
    pub pan_ms: bool,
    /// `phase_invert`: flip the polarity of the output.
    pub phase_invert: bool,
    /// `processing_mode`.
    pub mode: Mode,
    /// `lp_quality`: index into the linear-phase quality table
    /// (`convolver::QUALITY_TAPS`).
    pub quality: usize,
    /// `character`: 0 = clean, 1 = subtle, 2 = warm saturation after the EQ.
    pub character: usize,
    /// `gain_q`: analog-console style, a Bell's Q narrows as its gain grows.
    pub gain_q: bool,
}

impl Default for Globals {
    fn default() -> Self {
        Globals {
            bypass: false,
            output_gain_db: 0.0,
            gain_scale: 1.0,
            auto_gain: false,
            pan: 0.0,
            pan_ms: false,
            phase_invert: false,
            mode: Mode::ZeroLatency,
            quality: 2,
            character: 0,
            gain_q: false,
        }
    }
}

/// Effective Q with the optional gain-Q interaction (Bell only):
/// `q · (1 + |gain| / 30)`, so a ±30 dB bell is twice as narrow as a 0 dB
/// one, the way many analog console EQs behave (Pro-Q's "Gain-Q
/// Interaction"). Used by the engine and mirrored by the page's curve
/// renderer so the drawn bell matches.
#[inline]
pub fn effective_q(kind: Kind, q: f32, gain_db: f32, gain_q: bool) -> f32 {
    if gain_q && kind == Kind::Bell {
        q * (1.0 + gain_db.abs() / 30.0)
    } else {
        q
    }
}

/// Character saturation. `x` is the sample, `mode` indexes
/// [`CHARACTER_NAMES`]: 0 clean (identity); 1 subtle, `tanh(1.6 x) / 1.6`,
/// unity for small signals and about 4.8 dB of compression at 0 dBFS;
/// 2 warm, `tanh(2.5 y) / 2.5` with `y = x + 0.08 x |x|`, whose small
/// asymmetric term adds even harmonics, about 8 dB of compression at
/// 0 dBFS. Applied after the output gain, so the gain drives it.
#[inline]
fn saturate(x: f32, mode: usize) -> f32 {
    match mode {
        1 => {
            let d = 1.6;
            (d * x).tanh() / d
        }
        2 => {
            let d = 2.5;
            let bent = x + 0.08 * x * x.abs();
            (d * bent).tanh() / d
        }
        _ => x,
    }
}

/// One band's run-time state: settings, the filter cascade, the detector
/// filters, and the dynamics.
struct Band {
    /// Current settings, compared by value in `configure`.
    s: BandSettings,
    /// The cascade; only `stages[..n]` are live.
    stages: [Biquad; MAX_STAGES],
    /// Live section count from the last design.
    n: usize,
    /// Inputs of the last design `(kind, freq, gain, q, slope)`; `None`
    /// forces a redesign.
    designed: Option<(Kind, f32, f32, f32, usize)>,
    /// Region filter for solo listening (stereo state).
    det_solo: Biquad,
    /// Same coefficients, separate state, for the dynamics detector (mono).
    det_dyn: Biquad,
    /// Inputs of the last detector design `(kind, freq, q)`.
    det_key: Option<(Kind, f32, f32)>,
    /// Envelope follower and gain computer.
    dynamics: Dynamics,
    /// Settings the dynamics coefficients were computed for.
    dyn_key: Option<DynSettings>,
    /// Dynamic gain baked into the current design, dB.
    dyn_gain_db: f32,
}

impl Band {
    /// An off band with cleared state.
    fn new() -> Self {
        Band {
            s: BandSettings::default(),
            stages: [Biquad::default(); MAX_STAGES],
            n: 0,
            designed: None,
            det_solo: Biquad::default(),
            det_dyn: Biquad::default(),
            det_key: None,
            dynamics: Dynamics::default(),
            dyn_key: None,
            dyn_gain_db: 0.0,
        }
    }

    /// Gain the filter is designed with: scaled static gain plus dynamics.
    fn effective_gain(&self, scale: f32) -> f32 {
        if self.s.kind.has_gain() {
            self.s.gain_db * scale + self.dyn_gain_db
        } else {
            0.0
        }
    }

    /// (Re)design the filter if anything relevant changed (kind, frequency,
    /// scaled static gain plus dynamic gain, effective Q, slope). Returns
    /// `true` when it did. Sections that were already live keep their state
    /// so a coefficient update is click-free; sections added by a steeper
    /// slope start from silence.
    fn ensure_designed(&mut self, g: &Globals, sr: f32) -> bool {
        let gain = self.effective_gain(g.gain_scale);
        let q = effective_q(self.s.kind, self.s.q, gain, g.gain_q);
        let key = (self.s.kind, self.s.freq, gain, q, self.s.slope);
        if self.designed == Some(key) {
            return false;
        }
        let mut coefs = [Coefs::IDENTITY; MAX_STAGES];
        let n = design_band(
            self.s.kind,
            self.s.freq,
            gain,
            q,
            self.s.slope,
            sr,
            &mut coefs,
        );
        for (st, c) in self.stages.iter_mut().zip(coefs.iter()) {
            st.c = *c;
        }
        // Fresh sections must not carry stale state.
        for st in self
            .stages
            .iter_mut()
            .skip(self.n.min(n))
            .take(n.saturating_sub(self.n))
        {
            st.reset();
        }
        self.n = n;
        self.designed = Some(key);
        true
    }

    /// (Re)design the detector filter that isolates the band's region: a
    /// low-pass for low shelves and high cuts, a high-pass for high shelves
    /// and low cuts (the region those shapes act on), and a band-pass at the
    /// band's Q (at least 0.3) for everything else. Solo listening and the
    /// dynamics detector share the coefficients.
    fn ensure_detector(&mut self, sr: f32) {
        let key = (self.s.kind, self.s.freq, self.s.q);
        if self.det_key == Some(key) {
            return;
        }
        let c = match self.s.kind {
            Kind::LowShelf | Kind::HighCut => Coefs::rbj(Rbj::LowPass, self.s.freq, 0.0, 0.707, sr),
            Kind::HighShelf | Kind::LowCut => {
                Coefs::rbj(Rbj::HighPass, self.s.freq, 0.0, 0.707, sr)
            }
            _ => Coefs::rbj(Rbj::BandPass, self.s.freq, 0.0, self.s.q.max(0.3), sr),
        };
        self.det_solo.c = c;
        self.det_dyn.c = c;
        self.det_key = Some(key);
    }

    /// Run one sample of channel `ch` through the live cascade.
    #[inline]
    fn process(&mut self, ch: usize, mut x: f32) -> f32 {
        for st in &mut self.stages[..self.n] {
            x = st.process(ch, x);
        }
        x
    }

    /// Static magnitude (no dynamics) at `freq`, with gain scale applied.
    fn static_db(&self, freq: f32, g: &Globals, sr: f32) -> f32 {
        let gain = if self.s.kind.has_gain() {
            self.s.gain_db * g.gain_scale
        } else {
            0.0
        };
        let q = effective_q(self.s.kind, self.s.q, gain, g.gain_q);
        let mut coefs = [Coefs::IDENTITY; MAX_STAGES];
        let n = design_band(
            self.s.kind,
            self.s.freq,
            gain,
            q,
            self.s.slope,
            sr,
            &mut coefs,
        );
        band_magnitude_db(&coefs[..n], freq, sr)
    }

    /// Magnitude of the cascade as currently designed (dynamics included),
    /// dB. Feeds the linear-phase target.
    fn current_db(&self, freq: f32, sr: f32) -> f32 {
        self.stages[..self.n]
            .iter()
            .map(|st| st.c.magnitude_db(freq, sr))
            .sum()
    }
}

/// Points of the log-frequency grid the linear-phase designer samples the
/// response on before interpolating to the FFT grid.
const GRID: usize = 1024;

/// The EQ. See the module docs for the block order and the modes.
pub struct Engine {
    sr: f32,
    bands: Vec<Band>,
    /// Current globals.
    g: Globals,
    /// Compensation from auto gain, dB (0 when off).
    auto_gain_db: f32,
    /// Some enabled band is soloed.
    any_solo: bool,
    /// Force a curve / auto-gain update on the next `configure`.
    static_dirty: bool,
    /// FIR designer shared by all domains.
    fir: FirDesigner,
    /// The FIR being designed (`MAX_TAPS` samples).
    fir_buf: Vec<f32>,
    /// Target response on the log grid, dB.
    grid_db: Vec<f32>,
    /// Convolvers: 0 left, 1 right, 2 mid, 3 side.
    conv: Vec<Convolver>,
    /// Bypass delay lines (linear phase keeps its latency while bypassed).
    delay: [Delay; 2],
    /// The FIRs no longer match the bands.
    lp_dirty: bool,
    /// Both L/R-specific and M/S-specific bands exist: two stages.
    lp_two_stage: bool,
    /// Single stage running in the mid/side domain.
    lp_ms_domain: bool,
    /// FIR length for the current quality.
    lp_taps: usize,
    /// Blocks processed; paces linear-phase redesigns.
    blocks: u64,
}

impl Engine {
    /// Allocate everything for `sr`: 24 bands, the FIR designer, four
    /// convolvers sized for [`MAX_TAPS`], and bypass delays long enough for
    /// two stages at maximum quality. Starts with every band off.
    pub fn new(sr: f32) -> Self {
        let lp_max_latency = 2 * (PARTITION + MAX_TAPS / 2);
        Engine {
            sr,
            bands: (0..BANDS).map(|_| Band::new()).collect(),
            g: Globals::default(),
            auto_gain_db: 0.0,
            any_solo: false,
            static_dirty: true,
            fir: FirDesigner::new(),
            fir_buf: vec![0.0; MAX_TAPS],
            grid_db: vec![0.0; GRID],
            conv: (0..4).map(|_| Convolver::new(MAX_TAPS)).collect(),
            delay: [
                Delay::new(lp_max_latency + 1),
                Delay::new(lp_max_latency + 1),
            ],
            lp_dirty: true,
            lp_two_stage: false,
            lp_ms_domain: false,
            lp_taps: QUALITY_TAPS[2],
            blocks: 0,
        }
    }

    /// The sample rate the engine is designed for.
    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Change the sample rate: clears every filter, detector, convolver and
    /// delay, and forces every design to be redone on the next block. Call
    /// from the host's `initialize` / `reset`, never mid-stream.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        for b in &mut self.bands {
            b.designed = None;
            b.det_key = None;
            b.dyn_key = None;
            for st in &mut b.stages {
                st.reset();
            }
            b.det_solo.reset();
            b.det_dyn.reset();
            b.dynamics.reset();
        }
        for c in &mut self.conv {
            c.reset();
        }
        for d in &mut self.delay {
            d.reset();
        }
        self.static_dirty = true;
        self.lp_dirty = true;
    }

    /// Push settings; the hosts call this before every block and it is
    /// cheap when nothing changed. Per band, a change of the static settings
    /// marks the response dirty; a change of the dynamics settings recomputes
    /// the envelope coefficients and, when dynamics were switched off, clears
    /// the dynamic gain. A change of mode or quality resets the convolvers.
    /// Auto gain is recomputed whenever the static response changed.
    ///
    /// Returns `true` when the static response changed, which is the host's
    /// cue to republish the `curve` stream.
    pub fn configure(&mut self, bands: &[BandSettings], g: Globals) -> bool {
        let mut changed = false;
        for (b, s) in self.bands.iter_mut().zip(bands) {
            if b.s != *s {
                let dyn_changed = b.s.dynamics != s.dynamics;
                let static_changed = BandSettings {
                    dynamics: b.s.dynamics,
                    solo: b.s.solo,
                    ..b.s
                } != BandSettings {
                    dynamics: s.dynamics,
                    solo: s.solo,
                    ..*s
                };
                b.s = *s;
                if dyn_changed {
                    b.dynamics.set(&s.dynamics, self.sr);
                    b.dyn_key = Some(s.dynamics);
                    if !s.dynamics.on {
                        b.dyn_gain_db = 0.0;
                        b.dynamics.reset();
                    }
                }
                changed |= static_changed;
            }
            if b.dyn_key.is_none() {
                b.dynamics.set(&s.dynamics, self.sr);
                b.dyn_key = Some(s.dynamics);
            }
        }
        if g != self.g {
            let mode_changed = g.mode != self.g.mode || g.quality != self.g.quality;
            changed |= g.gain_scale != self.g.gain_scale
                || g.auto_gain != self.g.auto_gain
                || g.bypass != self.g.bypass;
            self.g = g;
            if mode_changed {
                self.lp_taps = QUALITY_TAPS[g.quality.min(QUALITY_TAPS.len() - 1)];
                for c in &mut self.conv {
                    c.reset();
                }
                self.lp_dirty = true;
            }
        }
        self.any_solo = self.bands.iter().any(|b| b.s.on && b.s.solo);
        if changed || self.static_dirty {
            self.static_dirty = false;
            self.lp_dirty = true;
            self.update_auto_gain();
            return true;
        }
        false
    }

    /// Auto gain = −(mean static response over 64 log-spaced points from
    /// 20 Hz to 20 kHz). Cheap enough to redo on every static change.
    fn update_auto_gain(&mut self) {
        if !self.g.auto_gain {
            self.auto_gain_db = 0.0;
            return;
        }
        // Mean of the static response over the audible range, log-spaced.
        let n = 64;
        let mut sum = 0.0;
        for i in 0..n {
            let f = 20.0 * (20_000.0f32 / 20.0).powf(i as f32 / (n - 1) as f32);
            sum += self.static_sum_db(f);
        }
        self.auto_gain_db = -(sum / n as f32);
    }

    /// Sum of every enabled band's static magnitude at `f`, dB (frequency
    /// clamped below Nyquist).
    fn static_sum_db(&self, f: f32) -> f32 {
        let f = f.min(self.sr * 0.499);
        self.bands
            .iter()
            .filter(|b| b.s.on)
            .map(|b| b.static_db(f, &self.g, self.sr))
            .sum()
    }

    /// Static composite response in dB (gain scale and auto gain included,
    /// output gain excluded, dynamics excluded, placement ignored).
    pub fn response_db(&self, freq: f32) -> f32 {
        if self.g.bypass {
            return 0.0;
        }
        self.static_sum_db(freq) + self.auto_gain_db
    }

    /// Fill `out` with [`response_db`](Self::response_db) at `out.len()`
    /// log-spaced frequencies from `min_hz` to `max_hz` inclusive. This is
    /// the `curve` stream; the hosts call it with `CURVE_POINTS` /
    /// `CURVE_MIN_HZ` / `CURVE_MAX_HZ` whenever `configure` reported a
    /// change. Costs `points × sections` magnitude evaluations, so it is
    /// meant for on-change, not per-block, use.
    pub fn curve(&self, out: &mut [f32], min_hz: f32, max_hz: f32) {
        let n = out.len().max(2);
        let ratio = max_hz / min_hz;
        for (i, o) in out.iter_mut().enumerate() {
            let f = min_hz * ratio.powf(i as f32 / (n - 1) as f32);
            *o = self.response_db(f);
        }
    }

    /// Current dynamic gain per band, dB (the `band_dyn` stream; 0 for bands
    /// without dynamics).
    pub fn band_dyn_gains(&self, out: &mut [f32]) {
        for (o, b) in out.iter_mut().zip(&self.bands) {
            *o = b.dyn_gain_db;
        }
    }

    /// Current detector level per band, dBFS (the `band_level` stream;
    /// −120 for bands whose dynamics are off).
    pub fn band_levels(&self, out: &mut [f32]) {
        for (o, b) in out.iter_mut().zip(&self.bands) {
            *o = if b.s.on && b.s.dynamics.on {
                b.dynamics.level_db()
            } else {
                -120.0
            };
        }
    }

    /// The auto-gain compensation currently applied, dB (0 when off).
    pub fn auto_gain_db(&self) -> f32 {
        self.auto_gain_db
    }

    /// Samples of latency for the current mode: 0 for the IIR modes;
    /// `PARTITION + taps / 2` per linear-phase stage, one stage normally, two
    /// when both L/R-specific and M/S-specific bands exist. The hosts report
    /// it whenever it changes.
    pub fn latency(&self) -> usize {
        match self.g.mode {
            Mode::LinearPhase => {
                let stages = if self.lp_two_stage { 2 } else { 1 };
                stages * (PARTITION + self.lp_taps / 2)
            }
            _ => 0,
        }
    }

    /// Whether dynamic EQ is active in the current mode / quality: always,
    /// except in linear phase at *Very High* and *Maximum* quality, where a
    /// per-block FIR redesign would be too costly. The page shows a warning
    /// in that case.
    pub fn dynamics_allowed(&self) -> bool {
        !(self.g.mode == Mode::LinearPhase
            && self.g.quality >= super::convolver::QUALITY_DYNAMICS_LIMIT)
    }

    /// The current processing mode.
    pub fn mode(&self) -> Mode {
        self.g.mode
    }

    /// Process one block in place; the steps are described in the module
    /// docs. `sc` is the optional external side-chain (both channels, at
    /// least as long as the block); bands with `dyn_sc` fall back to the
    /// input when it is `None`. `l` and `r` may differ in length, the
    /// shorter wins. A mono host passes a copy of its channel as `r`.
    pub fn process_block(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let len = l.len().min(r.len());
        let sr = self.sr;
        let g = self.g;
        let dyn_allowed = self.dynamics_allowed();
        self.blocks += 1;

        // 1. Detectors and dynamic gains.
        let mut dyn_changed = false;
        for b in self.bands.iter_mut().filter(|b| b.s.on) {
            b.ensure_detector(sr);
            if !(b.s.dynamics.on && b.s.kind.has_gain() && dyn_allowed) {
                if b.dyn_gain_db != 0.0 {
                    b.dyn_gain_db = 0.0;
                    dyn_changed = true;
                }
                continue;
            }
            match (b.s.dynamics.external, sc) {
                (true, Some((sl, sr_))) => {
                    for i in 0..len {
                        let x = b.det_dyn.process(0, 0.5 * (sl[i] + sr_[i]));
                        b.dynamics.feed(x);
                    }
                }
                _ => {
                    for i in 0..len {
                        let x = b.det_dyn.process(0, 0.5 * (l[i] + r[i]));
                        b.dynamics.feed(x);
                    }
                }
            }
            let g = b.dynamics.update_block(&b.s.dynamics, len);
            if (g - b.dyn_gain_db).abs() > 0.01 {
                b.dyn_gain_db = g;
                dyn_changed = true;
            }
        }

        // 2. Coefficients.
        let mut designed_any = false;
        for b in self.bands.iter_mut().filter(|b| b.s.on) {
            designed_any |= b.ensure_designed(&g, sr);
        }
        if designed_any && dyn_changed {
            self.lp_dirty = true;
        }
        if g.mode == Mode::LinearPhase {
            self.update_lp_staging();
        }

        // 3. Bypass keeps the mode's latency.
        if g.bypass {
            if g.mode == Mode::LinearPhase {
                let lat = self.latency();
                self.delay[0].set_delay(lat);
                self.delay[1].set_delay(lat);
                for i in 0..len {
                    l[i] = self.delay[0].process(l[i]);
                    r[i] = self.delay[1].process(r[i]);
                }
            }
            return;
        }

        // 4. The EQ itself.
        if self.any_solo {
            for i in 0..len {
                let (mut sl, mut sr_) = (0.0, 0.0);
                for b in self.bands.iter_mut().filter(|b| b.s.on && b.s.solo) {
                    sl += b.det_solo.process(0, l[i]);
                    sr_ += b.det_solo.process(1, r[i]);
                }
                l[i] = sl;
                r[i] = sr_;
            }
        } else {
            match g.mode {
                Mode::ZeroLatency | Mode::NaturalPhase => {
                    for i in 0..len {
                        let (yl, yr) = self.process_pair(l[i], r[i]);
                        l[i] = yl;
                        r[i] = yr;
                    }
                }
                Mode::LinearPhase => {
                    if self.lp_dirty && (self.blocks.is_multiple_of(2) || !dyn_changed) {
                        self.redesign_linear_phase();
                    }
                    if self.lp_two_stage {
                        for i in 0..len {
                            let yl = self.conv[0].process(l[i]);
                            let yr = self.conv[1].process(r[i]);
                            let m = self.conv[2].process(0.5 * (yl + yr));
                            let s = self.conv[3].process(0.5 * (yl - yr));
                            l[i] = m + s;
                            r[i] = m - s;
                        }
                    } else if self.lp_ms_domain {
                        for i in 0..len {
                            let m = self.conv[2].process(0.5 * (l[i] + r[i]));
                            let s = self.conv[3].process(0.5 * (l[i] - r[i]));
                            l[i] = m + s;
                            r[i] = m - s;
                        }
                    } else {
                        for i in 0..len {
                            l[i] = self.conv[0].process(l[i]);
                            r[i] = self.conv[1].process(r[i]);
                        }
                    }
                }
            }
        }

        // 5. Output stage: gain, pan (L/R or M/S), polarity, character.
        let gain = 10f32.powf((g.output_gain_db + self.auto_gain_db) / 20.0);
        let theta = (g.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let (pa, pb) = (
            theta.cos() * std::f32::consts::SQRT_2,
            theta.sin() * std::f32::consts::SQRT_2,
        );
        let sign = if g.phase_invert { -1.0 } else { 1.0 };
        let character = g.character;
        if g.pan_ms {
            let (gm, gs) = (gain * pa * sign, gain * pb * sign);
            for i in 0..len {
                let m = 0.5 * (l[i] + r[i]) * gm;
                let s = 0.5 * (l[i] - r[i]) * gs;
                l[i] = saturate(m + s, character);
                r[i] = saturate(m - s, character);
            }
        } else {
            let (gl, gr) = (gain * pa * sign, gain * pb * sign);
            for i in 0..len {
                l[i] = saturate(l[i] * gl, character);
                r[i] = saturate(r[i] * gr, character);
            }
        }
    }

    /// Decide how many linear-phase stages the current placements need and
    /// which domain a single stage runs in. Left/Right-only bands need the
    /// L/R domain, Mid/Side-only bands the M/S domain, both kinds together
    /// need two stages. A staging change resets the convolvers (the delay
    /// lines would otherwise carry samples from the other domain).
    fn update_lp_staging(&mut self) {
        let has_lr_only = self
            .bands
            .iter()
            .any(|b| b.s.on && matches!(b.s.placement, Placement::Left | Placement::Right));
        let has_ms = self.bands.iter().any(|b| b.s.on && b.s.placement.is_ms());
        let two_stage = has_lr_only && has_ms;
        let ms_domain = !has_lr_only && has_ms;
        if two_stage != self.lp_two_stage || ms_domain != self.lp_ms_domain {
            self.lp_two_stage = two_stage;
            self.lp_ms_domain = ms_domain;
            self.lp_dirty = true;
            for c in &mut self.conv {
                c.reset();
            }
        }
    }

    /// Minimum-phase chain with per-band stereo placement, one stereo
    /// sample at a time. Bands run in band order; the pair is converted to
    /// mid/side before the first M/S band and back to left/right before the
    /// next L/R band (or at the end), so a run of same-domain bands costs no
    /// conversions. `Stereo` bands process whichever pair is current, which
    /// is equivalent in both domains because both channels get the same
    /// filter.
    #[inline]
    fn process_pair(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (mut a, mut b) = (l, r);
        let mut ms = false;
        for band in self.bands.iter_mut().filter(|b| b.s.on) {
            let want_ms = band.s.placement.is_ms();
            if want_ms != ms {
                if want_ms {
                    let (m, s) = (0.5 * (a + b), 0.5 * (a - b));
                    a = m;
                    b = s;
                } else {
                    let (m, s) = (a, b);
                    a = m + s;
                    b = m - s;
                }
                ms = want_ms;
            }
            match band.s.placement {
                Placement::Stereo => {
                    a = band.process(0, a);
                    b = band.process(1, b);
                }
                Placement::Left | Placement::Mid => a = band.process(0, a),
                Placement::Right | Placement::Side => b = band.process(1, b),
            }
        }
        if ms { (a + b, a - b) } else { (a, b) }
    }

    /// Rebuild the linear-phase FIRs from the current (static + dynamic)
    /// band gains. For each convolver domain in use, the member bands'
    /// magnitudes are summed on a 1024-point log grid from 5 Hz to Nyquist
    /// (cheap: 1024 × sections evaluations), the FIR designer samples that
    /// grid with linear interpolation on its own linear-frequency grid, and
    /// the resulting impulse is loaded into the convolver. Cost is dominated
    /// by the inverse FFT of `lp_taps` points per domain.
    fn redesign_linear_phase(&mut self) {
        self.lp_dirty = false;
        let sr = self.sr;
        let taps = self.lp_taps;

        // Which bands feed which convolver. A band on both channels is the
        // same filter whether applied in L/R or in M/S, so it can join
        // whichever domain the single stage runs in.
        let domains: [&[Placement]; 4] = if self.lp_two_stage {
            [
                &[Placement::Stereo, Placement::Left],
                &[Placement::Stereo, Placement::Right],
                &[Placement::Mid],
                &[Placement::Side],
            ]
        } else if self.lp_ms_domain {
            [
                &[],
                &[],
                &[Placement::Stereo, Placement::Mid],
                &[Placement::Stereo, Placement::Side],
            ]
        } else {
            [
                &[Placement::Stereo, Placement::Left],
                &[Placement::Stereo, Placement::Right],
                &[],
                &[],
            ]
        };
        let fmin = 5.0f32;
        let fmax = sr * 0.5;
        let lratio = (fmax / fmin).ln();
        for (d, members) in domains.iter().enumerate() {
            if members.is_empty() {
                continue;
            }
            for (i, g) in self.grid_db.iter_mut().enumerate() {
                let f = fmin * (lratio * i as f32 / (GRID - 1) as f32).exp();
                *g = self
                    .bands
                    .iter()
                    .filter(|b| b.s.on && members.contains(&b.s.placement))
                    .map(|b| b.current_db(f, sr))
                    .sum();
            }
            let grid = &self.grid_db;
            let db_at = |f: f32| -> f32 {
                let x = ((f.max(fmin) / fmin).ln() / lratio * (GRID - 1) as f32)
                    .clamp(0.0, (GRID - 1) as f32);
                let i = (x as usize).min(GRID - 2);
                let t = x - i as f32;
                grid[i] + (grid[i + 1] - grid[i]) * t
            };
            self.fir.design(sr, &mut self.fir_buf[..taps], db_at);
            self.conv[d].set_impulse(&self.fir_buf[..taps]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn bell(freq: f32, gain: f32, q: f32) -> BandSettings {
        BandSettings {
            on: true,
            kind: Kind::Bell,
            freq,
            gain_db: gain,
            q,
            ..BandSettings::default()
        }
    }

    fn sine_rms(e: &mut Engine, f: f32, n: usize) -> f32 {
        let mut l: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * f * i as f32 / 48000.0).sin())
            .collect();
        let mut r = l.clone();
        for chunk in 0..(n / 256) {
            let s = chunk * 256;
            let (ls, rs) = (&mut l[s..s + 256], &mut r[s..s + 256]);
            e.process_block(ls, rs, None);
        }
        let tail = &l[n / 2..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt() * SQRT_2_F
    }
    const SQRT_2_F: f32 = std::f32::consts::SQRT_2;

    #[test]
    fn zero_latency_bell_boosts_its_center() {
        let mut e = Engine::new(48000.0);
        let mut bands = [BandSettings::default(); BANDS];
        bands[0] = bell(1000.0, 6.0, 1.0);
        e.configure(&bands, Globals::default());
        let amp = sine_rms(&mut e, 1000.0, 48000);
        assert!((20.0 * amp.log10() - 6.0).abs() < 0.3, "{amp}");
        assert_eq!(e.latency(), 0);
    }

    #[test]
    fn mid_side_placement_only_touches_its_channel() {
        let mut e = Engine::new(48000.0);
        let mut bands = [BandSettings::default(); BANDS];
        bands[0] = BandSettings {
            placement: Placement::Side,
            ..bell(1000.0, 12.0, 1.0)
        };
        e.configure(&bands, Globals::default());
        // Identical L and R => no side signal => the band does nothing.
        let amp = sine_rms(&mut e, 1000.0, 48000);
        assert!(20.0 * amp.log10() < 0.2, "{amp}");
    }

    #[test]
    fn linear_phase_matches_static_response_and_reports_latency() {
        let mut e = Engine::new(48000.0);
        let mut bands = [BandSettings::default(); BANDS];
        bands[0] = bell(1000.0, 6.0, 1.0);
        let g = Globals {
            mode: Mode::LinearPhase,
            quality: 1,
            ..Globals::default()
        };
        e.configure(&bands, g);
        assert_eq!(e.latency(), PARTITION + QUALITY_TAPS[1] / 2);
        let amp = sine_rms(&mut e, 1000.0, 96000);
        assert!(
            (20.0 * amp.log10() - 6.0).abs() < 0.4,
            "{}",
            20.0 * amp.log10()
        );
        let amp = sine_rms(&mut e, 200.0, 96000);
        assert!(20.0 * amp.log10() < 0.5, "{}", 20.0 * amp.log10());
    }

    #[test]
    fn auto_gain_centres_the_response() {
        let mut e = Engine::new(48000.0);
        let mut bands = [BandSettings::default(); BANDS];
        bands[0] = BandSettings {
            kind: Kind::FlatTilt,
            ..bell(1000.0, 0.0, 1.0)
        };
        bands[1] = BandSettings {
            kind: Kind::HighShelf,
            ..bell(100.0, 6.0, 0.7)
        };
        let g = Globals {
            auto_gain: true,
            ..Globals::default()
        };
        e.configure(&bands, g);
        assert!(
            e.auto_gain_db() < -3.0 && e.auto_gain_db() > -6.5,
            "{}",
            e.auto_gain_db()
        );
    }

    #[test]
    fn solo_isolates_the_band_region() {
        let mut e = Engine::new(48000.0);
        let mut bands = [BandSettings::default(); BANDS];
        bands[0] = BandSettings {
            solo: true,
            ..bell(1000.0, 0.0, 4.0)
        };
        e.configure(&bands, Globals::default());
        let inside = sine_rms(&mut e, 1000.0, 48000);
        let outside = sine_rms(&mut e, 8000.0, 48000);
        assert!(inside > 0.9, "{inside}");
        assert!(outside < 0.1, "{outside}");
    }
}
