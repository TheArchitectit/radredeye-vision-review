//! Thread-safe, ordered, removable sink registry (Phase 10.1).
//!
//! [`SinkRegistry`] holds an ordered list of `Arc<dyn CaptureSink>` keyed by an
//! opaque [`SinkHandle`]. Sinks can be registered and *unregistered* at runtime
//! with only a `&self` borrow, and [`SinkRegistry::snapshot`] returns an owned
//! `Vec` so callers iterate the sinks without holding the lock across I/O —
//! the same lock-snapshot pattern Phase 9.1 uses for `metrics_handle()`.
//!
//! This is the primitive that lets multiple consuming agents
//! subscribe/unsubscribe to a *running* [`crate::CapturePipeline`] without the
//! whole pipeline being behind an exclusive `&mut` borrow.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::CaptureSink;

/// One registered sink and the handle that identifies it.
type SinkEntry = (SinkHandle, Arc<dyn CaptureSink>);

/// The shared, lock-protected ordered sink list.
type SinkList = Arc<Mutex<Vec<SinkEntry>>>;

/// Opaque, stable identity for a sink registered in a [`SinkRegistry`].
///
/// Returned by [`SinkRegistry::register`] and accepted by
/// [`SinkRegistry::unregister`]. Cheap to copy/compare/hash so a managing agent
/// can hold a handle and later remove exactly the sink it added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SinkHandle(pub(crate) u64);

/// Thread-safe, ordered, reference-counted collection of sinks.
///
/// Cloning a `SinkRegistry` shares the same underlying list (the sinks and the
/// id counter are behind `Arc`), so a [`crate::CapturePipeline`] clone and its
/// original see the same sinks. Order of registration is preserved; removal is
/// by identity ([`SinkHandle`]).
#[derive(Clone, Default)]
pub struct SinkRegistry {
    sinks: SinkList,
    next_id: Arc<AtomicU64>,
}

impl std::fmt::Debug for SinkRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.sinks.lock().map(|s| s.len()).unwrap_or(0);
        f.debug_struct("SinkRegistry").field("len", &len).finish()
    }
}

impl SinkRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `sink` and return the handle that identifies it for later
    /// removal. Sinks are kept in registration order.
    pub fn register(&self, sink: Arc<dyn CaptureSink>) -> SinkHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let handle = SinkHandle(id);
        if let Ok(mut sinks) = self.sinks.lock() {
            sinks.push((handle, sink));
        }
        handle
    }

    /// Remove the sink identified by `handle`, returning it if it was present.
    /// Order of the remaining sinks is preserved.
    pub fn unregister(&self, handle: SinkHandle) -> Option<Arc<dyn CaptureSink>> {
        self.sinks.lock().ok().and_then(|mut sinks| {
            let pos = sinks.iter().position(|(k, _)| *k == handle)?;
            Some(sinks.remove(pos).1)
        })
    }

    /// Owned snapshot of the currently registered sinks in registration order.
    /// Iterating the returned `Vec` never touches the registry lock, so sink
    /// I/O cannot deadlock against a concurrent `register`/`unregister`.
    pub fn snapshot(&self) -> Vec<Arc<dyn CaptureSink>> {
        self.sinks
            .lock()
            .map(|sinks| sinks.iter().map(|(_, s)| Arc::clone(s)).collect())
            .unwrap_or_default()
    }

    /// Number of sinks currently registered.
    pub fn len(&self) -> usize {
        self.sinks.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// `true` if no sinks are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Stable `kind()` labels of the registered sinks, in registration order.
    ///
    /// Used by the MCP `list_sinks` tool to report what is currently subscribed
    /// to a stream without leaking sink internals.
    pub fn kinds(&self) -> Vec<&'static str> {
        self.snapshot().iter().map(|s| s.kind()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapturedFrame, PixelFormat, SinkError};
    use std::time::Instant;

    struct MarkerSink {
        label: &'static str,
    }
    impl CaptureSink for MarkerSink {
        fn submit(&self, _frame: &CapturedFrame) -> Result<(), SinkError> {
            Ok(())
        }
        fn kind(&self) -> &'static str {
            self.label
        }
    }

    fn frame() -> CapturedFrame {
        CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![0, 0, 0, 255],
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn register_preserves_order_and_unregisters_by_handle() {
        let reg = SinkRegistry::new();
        assert!(reg.is_empty());
        let h0 = reg.register(Arc::new(MarkerSink { label: "a" }));
        let h1 = reg.register(Arc::new(MarkerSink { label: "b" }));
        let h2 = reg.register(Arc::new(MarkerSink { label: "c" }));
        assert_eq!(reg.len(), 3);

        let snap = reg.snapshot();
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].kind(), "a");
        assert_eq!(snap[1].kind(), "b");
        assert_eq!(snap[2].kind(), "c");

        let removed = reg.unregister(h1).expect("h1 present");
        assert_eq!(removed.kind(), "b");
        assert_eq!(reg.len(), 2);
        let snap = reg.snapshot();
        assert_eq!(snap[0].kind(), "a");
        assert_eq!(snap[1].kind(), "c");

        // removing twice / unknown handle is None
        assert!(reg.unregister(h1).is_none());
        assert!(reg.unregister(h0).is_some());
        assert!(reg.unregister(h2).is_some());
        assert!(reg.is_empty());
    }

    #[test]
    fn snapshot_does_not_hold_lock_across_sink_io() {
        let reg = SinkRegistry::new();
        reg.register(Arc::new(MarkerSink { label: "x" }));
        let snap = reg.snapshot();
        // while iterating snap, the registry is mutable (no lock held):
        reg.register(Arc::new(MarkerSink { label: "y" }));
        for sink in &snap {
            let _ = sink.submit(&frame());
        }
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn clone_shares_underlying_list() {
        let reg = SinkRegistry::new();
        let h = reg.register(Arc::new(MarkerSink { label: "shared" }));
        let cloned = reg.clone();
        // removing via the clone is visible to the original (shared Arc state).
        assert!(cloned.unregister(h).is_some());
        assert_eq!(reg.len(), 0);
    }
}
