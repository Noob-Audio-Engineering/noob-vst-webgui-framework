//! The EQ itself, independent of any host: the processing [`engine`], the
//! [`filters`] it is built from, per-band [`dynamics`], the linear-phase
//! [`convolver`], the spectrum [`analyzer`] with block meters and demo
//! sources, and the parameter / stream layout shared by the standalone
//! binary and the plug-in. Nothing in here knows where parameter values
//! come from: both hosts read them, build [`BandSettings`] / [`Globals`],
//! and call [`Engine::configure`] before every block.
//!
//! ## Signal chain
//!
//! ```text
//! in L/R ─► pre analyzer ─► band 1 … band 24 ─► gain · pan · polarity ─► character ─► post analyzer ─► out
//!                                ▲       │
//!        side-chain in ─► detectors ─────┘   (per-band dynamic gain)
//! ```
//!
//! * Each enabled band is a cascade of second-order sections designed by
//!   [`filters::design_band`] and applied on left/right, left only, right
//!   only, mid only or side only ([`Placement`]). The engine converts
//!   between the L/R and M/S domains only where consecutive bands need it.
//! * A band with dynamics measures its own frequency region (or the external
//!   side-chain) and moves its gain within a signed range; its filter is
//!   redesigned once per block with the new gain ([`dynamics`]).
//! * In *Zero Latency* and *Natural Phase* mode the bands run as IIR
//!   biquads. In *Linear Phase* mode the composite response of the bands is
//!   sampled and turned into a symmetric FIR that runs in a partitioned FFT
//!   convolver, one per channel domain; latency is `PARTITION + taps / 2`
//!   samples per stage ([`convolver`]).
//! * Solo replaces the output with the summed detector band-passes of the
//!   soloed bands, so you hear the region a band works on.
//! * The output stage applies output gain plus auto gain, a constant-power
//!   pan in L/R or M/S, polarity, and the *Character* saturation.
//!
//! ## Real-time rules
//!
//! Every buffer is allocated in [`Engine::new`] and [`Analyzer::new`];
//! `process_block`, `configure`, `curve` and the analyzers never allocate,
//! lock or block. A filter redesign is a few dozen trigonometric
//! evaluations per band. The linear-phase redesign is one inverse FFT of the
//! FIR length (up to 65536 points) plus the partition FFTs, the only
//! expensive step, so it runs at most every other block while dynamics move
//! and is disabled at the two highest qualities.
//!
//! ## Layout shared by both hosts
//!
//! [`param_specs`] lists every parameter id (the plug-in's `Params`
//! implementation mirrors it exactly), [`streams`] every stream, and
//! [`STREAM_IX`] the stream indices both hosts publish on. [`ParamIx`] and
//! [`BandIx`] resolve ids to indices once, so the audio thread reads
//! parameters by index through [`read_band`] and [`read_globals`].

pub mod analyzer;
pub mod convolver;
pub mod dynamics;
pub mod engine;
pub mod filters;

pub use analyzer::{
    Analyzer, MAX_BINS, Meter, RESOLUTION_FFT, RESOLUTION_NAMES, SOURCE_NAMES, Source,
};
pub use convolver::{PARTITION, QUALITY_NAMES, QUALITY_TAPS};
pub use dynamics::DynSettings;
pub use engine::{
    BANDS, BandSettings, CHARACTER_NAMES, Engine, Globals, MODE_NAMES, Mode, PAN_MODE_NAMES,
    PLACEMENT_NAMES, Placement, effective_q,
};
pub use filters::{KIND_NAMES, Kind, SLOPE_NAMES, SLOPE_ORDERS};

use serde_json::json;
use vst3_web_stratum::{AudioHandle, ParamSpec, StreamKind, StreamSpec, Vst3WebStratum};

/// Lowest output gain, dB (`output_gain`).
pub const OUTPUT_GAIN_MIN_DB: f32 = -60.0;
/// Highest output gain, dB (`output_gain`).
pub const OUTPUT_GAIN_MAX_DB: f32 = 36.0;
/// Points in the `curve` stream, log-spaced from [`CURVE_MIN_HZ`] to
/// [`CURVE_MAX_HZ`]. 256 points is well above what a 2000 px wide display
/// needs once the page interpolates.
pub const CURVE_POINTS: usize = 256;
/// First frequency of the `curve` stream, Hz.
pub const CURVE_MIN_HZ: f32 = 10.0;
/// Last frequency of the `curve` stream, Hz (above Nyquist at 44.1 / 48 kHz;
/// the engine clamps internally, the page draws whatever it is given).
pub const CURVE_MAX_HZ: f32 = 30_000.0;
/// Lowest band frequency, Hz (`b<n>_freq`, log taper).
pub const FREQ_MIN: f32 = 10.0;
/// Highest band frequency, Hz (`b<n>_freq`, log taper).
pub const FREQ_MAX: f32 = 30_000.0;
/// Band gain and dynamic range are `±GAIN_RANGE_DB` dB (`b<n>_gain`,
/// `b<n>_dyn_range`).
pub const GAIN_RANGE_DB: f32 = 30.0;
/// Lowest band Q (`b<n>_q`, log taper). Very low Qs give shelf-like bells.
pub const Q_MIN: f32 = 0.025;
/// Highest band Q (`b<n>_q`, log taper). Notches at Q 40 are a few Hz wide.
pub const Q_MAX: f32 = 40.0;

/// Labels for `analyzer_range`: how many dB the spectrum display spans
/// below 0 dBFS. Page-only; the DSP never reads it.
pub const ANALYZER_RANGE_NAMES: [&str; 3] = ["60 dB", "90 dB", "120 dB"];
/// Labels for `analyzer_speed`: the page's spectrum averaging / fall time.
/// Page-only.
pub const ANALYZER_SPEED_NAMES: [&str; 5] = ["Very Slow", "Slow", "Medium", "Fast", "Very Fast"];
/// Labels for `analyzer_tilt`: a slope the page adds to the spectrum so
/// pink-ish material reads flat (4.5 dB/oct is Pro-Q's default). Page-only.
pub const ANALYZER_TILT_NAMES: [&str; 5] = [
    "0 dB/oct",
    "1.5 dB/oct",
    "3 dB/oct",
    "4.5 dB/oct",
    "6 dB/oct",
];
/// Labels for `display_range`: the ± dB span of the EQ curve display.
/// Page-only.
pub const DISPLAY_RANGE_NAMES: [&str; 4] = ["3 dB", "6 dB", "12 dB", "30 dB"];

/// Default frequency of band `i` (0-based): the 24 bands are spread
/// log-evenly from 30 Hz to 16 kHz, so a band enabled without a frequency
/// lands somewhere sensible and distinct from its neighbours.
pub fn default_band_freq(i: usize) -> f32 {
    30.0 * (16_000.0f32 / 30.0).powf(i as f32 / (BANDS - 1) as f32)
}

/// Parameter indices of one band, resolved once from the `b<n>_*` ids so
/// the audio thread can read them by index ([`read_band`]). The field names
/// are the id suffixes.
pub struct BandIx {
    /// `b<n>_on` — toggle.
    pub on: usize,
    /// `b<n>_shape` — index into [`KIND_NAMES`].
    pub shape: usize,
    /// `b<n>_freq` — Hz, [`FREQ_MIN`]..[`FREQ_MAX`], log.
    pub freq: usize,
    /// `b<n>_gain` — dB, ±[`GAIN_RANGE_DB`].
    pub gain: usize,
    /// `b<n>_q` — [`Q_MIN`]..[`Q_MAX`], log.
    pub q: usize,
    /// `b<n>_slope` — index into [`SLOPE_NAMES`].
    pub slope: usize,
    /// `b<n>_place` — index into [`PLACEMENT_NAMES`].
    pub place: usize,
    /// `b<n>_solo` — toggle, not automatable.
    pub solo: usize,
    /// `b<n>_dyn_on` — toggle.
    pub dyn_on: usize,
    /// `b<n>_dyn_range` — dB, ±[`GAIN_RANGE_DB`], signed.
    pub dyn_range: usize,
    /// `b<n>_dyn_thr` — dBFS, −60..0.
    pub dyn_thr: usize,
    /// `b<n>_dyn_auto` — toggle (auto threshold).
    pub dyn_auto: usize,
    /// `b<n>_dyn_attack` — ms, 0.1..500, log.
    pub dyn_attack: usize,
    /// `b<n>_dyn_release` — ms, 1..2000, log.
    pub dyn_release: usize,
    /// `b<n>_dyn_sc` — toggle (detect on the external side-chain).
    pub dyn_sc: usize,
}

/// Every parameter index the audio thread needs, resolved once from the ids
/// by [`build_bridge`]. Field names are the ids. Page-only parameters
/// (`analyzer_range`, `analyzer_speed`, `analyzer_tilt`, `analyzer_freeze`,
/// `display_range`, `piano_display`) are deliberately absent: the DSP never
/// reads them.
pub struct ParamIx {
    pub bypass: usize,
    pub output_gain: usize,
    pub gain_scale: usize,
    pub auto_gain: usize,
    pub output_pan: usize,
    pub pan_mode: usize,
    pub phase_invert: usize,
    pub processing_mode: usize,
    pub lp_quality: usize,
    pub character: usize,
    pub gain_q: usize,
    /// Whether to compute and publish the pre-EQ spectrum.
    pub analyzer_pre: usize,
    /// Whether to compute and publish the post-EQ spectrum.
    pub analyzer_post: usize,
    /// Whether to compute and publish the side-chain spectrum.
    pub analyzer_sc: usize,
    /// FFT size index ([`RESOLUTION_NAMES`]).
    pub analyzer_resolution: usize,
    /// Demo source kind (standalone only; [`SOURCE_NAMES`]).
    pub src_kind: usize,
    /// Demo source frequency for the saw / sine kinds (standalone only).
    pub src_freq: usize,
    /// Demo source level, 0..1 (standalone only).
    pub src_level: usize,
    /// Demo side-chain source kind (standalone only).
    pub sc_kind: usize,
    /// Demo side-chain level, 0..1 (standalone only).
    pub sc_level: usize,
    /// One entry per band, in band order.
    pub bands: Vec<BandIx>,
}

/// Stream indices, in the order [`streams`] declares them. Both hosts
/// publish through [`STREAM_IX`]; the page finds streams by id.
pub struct StreamIx {
    /// `spectrum_pre` — dBFS bins of the input, every 2nd block.
    pub spectrum_pre: usize,
    /// `spectrum_post` — dBFS bins of the output, every 2nd block.
    pub spectrum_post: usize,
    /// `spectrum_sc` — dBFS bins of the external side-chain, every 2nd block.
    pub spectrum_sc: usize,
    /// `meter_in` — `[peak L, peak R, rms L, rms R]` per block, linear.
    pub meter_in: usize,
    /// `meter_out` — same layout, after the output stage.
    pub meter_out: usize,
    /// `curve` — [`CURVE_POINTS`] dB values of the static response, sticky,
    /// published only when the response changes.
    pub curve: usize,
    /// `band_dyn` — current dynamic gain per band, dB, every block.
    pub band_dyn: usize,
    /// `band_level` — detector level per band, dBFS, every 4th block.
    pub band_level: usize,
}

/// The stream indices both hosts use (must match the order of [`streams`]).
pub const STREAM_IX: StreamIx = StreamIx {
    spectrum_pre: 0,
    spectrum_post: 1,
    spectrum_sc: 2,
    meter_in: 3,
    meter_out: 4,
    curve: 5,
    band_dyn: 6,
    band_level: 7,
};

/// Stream declarations, shared by the standalone binary and the plug-in.
///
/// | id | kind | capacity | rate | contents |
/// |---|---|---|---|---|
/// | `spectrum_pre` | spectrum | [`MAX_BINS`] | every 2nd block | input magnitude, dBFS per bin |
/// | `spectrum_post` | spectrum | [`MAX_BINS`] | every 2nd block | output magnitude, dBFS per bin |
/// | `spectrum_sc` | spectrum | [`MAX_BINS`] | every 2nd block (if a side-chain exists) | side-chain magnitude, dBFS per bin |
/// | `meter_in` | meter, 2 ch | 4 | every block | `[peak L, peak R, rms L, rms R]`, linear |
/// | `meter_out` | meter, 2 ch | 4 | every block | same, after the output stage |
/// | `curve` | curve, sticky | [`CURVE_POINTS`] | on change | static response, dB, log-spaced [`CURVE_MIN_HZ`]..[`CURVE_MAX_HZ`] |
/// | `band_dyn` | raw | [`BANDS`] | every block | dynamic gain per band, dB |
/// | `band_level` | raw | [`BANDS`] | every 4th block | detector level per band, dBFS (−120 when idle) |
///
/// The spectra carry the sample rate in their meta so the page can place
/// bins on a frequency axis; the actual bin count of a frame depends on the
/// `analyzer_resolution` parameter and is whatever the frame's length says.
/// `curve` is *sticky*: the server replays the last frame to clients that
/// connect later, so a freshly opened window shows the response at once.
pub fn streams(sr: f32) -> Vec<StreamSpec> {
    let spectrum_meta = json!({ "sample_rate": sr, "db": true, "window": "hann" });
    let meter_meta = json!({ "layout": "peak,peak,rms,rms" });
    vec![
        StreamSpec::new("spectrum_pre", MAX_BINS)
            .name("Input Spectrum")
            .kind(StreamKind::Spectrum)
            .meta(spectrum_meta.clone()),
        StreamSpec::new("spectrum_post", MAX_BINS)
            .name("Output Spectrum")
            .kind(StreamKind::Spectrum)
            .meta(spectrum_meta.clone()),
        StreamSpec::new("spectrum_sc", MAX_BINS)
            .name("Side-chain Spectrum")
            .kind(StreamKind::Spectrum)
            .meta(spectrum_meta),
        StreamSpec::new("meter_in", 4)
            .name("Input")
            .kind(StreamKind::Meter)
            .channels(2)
            .meta(meter_meta.clone()),
        StreamSpec::new("meter_out", 4)
            .name("Output")
            .kind(StreamKind::Meter)
            .channels(2)
            .meta(meter_meta),
        StreamSpec::new("curve", CURVE_POINTS)
            .name("EQ Response")
            .kind(StreamKind::Curve)
            .sticky()
            .meta(
                json!({ "min_hz": CURVE_MIN_HZ, "max_hz": CURVE_MAX_HZ, "log": true, "db": true }),
            ),
        StreamSpec::new("band_dyn", BANDS)
            .name("Dynamic Gain")
            .kind(StreamKind::Raw)
            .meta(json!({ "unit": "dB", "per": "band" })),
        StreamSpec::new("band_level", BANDS)
            .name("Trigger Level")
            .kind(StreamKind::Raw)
            .meta(json!({ "unit": "dBFS", "per": "band" })),
    ]
}

/// Parameter specs for the standalone binary. Ids, order, ranges and labels
/// match the plug-in's `Params` implementation, so the same page drives
/// both; only the tapers differ in flavour (vst3-web-stratum's `log` here, nih-plug's
/// skewed ranges there, which the adapter mirrors as 65-point tables).
///
/// Groups:
/// * `global` — `bypass`, `output_gain`, `gain_scale`, `auto_gain`,
///   `output_pan`, `pan_mode`, `phase_invert`, `processing_mode`,
///   `lp_quality`, `character`, `gain_q`.
/// * `analyzer` — `analyzer_pre`, `analyzer_post`, `analyzer_sc`,
///   `analyzer_resolution`, `analyzer_range`, `analyzer_speed`,
///   `analyzer_tilt`, `analyzer_freeze` (all non-automatable; the last four
///   are page-only).
/// * `display` — `display_range`, `piano_display` (page-only).
/// * `band1` … `band24` — the fifteen `b<n>_*` ids listed on [`BandIx`].
/// * `source` (only with `with_source`, standalone) — `src_kind`,
///   `src_freq`, `src_level`, `sc_kind`, `sc_level` for the demo signals.
pub fn param_specs(with_source: bool) -> Vec<ParamSpec> {
    let mut v = vec![
        ParamSpec::new("bypass", "Bypass").toggle().group("global"),
        ParamSpec::new("output_gain", "Output Gain")
            .range(OUTPUT_GAIN_MIN_DB, OUTPUT_GAIN_MAX_DB)
            .default(0.0)
            .unit("dB")
            .group("global"),
        ParamSpec::new("gain_scale", "Gain Scale")
            .range(0.0, 200.0)
            .default(100.0)
            .unit("%")
            .group("global"),
        ParamSpec::new("auto_gain", "Auto Gain")
            .toggle()
            .group("global"),
        ParamSpec::new("output_pan", "Output Pan")
            .range(-100.0, 100.0)
            .default(0.0)
            .unit("%")
            .group("global"),
        ParamSpec::new("pan_mode", "Output Pan Mode")
            .labels(PAN_MODE_NAMES)
            .default(0.0)
            .group("global"),
        ParamSpec::new("phase_invert", "Phase Invert")
            .toggle()
            .group("global"),
        ParamSpec::new("processing_mode", "Processing Mode")
            .labels(MODE_NAMES)
            .default(0.0)
            .group("global"),
        ParamSpec::new("lp_quality", "Linear Phase Quality")
            .labels(QUALITY_NAMES)
            .default(2.0)
            .group("global"),
        ParamSpec::new("character", "Character")
            .labels(CHARACTER_NAMES)
            .default(0.0)
            .group("global"),
        ParamSpec::new("gain_q", "Gain-Q Interaction")
            .toggle()
            .group("global"),
        ParamSpec::new("analyzer_pre", "Analyzer Pre")
            .toggle()
            .default(1.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_post", "Analyzer Post")
            .toggle()
            .default(1.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_sc", "Analyzer Side-chain")
            .toggle()
            .default(0.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_resolution", "Analyzer Resolution")
            .labels(RESOLUTION_NAMES)
            .default(1.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_range", "Analyzer Range")
            .labels(ANALYZER_RANGE_NAMES)
            .default(1.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_speed", "Analyzer Speed")
            .labels(ANALYZER_SPEED_NAMES)
            .default(2.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_tilt", "Analyzer Tilt")
            .labels(ANALYZER_TILT_NAMES)
            .default(3.0)
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("analyzer_freeze", "Analyzer Freeze")
            .toggle()
            .not_automatable()
            .group("analyzer"),
        ParamSpec::new("display_range", "Display Range")
            .labels(DISPLAY_RANGE_NAMES)
            .default(2.0)
            .not_automatable()
            .group("display"),
        ParamSpec::new("piano_display", "Piano Display")
            .toggle()
            .not_automatable()
            .group("display"),
    ];
    for i in 0..BANDS {
        let n = i + 1;
        let grp = format!("band{n}");
        let f = default_band_freq(i);
        v.push(
            ParamSpec::new(format!("b{n}_on"), format!("Band {n} Enabled"))
                .toggle()
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_shape"), format!("Band {n} Shape"))
                .labels(KIND_NAMES)
                .default(0.0)
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_freq"), format!("Band {n} Frequency"))
                .range(FREQ_MIN, FREQ_MAX)
                .log()
                .default(f)
                .unit("Hz")
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_gain"), format!("Band {n} Gain"))
                .range(-GAIN_RANGE_DB, GAIN_RANGE_DB)
                .default(0.0)
                .unit("dB")
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_q"), format!("Band {n} Q"))
                .range(Q_MIN, Q_MAX)
                .log()
                .default(1.0)
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_slope"), format!("Band {n} Slope"))
                .labels(SLOPE_NAMES)
                .default(1.0)
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_place"), format!("Band {n} Placement"))
                .labels(PLACEMENT_NAMES)
                .default(0.0)
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_solo"), format!("Band {n} Solo"))
                .toggle()
                .not_automatable()
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_dyn_on"), format!("Band {n} Dynamics"))
                .toggle()
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_dyn_range"), format!("Band {n} Dynamic Range"))
                .range(-GAIN_RANGE_DB, GAIN_RANGE_DB)
                .default(0.0)
                .unit("dB")
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_dyn_thr"), format!("Band {n} Threshold"))
                .range(-60.0, 0.0)
                .default(-24.0)
                .unit("dB")
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_dyn_auto"), format!("Band {n} Auto Threshold"))
                .toggle()
                .default(1.0)
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_dyn_attack"), format!("Band {n} Attack"))
                .range(0.1, 500.0)
                .log()
                .default(10.0)
                .unit("ms")
                .group(&grp),
        );
        v.push(
            ParamSpec::new(format!("b{n}_dyn_release"), format!("Band {n} Release"))
                .range(1.0, 2000.0)
                .log()
                .default(120.0)
                .unit("ms")
                .group(&grp),
        );
        v.push(
            ParamSpec::new(
                format!("b{n}_dyn_sc"),
                format!("Band {n} External Side-chain"),
            )
            .toggle()
            .group(&grp),
        );
    }
    if with_source {
        v.push(
            ParamSpec::new("src_kind", "Source")
                .labels(SOURCE_NAMES)
                .default(0.0)
                .not_automatable()
                .group("source"),
        );
        v.push(
            ParamSpec::new("src_freq", "Source Freq")
                .range(20.0, 20000.0)
                .log()
                .default(220.0)
                .unit("Hz")
                .not_automatable()
                .group("source"),
        );
        v.push(
            ParamSpec::new("src_level", "Source Level")
                .range(0.0, 1.0)
                .default(0.5)
                .not_automatable()
                .group("source"),
        );
        v.push(
            ParamSpec::new("sc_kind", "Side-chain Source")
                .labels(SOURCE_NAMES)
                .default(4.0)
                .not_automatable()
                .group("source"),
        );
        v.push(
            ParamSpec::new("sc_level", "Side-chain Level")
                .range(0.0, 1.0)
                .default(0.5)
                .not_automatable()
                .group("source"),
        );
    }
    v
}

/// Build the vst3-web-stratum bridge for the standalone binary: manifest meta
/// (vendor, version, sample rate, band count, ranges, `standalone: true` so
/// the page can show demo-source controls), every parameter from
/// [`param_specs`] with the demo sources, and every stream from
/// [`streams`]. Returns the bridge and the resolved parameter indices.
///
/// The plug-in does not use this; it builds its bridge through
/// `vst3_web_stratum_nih::Vst3WebStratumEditor::with_builder`, which mirrors the nih-plug
/// parameters instead and adds the same meta with `standalone: false`.
pub fn build_bridge(name: &str, sr: f32) -> (Vst3WebStratum, ParamIx) {
    let mut b = Vst3WebStratum::builder(name)
        .meta(json!({
            "vendor": "Ely Erin Fox",
            "version": env!("CARGO_PKG_VERSION"),
            "sample_rate": sr,
            "bands": BANDS,
            "freq_range": [FREQ_MIN, FREQ_MAX],
            "gain_range": GAIN_RANGE_DB,
            "standalone": true,
        }))
        .params(param_specs(true));
    for s in streams(sr) {
        b = b.stream(s);
    }
    let s = b.build();
    let ix = |id: &str| s.index_of(id).expect(id);
    let p = ParamIx {
        bypass: ix("bypass"),
        output_gain: ix("output_gain"),
        gain_scale: ix("gain_scale"),
        auto_gain: ix("auto_gain"),
        output_pan: ix("output_pan"),
        pan_mode: ix("pan_mode"),
        phase_invert: ix("phase_invert"),
        processing_mode: ix("processing_mode"),
        lp_quality: ix("lp_quality"),
        character: ix("character"),
        gain_q: ix("gain_q"),
        analyzer_pre: ix("analyzer_pre"),
        analyzer_post: ix("analyzer_post"),
        analyzer_sc: ix("analyzer_sc"),
        analyzer_resolution: ix("analyzer_resolution"),
        src_kind: ix("src_kind"),
        src_freq: ix("src_freq"),
        src_level: ix("src_level"),
        sc_kind: ix("sc_kind"),
        sc_level: ix("sc_level"),
        bands: (1..=BANDS)
            .map(|n| BandIx {
                on: ix(&format!("b{n}_on")),
                shape: ix(&format!("b{n}_shape")),
                freq: ix(&format!("b{n}_freq")),
                gain: ix(&format!("b{n}_gain")),
                q: ix(&format!("b{n}_q")),
                slope: ix(&format!("b{n}_slope")),
                place: ix(&format!("b{n}_place")),
                solo: ix(&format!("b{n}_solo")),
                dyn_on: ix(&format!("b{n}_dyn_on")),
                dyn_range: ix(&format!("b{n}_dyn_range")),
                dyn_thr: ix(&format!("b{n}_dyn_thr")),
                dyn_auto: ix(&format!("b{n}_dyn_auto")),
                dyn_attack: ix(&format!("b{n}_dyn_attack")),
                dyn_release: ix(&format!("b{n}_dyn_release")),
                dyn_sc: ix(&format!("b{n}_dyn_sc")),
            })
            .collect(),
    };
    (s, p)
}

/// Read one band's settings from the bridge on the audio thread (standalone
/// only; the plug-in reads nih-plug parameters directly). Every read is one
/// relaxed atomic load of the plain value; enum-like parameters are rounded
/// to their index and toggles compared against 0.5.
#[inline]
pub fn read_band(audio: &AudioHandle, ix: &BandIx) -> BandSettings {
    BandSettings {
        on: audio.param(ix.on) >= 0.5,
        kind: Kind::from_index(audio.param(ix.shape).round() as usize),
        freq: audio.param(ix.freq),
        gain_db: audio.param(ix.gain),
        q: audio.param(ix.q),
        slope: audio.param(ix.slope).round() as usize,
        placement: Placement::from_index(audio.param(ix.place).round() as usize),
        solo: audio.param(ix.solo) >= 0.5,
        dynamics: DynSettings {
            on: audio.param(ix.dyn_on) >= 0.5,
            range_db: audio.param(ix.dyn_range),
            threshold_db: audio.param(ix.dyn_thr),
            auto_threshold: audio.param(ix.dyn_auto) >= 0.5,
            attack_ms: audio.param(ix.dyn_attack),
            release_ms: audio.param(ix.dyn_release),
            external: audio.param(ix.dyn_sc) >= 0.5,
        },
    }
}

/// Read the global settings from the bridge on the audio thread (standalone
/// only). Percent parameters (`gain_scale`, `output_pan`) are converted to
/// the unit ranges [`Globals`] expects.
#[inline]
pub fn read_globals(audio: &AudioHandle, ix: &ParamIx) -> Globals {
    Globals {
        bypass: audio.param(ix.bypass) >= 0.5,
        output_gain_db: audio.param(ix.output_gain),
        gain_scale: audio.param(ix.gain_scale) / 100.0,
        auto_gain: audio.param(ix.auto_gain) >= 0.5,
        pan: audio.param(ix.output_pan) / 100.0,
        pan_ms: audio.param(ix.pan_mode) >= 0.5,
        phase_invert: audio.param(ix.phase_invert) >= 0.5,
        mode: Mode::from_index(audio.param(ix.processing_mode).round() as usize),
        quality: audio.param(ix.lp_quality).round() as usize,
        character: audio.param(ix.character).round() as usize,
        gain_q: audio.param(ix.gain_q) >= 0.5,
    }
}
