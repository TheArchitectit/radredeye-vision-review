# radredeye — Sprint Workflow Tracker

**Workflow type:** Dynamic (auto-advancing phases)  
**Last updated:** 2026-07-03

---

## Active Phase Status

| Phase | Name | Status | Sprints Done | Sprints Total | Progress |
|-------|------|--------|--------------|---------------|----------|
| 0 | Foundation | ✅ COMPLETE | 5/5 | 5 | 100% |
| 1 | Core Sinks & Daemon | ✅ COMPLETE | 7/7 | 7 | 100% |
| 2 | Testing & CI | ✅ COMPLETE | 7/7 | 7 | 100% |
| 3 | Additional Sinks | ✅ COMPLETE | 6/6 | 6 | 100% |
| 4 | Engine Adapters | ✅ COMPLETE | 6/6 | 6 | 100% |
| 5 | Production Hardening | 🔶 IN PROGRESS | 5/6 | 6 | 83% |
| 6 | Docs & Release | ⬜ BLOCKED | 0/5 | 5 | 0% |

**Overall: 36/42 sprints complete (86%)**

---

## Current Sprint: 5.6 — Benchmark Harness

**Deliverable:** Criterion setup for `submit()` throughput + baseline numbers  
**Complexity:** M  
**Dependencies:** 5.2 (backpressure) ✅  
**Status:** ⬜ NOT STARTED

### Exit Criteria
- [ ] `criterion` dev-dependency added to `radredeye-core`
- [ ] Benchmark group: `submit_throughput` — measure frames/sec through pipeline
- [ ] Benchmark: `resize_nearest` — measure resize latency for common resolutions
- [ ] Baseline numbers recorded in BENCHMARKS.md

### How to Run (after setup)
```bash
cargo bench -p radredeye-core
```

---

## Sprint Dependency Graph

```
Phase 0 (Foundation)
  └─→ Phase 1 (Sinks & Daemon)
        ├─→ Phase 2 (Testing & CI)
        │     ├─→ Phase 3 (Additional Sinks)
        │     │     └─→ Phase 5.1 (CaptureConfig) ✅
        │     │           └─→ Phase 5.2 (Backpressure) ✅
        │     │                 ├─→ Phase 5.5 (Graceful Shutdown) ✅
        │     │                 └─→ Phase 5.6 (Benchmarks) ← NEXT
        │     └─→ Phase 4 (Engine Adapters)
        └─→ Phase 5.3 (Structured Logging) ✅
        └─→ Phase 5.4 (Health Check) ✅

Phase 6 (Docs & Release) — unblocked once 5.6 completes
```

---

## Phase Transition Rules

1. **Auto-advance**: When all sprints in a phase are ✅, the phase is marked COMPLETE
2. **Blocking**: Phase 6 is blocked until Phase 5.6 completes
3. **Parallel work**: Sprints within a phase can run in parallel if no dependency
4. **Sprint failure**: If a sprint fails 3×, HALT and escalate (Three Strikes Rule)

---

## How to Use This Tracker

1. Pick the next ⬜ sprint from the current phase
2. Implement the deliverable
3. Verify exit criteria
4. Mark ✅ and update progress
5. If all sprints in a phase are ✅, advance to next phase

---

*This file is auto-updated by the sprint workflow. Manual edits are fine but will be reconciled on next update.*
