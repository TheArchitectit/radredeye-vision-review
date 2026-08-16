# radredeye — Review & Integrated Roadmap

**Companion to:** `STATUS.md`, `ROADMAP.md` (repo root), `docs/radredeye/`  
**Full spec + granular sprints:** see **[`SPEC_AND_SPRINTS.md`](SPEC_AND_SPRINTS.md)** (corrects the false premises below with code-grounded facts).
**Scope of this document:** Review + integration plan only. No source, Cargo, or crate changes.
**Last updated:** 2026-07-03

---

## 1. Executive Summary — The 3-Layer Agentic-Dev Stack

Autonomous and AI-assisted software development needs three cooperating planes, and
radredeye is the **Perception plane**. Together with the two sibling frameworks
from TheArchitectit they form a coherent stack:

- **Perception (radredeye — *this project*):** gives AI agents *eyes* into
  running game engines. It captures rendered frames from Bevy / Godot / Unity /
  Unreal / WebXR, normalizes them into a `CapturedFrame`, and fans them out through
  `CapturePipeline` to sinks (stdout, filesystem, HTTP, WebSocket, gRPC). This is the
  *data/observation* layer that supplies visual context to agents.
- **Safety (Guardrails / Four Laws — `agent-guardrails-template`):** the
  *behavior* layer. The Four Laws (Read-before-edit, Stay-in-scope,
  Verify-before-commit, Halt-when-uncertain) plus halt-conditions, three-strikes,
  and scope-validation keep agents operating safely and within authorized bounds.
- **Enforcement (DevGate — `DevGate-Agentic-Framework`):** the *quality-gate*
  layer. It is **not** an agent runtime — it is the polyglot (Node/Python) toolkit
  that validates, gates, and deploys code agents produce: regression scanner,
  pattern/semantic scanners, failure registry, prevention rules, and a 15-step
  deploy gate.

The thesis: **agents perceive through radredeye, behave under the Guardrails,
and ship only after DevGate enforces the gate.** radredeye is currently a
healthy, near-release (v0.1.0) *perception* component with *no* enforcement wired in.
This document reviews its health, analyzes the gap to DevGate/Guardrails, and lays
out a phased plan to plug the three layers together.

---

## 2. Project Review — radredeye Health

### What's working (strengths)
- **Clean three-layer pipeline:** `CapturedFrame` → `CapturePipeline` bus → trait-object
  `Arc<dyn CaptureSink>` sinks. Engine handles never leak past core
  (`docs/radredeye/ARCHITECTURE.md`).
- **Broad surface:** 5 crates (`radredeye-core`, `-bevy`, `-daemon`, `-unreal`,
  `-unity`), 4 sinks (Stdout/File/HTTP/WebSocket/gRPC — last two feature-gated),
  Godot addon + daemon bridge (`/capture`, `/health`), 3 engine adapters stubbed
  (Unity/Unreal/WebXR).
- **Testing & CI mature for the domain:** 20 tests passing; `.github/workflows/ci.yml`
  runs `cargo build --workspace`, `cargo test --workspace`, and
  `cargo clippy --workspace -- -D warnings`.
- **Production-hardening largely done (Phase 5):** config/resize (`5.1`), backpressure
  (`5.2`), `tracing` logging (`5.3`), `/health` (`5.4`), graceful shutdown (`5.5`).
- **Proven bus design that scales to multi-agent/replay later:** the fan-out bus is
  the natural seam for orchestration.

### Gaps (what's missing for v0.1.0)
- **Sprint 5.6 (Benchmark Harness / Criterion)** is the only remaining Phase-5 sprint
  and the sole blocker for Phase 6. No baseline latency budget exists yet, so there
  is no regression baseline to gate on later.
- **Phase 6 (Docs & Release) is entirely unstarted (0/5):** Rustdoc (`6.1`),
  Bevy/Godot usage guides (`6.2`/`6.3`), final `ARCHITECTURE.md` sink/engine matrix
  (`6.4`), version bump + CHANGELOG + tag (`6.5`).
- **Documentation drift (should be reconciled in Phase 6):**
  - `AGENTS.md` says *"Three crates in the workspace"* and *"No Rust tests in
    bevy/daemon crates yet — only `radredeye-core` has tests,"* but `STATUS.md`/
    `ROADMAP.md` report **5 crates** and **20 tests** (12 core, 4 daemon, 4 bevy).
    `AGENTS.md` predates Phase 4 (Unity/Unreal crates) and is stale.
  - `docs/radredeye/SUPPORT_MATRIX.md` still lists Unity/Unreal/WebXR as
    *"📝 planned (Phase 4)"* even though ROADMAP marks Phase 4 **COMPLETE**
    (sprints 4.1–4.6 ✅). The matrix lags the roadmap.
  - `ROADMAP.md` Phase 1 sprints show *"implemented, uncommitted"* notes, but the
    history shows commit `f28fb4e` landed them; the notes are residual.
- **No enforcement layer at all:** no quality gate, no pre-commit hook, no failure
  registry population, no regression scanner wired to the Rust code.

### The uncommitted CI-deletion loose end (NOTE — do not fix here)
- `ci/Jenkinsfile` and `ci/gitlab-ci.yml` were deleted (sprint 2.7 claimed to remove
  template-CI residue) but the deletion is **uncommitted** — the `ci/` directory no
  longer exists in the working tree. This is a real loose end: either commit the
  deletion or restore the files; it must not be silently left half-done. Flagged for
  the owner, **out of scope for this document.**

### Overall verdict
Architecturally sound and ~86% complete (36/42 sprints). The path to v0.1.0 is short
and well-defined; the bigger opportunity is to position it as the perception layer of
the 3-layer stack and add the enforcement plane it currently lacks.

---

## 3. Gap Analysis vs DevGate & Guardrails

radredeye already *contains* some Guardrails residue (the `skills/shared-prompts/`
Four Laws, `docs/AGENT_GUARDRAILS.md`, and a `.guardrails/` directory). But the
**enforcement** half is missing, and DevGate is **Node/Python-oriented** — its
scanners do not understand a Rust workspace out of the box.

| # | Capability | DevGate provides | Current state in this repo | Gap |
|---|-----------|------------------|----------------------------|-----|
| G1 | Vendored quality gate | `scripts/` (regression, guardrails-scan, semantic-scan, run-tests, schema-health, deploy) | Not vendored; only template residue docs | No gate executable in-repo |
| G2 | Failure registry populated | `.guardrails/failure-registry.jsonl` | **Present but empty** (header-only); the one known historical bug — the daemon `for mut request` borrow error from Sprint 1.7 — is **not** logged | Registry exists, unused |
| G3 | Rust-aware file-size regression | `regression_check.py` scans `src/`+`extensions/` for `*.ts/*.tsx` only | Rust lives in `crates/` (`*.rs`); **scanner ignores it entirely** | No size gate on Rust |
| G4 | Pattern scanner on Rust | `guardrails-scan.mjs` walks `*.ts/*.js` only | `.guardrails/prevention-rules/pattern-rules.json` *already contains* Rust rules (PREVENT-013 `unwrap()` in non-test, PREVENT-024 hallucinated imports, PREVENT-002 SQL for `*.go`) — **but the scanner never matches them** because it only globs ts/js | Rules data present, execution blind to Rust |
| G5 | Semantic scanner | `semantic-scan.mjs` = TypeScript AST, `SEMANTIC-001` unhandled promises | No Rust equivalent; Rust's type system + `clippy` cover this class instead | Wrong language; use clippy |
| G6 | Schema health | `schema-health-check.mjs` = PostgreSQL schema | No DB in a capture pipeline | N/A — skip |
| G7 | Test runner | `run-tests.mjs` = `node --test` | Rust uses `cargo test` (already in CI) | Keep `cargo test`; no Node tests |
| G8 | npm audit gate | `regression_check.py` npm audit (runtime HIGH/CRITICAL) | No `package.json`/npm deps here | Inapplicable; need `cargo audit` (RustSec) instead |
| G9 | Settings coverage | checks `MEGACOMPACT_*` env vars vs dashboard | Our only env var is `RADREDEYE_HTTP_SINK_URL`; no dashboard | Inapplicable; rewrite or skip |
| G10 | Pre-commit hook | README references `.claude/hooks/pre-commit.sh` | **That file does NOT exist** in the DevGate scaffold (only `.git/hooks/pre-commit.sample`) | Must be *authored*, not copied |
| G11 | CI workflows | README lists `ci.yml`, `regression-guard.yml`, `guardrails-lint.yml`, `secret-validation.yml`, `documentation-check.yml` | **None of those YAML files exist** in the scaffold; only `.github/workflows/ci.yml` (Rust CI) is ours | Must be *authored* to match |
| G12 | Four Laws enforcement | Prompts in `skills/shared-prompts/four-laws.md` (already here) | Soft only — not enforced by a hook/gate | Add hard enforcement via DevGate gate |

**Key insight:** the *rules data* (G4) and the *registry* (G2) are already vendored
into this repo's `.guardrails/`; what's missing is the **executable wiring** (G1, G3,
G10, G11) and **Rust-aware adaptation** (G3, G4, G8, G9). DevGate's shipped
`.github/workflows/` and `.claude/hooks/pre-commit.sh` do **not** exist as files — the
README describes intended behavior we must implement, not artifacts we can copy.

---

## 4. Integration Strategy

Goal: adopt DevGate's *enforcement* as the hard gate, formalize the Guardrails
*behavior* layer already partially present, and keep radredeye's existing
`cargo`-based Rust tooling as the source of truth for Rust-specific checks.

### 4.1 Vendor DevGate as scripts (not a second `.guardrails/`)
- **Option A (recommended):** copy the DevGate executors into `scripts/devgate/`
  (README explicitly supports "copy specific tools"). Keep **this repo's**
  `.guardrails/` (it already has Rust rules) as the single source of truth.
- **Option B:** `git submodule add … .devgate` for upgradability. Risk: DevGate's own
  `.guardrails/prevention-rules/pattern-rules.json` (pi-mega-compact oriented, with
  `PREVENT-PI-001/002/004`) would coexist and could be confused with ours. If chosen,
  point the scanners at our root `.guardrails/` via `FAILURE_REGISTRY_PATH` /
  `PREVENTION_RULES_PATH` and ignore the submodule's `.guardrails/`.
- **Decision needed (Law 4 halt):** submodule vs copied scripts — see §6.

### 4.2 Make the scanners Rust-aware (adapt, don't replace)
- **`scripts/devgate/regression_check.py`:** add `crates/` (and `engines/` for
  GDScript, `extensions/`) to `FILE_SIZE_DIRS`; extend the size scan to `*.rs`, `*.gd`,
  `*.cs`, `*.cpp`, `*.ts`; raise limits sensibly for Rust (e.g. 600 soft / 900 hard).
  Run with `--no-audit --no-settings` (or add a `--rust` mode) since npm audit and
  `MEGACOMPACT_*` settings checks are inapplicable. Wire a **`cargo audit`** step
  separately as the Rust security gate (RustSec advisory DB) — this replaces npm audit.
- **`scripts/devgate/guardrails-scan.mjs`:** extend `walk()` to emit `*.rs`/`*.gd`
  (currently `*.ts/*.js` only). `ruleAppliesTo()` already consults each rule's
  `file_glob`, so PREVENT-013 (`unwrap()` in non-test) and PREVENT-024 (hallucinated
  import) will then actually fire. The script resolves `.guardrails/` relative to its
  own location (parent of `scripts/`), which lands on our repo-root `.guardrails/` —
  correct by construction under `scripts/devgate/`.
- **`semantic-scan.mjs`:** do **not** run on Rust (TS-only `SEMANTIC-001`). Instead,
  treat `cargo clippy --workspace -- -D warnings` (already in CI) as the Rust semantic
  gate. Optionally add a Rust-specific `SEMANTIC-RUST-001` (ignored `Result`/`Future`)
  rule — but `#[must_use]` + clippy already cover this; low priority.
- **`schema-health-check.mjs` / `run-tests.mjs` / `deploy.sh`:** skip. No DB; `cargo
  test` is the runner; `cargo publish` (not npm) is the Rust release path. Document
  that `deploy.sh`'s *spirit* (clean tree → gate → tag-before-publish → publish) maps
  to a thin `scripts/release.sh` wrapper around `cargo publish` for sprint 6.5.

### 4.3 Formalize the Guardrails (behavior) layer
- The Four Laws are already at `skills/shared-prompts/four-laws.md` and
  `docs/AGENT_GUARDRAILS.md`. Make them *operational*:
  - Keep Law 1 (Read-before-edit) reinforced by `.guardrails/pre-work-check.md`
    (already present) — add a CI step that fails if a PR touches code without a
    corresponding doc reference, or simpler: document the read-before-edit rule in
    `CONTRIBUTING.md`.
  - Law 4 (Halt-when-uncertain) is the governing principle for the integration work
    itself (see §6).
- **Do NOT stand up the Guardrails Go MCP server** (PostgreSQL + Redis + dashboard).
  It is overkill for a Rust capture library and is not vendored here. DevGate's
  pre-commit + CI gates provide sufficient hard enforcement. (Confirm in §6.)

### 4.4 CI / pre-commit mapping (author, since files don't exist upstream)
- **Pre-commit hook** (`scripts/devgate/pre-commit.sh`, installed via
  `git config core.hooksPath scripts/devgate` or copied to `.git/hooks/`): runs
  `python3 scripts/devgate/regression_check.py --pre-commit --no-audit --no-settings`,
  `node scripts/devgate/guardrails-scan.mjs`, and `cargo clippy --workspace -- -D warnings`.
- **New workflow `.github/workflows/guardrails.yml`** (PR-triggered): DevGate
  regression + guardrails-scan + `cargo audit` + `cargo clippy`. This is the
  in-repo realization of DevGate's described `regression-guard.yml` +
  `guardrails-lint.yml`.
- **New workflow `.github/workflows/secret-validation.yml`:** Gitleaks-style scan for
  hardcoded secrets/credentials (DevGate's `secret-validation.yml` concept). Currently
  low risk (only `RADREDEYE_HTTP_SINK_URL` config, no secrets), but cheap insurance.
- **New workflow `.github/workflows/documentation-check.yml`:** enforce the
  guardrails-template 500-line doc rule + broken-link check on `docs/` (maps DevGate's
  `documentation-check.yml`). Keeps the large `docs/` tree maintainable.
- Keep the existing `ci.yml` (build/test/clippy) as the fast lane.

### 4.5 Populate the failure registry (prove the gate works)
Log the one known historical bug into `.guardrails/failure-registry.jsonl`:
the daemon `for mut request in server.incoming_requests()` borrow error (Sprint 1.7,
fixed in `crates/radredeye-mcp/src/main.rs:32`). This demonstrates the
registry → prevention-rule → scanner loop end-to-end before adding new rules.

---

## 5. Phased Roadmap Beyond v0.1.0

Existing plan (unchanged, shown for continuity): **Phase 5** (5.6 benchmark remaining)
→ **Phase 6** (Docs & Release, blocked by 5.6). New phases realize the 3-layer stack.

### Phase 5 — Production Hardening (IN PROGRESS, unchanged)
| Sprint | Deliverable | Exit criteria |
|--------|-------------|---------------|
| 5.6 | Benchmark Harness (Criterion) | `criterion` dep on `radredeye-core`; `submit_throughput` + `resize_nearest` benches; baseline numbers in `BENCHMARKS.md` |

### Phase 6 — Docs & Release (unchanged, + drift reconciliation)
| Sprint | Deliverable | Exit criteria |
|--------|-------------|---------------|
| 6.1 | Rustdoc for all public APIs | `cargo doc` warning-free |
| 6.2 | Bevy integration walkthrough | Guide builds & runs |
| 6.3 | Godot integration walkthrough | Guide builds & runs |
| 6.4 | Final `ARCHITECTURE.md` matrix | Sinks×engines table accurate |
| 6.5 | Version bump, CHANGELOG, tag v0.1.0 | `cargo publish` gate wrapper (`scripts/release.sh`) |
| 6.6 *(new)* | Reconcile doc drift | `AGENTS.md` (5 crates / 20 tests), `SUPPORT_MATRIX.md` (Unity/Unreal/WebXR = done), `ROADMAP.md` residual "uncommitted" notes fixed |

### Phase 7 — DevGate Adoption (Enforcement plane)
*Depends on: Phase 5.6 (so we have a latency baseline to gate on). Complexity: M/L.*
| Sprint | Deliverable | Exit criteria |
|--------|-------------|---------------|
| 7.1 | Vendor DevGate executors (`scripts/devgate/`) | Scripts present; decision A/B from §4.1 recorded |
| 7.2 | Rust-aware `regression_check.py` (crates/ size, `--no-audit --no-settings`) | Rust files size-checked; npm/settings skipped |
| 7.3 | Rust-aware `guardrails-scan.mjs` (`*.rs`/`*.gd`) | PREVENT-013/024 fire on a seeded Rust violation |
| 7.4 | `cargo audit` step (RustSec) as npm-audit replacement | High/critical advisory blocks |
| 7.5 | Populate failure-registry (Sprint 1.7 daemon bug) | `log_failure.py` entry; scanner cross-refs it |
| 7.6 | `guardrails.yml` CI workflow | PR gate runs regression + scan + audit + clippy |

### Phase 8 — Guardrails Enforcement + Rust Prevention Rules (Safety↔Enforcement wiring)
*Depends on: Phase 7. Complexity: M.*
| Sprint | Deliverable | Exit criteria |
|--------|-------------|---------------|
| 8.1 | Pre-commit hook (`scripts/devgate/pre-commit.sh`) | Local commits blocked on gate failure |
| 8.2 | Expand Rust prevention rules (PREVENT-RUST-00x: `panic!` outside tests, `unsafe` without `// SAFETY`, `TODO` without ticket, `expect()` w/o message) | New rules in `.guardrails/prevention-rules/pattern-rules.json`; scanner covers them |
| 8.3 | `secret-validation.yml` workflow (Gitleaks-style) | No hardcoded secrets; `RADREDEYE_HTTP_SINK_URL` confirmed config-not-secret |
| 8.4 | `documentation-check.yml` (500-line + broken-link) | `docs/` linted in CI |
| 8.5 | Four-Laws operationalization in `CONTRIBUTING.md` + read-before-edit CI note | Contributors see the laws; pre-work-check referenced |

### Phase 9 — radredeye as a First-Class DevGate Observation / Perception Source
*Depends on: Phase 5.6 + Phase 7. Complexity: L (new territory).*
| Sprint | Deliverable | Exit criteria |
|--------|-------------|---------------|
| 9.1 | Capture telemetry → DevGate observability: emit frame-latency / drop-rate metrics that feed the 5.6 benchmark baseline | Latency budget enforced as a regression gate (block PRs that regress beyond budget) |
| 9.2 | Visual/screenshot regression: golden-frame comparison across engine adapters (Bevy vs Godot vs WebXR) | Per-adapter golden tests catch engine UI regressions — perception plane feeding guardrails |
| 9.3 | Perception-assisted debugging hook: on `cargo test` failure in CI, the DevGate hook can request a fresh captured frame from a running engine via the daemon bridge (`/capture`) | Failed runs attach a current frame for triage |

### Future (Phase 10+) — Maturing the Perception Plane
- **Multi-agent capture orchestration:** extend the fan-out bus to multiple cameras /
  engines and multiple consuming agents (the bus already supports N sinks).
- **Replay buffer:** ring-buffer of recent `CapturedFrame`s for agent replay / step-back.
- **Semantic frame diffing:** a vision model computes *semantic* deltas between frames
  (not just pixels) and feeds them to agents — the perception plane graduating from
  "pixels" to "meaning," validated by the guardrails/enforcement layers.
- **Distributed capture:** WebSocket/gRPC sinks already enable remote observers;
  add a registry so agents subscribe to named capture streams.

---

## 6. Risks & Open Questions (per Law 4 — Halt When Uncertain)

1. **Submodule vs copied scripts (§4.1).** Vendoring DevGate as `.devgate` submodule
   keeps it upgradable but introduces a *second* `.guardrails/` (pi-mega-compact
   rules). Copying into `scripts/devgate/` avoids that but diverges from upstream on
   update. **→ HALT: confirm A vs B with the owner before Phase 7.1.**
2. **DevGate's `.github/workflows/` and `.claude/hooks/pre-commit.sh` do not exist in
   the scaffold** (only described in README; confirmed by directory scan). We must
   *author* them, not copy. **→ Acceptable to proceed, but flag the expectation gap.**
3. **npm-audit / settings-coverage inapplicability (G8/G9).** Running
   `regression_check.py` unmodified would either error (no `package.json`) or false-
   block. We run with `--no-audit --no-settings` and add `cargo audit` instead.
   **→ Confirm `cargo audit` (RustSec) is an acceptable Rust security gate.**
4. **Should we stand up the Guardrails Go MCP server?** It provides active
   enforcement but needs PostgreSQL + Redis + a dashboard — disproportionate for a
   Rust library. **→ Recommend NO; confirm before Phase 8.**
5. **Uncommitted `ci/` deletions (Jenkinsfile, gitlab-ci.yml).** Out of scope here;
   must be committed or restored. **→ Flag to owner; not addressed in this doc.**
6. **Doc drift (§2).** `AGENTS.md`, `SUPPORT_MATRIX.md`, `ROADMAP.md` lag reality.
   **→ Fold into Phase 6.6; harmless but erodes trust.**
7. **Benchmark budget undefined (5.6).** No explicit latency/throughput budget yet.
   Phase 9.1 makes this budget a regression gate, so **defining it in 5.6 is
   load-bearing** — recommend setting concrete numbers now rather than after the fact.
8. **License interplay.** DevGate & guardrails-template are BSD-3-Clause; Vision
   Enabler is dual MIT/Apache-2.0. Vendoring (submodule) keeps licenses separate;
   copying scripts requires retaining BSD headers. **→ Note for Phase 7.1; no block.**

---

*This document is review + planning only. It does not modify source, Cargo, or crates.
Persist at `docs/radredeye/ROADMAP_INTEGRATION.md`.*
