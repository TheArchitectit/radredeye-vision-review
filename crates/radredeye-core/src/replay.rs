//! Bounded ring buffer of recent frames (Phase 10.3).
//!
//! [`ReplayBuffer`] promotes the Phase 9.3 daemon `FrameStore`
//! (`VecDeque<CapturedFrame>`, cap 30, `get(index)` newest-first) into core
//! almost verbatim, adding [`ReplayBuffer::replay`] for ordered retrieval of a
//! range of recent frames. The daemon re-exports this type as a deprecated
//! `FrameStore` alias so its `GET /frame` path keeps working unchanged.
//!
//! ## Semantics (preserved from 9.3 `FrameStore`)
//!
//! - `index = 0` in [`ReplayBuffer::get`] is the **newest** frame; higher
//!   indices reach progressively older frames.
//! - When the buffer is over capacity, the **oldest** entry is evicted.
//! - Default capacity is 30 (matches [`FRAME_STORE_CAP`]).
//!
//! [`ReplayBuffer::new`] never panics: a cap below 1 is clamped to 1 so a
//! misconfigured capacity cannot construct an unusable buffer.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Mutex;

use crate::CapturedFrame;

/// Default ring-buffer capacity (spec F4: cap 30). Matches the daemon's
/// historical `FRAME_STORE_CAP`.
pub const FRAME_STORE_CAP: usize = 30;

/// Bounded ring buffer of recently captured frames.
///
/// Newest frames are pushed at the back; [`ReplayBuffer::get`] indexes from the
/// newest (`index = 0`) backwards, so `get(0)` returns the latest frame and
/// higher indices reach progressively older frames. The buffer drops the
/// oldest entry when it exceeds its capacity. Thread-safe via an internal
/// `Mutex`.
pub struct ReplayBuffer {
    frames: Mutex<VecDeque<CapturedFrame>>,
    cap: usize,
}

impl ReplayBuffer {
    /// Create an empty buffer with the given capacity. A cap below 1 is
    /// clamped to 1 (no panic) — a zero/negative-style capacity would otherwise
    /// construct a buffer that can never hold a frame.
    pub fn new(cap: usize) -> Self {
        let cap = if cap < 1 { 1 } else { cap };
        Self {
            frames: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
        }
    }

    /// Append a frame, dropping the oldest when over capacity.
    pub fn push(&self, frame: CapturedFrame) {
        if let Ok(mut frames) = self.frames.lock() {
            if frames.len() >= self.cap {
                frames.pop_front();
            }
            frames.push_back(frame);
        }
    }

    /// Number of frames currently stored.
    pub fn len(&self) -> usize {
        self.frames.lock().map(|f| f.len()).unwrap_or(0)
    }

    /// `true` if no frames are stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieve a frame by age index. `index = 0` = newest; higher indices =
    /// older. Returns `None` if the index is out of range or the buffer is
    /// empty.
    pub fn get(&self, index: usize) -> Option<CapturedFrame> {
        let frames = self.frames.lock().ok()?;
        let len = frames.len();
        if index >= len {
            return None;
        }
        // index 0 = newest (back); index n = n-th from the back.
        let pos = len - 1 - index;
        frames.get(pos).cloned()
    }

    /// Ordered oldest→newest slice of the frames whose age index falls in
    /// `range` (clamped to the buffer length). Age index `0` is the newest
    /// (see [`get`]); the returned vector is ordered oldest first so a caller
    /// replaying "the last N frames" iterates them in capture order.
    ///
    /// [`get`]: ReplayBuffer::get
    pub fn replay(&self, range: Range<usize>) -> Vec<CapturedFrame> {
        let frames = match self.frames.lock() {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let len = frames.len();
        let mut out = Vec::new();
        // VecDeque front = oldest, back = newest; iterate front→back (oldest
        // → newest) and keep the frames whose age index is in `range`.
        for pos in 0..len {
            let age = len - 1 - pos;
            if range.contains(&age) {
                if let Some(f) = frames.get(pos) {
                    out.push(f.clone());
                }
            }
        }
        out
    }

    /// Configured capacity (frames retained before oldest eviction).
    pub fn cap(&self) -> usize {
        self.cap
    }
}

impl std::fmt::Debug for ReplayBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayBuffer")
            .field("len", &self.len())
            .field("cap", &self.cap)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PixelFormat;
    use std::time::Instant;

    fn frame(tag: u8) -> CapturedFrame {
        CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![tag, 0, 0, 255],
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn new_clamps_cap_below_one_without_panicking() {
        let buf = ReplayBuffer::new(0);
        assert_eq!(buf.cap(), 1);
        buf.push(frame(7));
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.get(0).expect("frame").data, vec![7, 0, 0, 255]);
    }

    #[test]
    fn default_cap_constant() {
        assert_eq!(FRAME_STORE_CAP, 30);
    }

    #[test]
    fn retains_latest_and_evicts_oldest() {
        let buf = ReplayBuffer::new(2);
        buf.push(frame(1));
        buf.push(frame(2));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.get(0).expect("newest").data, vec![2, 0, 0, 255]);
        // over capacity → evicts oldest (frame 1)
        buf.push(frame(3));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.get(0).expect("newest").data, vec![3, 0, 0, 255]);
        assert_eq!(buf.get(1).expect("older").data, vec![2, 0, 0, 255]);
        assert!(buf.get(2).is_none());
    }

    #[test]
    fn index_zero_is_newest() {
        let buf = ReplayBuffer::new(FRAME_STORE_CAP);
        buf.push(frame(10));
        buf.push(frame(20));
        assert_eq!(buf.get(0).expect("newest").data, vec![20, 0, 0, 255]);
        assert_eq!(buf.get(1).expect("older").data, vec![10, 0, 0, 255]);
        assert!(buf.get(2).is_none());
    }

    #[test]
    fn empty_returns_none() {
        let buf = ReplayBuffer::new(FRAME_STORE_CAP);
        assert!(buf.is_empty());
        assert!(buf.get(0).is_none());
    }

    #[test]
    fn replay_returns_oldest_first_within_range() {
        let buf = ReplayBuffer::new(5);
        for t in 1..=4u8 {
            buf.push(frame(t));
        }
        // age indices: 0=newest(4),1(3),2(2),3(1)=oldest
        let all = buf.replay(0..4);
        assert_eq!(all.len(), 4);
        // oldest → newest
        assert_eq!(all[0].data, vec![1, 0, 0, 255]);
        assert_eq!(all[1].data, vec![2, 0, 0, 255]);
        assert_eq!(all[2].data, vec![3, 0, 0, 255]);
        assert_eq!(all[3].data, vec![4, 0, 0, 255]);

        // replay only the two newest (age 0,1) → oldest-first of those two.
        let recent = buf.replay(0..2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].data, vec![3, 0, 0, 255]);
        assert_eq!(recent[1].data, vec![4, 0, 0, 255]);

        // clamped range yields empty when out of bounds.
        assert!(buf.replay(10..20).is_empty());
    }
}
