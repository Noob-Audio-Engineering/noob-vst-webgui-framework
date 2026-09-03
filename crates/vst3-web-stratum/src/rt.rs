//! Real-time-safe primitives used on the audio thread.
//!
//! Nothing in here allocates, locks, or blocks after construction, and
//! nothing here depends on the rest of the crate: [`AtomicF32`] and the
//! triple-buffer [`mailbox`] are usable on their own.
//!
//! # Why a triple buffer
//!
//! Telemetry wants *latest wins*: a meter or a spectrum should show what is
//! happening now, not a backlog from a slow consumer. A bounded queue would
//! either block the producer or make it decide what to drop; a mutex would
//! let the network thread stall the audio thread. A triple buffer gives the
//! producer a private slot to write into, the consumer a private slot to
//! read from, and one "back" slot that is exchanged atomically. Both sides
//! are wait-free: each operation is one atomic swap, there is no retry loop
//! and no spinning.
//!
//! The state word packs the back-slot index (2 bits) with a *dirty* bit that
//! says the back slot holds a frame the reader has not seen. A publish swaps
//! the writer's slot into the back position with the dirty bit set; a read
//! sees the dirty bit, swaps its slot into the back position (clearing the
//! bit) and takes the slot that was there. Intermediate frames the reader
//! never got to are simply overwritten, which is the intended semantics.

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// An `f32` stored in an `AtomicU32`. Loads and stores are relaxed: the value
/// is a plain "latest wins" cell and callers never need ordering with other
/// memory.
///
/// Used for every parameter value in the bridge, so the audio thread can
/// read a parameter with one relaxed load and any thread can write it with
/// one relaxed store.
///
/// ```
/// use vst3_web_stratum::rt::AtomicF32;
///
/// let gain = AtomicF32::new(0.5);
/// gain.store(0.75);
/// assert_eq!(gain.load(), 0.75);
/// assert_eq!(gain.swap(1.0), 0.75);
/// ```
#[derive(Debug)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    /// A cell holding `v`. `const`, so it can live in a `static`.
    pub const fn new(v: f32) -> Self {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }
    /// The current value (relaxed load). Wait-free.
    #[inline]
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    /// Replace the value (relaxed store). Wait-free.
    #[inline]
    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
    /// Store and return the previous value.
    #[inline]
    pub fn swap(&self, v: f32) -> f32 {
        f32::from_bits(self.0.swap(v.to_bits(), Ordering::Relaxed))
    }
}

impl Default for AtomicF32 {
    /// `0.0`.
    fn default() -> Self {
        AtomicF32::new(0.0)
    }
}

// ---------------------------------------------------------------------------
// Triple buffer
// ---------------------------------------------------------------------------

/// Low two bits of the state word: index of the back slot.
const IDX_MASK: u8 = 0b011;
/// Bit 2 of the state word: the back slot holds an unread frame.
const DIRTY: u8 = 0b100;

/// The three slots plus the packed state word, shared by both halves.
struct Shared<T> {
    slots: [UnsafeCell<T>; 3],
    /// Bits 0..1: index of the "back" slot (the one neither side owns).
    /// Bit 2: set when the back slot holds a value the reader has not seen.
    state: AtomicU8,
}

// SAFETY: each slot is accessed by exactly one side at a time. Ownership of the
// back slot is transferred by the atomic swap in `publish`/`read`.
unsafe impl<T: Send> Sync for Shared<T> {}
unsafe impl<T: Send> Send for Shared<T> {}

/// Producer half of a [`mailbox`]. Lives on the audio thread.
///
/// Write into [`slot`](Self::slot), then [`publish`](Self::publish); or do
/// both with [`write`](Self::write). Publishing never waits for the reader.
pub struct MailboxWriter<T> {
    shared: Arc<Shared<T>>,
    /// The slot this side currently owns.
    write_idx: u8,
}

/// Consumer half of a [`mailbox`]. Lives on the network thread.
///
/// [`read`](Self::read) returns the newest published value once; between
/// publishes it returns `None` and [`latest`](Self::latest) keeps giving the
/// last value read.
pub struct MailboxReader<T> {
    shared: Arc<Shared<T>>,
    /// The slot this side currently owns.
    read_idx: u8,
}

/// Build a wait-free single-producer / single-consumer "latest value wins"
/// mailbox. The producer always has a slot to write into and never waits for
/// the consumer; if the consumer is slow, intermediate values are dropped.
/// That is exactly the semantics telemetry wants: meters and spectra should
/// show *now*, not a backlog.
///
/// `init` is called three times to build the slots, so a slot can be a
/// pre-sized buffer that is reused for every frame (the bridge uses
/// `StreamFrame::with_capacity`). After this call nothing allocates.
///
/// ```
/// use vst3_web_stratum::rt::mailbox;
///
/// let (mut writer, mut reader) = mailbox(|| vec![0.0f32; 4]);
/// assert!(reader.read().is_none());          // nothing published yet
///
/// writer.write(|slot| slot[0] = 1.0);
/// writer.write(|slot| slot[0] = 2.0);        // overwrites the unread frame
/// assert_eq!(reader.read().map(|v| v[0]), Some(2.0));
/// assert!(reader.read().is_none());          // consumed
/// assert_eq!(reader.latest()[0], 2.0);       // still available
/// ```
pub fn mailbox<T>(mut init: impl FnMut() -> T) -> (MailboxWriter<T>, MailboxReader<T>) {
    let shared = Arc::new(Shared {
        slots: [
            UnsafeCell::new(init()),
            UnsafeCell::new(init()),
            UnsafeCell::new(init()),
        ],
        // writer owns 0, reader owns 1, back is 2, nothing pending.
        state: AtomicU8::new(2),
    });
    (
        MailboxWriter {
            shared: shared.clone(),
            write_idx: 0,
        },
        MailboxReader {
            shared,
            read_idx: 1,
        },
    )
}

impl<T> MailboxWriter<T> {
    /// Mutable access to the slot that will be published next. The slot
    /// holds whatever was written into it last time it came around (two
    /// publishes ago, or the initial value), so callers overwrite rather
    /// than append.
    #[inline]
    pub fn slot(&mut self) -> &mut T {
        // SAFETY: the writer exclusively owns `write_idx` until `publish`.
        unsafe { &mut *self.shared.slots[self.write_idx as usize].get() }
    }

    /// Hand the written slot to the reader and take ownership of the old back
    /// slot for the next write. Wait-free: one atomic swap.
    #[inline]
    pub fn publish(&mut self) {
        let old = self
            .shared
            .state
            .swap(self.write_idx | DIRTY, Ordering::AcqRel);
        self.write_idx = old & IDX_MASK;
    }

    /// Convenience: write with a closure then publish.
    #[inline]
    pub fn write(&mut self, f: impl FnOnce(&mut T)) {
        f(self.slot());
        self.publish();
    }
}

impl<T> MailboxReader<T> {
    /// If the producer published since the last call, swap it in and return
    /// `Some`. Wait-free: one atomic load and, when dirty, one swap.
    #[inline]
    pub fn read(&mut self) -> Option<&T> {
        if self.shared.state.load(Ordering::Acquire) & DIRTY == 0 {
            return None;
        }
        let old = self.shared.state.swap(self.read_idx, Ordering::AcqRel);
        self.read_idx = old & IDX_MASK;
        Some(self.latest())
    }

    /// The most recently read value (whatever `read` last returned, or the
    /// initial value).
    #[inline]
    pub fn latest(&self) -> &T {
        // SAFETY: the reader exclusively owns `read_idx`.
        unsafe { &*self.shared.slots[self.read_idx as usize].get() }
    }

    /// True if there is an unread value waiting. Lets a consumer check many
    /// mailboxes cheaply before touching any of them.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.shared.state.load(Ordering::Acquire) & DIRTY != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn atomic_f32() {
        let a = AtomicF32::new(1.5);
        assert_eq!(a.load(), 1.5);
        assert_eq!(a.swap(2.0), 1.5);
        assert_eq!(a.load(), 2.0);
    }

    #[test]
    fn mailbox_latest_wins() {
        let (mut w, mut r) = mailbox(|| 0u32);
        assert!(r.read().is_none());
        w.write(|v| *v = 1);
        w.write(|v| *v = 2);
        w.write(|v| *v = 3);
        assert_eq!(r.read(), Some(&3));
        assert!(r.read().is_none());
        assert_eq!(*r.latest(), 3);
        w.write(|v| *v = 4);
        assert!(r.is_dirty());
        assert_eq!(r.read(), Some(&4));
    }

    #[test]
    fn mailbox_never_tears_across_threads() {
        // Each published value is a block whose every element equals its
        // sequence number; a torn read would show mixed elements.
        let (mut w, mut r) = mailbox(|| vec![0u64; 256]);
        let n = 200_000u64;
        let producer = thread::spawn(move || {
            for i in 1..=n {
                w.write(|v| v.iter_mut().for_each(|x| *x = i));
            }
        });
        let mut last = 0u64;
        let mut seen = 0usize;
        loop {
            if let Some(v) = r.read() {
                let first = v[0];
                assert!(v.iter().all(|&x| x == first), "torn read");
                assert!(first >= last, "went backwards");
                last = first;
                seen += 1;
                if first == n {
                    break;
                }
            } else if producer.is_finished() {
                // Drain the final value.
                if let Some(v) = r.read() {
                    assert!(v.iter().all(|&x| x == v[0]));
                    last = v[0];
                }
                break;
            }
        }
        assert_eq!(last, n);
        assert!(seen > 0);
    }
}
