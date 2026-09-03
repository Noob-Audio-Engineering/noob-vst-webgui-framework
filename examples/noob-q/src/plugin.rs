//! The nih-plug plug-in: VST3 + CLAP, stereo (or mono) with a stereo
//! side-chain input, 24 bands. Its editor is the OS web view showing the
//! Vue SPA from `web/dist`, embedded in the binary at compile time.
//!
//! ## How the pieces connect
//!
//! * [`NoobQParams`] owns every host-visible parameter as a nih-plug param.
//!   Its `Params` impl is written by hand so the ids are the ones the SPA
//!   and the standalone use (`b1_freq`, not the derive macro's `freq_1`).
//! * [`NoobQ::default`] builds the vst3-web-stratum bridge through
//!   `Vst3WebStratumEditor::with_builder`: the adapter mirrors the nih-plug
//!   parameters into vst3-web-stratum specs (65-point tables carry nih-plug's skewed
//!   ranges to the page), declares the streams from [`dsp::streams`], and
//!   adds the manifest meta. `Plugin::editor` hands out the adapter's editor
//!   handle, which starts the server lazily and opens the web view inside
//!   the host's window.
//! * `process` reads the parameters into [`BandSettings`] / [`Globals`],
//!   configures the [`Engine`], runs the block, feeds the analyzers and
//!   meters, and publishes the streams through the audio handle.
//! * Latency is reported to the host whenever the engine's latency changes
//!   (processing mode, linear-phase quality, or a switch to two stages).
//! * The page's own state (user presets, favourites, EQ Match references)
//!   lives in vst3-web-stratum's UI store and is persisted through
//!   [`NoobQParams::ui_store`], a `StoreSlot` that plugs into
//!   `Params::serialize_fields` / `deserialize_fields`, so it travels with
//!   the session and with host presets.
//!
//! ## Layouts
//!
//! Stereo in / stereo out with a stereo auxiliary input named "Side-chain"
//! (the first layout, which hosts pick by default), plain stereo, and mono.
//! Mono is processed by duplicating the channel into a scratch right buffer,
//! so mid/side placement still behaves (mid = the signal, side = silence).
//!
//! ## Parameter ids
//!
//! Global: `bypass`, `output_gain`, `gain_scale`, `auto_gain`, `output_pan`,
//! `pan_mode`, `phase_invert`, `processing_mode`, `lp_quality`, `character`,
//! `gain_q`. Analyzer (non-automatable): `analyzer_pre`, `analyzer_post`,
//! `analyzer_sc`, `analyzer_resolution`, `analyzer_range`, `analyzer_speed`,
//! `analyzer_tilt`, `analyzer_freeze`. Display (non-automatable):
//! `display_range`, `piano_display`. Per band `n` in 1..=24: `b<n>_on`,
//! `b<n>_shape`, `b<n>_freq`, `b<n>_gain`, `b<n>_q`, `b<n>_slope`,
//! `b<n>_place`, `b<n>_solo`, `b<n>_dyn_on`, `b<n>_dyn_range`,
//! `b<n>_dyn_thr`, `b<n>_dyn_auto`, `b<n>_dyn_attack`, `b<n>_dyn_release`,
//! `b<n>_dyn_sc`. Ranges and defaults are on [`BandParams`] and
//! [`NoobQParams`].

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use include_dir::{Dir, include_dir};
use nih_plug::prelude::*;
use vst3_web_stratum::{Assets, AudioHandle, Vst3WebStratum};
use vst3_web_stratum_nih::{EditorConfig, StoreSlot, Vst3WebStratumEditor};

use crate::dsp::{
    self, Analyzer, BANDS, BandSettings, CURVE_MAX_HZ, CURVE_MIN_HZ, CURVE_POINTS, DynSettings,
    Engine, Globals, Kind, MAX_BINS, Meter, Mode, Placement, STREAM_IX,
};

/// The built SPA, embedded at compile time (`cd web && npm run build`
/// first). Served by the plug-in's own HTTP server, so the editor works
/// without any files on disk.
static UI: Dir = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

/// Asset lookup for `Assets::Lookup`: a request path relative to the dist
/// root (`index.html`, `assets/index-….js`) to its bytes.
fn ui_lookup(path: &str) -> Option<&'static [u8]> {
    UI.get_file(path).map(|f| f.contents())
}

// ---------------------------------------------------------------------------
// Enum parameters
// ---------------------------------------------------------------------------
//
// Each enum mirrors a `*_NAMES` table in `dsp`, in the same order, so the
// index the page sends is the index the DSP expects. nih-plug shows the
// `#[name]` strings to the host.

/// `b<n>_shape`: the band's filter shape (see `dsp::Kind`).
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Bell,
    #[name = "Low Shelf"]
    LowShelf,
    #[name = "Low Cut"]
    LowCut,
    #[name = "High Shelf"]
    HighShelf,
    #[name = "High Cut"]
    HighCut,
    Notch,
    #[name = "Band Pass"]
    BandPass,
    #[name = "Tilt Shelf"]
    TiltShelf,
    #[name = "Flat Tilt"]
    FlatTilt,
    #[name = "All Pass"]
    AllPass,
}

/// `pan_mode`: whether `output_pan` pans left/right or mid/side.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanMode {
    #[name = "L/R"]
    LeftRight,
    #[name = "M/S"]
    MidSide,
}

/// `character`: saturation after the EQ (clean, subtle `tanh`, warm
/// asymmetric `tanh`).
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Character {
    Clean,
    Subtle,
    Warm,
}

/// `b<n>_slope`: dB per octave for cuts and shelves (see
/// `dsp::SLOPE_ORDERS`); Brickwall is a 32nd-order Butterworth.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlopeParam {
    #[name = "6 dB"]
    S6,
    #[name = "12 dB"]
    S12,
    #[name = "18 dB"]
    S18,
    #[name = "24 dB"]
    S24,
    #[name = "30 dB"]
    S30,
    #[name = "36 dB"]
    S36,
    #[name = "48 dB"]
    S48,
    #[name = "72 dB"]
    S72,
    #[name = "96 dB"]
    S96,
    Brickwall,
}

/// `b<n>_place`: which channels the band processes (see `dsp::Placement`).
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlacementParam {
    Stereo,
    Left,
    Right,
    Mid,
    Side,
}

/// `processing_mode` (see `dsp::Mode`). Changing it changes the reported
/// latency.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeParam {
    #[name = "Zero Latency"]
    ZeroLatency,
    #[name = "Natural Phase"]
    NaturalPhase,
    #[name = "Linear Phase"]
    LinearPhase,
}

/// `lp_quality`: linear-phase FIR length, 4096 … 65536 taps (see
/// `dsp::QUALITY_TAPS`). The top two disable dynamic EQ.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Low,
    Medium,
    High,
    #[name = "Very High"]
    VeryHigh,
    Maximum,
}

/// `analyzer_resolution`: analyzer FFT size, 1024 … 8192 (see
/// `dsp::RESOLUTION_FFT`).
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    Low,
    Medium,
    High,
    Maximum,
}

/// `analyzer_range`: dB span of the spectrum display. Page-only; stored
/// here so it persists with the state.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnalyzerRange {
    #[name = "60 dB"]
    R60,
    #[name = "90 dB"]
    R90,
    #[name = "120 dB"]
    R120,
}

/// `analyzer_speed`: spectrum averaging / fall speed. Page-only.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnalyzerSpeed {
    #[name = "Very Slow"]
    VerySlow,
    Slow,
    Medium,
    Fast,
    #[name = "Very Fast"]
    VeryFast,
}

/// `analyzer_tilt`: slope added to the displayed spectrum so pink-ish
/// material reads flat. Page-only.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnalyzerTilt {
    #[name = "0 dB/oct"]
    T0,
    #[name = "1.5 dB/oct"]
    T15,
    #[name = "3 dB/oct"]
    T3,
    #[name = "4.5 dB/oct"]
    T45,
    #[name = "6 dB/oct"]
    T6,
}

/// `display_range`: ± dB span of the EQ curve display. Page-only.
#[derive(Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayRange {
    #[name = "3 dB"]
    D3,
    #[name = "6 dB"]
    D6,
    #[name = "12 dB"]
    D12,
    #[name = "30 dB"]
    D30,
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// One band's fifteen parameters. Ids are `b<n>_<field>`; the host sees
/// them in the group `Band <n>`.
pub struct BandParams {
    /// `b<n>_on`, default off.
    pub on: BoolParam,
    /// `b<n>_shape`, default Bell.
    pub shape: EnumParam<Shape>,
    /// `b<n>_freq`, 10 Hz … 30 kHz, skewed toward the lows, default spread
    /// over the range per band (`dsp::default_band_freq`).
    pub freq: FloatParam,
    /// `b<n>_gain`, ±30 dB in 0.01 dB steps, default 0.
    pub gain: FloatParam,
    /// `b<n>_q`, 0.025 … 40, skewed, default 1.
    pub q: FloatParam,
    /// `b<n>_slope`, default 12 dB/oct.
    pub slope: EnumParam<SlopeParam>,
    /// `b<n>_place`, default Stereo.
    pub place: EnumParam<PlacementParam>,
    /// `b<n>_solo`, non-automatable (a listening aid, not a mix decision).
    pub solo: BoolParam,
    /// `b<n>_dyn_on`, default off.
    pub dyn_on: BoolParam,
    /// `b<n>_dyn_range`, ±30 dB signed, default 0.
    pub dyn_range: FloatParam,
    /// `b<n>_dyn_thr`, −60 … 0 dBFS, default −24.
    pub dyn_thr: FloatParam,
    /// `b<n>_dyn_auto`, default on.
    pub dyn_auto: BoolParam,
    /// `b<n>_dyn_attack`, 0.1 … 500 ms, skewed, default 10.
    pub dyn_attack: FloatParam,
    /// `b<n>_dyn_release`, 1 … 2000 ms, skewed, default 120.
    pub dyn_release: FloatParam,
    /// `b<n>_dyn_sc`, default off.
    pub dyn_sc: BoolParam,
}

impl BandParams {
    /// The parameters of band `n` (1-based, used in the display names).
    fn new(n: usize) -> Self {
        let f = dsp::default_band_freq(n - 1);
        BandParams {
            on: BoolParam::new(format!("Band {n} Enabled"), false),
            shape: EnumParam::new(format!("Band {n} Shape"), Shape::Bell),
            freq: FloatParam::new(
                format!("Band {n} Frequency"),
                f,
                FloatRange::Skewed {
                    min: dsp::FREQ_MIN,
                    max: dsp::FREQ_MAX,
                    factor: FloatRange::skew_factor(-2.4),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            gain: FloatParam::new(
                format!("Band {n} Gain"),
                0.0,
                FloatRange::Linear {
                    min: -dsp::GAIN_RANGE_DB,
                    max: dsp::GAIN_RANGE_DB,
                },
            )
            .with_unit(" dB")
            .with_step_size(0.01),
            q: FloatParam::new(
                format!("Band {n} Q"),
                1.0,
                FloatRange::Skewed {
                    min: dsp::Q_MIN,
                    max: dsp::Q_MAX,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(3)),
            slope: EnumParam::new(format!("Band {n} Slope"), SlopeParam::S12),
            place: EnumParam::new(format!("Band {n} Placement"), PlacementParam::Stereo),
            solo: BoolParam::new(format!("Band {n} Solo"), false).non_automatable(),
            dyn_on: BoolParam::new(format!("Band {n} Dynamics"), false),
            dyn_range: FloatParam::new(
                format!("Band {n} Dynamic Range"),
                0.0,
                FloatRange::Linear {
                    min: -dsp::GAIN_RANGE_DB,
                    max: dsp::GAIN_RANGE_DB,
                },
            )
            .with_unit(" dB")
            .with_step_size(0.01),
            dyn_thr: FloatParam::new(
                format!("Band {n} Threshold"),
                -24.0,
                FloatRange::Linear {
                    min: -60.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_step_size(0.1),
            dyn_auto: BoolParam::new(format!("Band {n} Auto Threshold"), true),
            dyn_attack: FloatParam::new(
                format!("Band {n} Attack"),
                10.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            dyn_release: FloatParam::new(
                format!("Band {n} Release"),
                120.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            dyn_sc: BoolParam::new(format!("Band {n} External Side-chain"), false),
        }
    }

    /// Snapshot the parameters into the engine's settings struct. Called on
    /// the audio thread every block; nih-plug's `value()` is a relaxed
    /// atomic load (smoothing is not used, the engine smooths by design).
    fn settings(&self) -> BandSettings {
        BandSettings {
            on: self.on.value(),
            kind: Kind::from_index(self.shape.value() as usize),
            freq: self.freq.value(),
            gain_db: self.gain.value(),
            q: self.q.value(),
            slope: self.slope.value() as usize,
            placement: Placement::from_index(self.place.value() as usize),
            solo: self.solo.value(),
            dynamics: DynSettings {
                on: self.dyn_on.value(),
                range_db: self.dyn_range.value(),
                threshold_db: self.dyn_thr.value(),
                auto_threshold: self.dyn_auto.value(),
                attack_ms: self.dyn_attack.value(),
                release_ms: self.dyn_release.value(),
                external: self.dyn_sc.value(),
            },
        }
    }
}

/// Every host-visible parameter, plus the UI store slot. Field names are
/// the ids; the host groups are `global`, `analyzer`, `display` and
/// `Band 1` … `Band 24`.
pub struct NoobQParams {
    /// `bypass`. Shown to hosts as the bypass parameter.
    pub bypass: BoolParam,
    /// `output_gain`, −60 … +36 dB in 0.01 dB steps, default 0.
    pub output_gain: FloatParam,
    /// `gain_scale`, 0 … 200 %, default 100: scales every band's gain.
    pub gain_scale: FloatParam,
    /// `auto_gain`, default off.
    pub auto_gain: BoolParam,
    /// `output_pan`, −100 … 100 %, default 0.
    pub output_pan: FloatParam,
    /// `pan_mode`, default L/R.
    pub pan_mode: EnumParam<PanMode>,
    /// `phase_invert`, default off.
    pub phase_invert: BoolParam,
    /// `processing_mode`, default Zero Latency.
    pub processing_mode: EnumParam<ModeParam>,
    /// `lp_quality`, default High (16384 taps).
    pub lp_quality: EnumParam<Quality>,
    /// `character`, default Clean.
    pub character: EnumParam<Character>,
    /// `gain_q`, default off.
    pub gain_q: BoolParam,
    /// `analyzer_pre`, default on. Non-automatable, like everything below.
    pub analyzer_pre: BoolParam,
    /// `analyzer_post`, default on.
    pub analyzer_post: BoolParam,
    /// `analyzer_sc`, default off (the side-chain spectrum costs an FFT).
    pub analyzer_sc: BoolParam,
    /// `analyzer_resolution`, default Medium (2048).
    pub analyzer_resolution: EnumParam<Resolution>,
    /// `analyzer_range`, default 90 dB. Page-only.
    pub analyzer_range: EnumParam<AnalyzerRange>,
    /// `analyzer_speed`, default Medium. Page-only.
    pub analyzer_speed: EnumParam<AnalyzerSpeed>,
    /// `analyzer_tilt`, default 4.5 dB/oct. Page-only.
    pub analyzer_tilt: EnumParam<AnalyzerTilt>,
    /// `analyzer_freeze`, default off. Page-only.
    pub analyzer_freeze: BoolParam,
    /// `display_range`, default 12 dB. Page-only.
    pub display_range: EnumParam<DisplayRange>,
    /// `piano_display`, default off: a piano keyboard under the frequency
    /// axis. Page-only.
    pub piano_display: BoolParam,
    /// The 24 bands, in order.
    pub bands: Vec<BandParams>,
    /// The page's user presets, favourites and EQ Match references; not a
    /// parameter, but saved and restored with the state.
    pub ui_store: StoreSlot,
}

impl Default for NoobQParams {
    /// The defaults listed on the fields. nih-plug's `FloatRange::Skewed`
    /// with negative factors gives the log-like feel of the frequency, Q and
    /// time controls; the adapter mirrors the exact mapping to the page as a
    /// table, so the knobs there match.
    fn default() -> Self {
        NoobQParams {
            bypass: BoolParam::new("Bypass", false)
                .with_value_to_string(formatters::v2s_bool_bypass()),
            output_gain: FloatParam::new(
                "Output Gain",
                0.0,
                FloatRange::Linear {
                    min: dsp::OUTPUT_GAIN_MIN_DB,
                    max: dsp::OUTPUT_GAIN_MAX_DB,
                },
            )
            .with_unit(" dB")
            .with_step_size(0.01),
            gain_scale: FloatParam::new(
                "Gain Scale",
                100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 200.0,
                },
            )
            .with_unit(" %")
            .with_step_size(1.0),
            auto_gain: BoolParam::new("Auto Gain", false),
            output_pan: FloatParam::new(
                "Output Pan",
                0.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 100.0,
                },
            )
            .with_unit(" %")
            .with_step_size(1.0),
            pan_mode: EnumParam::new("Output Pan Mode", PanMode::LeftRight),
            phase_invert: BoolParam::new("Phase Invert", false),
            processing_mode: EnumParam::new("Processing Mode", ModeParam::ZeroLatency),
            lp_quality: EnumParam::new("Linear Phase Quality", Quality::High),
            character: EnumParam::new("Character", Character::Clean),
            gain_q: BoolParam::new("Gain-Q Interaction", false),
            analyzer_pre: BoolParam::new("Analyzer Pre", true).non_automatable(),
            analyzer_post: BoolParam::new("Analyzer Post", true).non_automatable(),
            analyzer_sc: BoolParam::new("Analyzer Side-chain", false).non_automatable(),
            analyzer_resolution: EnumParam::new("Analyzer Resolution", Resolution::Medium)
                .non_automatable(),
            analyzer_range: EnumParam::new("Analyzer Range", AnalyzerRange::R90).non_automatable(),
            analyzer_speed: EnumParam::new("Analyzer Speed", AnalyzerSpeed::Medium)
                .non_automatable(),
            analyzer_tilt: EnumParam::new("Analyzer Tilt", AnalyzerTilt::T45).non_automatable(),
            analyzer_freeze: BoolParam::new("Analyzer Freeze", false).non_automatable(),
            display_range: EnumParam::new("Display Range", DisplayRange::D12).non_automatable(),
            piano_display: BoolParam::new("Piano Display", false).non_automatable(),
            bands: (1..=BANDS).map(BandParams::new).collect(),
            ui_store: StoreSlot::new(),
        }
    }
}

// Implemented by hand so the ids match the standalone binary and the SPA
// (`b1_freq`, ...) instead of the derive macro's `freq_1` scheme, and so the
// UI store can ride along in the persistent fields.
//
// SAFETY (why the trait is unsafe): the host and the editor keep raw
// `ParamPtr`s from `param_map` for the lifetime of the `Params` object. That
// is sound here because every pointer targets a field of `self`, the struct
// lives behind an `Arc` owned by the plug-in, and the ids are unique and
// stable.
unsafe impl Params for NoobQParams {
    /// `(id, pointer, group)` for every parameter, globals first, then the
    /// bands in order with their fifteen parameters each.
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        let g = |s: &str| s.to_string();
        let mut v = vec![
            (g("bypass"), self.bypass.as_ptr(), g("global")),
            (g("output_gain"), self.output_gain.as_ptr(), g("global")),
            (g("gain_scale"), self.gain_scale.as_ptr(), g("global")),
            (g("auto_gain"), self.auto_gain.as_ptr(), g("global")),
            (g("output_pan"), self.output_pan.as_ptr(), g("global")),
            (g("pan_mode"), self.pan_mode.as_ptr(), g("global")),
            (g("phase_invert"), self.phase_invert.as_ptr(), g("global")),
            (
                g("processing_mode"),
                self.processing_mode.as_ptr(),
                g("global"),
            ),
            (g("lp_quality"), self.lp_quality.as_ptr(), g("global")),
            (g("character"), self.character.as_ptr(), g("global")),
            (g("gain_q"), self.gain_q.as_ptr(), g("global")),
            (g("analyzer_pre"), self.analyzer_pre.as_ptr(), g("analyzer")),
            (
                g("analyzer_post"),
                self.analyzer_post.as_ptr(),
                g("analyzer"),
            ),
            (g("analyzer_sc"), self.analyzer_sc.as_ptr(), g("analyzer")),
            (
                g("analyzer_resolution"),
                self.analyzer_resolution.as_ptr(),
                g("analyzer"),
            ),
            (
                g("analyzer_range"),
                self.analyzer_range.as_ptr(),
                g("analyzer"),
            ),
            (
                g("analyzer_speed"),
                self.analyzer_speed.as_ptr(),
                g("analyzer"),
            ),
            (
                g("analyzer_tilt"),
                self.analyzer_tilt.as_ptr(),
                g("analyzer"),
            ),
            (
                g("analyzer_freeze"),
                self.analyzer_freeze.as_ptr(),
                g("analyzer"),
            ),
            (
                g("display_range"),
                self.display_range.as_ptr(),
                g("display"),
            ),
            (
                g("piano_display"),
                self.piano_display.as_ptr(),
                g("display"),
            ),
        ];
        for (i, b) in self.bands.iter().enumerate() {
            let n = i + 1;
            let grp = format!("Band {n}");
            let mut push = |id: String, ptr: ParamPtr| v.push((id, ptr, grp.clone()));
            push(format!("b{n}_on"), b.on.as_ptr());
            push(format!("b{n}_shape"), b.shape.as_ptr());
            push(format!("b{n}_freq"), b.freq.as_ptr());
            push(format!("b{n}_gain"), b.gain.as_ptr());
            push(format!("b{n}_q"), b.q.as_ptr());
            push(format!("b{n}_slope"), b.slope.as_ptr());
            push(format!("b{n}_place"), b.place.as_ptr());
            push(format!("b{n}_solo"), b.solo.as_ptr());
            push(format!("b{n}_dyn_on"), b.dyn_on.as_ptr());
            push(format!("b{n}_dyn_range"), b.dyn_range.as_ptr());
            push(format!("b{n}_dyn_thr"), b.dyn_thr.as_ptr());
            push(format!("b{n}_dyn_auto"), b.dyn_auto.as_ptr());
            push(format!("b{n}_dyn_attack"), b.dyn_attack.as_ptr());
            push(format!("b{n}_dyn_release"), b.dyn_release.as_ptr());
            push(format!("b{n}_dyn_sc"), b.dyn_sc.as_ptr());
        }
        v
    }

    /// Extra state nih-plug saves next to the parameters: the UI store as
    /// one JSON string under `StoreSlot::KEY` (only when non-empty).
    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        self.ui_store.serialize_into(&mut m);
        m
    }

    /// Restore the UI store from a saved state; every connected page is
    /// re-hydrated. A state without the key empties the store.
    fn deserialize_fields(&self, serialized: &BTreeMap<String, String>) {
        self.ui_store.deserialize_from(serialized);
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// The plug-in instance: parameters, editor, bridge, engine, and the
/// analysis state that lives on the audio thread.
pub struct NoobQ {
    params: Arc<NoobQParams>,
    /// The web-view editor; one per instance, handed to the host on demand.
    editor: Arc<Vst3WebStratumEditor>,
    /// The bridge, for host-side messages (`sample_rate`).
    bridge: Vst3WebStratum,
    /// The audio-thread handle for publishing streams.
    audio: Option<AudioHandle>,
    engine: Engine,
    /// Input spectrum analyzer.
    an_pre: Analyzer,
    /// Output spectrum analyzer.
    an_post: Analyzer,
    /// Side-chain spectrum analyzer.
    an_sc: Analyzer,
    meter_in: Meter,
    meter_out: Meter,
    /// Settings read from the parameters each block.
    bands: [BandSettings; BANDS],
    /// Scratch for spectrum frames.
    spectrum: Vec<f32>,
    /// Scratch for the response curve.
    curve: Vec<f32>,
    /// Scratch for per-band values (dynamic gain, then detector level).
    band_dyn: Vec<f32>,
    /// Mono hosts: a copy of the channel processed as "right".
    scratch_r: Vec<f32>,
    /// Blocks processed; paces the telemetry.
    blocks: u64,
    sample_rate: f32,
    /// Last latency reported to the host, samples.
    last_latency: usize,
    /// Last analyzer resolution applied to the analyzers.
    last_resolution: usize,
}

impl Default for NoobQ {
    /// Build the parameters, the bridge and the editor. The bridge mirrors
    /// the nih-plug parameters (ids, names, units, groups, 65-point mapping
    /// tables, labels, automatable flags), declares the streams, and gets
    /// the same manifest meta as the standalone with `standalone: false`.
    /// The editor is configured with the embedded SPA and its default
    /// window size; the server starts lazily when the host first opens the
    /// editor. Finally the UI store slot is attached so a state restored
    /// before this point (hosts sometimes deserialize early) is applied.
    fn default() -> Self {
        let params = Arc::new(NoobQParams::default());
        let (editor, bridge) = Vst3WebStratumEditor::with_builder(
            "Noob-Q",
            params.as_ref(),
            dsp::streams(48_000.0),
            EditorConfig::new(1180, 720).assets(Assets::Lookup(ui_lookup)),
            |b| {
                b.meta(serde_json::json!({
                    "vendor": "Ely Erin Fox",
                    "version": env!("CARGO_PKG_VERSION"),
                    "sample_rate": 48_000.0,
                    "bands": BANDS,
                    "freq_range": [dsp::FREQ_MIN, dsp::FREQ_MAX],
                    "gain_range": dsp::GAIN_RANGE_DB,
                    "standalone": false,
                }))
            },
        );
        let audio = bridge.take_audio();
        params.ui_store.attach(&bridge);
        NoobQ {
            params,
            editor,
            bridge,
            audio,
            engine: Engine::new(48_000.0),
            an_pre: Analyzer::new(),
            an_post: Analyzer::new(),
            an_sc: Analyzer::new(),
            meter_in: Meter::default(),
            meter_out: Meter::default(),
            bands: [BandSettings::default(); BANDS],
            spectrum: vec![0.0; MAX_BINS],
            curve: vec![0.0; CURVE_POINTS],
            band_dyn: vec![0.0; BANDS],
            scratch_r: Vec::new(),
            blocks: 0,
            sample_rate: 48_000.0,
            last_latency: usize::MAX,
            last_resolution: usize::MAX,
        }
    }
}

impl NoobQ {
    /// Snapshot the global parameters into the engine's settings struct
    /// (percent parameters become unit ranges).
    fn globals(&self) -> Globals {
        let p = &self.params;
        Globals {
            bypass: p.bypass.value(),
            output_gain_db: p.output_gain.value(),
            gain_scale: p.gain_scale.value() / 100.0,
            auto_gain: p.auto_gain.value(),
            pan: p.output_pan.value() / 100.0,
            pan_ms: p.pan_mode.value() == PanMode::MidSide,
            phase_invert: p.phase_invert.value(),
            mode: Mode::from_index(p.processing_mode.value() as usize),
            quality: p.lp_quality.value() as usize,
            character: p.character.value() as usize,
            gain_q: p.gain_q.value(),
        }
    }
}

impl Plugin for NoobQ {
    const NAME: &'static str = "Noob-Q";
    const VENDOR: &'static str = "Ely Erin Fox";
    const URL: &'static str = env!("CARGO_PKG_HOMEPAGE");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// Stereo with a stereo "Side-chain" aux input (default), plain stereo,
    /// and mono. Hosts without side-chain routing pick the second.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            aux_output_ports: &[],
            names: PortNames {
                layout: Some("Stereo"),
                main_input: None,
                main_output: None,
                aux_inputs: &["Side-chain"],
                aux_outputs: &[],
            },
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    /// Parameters are read once per block (the engine smooths gains itself),
    /// so splitting blocks at automation points would buy nothing.
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    /// A fresh editor handle each time the host asks; all handles share the
    /// one server and bridge, so opening and closing the window is cheap.
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(self.editor.handle()))
    }

    /// Adopt the host's sample rate and buffer size, report the initial
    /// latency, and tell every connected page the new sample rate (it
    /// places spectrum bins and draws the curve with it).
    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.engine.set_sample_rate(buffer_config.sample_rate);
        self.scratch_r = vec![0.0; buffer_config.max_buffer_size as usize];
        self.last_latency = self.engine.latency();
        context.set_latency_samples(self.last_latency as u32);
        self.bridge.send_json(
            "sample_rate",
            serde_json::json!({ "sample_rate": buffer_config.sample_rate }),
        );
        true
    }

    /// Transport jumps and the like: clear all filter, detector and
    /// convolver state (a sample-rate re-set does exactly that).
    fn reset(&mut self) {
        self.engine.set_sample_rate(self.sample_rate);
    }

    /// One block. Real-time safe: no allocation (the scratch buffers were
    /// sized in `initialize`; a mono host larger than `max_buffer_size` is
    /// the one defensive resize), no locks, and every stream publish is a
    /// wait-free write into a triple buffer.
    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // 1. Parameters → engine; report the latency if the mode changed it,
        //    and retune the analyzers if the resolution changed.
        for (b, p) in self.bands.iter_mut().zip(&self.params.bands) {
            *b = p.settings();
        }
        let changed = self.engine.configure(&self.bands, self.globals());
        let latency = self.engine.latency();
        if latency != self.last_latency {
            self.last_latency = latency;
            context.set_latency_samples(latency as u32);
        }
        let resolution = self.params.analyzer_resolution.value() as usize;
        if resolution != self.last_resolution {
            self.last_resolution = resolution;
            self.an_pre.set_resolution(resolution);
            self.an_post.set_resolution(resolution);
            self.an_sc.set_resolution(resolution);
        }
        let want_pre = self.params.analyzer_pre.value();
        let want_post = self.params.analyzer_post.value();
        let want_sc = self.params.analyzer_sc.value();

        // 2. Side-chain: analyze it and hand it to the engine's detectors.
        //    Missing or too-short aux buffers simply mean "no side-chain".
        let frames = buffer.samples();
        let sc: Option<(&[f32], &[f32])> = aux.inputs.first().and_then(|b| {
            let s = b.as_slice_immutable();
            if s.len() >= 2 && s[0].len() >= frames {
                Some((&s[0][..frames], &s[1][..frames]))
            } else {
                None
            }
        });
        if let Some((sl, sr)) = sc {
            for i in 0..frames {
                self.an_sc.push(0.5 * (sl[i] + sr[i]));
            }
        }

        // 3. Main path: pre analyzer + input meter, the EQ, post analyzer +
        //    output meter. Mono duplicates the channel so the engine's
        //    stereo code path (and mid/side placement) just works.
        let channels = buffer.channels();
        let slices = buffer.as_slice();
        if channels >= 2 {
            let (a, b) = slices.split_at_mut(1);
            let (l, r) = (&mut *a[0], &mut *b[0]);
            for i in 0..frames {
                self.an_pre.push(0.5 * (l[i] + r[i]));
                self.meter_in.feed(l[i], r[i]);
            }
            self.engine.process_block(l, r, sc);
            for i in 0..frames {
                self.an_post.push(0.5 * (l[i] + r[i]));
                self.meter_out.feed(l[i], r[i]);
            }
        } else if channels == 1 {
            let l = &mut *slices[0];
            if self.scratch_r.len() < frames {
                self.scratch_r.resize(frames, 0.0);
            }
            let r = &mut self.scratch_r[..frames];
            r.copy_from_slice(l);
            for i in 0..frames {
                self.an_pre.push(l[i]);
                self.meter_in.feed(l[i], l[i]);
            }
            self.engine.process_block(l, r, sc);
            for i in 0..frames {
                self.an_post.push(l[i]);
                self.meter_out.feed(l[i], l[i]);
            }
        }
        self.blocks += 1;

        // 4. Telemetry, at the same rates as the standalone: meters and
        //    dynamic gains every block, detector levels every 4th, spectra
        //    every 2nd (each is one FFT), the curve only when it changed.
        if let Some(audio) = self.audio.as_mut() {
            audio.publish_slice(STREAM_IX.meter_in, &self.meter_in.take());
            audio.publish_slice(STREAM_IX.meter_out, &self.meter_out.take());
            self.engine.band_dyn_gains(&mut self.band_dyn);
            audio.publish_slice(STREAM_IX.band_dyn, &self.band_dyn);
            if self.blocks.is_multiple_of(4) {
                self.engine.band_levels(&mut self.band_dyn);
                audio.publish_slice(STREAM_IX.band_level, &self.band_dyn);
            }
            if self.blocks.is_multiple_of(2) {
                if want_pre {
                    let bins = self.an_pre.compute(&mut self.spectrum);
                    audio.publish_slice(STREAM_IX.spectrum_pre, &self.spectrum[..bins]);
                }
                if want_post {
                    let bins = self.an_post.compute(&mut self.spectrum);
                    audio.publish_slice(STREAM_IX.spectrum_post, &self.spectrum[..bins]);
                }
                if want_sc && sc.is_some() {
                    let bins = self.an_sc.compute(&mut self.spectrum);
                    audio.publish_slice(STREAM_IX.spectrum_sc, &self.spectrum[..bins]);
                }
            }
            if changed || self.blocks == 1 {
                self.engine
                    .curve(&mut self.curve, CURVE_MIN_HZ, CURVE_MAX_HZ);
                audio.publish_slice(STREAM_IX.curve, &self.curve);
            }
        }
        ProcessStatus::Normal
    }
}

/// VST3 identity. The class id must never change once the plug-in has been
/// used in a session, or hosts will not find it again.
impl Vst3Plugin for NoobQ {
    const VST3_CLASS_ID: [u8; 16] = *b"NoobQVst3WebStr1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Eq];
}

/// CLAP identity and feature tags (the same stability rule as the VST3 id).
impl ClapPlugin for NoobQ {
    const CLAP_ID: &'static str = "io.github.elyerinfox.noob-q";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Pro-Q style EQ with a web-view editor over bridge");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Equalizer,
        ClapFeature::Stereo,
    ];
}
