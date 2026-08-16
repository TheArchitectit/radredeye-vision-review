# Phase 9.2 — Visual / Screenshot Regression (Golden Frames)

Golden-frame comparison harness for the radredeye capture pipeline. This
directory holds **committed golden PNGs** that the test suite compares freshly
encoded frames against, so a rendering/encoding regression is caught before it
ships.

**Companion test:** [`crates/radredeye-core/tests/golden.rs`](../../crates/radredeye-core/tests/golden.rs)

---

## What lives here

| File                  | Origin                                                      |
|-----------------------|-------------------------------------------------------------|
| `gradient_8x4.png`    | Committed golden for the deterministic CPU-only gradient fixture. |
| `README.md`           | This file.                                                   |

The `gradient_8x4.png` golden is produced by the test's **record** mode from a
fully deterministic 8×4 RGBA gradient (`R = x*32, G = y*64, B = 128, A = 255`).
Because the fixture is a pure function of `(x, y)` and the `image` PNG encoder is
deterministic, the golden is reproducible across machines and toolchain versions.

---

## Why a CPU-only synthetic frame (not a real adapter screenshot)

True per-adapter golden capture — rendering the *same* scene in Bevy, Godot, and
WebXR and diffing the output — **requires a display and a GPU**. Headless CI
runners have neither, and even with a virtual framebuffer (`xvfb`) the rendered
pixels vary across GPU drivers, window managers, and font-rasterization versions.
A brittle golden that flips on every runner change erodes trust and gets
disabled — the opposite of a regression gate.

So Phase 9.2 splits the work into two layers:

1. **Comparison mechanism (this directory, CPU-only, always runs in CI).**
   `golden.rs` builds a known synthetic `CapturedFrame`, encodes it to PNG, and
   compares it against the committed golden via a mean per-channel pixel-diff
   threshold. This proves the encode → decode → diff pipeline end-to-end
   *without* a GPU, and a second test (`golden_diff_detects_regression`) encodes
   a *different* frame and asserts the harness flags it — so we know the gate
   catches regressions, not just that it always passes.

2. **Per-adapter golden capture (CI-separate, display-required).**
   Real Bevy / Godot / WebXR screenshots are captured on a display-equipped
   runner (or locally with `xvfb`) and compared with a **permissive threshold**
   (pixel-diff accounting for AA/font/driver variance) — or, better, via a
   perceptual/structural diff. This is wired into CI separately under the
   adapter integration suites:

   - **Bevy** — Phase 6.2 ([`docs/radredeye/bevy-integration.md`](../../docs/radredeye/bevy-integration.md)).
   - **Godot** — Phase 6.3 ([`docs/radredeye/godot-integration.md`](../../docs/radredeye/godot-integration.md)).
   - **WebXR** — adapter stub in `crates/radredeye-webxr` (excluded from the
     default workspace; built under a `wasm32` CI job).

   Per-adapter goldens are **not** committed here; they live with each adapter's
   integration test fixtures and are regenerated when the rendering intent
   deliberately changes.

---

## Running the tests

```sh
# Compare mode (default, CI): asserts the encoded frame matches the golden.
cargo test -p radredeye-core --test golden

# Record mode: (re)writes the golden PNG from the current fixture.
# Run this after an intentional change to the gradient fixture, then commit
# tests/golden/gradient_8x4.png.
GOLDEN_RECORD=1 cargo test -p radredeye-core --test golden
```

---

## Threshold

The CPU-only gradient fixture uses a **0.0** mean-pixel-diff threshold (identical
input → identical pixels; the PNG encoder is lossless). Per-adapter goldens use a
non-zero, adapter-specific threshold tuned to the expected driver/AA variance and
documented alongside their fixtures.

---

## Acceptance

> **"Golden diff catches regressions."** The CPU-only test demonstrates the
> mechanism and passes; `golden_diff_detects_regression` proves a changed frame
> is flagged. Per-adapter golden capture is wired into CI separately (display
> required) per Phase 6.2 / 6.3.
