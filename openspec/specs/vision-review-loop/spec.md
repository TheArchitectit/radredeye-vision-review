# Vision-Review Loop (bevy-vision-review integration)

Status: Proposed. Merges radredeye capture with a capture->score->feedback review loop.

## Loop
1. CAPTURE: radredeye captures a rendered frame (Bevy plugin / any adapter) -> CapturedFrame.
2. SUBMIT: frame POSTed via submit_frame /radredeye-mcp or /capture.
3. SCORE: vision model (RADIMAGEMAKER or a review model) returns quality score + issues.
4. FEEDBACK: score/issues written to review artifact; agent consumes to drive fixes.
5. REGRESSION: scored frames append to failure-registry.jsonl; determinism seed logged.

## Gates (fail closed)
- CAPTURE_OK: every screen can produce a CapturedFrame without error.
- SCORE_PRESENT: every submitted frame returns a score (no silent drop).
- LOOP_CLOSED: review artifact exists and is consumed (no orphaned capture).
- PERF: capture pipeline stays within frame-time budget.
- DETERMINISM: seeded replay reproduces the same captured frames + scores.

## Origins
- radredeye (Rust/MCP stateless capture, Bevy/U3/Unreal/Godot/WebXR adapters)
- bevy-vision-review: capture-frame -> vision-model -> quality score -> agent feedback
  (reference: Sword of Hope / BiteClub SPRINT_PLAN_VISION_REVIEW.md, prod-015 vision pipeline)
- RADIMAGEMAKER as art-generation source (separate repo C)
