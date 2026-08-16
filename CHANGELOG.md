# Changelog

All notable changes to the **radredeye** project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> Note: this repo was bootstrapped from the `agent-guardrails-template`, whose own
> changelog history is not reproduced here. The entries below track the Vision
> Enabler capture pipeline only.

---

## [0.1.0] - 2026-08-13

First tagged release of the radredeye cross-engine viewport capture
pipeline. Covers Phases 0–6 (foundation → docs & release).

### Added

#### Phase 0 — Foundation
- Rust workspace scaffold with `radredeye-core` and `radredeye-bevy`
  crates.
- Core engine-agnostic types: `CapturedFrame`, `PixelFormat`, `CaptureSink`
  trait, `CapturePipeline` fan-out bus.
- `StdoutSink` (EPIPE-safe metadata line) with 2 initial core tests.
- Guardrails template imported; docs and CI scaffolded.
- Godot auto-capture addon ported from the template
  (`engines/godot/addons/radredeye_capture/`).

#### Phase 1 — Core sinks & daemon bridge
- `FileSink` with PNG encoding (`feature = "file-sink"`, on by default) — writes
  `frame_NNNN.png` files to a directory.
- `HttpSink` with `ureq` POST (`feature = "http-sink"`, on by default) — POSTs PNG
  frames to a configurable URL.
- `radredeye-mcp` HTTP bridge (`tiny_http`) — listens on `0.0.0.0:8765`,
  receives Godot PNG POSTs at `/capture`, decodes into `CapturedFrame`, feeds the
  core pipeline.
- Bevy plugin rewrite to Bevy 0.15's observer-based screenshot API
  (`Screenshot::window()` + `observe(on_screenshot)`); `CaptureCamera` marker
  component with per-camera throttle.
- Godot addon `emit_to_bridge` toggle + HTTP POST to the daemon bridge.
- Fixed the daemon `for mut request` borrow error (Sprint 1.7).

#### Phase 2 — Testing & CI
- Unit tests for `FileSink` (writes PNG to temp dir, increments filenames).
- Unit test for `HttpSink` (transport error on unreachable URL).
- Integration test for `CapturePipeline` fan-out / invalid-frame dropping.
- 4 daemon tests (PNG decode, handle_capture submit, bad-PNG rejection).
- 4 Bevy plugin tests (capture camera defaults, deref, resource registration).
- 20 tests total passing (12 core, 4 daemon, 4 bevy).
- GitHub Actions CI (`ci.yml`): `cargo build`, `cargo test`, `cargo clippy -D warnings`.
- Template CI residue (Jenkinsfile, gitlab-ci.yml) removed.

#### Phase 3 — Additional sinks
- `WebSocketSink` (`feature = "websocket-sink"`) — connect-and-push via
  `tungstenite`, auto-reconnect on send failure.
- `GrpcSink` (`feature = "grpc-sink"`) — client-streaming RPC via `tonic`.
- `.proto` schema for frame streaming (`proto/radredeye.proto`,
  `FrameStreaming` service).
- `GrpcSink` unit tests (RGBA/BGRA frame conversion).
- `SUPPORT_MATRIX.md` updated with the new sinks.

#### Phase 4 — Engine adapters
- `radredeye-unity` crate scaffold (re-exports core types for Unity ↔ Rust
  interop; Unity C# side POSTs to the daemon bridge).
- `radredeye-unreal` crate scaffold (re-exports core types for Unreal ↔
  Rust interop; Unreal C++ side POSTs to the daemon bridge).
- `radredeye-webxr` adapter — canvas capture via `wasm-bindgen`
  (`WebGLRenderingContext.readPixels` → `CapturedFrame`).
- `SUPPORT_MATRIX.md` updated with engine status (5 crates total).

#### Phase 5 — Production hardening
- `CaptureConfig`: configurable resolution/format overrides + `resize_nearest`
  (nearest-neighbour RGBA8 scaling).
- Backpressure: `CaptureConfig::min_interval` drops frames arriving faster than
  the throttle.
- Structured logging via `tracing` across all crates.
- `GET /health` liveness endpoint on the daemon.
- Graceful shutdown: `CaptureSink::on_shutdown()` + `CapturePipeline::shutdown()`.
- Benchmark harness (Sprint 5.6): `criterion` dev-dep + `submit_throughput` /
  `resize_nearest` benches; baselines in `BENCHMARKS.md`.

#### Phase 6 — Docs & release
- Rustdoc for the full public API surface (`CapturedFrame`, `PixelFormat`,
  `CaptureSink` + methods, `CapturePipeline` + `add_sink`/`submit`/`configure`/
  `shutdown`, `CaptureConfig` + fields, all sinks, Bevy plugin types, daemon
  bridge). `cargo doc --no-deps --workspace` builds warning-free.
- Bevy integration walkthrough (`docs/radredeye/BEVY_INTEGRATION.md`).
- Godot integration walkthrough (`docs/radredeye/GODOT_INTEGRATION.md`).
- Final `ARCHITECTURE.md` sink × engine matrix (5 crates, 4 sinks, 3 stubbed
  adapters).
- `CHANGELOG.md` (this file) and `scripts/release.sh` release wrapper.
- Doc drift reconciled: `AGENTS.md` (5 crates / 20 tests), `SUPPORT_MATRIX.md`
  (Unity/Unreal/WebXR marked done), `ROADMAP.md` residual "uncommitted" notes fixed.

#### DevGate / guardrails enforcement (wired alongside Phase 5.6)
- `scripts/devgate/` executors vendored (regression check, pattern scan).
- `.guardrails/prevention-rules/pattern-rules.json` Rust rules enforced
  (`PREVENT-013` `unwrap()`, `PREVENT-RUST-001..004` unsafe/panic/todo/unchecked).
- Pre-commit hook + CI workflows (`guardrails.yml`, `secret-validation.yml`,
  `documentation-check.yml`) gate all changes.
- 600/900 soft/hard line limits enforced on `crates/**/*.rs` + engine bindings.

### Changed
- Workspace `Cargo.toml` pinned at `version = "0.1.0"` (MIT OR Apache-2.0).
- `STATUS.md` updated to reflect 5 crates, 20 tests, 37/42 sprints complete.

### Fixed
- Daemon `for mut request in server.incoming_requests()` borrow error (Sprint 1.7).
- `StdoutSink` now ignores broken-pipe (`EPIPE`) errors instead of failing.

---

## Versioning

This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html):

- **MAJOR**: incompatible API changes.
- **MINOR**: backwards-compatible functionality additions.
- **PATCH**: backwards-compatible bug fixes.

### Release process

1. Ensure the working tree is clean.
2. Run `scripts/release.sh` (or `scripts/release.sh --dry-run` to preview).
3. The script runs `cargo test --workspace` + `cargo clippy --workspace -- -D warnings`,
   creates a `v$(version)` git tag, and (with `--publish`) runs `cargo publish` in
   dependency order. It does **not** `git push` or `cargo publish` without an
   explicit flag.
