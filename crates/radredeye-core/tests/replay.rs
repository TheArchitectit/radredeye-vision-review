//! Phase 10.3 — Core `ReplayBuffer` (promoted daemon `FrameStore`).
//!
//! Exercises the public [`ReplayBuffer`] API end-to-end (including the
//! [`ReplayBuffer::replay`] range query) and the integrated path where a
//! [`CaptureStream`] appends accepted frames to an attached buffer.

use std::sync::Arc;
use std::time::Instant;
use radredeye_core::{
    CapturePipeline, CaptureSink, CapturedFrame, PixelFormat, ReplayBuffer, SinkError,
    FRAME_STORE_CAP,
};

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
fn replay_buffer_preserves_framestore_semantics() {
    // index 0 = newest, oldest-eviction, default cap 30 — identical to the
    // daemon's former FrameStore.
    let buf = ReplayBuffer::new(FRAME_STORE_CAP);
    assert_eq!(FRAME_STORE_CAP, 30);
    assert_eq!(buf.cap(), 30);
    assert!(buf.is_empty());

    buf.push(frame(1));
    buf.push(frame(2));
    assert_eq!(buf.len(), 2);
    assert_eq!(buf.get(0).expect("newest").data, vec![2, 0, 0, 255]);
    assert_eq!(buf.get(1).expect("older").data, vec![1, 0, 0, 255]);
    assert!(buf.get(2).is_none());
}

#[test]
fn replay_buffer_evicts_oldest_at_cap() {
    let buf = ReplayBuffer::new(3);
    for t in 1..=4u8 {
        buf.push(frame(t));
    }
    // cap 3 → frame 1 (oldest) evicted.
    assert_eq!(buf.len(), 3);
    assert_eq!(buf.get(0).expect("newest").data, vec![4, 0, 0, 255]);
    assert_eq!(buf.get(2).expect("oldest remaining").data, vec![2, 0, 0, 255]);
    assert!(buf.get(3).is_none());
}

#[test]
fn replay_returns_oldest_to_newest_in_range() {
    let buf = ReplayBuffer::new(10);
    for t in 1..=5u8 {
        buf.push(frame(t));
    }
    // age indices: 0=newest(5),1(4),2(3),3(2),4(1)=oldest
    let recent = buf.replay(0..3);
    assert_eq!(recent.len(), 3);
    // oldest→newest of the three newest: 3,4,5
    assert_eq!(recent[0].data, vec![3, 0, 0, 255]);
    assert_eq!(recent[1].data, vec![4, 0, 0, 255]);
    assert_eq!(recent[2].data, vec![5, 0, 0, 255]);

    let all = buf.replay(0..5);
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].data, vec![1, 0, 0, 255]);
    assert_eq!(all[4].data, vec![5, 0, 0, 255]);

    // clamped / empty ranges
    assert!(buf.replay(5..10).is_empty());
    assert!(buf.replay(3..3).is_empty());
}

/// A sink that records the tag byte of every frame it sees.
struct TagSink {
    tags: std::sync::Mutex<Vec<u8>>,
}
impl CaptureSink for TagSink {
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
        if let Some(&t) = frame.data.first() {
            if let Ok(mut g) = self.tags.lock() {
                g.push(t);
            }
        }
        Ok(())
    }
}

#[test]
fn attached_replay_buffer_appends_accepted_frames() {
    // Opt-in replay on the default stream: accepted frames are appended and
    // retrievable in capture order via replay().
    let pipeline = CapturePipeline::new();
    let sink = Arc::new(TagSink {
        tags: std::sync::Mutex::new(Vec::new()),
    });
    pipeline.register_sink(sink.clone());
    let buf = Arc::new(ReplayBuffer::new(10));
    pipeline.attach_replay(Arc::clone(&buf));

    pipeline.submit(&frame(10));
    pipeline.submit(&frame(20));
    pipeline.submit(&frame(30));

    // the sink saw all three (effective frames == submitted)
    let seen: Vec<u8> = sink.tags.lock().expect("lock").clone();
    assert_eq!(seen, vec![10, 20, 30]);

    // replay oldest→newest returns all three in capture order
    let all = buf.replay(0..3);
    assert_eq!(all.len(), 3);
    let tags: Vec<u8> = all.iter().filter_map(|f| f.data.first().copied()).collect();
    assert_eq!(tags, vec![10, 20, 30]);

    // get(0) is the newest
    assert_eq!(buf.get(0).expect("newest").data, vec![30, 0, 0, 255]);
}

#[test]
fn replay_is_opt_in_off_by_default_in_core() {
    // A fresh pipeline has no replay buffer attached.
    let pipeline = CapturePipeline::new();
    pipeline.submit(&frame(99));
    let default = pipeline.stream("default").expect("default stream");
    assert!(default.replay().is_none(), "core replay must be opt-in");
}
