//! Parameter declarations and the lock-free value store.
//!
//! Every parameter has a dense `u16` index (its position in the manifest), a
//! stable string id, and a plain-value range with a taper. Values on the wire
//! are always *normalized* (`0.0..=1.0`); the taper and range let both ends
//! convert to plain units for display.
//!
//! # Normalized vs plain
//!
//! * **Normalized** values are what the store holds, what travels in
//!   `ParamValues` / `ParamEdit` frames, and what hosts automate. They are
//!   clamped to `0.0..=1.0` on every write.
//! * **Plain** values are in the parameter's own units (Hz, dB, an enum
//!   index). [`ParamSpec::normalize`] and [`ParamSpec::denormalize`] convert
//!   between the two using the [`Taper`]; discrete parameters snap to their
//!   steps on the way to plain.
//!
//! # Tapers and the manifest table
//!
//! The manifest ships every parameter with a 65-point table
//! ([`TABLE_POINTS`]) sampling the normalized-to-plain mapping. A UI can draw
//! a correct scale and format values for any taper from that table alone,
//! including [`Taper::Table`] parameters whose formula lives in another
//! framework (the nih-plug adapter mirrors its parameters that way).
//!
//! # Threads
//!
//! [`ParamStore`] values are [`AtomicF32`]s: any thread may read or write
//! any parameter without locking. Specs are immutable after construction.

use std::collections::HashMap;

use serde::Serialize;

use crate::rt::AtomicF32;

/// How the normalized `0..1` range maps onto `min..max`.
///
/// ```
/// use noob_vst_webgui_framework::ParamSpec;
///
/// // Log: equal normalized steps are equal ratios.
/// let f = ParamSpec::new("f", "Freq").range(20.0, 20_000.0).log();
/// assert!((f.denormalize(0.5) - 632.46).abs() < 0.1);
///
/// // Skew < 1: more resolution near the minimum.
/// let t = ParamSpec::new("t", "Time").range(0.0, 10.0).skew(0.5);
/// assert!((t.denormalize(0.5) - 2.5).abs() < 1e-5);
/// assert!((t.normalize(2.5) - 0.5).abs() < 1e-5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Taper {
    /// `plain = min + (max - min) * norm`. The default.
    Linear,
    /// Logarithmic: `plain = min * (max / min) ^ norm`. Requires `min > 0`
    /// (a non-positive `min` is clamped to `f32::MIN_POSITIVE`).
    /// Frequencies, mostly.
    Log,
    /// JUCE-style skew: `plain = min + (max - min) * norm.powf(1 / skew)`.
    /// `skew < 1` gives more resolution near `min`; `skew == 1` is linear.
    Skew(f32),
    /// Piecewise-linear through [`ParamSpec::custom_table`]. Used to mirror
    /// parameters whose mapping lives elsewhere (a plugin framework's own
    /// range types) without knowing its formula.
    Table,
}

impl Taper {
    /// The manifest spelling: `linear`, `log`, `skew` or `table`.
    fn as_str(&self) -> &'static str {
        match self {
            Taper::Linear => "linear",
            Taper::Log => "log",
            Taper::Skew(_) => "skew",
            Taper::Table => "table",
        }
    }
}

/// Declaration of one parameter. Build with the fluent methods, then hand it
/// to [`crate::NoobVstWebguiFrameworkBuilder::param`].
///
/// ```
/// use noob_vst_webgui_framework::ParamSpec;
///
/// let cutoff = ParamSpec::new("cutoff", "Cutoff")
///     .range(20.0, 20_000.0)
///     .log()
///     .default(1000.0)
///     .unit("Hz")
///     .group("filter");
/// let mode = ParamSpec::new("mode", "Mode").labels(["Clean", "Warm", "Hot"]);
/// let bypass = ParamSpec::new("bypass", "Bypass").toggle().not_automatable();
///
/// assert!((cutoff.normalize(20_000.0) - 1.0).abs() < 1e-6);
/// assert_eq!(mode.steps, 3);
/// assert_eq!(mode.denormalize(0.4), 1.0);   // snaps to "Warm"
/// assert_eq!(bypass.denormalize(0.51), 1.0);
/// ```
#[derive(Debug, Clone)]
pub struct ParamSpec {
    /// Stable identifier used by the page (`client.param("cutoff")`) and by
    /// [`crate::NoobVstWebguiFramework::index_of`]. Must be unique within a bridge.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Unit suffix for display (`"Hz"`, `"dB"`). Empty by default.
    pub unit: String,
    /// Free-form group name for UI layout. Empty by default.
    pub group: String,
    /// Plain value at normalized `0`. Default `0.0`.
    pub min: f32,
    /// Plain value at normalized `1`. Default `1.0`.
    pub max: f32,
    /// Default in plain units. Default `0.0`.
    pub default: f32,
    /// Mapping between normalized and plain. Default [`Taper::Linear`].
    pub taper: Taper,
    /// `0` for continuous, otherwise the number of discrete steps (a toggle
    /// is `2`). Discrete values are snapped in [`ParamSpec::denormalize`].
    pub steps: u32,
    /// Optional names for discrete steps (enum-style parameters).
    pub labels: Vec<String>,
    /// Whether hosts may automate this parameter. Purely advisory here;
    /// adapters read it when registering the parameter with the host.
    /// Default `true`.
    pub automatable: bool,
    /// How many decimal places the plain value is meaningful to. `None`
    /// means "no opinion".
    ///
    /// **It states how exactly the value is known, not how it should look.**
    /// That distinction decides more than formatting: a consumer that stores
    /// the value --- into a preset, a slot, a saved project --- should store
    /// it at this precision too, because at the stated precision
    /// `7.0000000000000036` and `7` are the same number, and carrying the
    /// difference carries a claim about precision that is not true. A page
    /// that read this as a display hint fixed its knob text and then wrote
    /// the raw float into a saved project, which is one fault treated as two.
    ///
    /// This exists because a count has no honest continuous rendering: a
    /// mode budget of 1,024 must not print as `1024.0`, and the two obvious
    /// ways to make the value itself integral both cost fidelity --- snapping
    /// with `steps` quantises in the normalized domain *before* the taper, so
    /// a request for 1,024 lands on 1,021, and an integer table does the
    /// same. So the value stays exact and the hint travels beside it, and
    /// **nothing in this crate rounds anything**: it is a note to whoever
    /// draws the number.
    pub decimals: Option<u8>,
    /// Normalized -> plain samples at evenly spaced normalized positions,
    /// used when `taper` is [`Taper::Table`]. Must be monotonic (ascending
    /// or descending) and hold at least two points.
    pub custom_table: Option<Vec<f32>>,
}

impl ParamSpec {
    /// A continuous linear parameter over `0..=1` with default `0`.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        ParamSpec {
            id: id.into(),
            name: name.into(),
            unit: String::new(),
            group: String::new(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            taper: Taper::Linear,
            steps: 0,
            labels: Vec::new(),
            automatable: true,
            decimals: None,
            custom_table: None,
        }
    }

    /// Set the plain range. `min` may exceed `max` for an inverted linear
    /// range; a log taper needs `min > 0`.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }
    /// Set the default, in plain units.
    pub fn default(mut self, plain: f32) -> Self {
        self.default = plain;
        self
    }
    /// Set the display unit.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }
    /// Set the UI group.
    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }
    /// Use a [`Taper::Log`] mapping.
    pub fn log(mut self) -> Self {
        self.taper = Taper::Log;
        self
    }
    /// Use a [`Taper::Skew`] mapping with the given factor.
    pub fn skew(mut self, skew: f32) -> Self {
        self.taper = Taper::Skew(skew);
        self
    }
    /// Use a sampled mapping (at least two points, monotonic). Also sets
    /// `min` / `max` from the table's ends. A table with fewer than two
    /// points is ignored.
    ///
    /// ```
    /// use noob_vst_webgui_framework::{ParamSpec, Taper};
    ///
    /// let p = ParamSpec::new("x", "X").with_table(vec![0.0, 10.0, 100.0]);
    /// assert_eq!(p.taper, Taper::Table);
    /// assert_eq!((p.min, p.max), (0.0, 100.0));
    /// assert_eq!(p.denormalize(0.25), 5.0);     // linear between samples
    /// assert_eq!(p.normalize(55.0), 0.75);
    /// ```
    pub fn with_table(mut self, table: Vec<f32>) -> Self {
        if table.len() >= 2 {
            self.min = table[0];
            self.max = table[table.len() - 1];
            self.taper = Taper::Table;
            self.custom_table = Some(table);
        }
        self
    }
    /// Set the number of discrete steps (`0` = continuous).
    pub fn steps(mut self, steps: u32) -> Self {
        self.steps = steps;
        self
    }
    /// A two-state parameter over `0..=1`.
    pub fn toggle(mut self) -> Self {
        self.min = 0.0;
        self.max = 1.0;
        self.steps = 2;
        self
    }
    /// An enum-style parameter; sets range (`0..=labels-1`) and step count
    /// from the labels. Plain values are the label indices.
    pub fn labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.labels = labels.into_iter().map(Into::into).collect();
        let n = self.labels.len().max(1) as u32;
        self.steps = n;
        self.min = 0.0;
        self.max = (n - 1) as f32;
        self
    }
    /// Mark the parameter as not automatable by hosts.
    pub fn not_automatable(mut self) -> Self {
        self.automatable = false;
        self
    }
    /// Say how many decimal places the plain value is meaningful to.
    ///
    /// `.decimals(0)` is the common case: a count, a mode budget, a number
    /// of voices --- something that should read `1024` and never `1024.0`.
    /// It changes **nothing** about the value, the taper or the automation;
    /// it only tells a page how to print it, which is the one thing a page
    /// otherwise has to special-case by id.
    pub fn decimals(mut self, places: u8) -> Self {
        self.decimals = Some(places);
        self
    }
    /// Shorthand for [`decimals(0)`](Self::decimals): the value is a whole
    /// number.
    pub fn integer(self) -> Self {
        self.decimals(0)
    }

    /// Plain -> normalized, clamped to `0.0..=1.0`. A zero-width range maps
    /// everything to `0`.
    pub fn normalize(&self, plain: f32) -> f32 {
        let span = self.max - self.min;
        if span == 0.0 {
            return 0.0;
        }
        let n = match self.taper {
            Taper::Linear => (plain - self.min) / span,
            Taper::Log => {
                let lo = self.min.max(f32::MIN_POSITIVE);
                (plain.max(lo) / lo).ln() / (self.max / lo).ln()
            }
            Taper::Skew(s) => ((plain - self.min) / span).max(0.0).powf(s),
            Taper::Table => match &self.custom_table {
                Some(t) if t.len() >= 2 => table_normalize(t, plain),
                _ => (plain - self.min) / span,
            },
        };
        n.clamp(0.0, 1.0)
    }

    /// Normalized -> plain, snapping to steps when discrete. The input is
    /// clamped to `0.0..=1.0` first.
    pub fn denormalize(&self, norm: f32) -> f32 {
        let mut n = norm.clamp(0.0, 1.0);
        if self.steps > 1 {
            let last = (self.steps - 1) as f32;
            n = (n * last).round() / last;
        }
        let span = self.max - self.min;
        match self.taper {
            Taper::Linear => self.min + span * n,
            Taper::Log => {
                let lo = self.min.max(f32::MIN_POSITIVE);
                lo * (self.max / lo).powf(n)
            }
            Taper::Skew(s) => self.min + span * n.powf(1.0 / s),
            Taper::Table => match &self.custom_table {
                Some(t) if t.len() >= 2 => table_denormalize(t, n),
                _ => self.min + span * n,
            },
        }
    }

    /// Normalized default: `normalize(default)`.
    pub fn default_normalized(&self) -> f32 {
        self.normalize(self.default)
    }

    /// Sample the normalized -> plain mapping at `n` evenly spaced points
    /// (at least two). Shipped in the manifest so a UI can draw scales for
    /// any taper, including ones it has no formula for.
    pub fn table(&self, n: usize) -> Vec<f32> {
        let n = n.max(2);
        (0..n)
            .map(|i| self.denormalize(i as f32 / (n - 1) as f32))
            .collect()
    }
}

/// Linear interpolation into a monotonic sample table.
fn table_denormalize(t: &[f32], n: f32) -> f32 {
    let x = n * (t.len() - 1) as f32;
    let i = (x.floor() as usize).min(t.len() - 2);
    let f = x - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

/// Inverse of [`table_denormalize`]: binary search for the segment, then
/// interpolate. Works for ascending and descending tables.
fn table_normalize(t: &[f32], plain: f32) -> f32 {
    let asc = t[t.len() - 1] >= t[0];
    let mut lo = 0usize;
    let mut hi = t.len() - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if (t[mid] <= plain) == asc {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let d = t[hi] - t[lo];
    let f = if d == 0.0 {
        0.0
    } else {
        ((plain - t[lo]) / d).clamp(0.0, 1.0)
    };
    (lo as f32 + f) / (t.len() - 1) as f32
}

/// Number of points in the manifest lookup table (`table` field of every
/// parameter): 64 segments, so a 65th point lands exactly on `max`.
pub const TABLE_POINTS: usize = 65;

/// Serialized form of a parameter as it appears in the manifest.
///
/// Field names are the JSON keys. See `docs/WIRE.md` for the full manifest.
#[derive(Debug, Clone, Serialize)]
pub struct ParamManifest {
    /// Dense index used in `ParamValues` / `ParamEdit` frames.
    pub index: u16,
    /// See [`ParamSpec::id`].
    pub id: String,
    /// See [`ParamSpec::name`].
    pub name: String,
    /// See [`ParamSpec::unit`].
    pub unit: String,
    /// See [`ParamSpec::group`].
    pub group: String,
    /// See [`ParamSpec::min`].
    pub min: f32,
    /// See [`ParamSpec::max`].
    pub max: f32,
    /// Default in plain units.
    pub default: f32,
    /// Default in normalized units, so a client can reset without converting.
    pub default_norm: f32,
    /// `linear`, `log`, `skew` or `table`.
    pub taper: &'static str,
    /// The skew factor; present only when `taper` is `skew`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skew: Option<f32>,
    /// See [`ParamSpec::steps`].
    pub steps: u32,
    /// See [`ParamSpec::labels`].
    pub labels: Vec<String>,
    /// See [`ParamSpec::automatable`].
    pub automatable: bool,
    /// See [`ParamSpec::decimals`]. Absent when the spec has no opinion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// [`TABLE_POINTS`] samples of the normalized -> plain mapping.
    pub table: Vec<f32>,
}

impl ParamManifest {
    /// The manifest entry for `spec` at position `index`.
    pub fn from_spec(index: u16, spec: &ParamSpec) -> Self {
        ParamManifest {
            index,
            id: spec.id.clone(),
            name: spec.name.clone(),
            unit: spec.unit.clone(),
            group: spec.group.clone(),
            min: spec.min,
            max: spec.max,
            default: spec.default,
            default_norm: spec.default_normalized(),
            taper: spec.taper.as_str(),
            skew: match spec.taper {
                Taper::Skew(s) => Some(s),
                _ => None,
            },
            steps: spec.steps,
            labels: spec.labels.clone(),
            automatable: spec.automatable,
            decimals: spec.decimals,
            table: spec.table(TABLE_POINTS),
        }
    }
}

/// Lock-free store of normalized parameter values, readable and writable
/// from any thread. Values start at each spec's default.
///
/// ```
/// use noob_vst_webgui_framework::params::{ParamSpec, ParamStore};
///
/// let store = ParamStore::new(vec![
///     ParamSpec::new("gain", "Gain").range(-24.0, 24.0).default(0.0),
/// ]);
/// assert_eq!(store.index_of("gain"), Some(0));
/// assert_eq!(store.get_normalized(0), 0.5);
/// assert!(store.set_normalized(0, 1.0));
/// assert_eq!(store.get_plain(0), 24.0);
/// assert!(!store.set_normalized(7, 0.0));   // unknown index
/// ```
pub struct ParamStore {
    specs: Vec<ParamSpec>,
    values: Vec<AtomicF32>,
    by_id: HashMap<String, usize>,
}

impl ParamStore {
    /// A store for `specs`, indexed in the given order, every value at its
    /// default.
    pub fn new(specs: Vec<ParamSpec>) -> Self {
        let values = specs
            .iter()
            .map(|s| AtomicF32::new(s.default_normalized()))
            .collect();
        let by_id = specs
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.clone(), i))
            .collect();
        ParamStore {
            specs,
            values,
            by_id,
        }
    }

    /// Number of parameters.
    pub fn len(&self) -> usize {
        self.specs.len()
    }
    /// Whether there are no parameters.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
    /// Every spec, in index order.
    pub fn specs(&self) -> &[ParamSpec] {
        &self.specs
    }
    /// The spec at `index`, if any.
    pub fn spec(&self, index: usize) -> Option<&ParamSpec> {
        self.specs.get(index)
    }
    /// The index of the parameter with id `id`, if any.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// Normalized value. Out-of-range indices read as `0.0`. Wait-free.
    #[inline]
    pub fn get_normalized(&self, index: usize) -> f32 {
        self.values.get(index).map(AtomicF32::load).unwrap_or(0.0)
    }

    /// Plain value (`denormalize` of the stored normalized value).
    /// Out-of-range indices read as `0.0`. Wait-free.
    #[inline]
    pub fn get_plain(&self, index: usize) -> f32 {
        match self.specs.get(index) {
            Some(s) => s.denormalize(self.values[index].load()),
            None => 0.0,
        }
    }

    /// Store a normalized value (clamped). Returns `false` for a bad index.
    /// Wait-free.
    #[inline]
    pub fn set_normalized(&self, index: usize, norm: f32) -> bool {
        match self.values.get(index) {
            Some(v) => {
                v.store(norm.clamp(0.0, 1.0));
                true
            }
            None => false,
        }
    }

    /// Store a plain value. Returns the normalized value that was stored,
    /// or `None` for a bad index. Wait-free.
    #[inline]
    pub fn set_plain(&self, index: usize, plain: f32) -> Option<f32> {
        let spec = self.specs.get(index)?;
        let n = spec.normalize(plain);
        self.values[index].store(n);
        Some(n)
    }

    /// The manifest entries for every parameter.
    pub fn manifest(&self) -> Vec<ParamManifest> {
        self.specs
            .iter()
            .enumerate()
            .map(|(i, s)| ParamManifest::from_spec(i as u16, s))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The display hint reaches the manifest, and stays out of it when the
    /// spec has no opinion --- an older page must not see a field at all
    /// rather than see a default it would then obey.
    #[test]
    fn the_decimals_hint_travels_and_is_absent_by_default() {
        let plain = ParamSpec::new("gain", "Gain");
        assert_eq!(plain.decimals, None);
        let j = serde_json::to_string(&ParamManifest::from_spec(0, &plain)).unwrap();
        assert!(!j.contains("decimals"), "absent when unset, got {j}");

        // A count: the value stays exact, only its rendering is pinned.
        let counted = ParamSpec::new("mode_budget", "Modes")
            .range(4.0, 4096.0)
            .default(1024.0)
            .integer();
        assert_eq!(counted.decimals, Some(0));
        assert_eq!(
            counted.default, 1024.0,
            "the hint must not quantise the value --- that is what `steps`              would have done, and it lands on 1021"
        );
        let m = ParamManifest::from_spec(1, &counted);
        assert_eq!(m.decimals, Some(0));
        assert_eq!(m.default, 1024.0);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-3 * b.abs().max(1.0)
    }

    #[test]
    fn linear_round_trip() {
        let p = ParamSpec::new("g", "Gain").range(-24.0, 24.0).default(0.0);
        assert!(close(p.normalize(0.0), 0.5));
        assert!(close(p.denormalize(0.5), 0.0));
        assert!(close(p.denormalize(1.0), 24.0));
        assert!(close(p.normalize(p.denormalize(0.3)), 0.3));
    }

    #[test]
    fn log_round_trip() {
        let p = ParamSpec::new("f", "Freq").range(20.0, 20000.0).log();
        assert!(close(p.denormalize(0.0), 20.0));
        assert!(close(p.denormalize(1.0), 20000.0));
        // Geometric midpoint.
        assert!(close(p.denormalize(0.5), (20.0f32 * 20000.0).sqrt()));
        for n in [0.0, 0.1, 0.33, 0.5, 0.9, 1.0] {
            assert!(close(p.normalize(p.denormalize(n)), n));
        }
    }

    #[test]
    fn skew_round_trip_and_clamp() {
        let p = ParamSpec::new("t", "Time").range(0.0, 10.0).skew(0.5);
        for n in [0.0, 0.25, 0.5, 1.0] {
            assert!(close(p.normalize(p.denormalize(n)), n));
        }
        assert_eq!(p.normalize(-5.0), 0.0);
        assert_eq!(p.normalize(50.0), 1.0);
    }

    #[test]
    fn table_round_trip() {
        // A sampled log mapping, as a mirrored plugin parameter would ship.
        let log = ParamSpec::new("f", "Freq").range(20.0, 20000.0).log();
        let p = ParamSpec::new("f", "Freq").with_table(log.table(65));
        assert_eq!(p.taper, Taper::Table);
        assert!(close(p.min, 20.0));
        assert!(close(p.max, 20000.0));
        for n in [0.0, 0.2, 0.5, 0.77, 1.0] {
            assert!(close(p.normalize(p.denormalize(n)), n));
            // Within 1% of the analytic mapping between sample points.
            let a = log.denormalize(n);
            assert!((p.denormalize(n) - a).abs() <= 0.01 * a);
        }
        // Descending tables work too.
        let d = ParamSpec::new("d", "Desc").with_table(vec![10.0, 5.0, 0.0]);
        assert!(close(d.denormalize(0.5), 5.0));
        assert!(close(d.normalize(7.5), 0.25));
    }

    #[test]
    fn steps_snap() {
        let p = ParamSpec::new("m", "Mode").labels(["A", "B", "C"]);
        assert_eq!(p.steps, 3);
        assert_eq!(p.max, 2.0);
        assert_eq!(p.denormalize(0.4), 1.0);
        assert_eq!(p.denormalize(0.8), 2.0);
        assert_eq!(p.denormalize(0.1), 0.0);
        let t = ParamSpec::new("b", "Bypass").toggle();
        assert_eq!(t.denormalize(0.49), 0.0);
        assert_eq!(t.denormalize(0.51), 1.0);
    }

    #[test]
    fn store_and_manifest() {
        let store = ParamStore::new(vec![
            ParamSpec::new("a", "A").range(0.0, 100.0).default(50.0),
            ParamSpec::new("b", "B")
                .range(1.0, 1000.0)
                .log()
                .default(10.0),
        ]);
        assert_eq!(store.len(), 2);
        assert_eq!(store.index_of("b"), Some(1));
        assert!(close(store.get_normalized(0), 0.5));
        assert!(close(store.get_plain(1), 10.0));
        assert!(store.set_normalized(0, 2.0));
        assert_eq!(store.get_normalized(0), 1.0);
        assert!(!store.set_normalized(9, 0.0));
        assert!(close(store.set_plain(1, 100.0).unwrap(), 2.0 / 3.0));
        let m = store.manifest();
        assert_eq!(m[1].taper, "log");
        assert_eq!(m[1].table.len(), TABLE_POINTS);
        assert!(close(m[1].table[0], 1.0));
        assert!(close(m[1].table[TABLE_POINTS - 1], 1000.0));
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"id\":\"a\""));
    }
}
