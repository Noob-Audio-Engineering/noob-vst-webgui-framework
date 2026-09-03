//! Telemetry streams: fixed-capacity `f32` frames published from the audio
//! thread with latest-wins semantics.
//!
//! A stream is declared once with a [`StreamSpec`] (id, kind, capacity,
//! channel count, free-form metadata) and gets a dense `u16` index, its
//! position in declaration order. The audio thread fills a [`StreamFrame`]
//! through [`crate::AudioHandle::publish`]; the pump thread encodes it as a
//! `StreamF32` wire frame and sends it to every subscribed client.
//!
//! Frames are *not* queued: each stream is a triple buffer
//! ([`crate::rt::mailbox`]), so a slow consumer sees the newest frame and
//! skips the ones in between. `seq` increments per publish so a UI can tell
//! how many it missed. Streams that describe *state* rather than a signal
//! (a response curve, a wavetable) should be marked [`sticky`](StreamSpec::sticky)
//! so a client that connects later receives the last frame at once.

use serde::Serialize;
use serde_json::Value;

/// A hint for the UI about what a stream contains. Purely descriptive; the
/// bytes are always a flat `f32` array.
///
/// Serialized in lowercase in the manifest (`"meter"`, `"spectrum"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    /// Peak / RMS levels, one or more channels.
    Meter,
    /// Magnitude spectrum in dB, one value per bin.
    Spectrum,
    /// Time-domain samples.
    Waveform,
    /// Sampled transfer curve (e.g. EQ magnitude response).
    Curve,
    /// Anything else.
    Raw,
}

/// Declaration of one stream.
///
/// ```
/// use vst3_web_stratum::{StreamKind, StreamSpec};
///
/// let spectrum = StreamSpec::new("spectrum_post", 1025)
///     .name("Output Spectrum")
///     .kind(StreamKind::Spectrum)
///     .meta(serde_json::json!({ "fft_size": 2048, "db": true }));
/// let curve = StreamSpec::new("curve", 512).kind(StreamKind::Curve).sticky();
/// assert_eq!(spectrum.channels, 1);
/// assert!(curve.sticky);
/// ```
#[derive(Debug, Clone)]
pub struct StreamSpec {
    /// Stable identifier the page subscribes by (`client.stream("meter")`).
    pub id: String,
    /// Human-readable name. Defaults to `id`.
    pub name: String,
    /// What the values mean. Defaults to [`StreamKind::Raw`].
    pub kind: StreamKind,
    /// Maximum number of `f32` values in one frame. Storage is allocated up
    /// front; frames may be shorter. At least `1`.
    pub capacity: usize,
    /// Interleaved channel count, for the UI's benefit. At least `1`.
    pub channels: u16,
    /// Free-form metadata (sample rate, FFT size, dB range, ...). Shipped
    /// verbatim in the manifest.
    pub meta: Value,
    /// Replay the most recent frame to every client that connects. For
    /// state-like streams published only on change (a response curve, a
    /// wavetable), so a late client is not left waiting for the next change.
    pub sticky: bool,
}

impl StreamSpec {
    /// A raw, single-channel, non-sticky stream named after its id, holding
    /// up to `capacity` values per frame (clamped to at least `1`).
    pub fn new(id: impl Into<String>, capacity: usize) -> Self {
        let id = id.into();
        StreamSpec {
            name: id.clone(),
            id,
            kind: StreamKind::Raw,
            capacity: capacity.max(1),
            channels: 1,
            meta: Value::Null,
            sticky: false,
        }
    }
    /// Mark the stream sticky (see [`StreamSpec::sticky`]).
    pub fn sticky(mut self) -> Self {
        self.sticky = true;
        self
    }
    /// Set the display name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
    /// Set the kind hint.
    pub fn kind(mut self, kind: StreamKind) -> Self {
        self.kind = kind;
        self
    }
    /// Set the interleaved channel count (clamped to at least `1`).
    pub fn channels(mut self, channels: u16) -> Self {
        self.channels = channels.max(1);
        self
    }
    /// Attach free-form metadata.
    pub fn meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }
}

/// Serialized form of a stream as it appears in the manifest.
#[derive(Debug, Clone, Serialize)]
pub struct StreamManifest {
    /// Dense index, the `arg` of the stream's wire frames.
    pub index: u16,
    /// See [`StreamSpec::id`].
    pub id: String,
    /// See [`StreamSpec::name`].
    pub name: String,
    /// See [`StreamSpec::kind`].
    pub kind: StreamKind,
    /// See [`StreamSpec::capacity`].
    pub capacity: usize,
    /// See [`StreamSpec::channels`].
    pub channels: u16,
    /// See [`StreamSpec::meta`].
    pub meta: Value,
    /// See [`StreamSpec::sticky`].
    pub sticky: bool,
}

impl StreamManifest {
    /// The manifest entry for `spec` at position `index`.
    pub fn from_spec(index: u16, spec: &StreamSpec) -> Self {
        StreamManifest {
            index,
            id: spec.id.clone(),
            name: spec.name.clone(),
            kind: spec.kind,
            capacity: spec.capacity,
            channels: spec.channels,
            meta: spec.meta.clone(),
            sticky: spec.sticky,
        }
    }
}

/// One published frame. Lives inside a mailbox slot; never reallocated.
///
/// The audio thread fills `data[..len]`; the pump thread reads
/// [`samples`](Self::samples). `seq` and `ts_ns` are stamped by
/// [`crate::AudioHandle::publish`].
pub struct StreamFrame {
    /// Publish counter of the stream, starting at `1`, wrapping. Gaps mean
    /// frames were skipped by a slow consumer.
    pub seq: u32,
    /// Publish time in nanoseconds since the bridge was created (the same
    /// clock as `Pong.server_time_us`).
    pub ts_ns: u64,
    /// Number of valid values at the front of `data`.
    pub len: usize,
    /// Backing storage of `capacity` values.
    pub data: Box<[f32]>,
}

impl StreamFrame {
    /// An empty frame with room for `capacity` values.
    pub fn with_capacity(capacity: usize) -> Self {
        StreamFrame {
            seq: 0,
            ts_ns: 0,
            len: 0,
            data: vec![0.0; capacity].into_boxed_slice(),
        }
    }

    /// The valid portion of `data`.
    #[inline]
    pub fn samples(&self) -> &[f32] {
        &self.data[..self.len.min(self.data.len())]
    }
}
