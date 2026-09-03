//! Binary wire format shared by the Rust bridge and the browser client.
//!
//! Every frame is a WebSocket *binary* message laid out as:
//!
//! ```text
//! offset  size  field
//! 0       u8    kind      (see [`Kind`])
//! 1       u8    flags     (frame-level flags; reserved, always 0 in protocol 1)
//! 2       u16   arg       (kind specific: entry count or stream index)
//! 4       ...   payload
//! ```
//!
//! All integers are little-endian. Payloads are laid out so that any `f32`
//! array starts at an offset that is a multiple of four; the browser can then
//! wrap it in a `Float32Array` view with no copy at all.
//!
//! | kind | name | direction | payload |
//! |---|---|---|---|
//! | `0x01` | [`Kind::Hello`] | s → c | `u16 protocol, u16 param_count, u16 stream_count, u16 client_id` |
//! | `0x10` | [`Kind::ParamValues`] | s → c | `arg` entries of `u16 index, u16 flags, f32 value` |
//! | `0x11` | [`Kind::ParamEdit`] | c → s | `arg` entries of `u16 index, u8 phase, u8 pad, f32 value` |
//! | `0x12` | [`Kind::Events`] | c → s | `arg` entries of `u8 kind, u8 channel, u8 a, u8 b, f32 value, u32 offset` |
//! | `0x13` | [`Kind::EventsOut`] | s → c | same as Events |
//! | `0x20` | [`Kind::StreamF32`] | s → c | `u32 seq, u64 ts_ns, u32 len`, then `len` × `f32` at offset [`STREAM_DATA_OFFSET`] |
//! | `0x21` | [`Kind::StreamU8`] | s → c | `u32 seq, u64 ts_ns, u32 len`, then `len` bytes at offset 20 |
//! | `0x30` | [`Kind::Ping`] | c → s | `f64 client_time` |
//! | `0x31` | [`Kind::Pong`] | s → c | `f64 client_time, f64 server_time_us` |
//! | `0x40` | [`Kind::Subscribe`] | c → s | `u32 min_interval_us, u8 enabled, 3 × pad` |
//!
//! # Encoding and decoding
//!
//! Encoders append to a caller-owned `Vec<u8>` (clearing it first) so one
//! buffer can be reused frame after frame without allocating; the batch
//! writers ([`ParamValuesWriter`], [`ParamEditWriter`], [`EventsWriter`])
//! patch the entry count into the header on `finish`. [`Frame::decode`]
//! borrows from the receive buffer and never panics on malformed input;
//! entries are decoded lazily by the iterators on [`ParamValues`],
//! [`ParamEdits`] and [`Events`].
//!
//! ```
//! use noob_vst_webgui_framework::wire::{EditPhase, Frame, ParamEditWriter};
//!
//! let mut buf = Vec::new();
//! let mut w = ParamEditWriter::begin(&mut buf);
//! w.push(3, EditPhase::Begin, 0.5).push(3, EditPhase::End, 0.5);
//! assert_eq!(w.finish(), 2);
//!
//! match Frame::decode(&buf).unwrap() {
//!     Frame::ParamEdits(edits) => {
//!         let v: Vec<_> = edits.iter().collect();
//!         assert_eq!((v[0].index, v[0].phase, v[0].value), (3, EditPhase::Begin, 0.5));
//!         assert_eq!(v[1].phase, EditPhase::End);
//!     }
//!     other => panic!("unexpected {other:?}"),
//! }
//! ```
//!
//! Control-plane data (the manifest, ad-hoc JSON messages, the UI store)
//! travels as WebSocket *text* frames. The complete protocol, byte tables,
//! text frames and versioning rules are in `docs/WIRE.md`.

use core::fmt;

/// Protocol version advertised in the `Hello` frame, the manifest and
/// `/instance`. Bump on breaking changes (any change to an existing layout,
/// flag meaning or the connect sequence); additions do not bump it.
pub const PROTOCOL_VERSION: u16 = 1;

/// Size of the fixed frame header (`kind`, `flags`, `arg`).
pub const HEADER_LEN: usize = 4;

/// Offset of the float payload inside a `StreamF32` frame: header (4) plus
/// `seq` (4), `ts_ns` (8) and `len` (4). A multiple of four, so the browser
/// can view the payload as a `Float32Array` in place.
pub const STREAM_DATA_OFFSET: usize = 20;

/// Size of one entry in a `ParamValues` frame: `u16 index, u16 flags, f32 value`.
pub const PARAM_VALUE_ENTRY_LEN: usize = 8;

/// Size of one entry in a `ParamEdit` frame: `u16 index, u8 phase, u8 pad, f32 value`.
pub const PARAM_EDIT_ENTRY_LEN: usize = 8;

/// Frame kinds. The numeric value is what goes on the wire (byte 0).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// server -> client. First binary frame after connect. `arg` = 0.
    Hello = 0x01,
    /// server -> client. Batch of normalized parameter values. `arg` = count.
    ParamValues = 0x10,
    /// client -> server. Batch of parameter edits with gesture phase.
    /// `arg` = count.
    ParamEdit = 0x11,
    /// client -> server. Batch of events (notes, controllers, custom).
    /// `arg` = count.
    Events = 0x12,
    /// server -> client. Batch of events the plugin wants the UI to see.
    /// `arg` = count.
    EventsOut = 0x13,
    /// server -> client. One frame of `f32` telemetry for a stream.
    /// `arg` = stream index.
    StreamF32 = 0x20,
    /// server -> client. One frame of opaque bytes for a stream.
    /// `arg` = stream index. Reserved; the server currently sends only
    /// `StreamF32`.
    StreamU8 = 0x21,
    /// client -> server. Latency probe. `arg` = 0.
    Ping = 0x30,
    /// server -> client. Latency probe reply. `arg` = 0.
    Pong = 0x31,
    /// client -> server. Per-stream rate limit / enable. `arg` = stream index.
    Subscribe = 0x40,
}

impl Kind {
    /// The kind for a wire byte, or `None` for an unassigned value.
    pub fn from_u8(v: u8) -> Option<Kind> {
        Some(match v {
            0x01 => Kind::Hello,
            0x10 => Kind::ParamValues,
            0x11 => Kind::ParamEdit,
            0x12 => Kind::Events,
            0x13 => Kind::EventsOut,
            0x20 => Kind::StreamF32,
            0x21 => Kind::StreamU8,
            0x30 => Kind::Ping,
            0x31 => Kind::Pong,
            0x40 => Kind::Subscribe,
            _ => return None,
        })
    }
}

/// Per-entry flag on a `ParamValues` entry: this value is the echo of an edit
/// the receiving client itself sent. Clients use it to measure round-trip
/// latency and to avoid fighting their own drag gesture. Set by the pump for
/// the originating client only, and only when `ServerConfig::echo_edits` is
/// on.
pub const PARAM_FLAG_ECHO: u16 = 0x0001;
/// Per-entry flag: the change originated from the host (automation, preset
/// load) via `NoobVstWebguiFramework::set_param*` / `sync_all_params`, not from a client
/// or the audio thread.
pub const PARAM_FLAG_HOST: u16 = 0x0002;

/// Gesture phase of a parameter edit, mirroring VST3 `beginEdit` /
/// `performEdit` / `endEdit`. The numeric value is the wire byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPhase {
    /// The user grabbed the control; hosts start an automation gesture.
    Begin = 0,
    /// A value while the gesture is in progress (or a one-shot change).
    Perform = 1,
    /// The user released the control; hosts close the gesture.
    End = 2,
}

impl EditPhase {
    /// The phase for a wire byte, or `None` for anything but `0..=2`.
    pub fn from_u8(v: u8) -> Option<EditPhase> {
        Some(match v {
            0 => EditPhase::Begin,
            1 => EditPhase::Perform,
            2 => EditPhase::End,
            _ => return None,
        })
    }
}

/// One entry of a `ParamValues` frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamValue {
    /// Dense parameter index.
    pub index: u16,
    /// [`PARAM_FLAG_ECHO`] and / or [`PARAM_FLAG_HOST`], or `0`.
    pub flags: u16,
    /// Normalized value in `0.0..=1.0`.
    pub value: f32,
}

/// One entry of a `ParamEdit` frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEdit {
    /// Dense parameter index.
    pub index: u16,
    /// Where in the gesture this edit sits.
    pub phase: EditPhase,
    /// Normalized value in `0.0..=1.0` (clamped by the receiver).
    pub value: f32,
}

/// Size of one entry in an `Events` / `EventsOut` frame:
/// `u8 kind, u8 channel, u8 a, u8 b, f32 value, u32 offset`.
pub const EVENT_ENTRY_LEN: usize = 12;

/// Well-known event kinds (the `kind` byte of a [`UiEvent`]). Values
/// `>= 0x80` are plugin-defined; `0` is unused.
pub mod event_kind {
    /// `a` = note number, `value` = velocity 0..1.
    pub const NOTE_ON: u8 = 1;
    /// `a` = note number, `value` = release velocity 0..1.
    pub const NOTE_OFF: u8 = 2;
    /// `a` = controller number, `value` = 0..1.
    pub const CONTROL: u8 = 3;
    /// `value` = -1..1.
    pub const PITCH_BEND: u8 = 4;
    /// `a` = note (or 0 for channel pressure), `value` = 0..1.
    pub const AFTERTOUCH: u8 = 5;
    /// `a` = program number.
    pub const PROGRAM: u8 = 6;
    /// First plugin-defined kind.
    pub const CUSTOM: u8 = 0x80;
}

/// A small, fixed-size event: notes and controllers from an on-screen
/// keyboard, or anything a plugin wants to signal to its UI. Real-time safe
/// to pass around (plain `Copy` data, 12 bytes on the wire).
///
/// ```
/// use noob_vst_webgui_framework::{UiEvent, event_kind};
///
/// let on = UiEvent::note_on(0, 60, 0.8);
/// assert_eq!((on.kind, on.a), (event_kind::NOTE_ON, 60));
/// let custom = UiEvent::custom(0x81, 3, 4, 0.5);
/// assert!(custom.kind >= event_kind::CUSTOM);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiEvent {
    /// See [`event_kind`].
    pub kind: u8,
    /// MIDI-style channel, `0..=15`; `0` when it does not apply.
    pub channel: u8,
    /// Note number, controller number, or plugin-defined.
    pub a: u8,
    /// Plugin-defined.
    pub b: u8,
    /// Velocity, amount, or plugin-defined.
    pub value: f32,
    /// Sample offset within the current block (server -> client: unused).
    pub offset: u32,
}

impl UiEvent {
    /// A [`event_kind::NOTE_ON`] for `note` with `velocity` in `0..1`.
    pub fn note_on(channel: u8, note: u8, velocity: f32) -> Self {
        UiEvent {
            kind: event_kind::NOTE_ON,
            channel,
            a: note,
            b: 0,
            value: velocity,
            offset: 0,
        }
    }
    /// A [`event_kind::NOTE_OFF`] for `note` with release `velocity`.
    pub fn note_off(channel: u8, note: u8, velocity: f32) -> Self {
        UiEvent {
            kind: event_kind::NOTE_OFF,
            channel,
            a: note,
            b: 0,
            value: velocity,
            offset: 0,
        }
    }
    /// A plugin-defined event. `kind` is raised to at least
    /// [`event_kind::CUSTOM`] so it can never collide with a well-known kind.
    pub fn custom(kind: u8, a: u8, b: u8, value: f32) -> Self {
        UiEvent {
            kind: kind.max(event_kind::CUSTOM),
            channel: 0,
            a,
            b,
            value,
            offset: 0,
        }
    }
}

/// Borrowed view over `Events` / `EventsOut` entries. Entries are decoded
/// lazily by [`iter`](Self::iter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Events<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> Events<'a> {
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.count
    }
    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Decode the entries in order.
    pub fn iter(&self) -> impl Iterator<Item = UiEvent> + 'a {
        self.bytes
            .as_chunks::<EVENT_ENTRY_LEN>()
            .0
            .iter()
            .map(|c| UiEvent {
                kind: c[0],
                channel: c[1],
                a: c[2],
                b: c[3],
                value: f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                offset: u32::from_le_bytes([c[8], c[9], c[10], c[11]]),
            })
    }
}

/// Why [`Frame::decode`] rejected a buffer. The whole frame is rejected;
/// nothing is applied partially.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// Fewer bytes than the header, or than the kind's fixed payload.
    TooShort,
    /// Byte 0 is not a [`Kind`]; carries the byte.
    UnknownKind(u8),
    /// A `ParamEdit` entry has a phase byte outside `0..=2`; carries the byte.
    BadPhase(u8),
    /// Declared length does not match the bytes present.
    LengthMismatch,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::TooShort => write!(f, "frame too short"),
            WireError::UnknownKind(k) => write!(f, "unknown frame kind 0x{k:02x}"),
            WireError::BadPhase(p) => write!(f, "bad edit phase {p}"),
            WireError::LengthMismatch => write!(f, "frame length mismatch"),
        }
    }
}

impl std::error::Error for WireError {}

/// A decoded frame borrowing from the receive buffer. Produced by
/// [`Frame::decode`]; the batch variants hold views that decode entries on
/// iteration.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame<'a> {
    /// First frame from the server.
    Hello {
        /// The server's [`PROTOCOL_VERSION`].
        version: u16,
        /// Number of parameters in the manifest.
        param_count: u16,
        /// Number of streams in the manifest.
        stream_count: u16,
        /// This connection's id, never `0`.
        client_id: u16,
    },
    /// Normalized values from the server.
    ParamValues(ParamValues<'a>),
    /// Edits from a client; every phase byte was validated on decode.
    ParamEdits(ParamEdits<'a>),
    /// client -> server events.
    Events(Events<'a>),
    /// server -> client events.
    EventsOut(Events<'a>),
    /// One frame of `f32` telemetry.
    StreamF32 {
        /// Stream index.
        stream: u16,
        /// Publish counter of the stream.
        seq: u32,
        /// Publish time, nanoseconds since the bridge was created.
        ts_ns: u64,
        /// Raw little-endian `f32` bytes. Not necessarily aligned; use
        /// [`f32_iter`] or [`read_f32s`] to consume.
        data: &'a [u8],
    },
    /// One frame of opaque bytes (reserved).
    StreamU8 {
        /// Stream index.
        stream: u16,
        /// Publish counter of the stream.
        seq: u32,
        /// Publish time, nanoseconds since the bridge was created.
        ts_ns: u64,
        /// The bytes.
        data: &'a [u8],
    },
    /// Latency probe from a client.
    Ping {
        /// Whatever clock value the client sent; echoed back in `Pong`.
        client_time: f64,
    },
    /// Reply to a `Ping`.
    Pong {
        /// The client's value, untouched.
        client_time: f64,
        /// Microseconds since the bridge was created, at the time of reply.
        server_time_us: f64,
    },
    /// Per-stream rate limit from a client.
    Subscribe {
        /// Stream index.
        stream: u16,
        /// Minimum interval between frames in microseconds; `0` = every frame.
        min_interval_us: u32,
        /// `false` turns the stream off for this client entirely.
        enabled: bool,
    },
}

/// Borrowed view over `ParamValues` entries. Entries are decoded lazily by
/// [`iter`](Self::iter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamValues<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> ParamValues<'a> {
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.count
    }
    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Decode the entries in order.
    pub fn iter(&self) -> impl Iterator<Item = ParamValue> + 'a {
        self.bytes
            .as_chunks::<PARAM_VALUE_ENTRY_LEN>()
            .0
            .iter()
            .map(|c| ParamValue {
                index: u16::from_le_bytes([c[0], c[1]]),
                flags: u16::from_le_bytes([c[2], c[3]]),
                value: f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            })
    }
}

/// Borrowed view over `ParamEdit` entries. Phases were validated on decode,
/// so [`iter`](Self::iter) never sees an invalid one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEdits<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> ParamEdits<'a> {
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.count
    }
    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Decode the entries in order.
    pub fn iter(&self) -> impl Iterator<Item = ParamEdit> + 'a {
        self.bytes
            .as_chunks::<PARAM_EDIT_ENTRY_LEN>()
            .0
            .iter()
            .map(|c| ParamEdit {
                index: u16::from_le_bytes([c[0], c[1]]),
                phase: EditPhase::from_u8(c[2]).unwrap_or(EditPhase::Perform),
                value: f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            })
    }
}

/// Iterate the `f32`s of a `StreamF32` payload (unaligned little-endian
/// reads, so it works on any slice).
pub fn f32_iter(data: &[u8]) -> impl Iterator<Item = f32> + '_ {
    data.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
}

/// Copy the `f32`s of a `StreamF32` payload into `out`, returning how many
/// were written (the smaller of the two lengths).
pub fn read_f32s(data: &[u8], out: &mut [f32]) -> usize {
    let mut n = 0;
    for (dst, v) in out.iter_mut().zip(f32_iter(data)) {
        *dst = v;
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

// Unaligned little-endian reads. Callers check the length first; these index
// directly and would panic on a short slice, which `decode` never allows.

#[inline]
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
#[inline]
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
#[inline]
fn u64_at(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}
#[inline]
fn f64_at(b: &[u8], o: usize) -> f64 {
    f64::from_bits(u64_at(b, o))
}

impl<'a> Frame<'a> {
    /// Decode one frame. Never panics on malformed input: every length is
    /// checked before it is read, and a `ParamEdit` frame is scanned for
    /// invalid phases up front so the returned view is safe to iterate.
    /// Trailing bytes beyond a batch's declared count are ignored.
    ///
    /// # Errors
    ///
    /// See [`WireError`].
    pub fn decode(b: &'a [u8]) -> Result<Frame<'a>, WireError> {
        if b.len() < HEADER_LEN {
            return Err(WireError::TooShort);
        }
        let kind = Kind::from_u8(b[0]).ok_or(WireError::UnknownKind(b[0]))?;
        let arg = u16_at(b, 2);
        let p = &b[HEADER_LEN..];
        Ok(match kind {
            Kind::Hello => {
                if p.len() < 8 {
                    return Err(WireError::TooShort);
                }
                Frame::Hello {
                    version: u16_at(p, 0),
                    param_count: u16_at(p, 2),
                    stream_count: u16_at(p, 4),
                    client_id: u16_at(p, 6),
                }
            }
            Kind::ParamValues => {
                let count = arg as usize;
                let need = count * PARAM_VALUE_ENTRY_LEN;
                if p.len() < need {
                    return Err(WireError::LengthMismatch);
                }
                Frame::ParamValues(ParamValues {
                    bytes: &p[..need],
                    count,
                })
            }
            Kind::ParamEdit => {
                let count = arg as usize;
                let need = count * PARAM_EDIT_ENTRY_LEN;
                if p.len() < need {
                    return Err(WireError::LengthMismatch);
                }
                for c in p[..need].as_chunks::<PARAM_EDIT_ENTRY_LEN>().0 {
                    if EditPhase::from_u8(c[2]).is_none() {
                        return Err(WireError::BadPhase(c[2]));
                    }
                }
                Frame::ParamEdits(ParamEdits {
                    bytes: &p[..need],
                    count,
                })
            }
            Kind::Events | Kind::EventsOut => {
                let count = arg as usize;
                let need = count * EVENT_ENTRY_LEN;
                if p.len() < need {
                    return Err(WireError::LengthMismatch);
                }
                let ev = Events {
                    bytes: &p[..need],
                    count,
                };
                if kind == Kind::Events {
                    Frame::Events(ev)
                } else {
                    Frame::EventsOut(ev)
                }
            }
            Kind::StreamF32 | Kind::StreamU8 => {
                if p.len() < 16 {
                    return Err(WireError::TooShort);
                }
                let seq = u32_at(p, 0);
                let ts_ns = u64_at(p, 4);
                let len = u32_at(p, 12) as usize;
                let data = &p[16..];
                let need = if kind == Kind::StreamF32 {
                    len * 4
                } else {
                    len
                };
                if data.len() < need {
                    return Err(WireError::LengthMismatch);
                }
                let data = &data[..need];
                if kind == Kind::StreamF32 {
                    Frame::StreamF32 {
                        stream: arg,
                        seq,
                        ts_ns,
                        data,
                    }
                } else {
                    Frame::StreamU8 {
                        stream: arg,
                        seq,
                        ts_ns,
                        data,
                    }
                }
            }
            Kind::Ping => {
                if p.len() < 8 {
                    return Err(WireError::TooShort);
                }
                Frame::Ping {
                    client_time: f64_at(p, 0),
                }
            }
            Kind::Pong => {
                if p.len() < 16 {
                    return Err(WireError::TooShort);
                }
                Frame::Pong {
                    client_time: f64_at(p, 0),
                    server_time_us: f64_at(p, 8),
                }
            }
            Kind::Subscribe => {
                if p.len() < 5 {
                    return Err(WireError::TooShort);
                }
                Frame::Subscribe {
                    stream: arg,
                    min_interval_us: u32_at(p, 0),
                    enabled: p[4] != 0,
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Encoding. All encoders append to a caller-owned Vec so buffers can be reused
// frame after frame without allocating.
// ---------------------------------------------------------------------------

/// Append the 4-byte header.
#[inline]
fn header(buf: &mut Vec<u8>, kind: Kind, flags: u8, arg: u16) {
    buf.push(kind as u8);
    buf.push(flags);
    buf.extend_from_slice(&arg.to_le_bytes());
}

/// Encode a `Hello` frame (12 bytes) advertising [`PROTOCOL_VERSION`].
pub fn encode_hello(buf: &mut Vec<u8>, param_count: u16, stream_count: u16, client_id: u16) {
    buf.clear();
    header(buf, Kind::Hello, 0, 0);
    buf.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    buf.extend_from_slice(&param_count.to_le_bytes());
    buf.extend_from_slice(&stream_count.to_le_bytes());
    buf.extend_from_slice(&client_id.to_le_bytes());
}

/// Incrementally build a `ParamValues` frame. Call [`ParamValuesWriter::finish`]
/// when done; the entry count is patched into the header at that point.
/// The count saturates at `u16::MAX` entries.
///
/// ```
/// use noob_vst_webgui_framework::wire::{Frame, ParamValuesWriter, PARAM_FLAG_HOST};
///
/// let mut buf = Vec::new();
/// let mut w = ParamValuesWriter::begin(&mut buf);
/// w.push(0, PARAM_FLAG_HOST, 0.25).push(7, 0, 1.0);
/// assert_eq!(w.finish(), 2);
/// assert!(matches!(Frame::decode(&buf), Ok(Frame::ParamValues(v)) if v.len() == 2));
/// ```
pub struct ParamValuesWriter<'b> {
    buf: &'b mut Vec<u8>,
    count: u16,
}

impl<'b> ParamValuesWriter<'b> {
    /// Clear `buf` and write the header with a placeholder count.
    pub fn begin(buf: &'b mut Vec<u8>) -> Self {
        buf.clear();
        header(buf, Kind::ParamValues, 0, 0);
        ParamValuesWriter { buf, count: 0 }
    }
    /// Append one entry.
    pub fn push(&mut self, index: u16, flags: u16, value: f32) -> &mut Self {
        self.buf.extend_from_slice(&index.to_le_bytes());
        self.buf.extend_from_slice(&flags.to_le_bytes());
        self.buf.extend_from_slice(&value.to_le_bytes());
        self.count = self.count.saturating_add(1);
        self
    }
    /// Entries pushed so far.
    pub fn len(&self) -> usize {
        self.count as usize
    }
    /// Whether nothing was pushed.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Patch the count into the header. Returns the entry count.
    pub fn finish(self) -> usize {
        self.buf[2..4].copy_from_slice(&self.count.to_le_bytes());
        self.count as usize
    }
}

/// Incrementally build a `ParamEdit` frame (client side, or tests). Same
/// shape as [`ParamValuesWriter`]: `begin`, `push`, `finish`.
pub struct ParamEditWriter<'b> {
    buf: &'b mut Vec<u8>,
    count: u16,
}

impl<'b> ParamEditWriter<'b> {
    /// Clear `buf` and write the header with a placeholder count.
    pub fn begin(buf: &'b mut Vec<u8>) -> Self {
        buf.clear();
        header(buf, Kind::ParamEdit, 0, 0);
        ParamEditWriter { buf, count: 0 }
    }
    /// Append one entry.
    pub fn push(&mut self, index: u16, phase: EditPhase, value: f32) -> &mut Self {
        self.buf.extend_from_slice(&index.to_le_bytes());
        self.buf.push(phase as u8);
        self.buf.push(0);
        self.buf.extend_from_slice(&value.to_le_bytes());
        self.count = self.count.saturating_add(1);
        self
    }
    /// Patch the count into the header. Returns the entry count.
    pub fn finish(self) -> usize {
        self.buf[2..4].copy_from_slice(&self.count.to_le_bytes());
        self.count as usize
    }
}

/// Incrementally build an `Events` (client -> server) or `EventsOut`
/// (server -> client) frame. Same shape as [`ParamValuesWriter`].
pub struct EventsWriter<'b> {
    buf: &'b mut Vec<u8>,
    count: u16,
}

impl<'b> EventsWriter<'b> {
    /// Clear `buf` and write the header; `outbound` selects `EventsOut`
    /// (server -> client) over `Events` (client -> server).
    pub fn begin(buf: &'b mut Vec<u8>, outbound: bool) -> Self {
        buf.clear();
        header(
            buf,
            if outbound {
                Kind::EventsOut
            } else {
                Kind::Events
            },
            0,
            0,
        );
        EventsWriter { buf, count: 0 }
    }
    /// Append one event.
    pub fn push(&mut self, e: UiEvent) -> &mut Self {
        self.buf.push(e.kind);
        self.buf.push(e.channel);
        self.buf.push(e.a);
        self.buf.push(e.b);
        self.buf.extend_from_slice(&e.value.to_le_bytes());
        self.buf.extend_from_slice(&e.offset.to_le_bytes());
        self.count = self.count.saturating_add(1);
        self
    }
    /// Events pushed so far.
    pub fn len(&self) -> usize {
        self.count as usize
    }
    /// Whether nothing was pushed.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Patch the count into the header. Returns the event count.
    pub fn finish(self) -> usize {
        self.buf[2..4].copy_from_slice(&self.count.to_le_bytes());
        self.count as usize
    }
}

/// Encode a `StreamF32` frame: header, `seq`, `ts_ns`, `len`, then the
/// values at [`STREAM_DATA_OFFSET`]. Reserves the exact size up front.
pub fn encode_stream_f32(buf: &mut Vec<u8>, stream: u16, seq: u32, ts_ns: u64, data: &[f32]) {
    buf.clear();
    buf.reserve(STREAM_DATA_OFFSET + data.len() * 4);
    header(buf, Kind::StreamF32, 0, stream);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&ts_ns.to_le_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    debug_assert_eq!(buf.len(), STREAM_DATA_OFFSET);
    for v in data {
        buf.extend_from_slice(&v.to_le_bytes());
    }
}

/// Encode a `StreamU8` frame: same header fields as `StreamF32`, then the
/// raw bytes at offset 20.
pub fn encode_stream_u8(buf: &mut Vec<u8>, stream: u16, seq: u32, ts_ns: u64, data: &[u8]) {
    buf.clear();
    buf.reserve(STREAM_DATA_OFFSET + data.len());
    header(buf, Kind::StreamU8, 0, stream);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&ts_ns.to_le_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
}

/// Encode a `Ping` frame (12 bytes) carrying any client clock value.
pub fn encode_ping(buf: &mut Vec<u8>, client_time: f64) {
    buf.clear();
    header(buf, Kind::Ping, 0, 0);
    buf.extend_from_slice(&client_time.to_bits().to_le_bytes());
}

/// Encode a `Pong` frame (20 bytes): the client's value back, plus the
/// server clock in microseconds.
pub fn encode_pong(buf: &mut Vec<u8>, client_time: f64, server_time_us: f64) {
    buf.clear();
    header(buf, Kind::Pong, 0, 0);
    buf.extend_from_slice(&client_time.to_bits().to_le_bytes());
    buf.extend_from_slice(&server_time_us.to_bits().to_le_bytes());
}

/// Encode a `Subscribe` frame (12 bytes) for `stream`: deliver at most one
/// frame per `min_interval_us` (`0` = every frame), or nothing at all when
/// `enabled` is `false`.
pub fn encode_subscribe(buf: &mut Vec<u8>, stream: u16, min_interval_us: u32, enabled: bool) {
    buf.clear();
    header(buf, Kind::Subscribe, 0, stream);
    buf.extend_from_slice(&min_interval_us.to_le_bytes());
    buf.push(enabled as u8);
    // Pad to 4 bytes so the frame is a tidy 12 bytes.
    buf.extend_from_slice(&[0, 0, 0]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trip() {
        let mut b = Vec::new();
        encode_hello(&mut b, 12, 3, 7);
        assert_eq!(b.len(), 12);
        assert_eq!(
            Frame::decode(&b).unwrap(),
            Frame::Hello {
                version: PROTOCOL_VERSION,
                param_count: 12,
                stream_count: 3,
                client_id: 7
            }
        );
    }

    #[test]
    fn param_values_round_trip() {
        let mut b = Vec::new();
        let mut w = ParamValuesWriter::begin(&mut b);
        w.push(0, 0, 0.25).push(5, PARAM_FLAG_ECHO, 1.0);
        assert_eq!(w.finish(), 2);
        assert_eq!(b.len(), HEADER_LEN + 2 * PARAM_VALUE_ENTRY_LEN);
        match Frame::decode(&b).unwrap() {
            Frame::ParamValues(v) => {
                let e: Vec<_> = v.iter().collect();
                assert_eq!(e.len(), 2);
                assert_eq!(
                    e[0],
                    ParamValue {
                        index: 0,
                        flags: 0,
                        value: 0.25
                    }
                );
                assert_eq!(e[1].index, 5);
                assert_eq!(e[1].flags, PARAM_FLAG_ECHO);
                assert_eq!(e[1].value, 1.0);
            }
            other => panic!("wrong frame {other:?}"),
        }
    }

    #[test]
    fn param_edit_round_trip_and_validation() {
        let mut b = Vec::new();
        let mut w = ParamEditWriter::begin(&mut b);
        w.push(3, EditPhase::Begin, 0.5)
            .push(3, EditPhase::Perform, 0.6)
            .push(3, EditPhase::End, 0.6);
        assert_eq!(w.finish(), 3);
        match Frame::decode(&b).unwrap() {
            Frame::ParamEdits(e) => {
                let v: Vec<_> = e.iter().collect();
                assert_eq!(v[0].phase, EditPhase::Begin);
                assert_eq!(v[1].phase, EditPhase::Perform);
                assert_eq!(v[2].phase, EditPhase::End);
                assert_eq!(v[2].value, 0.6);
            }
            other => panic!("wrong frame {other:?}"),
        }
        // Corrupt a phase byte.
        b[HEADER_LEN + 2] = 9;
        assert_eq!(Frame::decode(&b), Err(WireError::BadPhase(9)));
        // Truncate.
        b.truncate(HEADER_LEN + 12);
        assert_eq!(Frame::decode(&b), Err(WireError::LengthMismatch));
    }

    #[test]
    fn events_round_trip_both_directions() {
        let mut b = Vec::new();
        let mut w = EventsWriter::begin(&mut b, false);
        w.push(UiEvent::note_on(0, 60, 0.8)).push(UiEvent {
            offset: 17,
            ..UiEvent::note_off(1, 60, 0.0)
        });
        assert_eq!(w.finish(), 2);
        assert_eq!(b.len(), HEADER_LEN + 2 * EVENT_ENTRY_LEN);
        match Frame::decode(&b).unwrap() {
            Frame::Events(e) => {
                let v: Vec<_> = e.iter().collect();
                assert_eq!(v[0].kind, event_kind::NOTE_ON);
                assert_eq!(v[0].a, 60);
                assert!((v[0].value - 0.8).abs() < 1e-6);
                assert_eq!(v[1].kind, event_kind::NOTE_OFF);
                assert_eq!(v[1].channel, 1);
                assert_eq!(v[1].offset, 17);
            }
            other => panic!("wrong frame {other:?}"),
        }
        let mut w = EventsWriter::begin(&mut b, true);
        w.push(UiEvent::custom(0x81, 3, 4, 0.5));
        w.finish();
        match Frame::decode(&b).unwrap() {
            Frame::EventsOut(e) => {
                let v: Vec<_> = e.iter().collect();
                assert_eq!(v[0].kind, 0x81);
                assert_eq!((v[0].a, v[0].b), (3, 4));
            }
            other => panic!("wrong frame {other:?}"),
        }
        b.truncate(b.len() - 1);
        assert_eq!(Frame::decode(&b), Err(WireError::LengthMismatch));
    }

    #[test]
    fn stream_f32_round_trip_and_alignment() {
        let data: Vec<f32> = (0..1024).map(|i| i as f32 * 0.5).collect();
        let mut b = Vec::new();
        encode_stream_f32(&mut b, 2, 99, 123_456_789, &data);
        assert_eq!(STREAM_DATA_OFFSET % 4, 0);
        assert_eq!(b.len(), STREAM_DATA_OFFSET + 1024 * 4);
        match Frame::decode(&b).unwrap() {
            Frame::StreamF32 {
                stream,
                seq,
                ts_ns,
                data: d,
            } => {
                assert_eq!((stream, seq, ts_ns), (2, 99, 123_456_789));
                let mut out = vec![0f32; 1024];
                assert_eq!(read_f32s(d, &mut out), 1024);
                assert_eq!(out, data);
            }
            other => panic!("wrong frame {other:?}"),
        }
    }

    #[test]
    fn stream_u8_round_trip() {
        let mut b = Vec::new();
        encode_stream_u8(&mut b, 1, 1, 2, &[9, 8, 7]);
        match Frame::decode(&b).unwrap() {
            Frame::StreamU8 { data, .. } => assert_eq!(data, &[9, 8, 7]),
            other => panic!("wrong frame {other:?}"),
        }
    }

    #[test]
    fn ping_pong_subscribe() {
        let mut b = Vec::new();
        encode_ping(&mut b, 1234.5);
        assert_eq!(
            Frame::decode(&b).unwrap(),
            Frame::Ping {
                client_time: 1234.5
            }
        );
        encode_pong(&mut b, 1234.5, 99.25);
        assert_eq!(
            Frame::decode(&b).unwrap(),
            Frame::Pong {
                client_time: 1234.5,
                server_time_us: 99.25
            }
        );
        encode_subscribe(&mut b, 4, 16_667, true);
        assert_eq!(b.len(), 12);
        assert_eq!(
            Frame::decode(&b).unwrap(),
            Frame::Subscribe {
                stream: 4,
                min_interval_us: 16_667,
                enabled: true
            }
        );
    }

    #[test]
    fn garbage_is_rejected_without_panic() {
        assert_eq!(Frame::decode(&[]), Err(WireError::TooShort));
        assert_eq!(
            Frame::decode(&[0xEE, 0, 0, 0]),
            Err(WireError::UnknownKind(0xEE))
        );
        assert_eq!(Frame::decode(&[0x20, 0, 0, 0, 1]), Err(WireError::TooShort));
        // Stream frame claiming more floats than present.
        let mut b = Vec::new();
        encode_stream_f32(&mut b, 0, 0, 0, &[1.0, 2.0]);
        b.truncate(b.len() - 1);
        assert_eq!(Frame::decode(&b), Err(WireError::LengthMismatch));
    }
}
