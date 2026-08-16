# Contributing to radredeye

radredeye is a cross-engine viewport-capture pipeline for AI agents. This
project is built and maintained with the help of AI coding agents, so we lean on
**explicit guardrails** rather than tribal knowledge. Read this before opening a
PR.

## The Four Laws of Toolmaking (our north star)

Every change — code, docs, or config — is judged against these:

1. **Explicit is better than implicit.** No silent behavior. Errors are returned,
   not swallowed. Magic should be documented.
2. **Small is beautiful.** Files, functions, and crates stay small and focused.
   See the size limits below.
3. **Composition over configuration.** Prefer wiring small pieces together over
   a tangle of flags and globals.
4. **Fail loudly, not silently.** A guardrail that's hit should say exactly what
   broke and why, and how to fix it.

## How the guardrails are wired

Three layers, all enforced:

| Layer | What | Where |
|-------|------|-------|
| **Pattern rules** | Forbidden code patterns (`.unwrap()` in prod, `unsafe`, hallucinated imports, Godot `.free()`…) | `.guardrails/prevention-rules/pattern-rules.json` + `scripts/devgate/guardrails-scan.mjs` |
| **Regression / size** | File-size headroom, failure-registry scan | `scripts/devgate/regression_check.py` |
| **Rust safety** | `clippy -D warnings`, `cargo audit` | pre-commit hook + `guardrails.yml` |

### Pre-commit hook (installed locally)
```bash
git config core.hooksPath .claude/hooks   # already set in this repo
```
The hook runs, in order: AI-attribution check → secret scan → the DevGate
pattern scan → `regression_check.py` → (if Rust changed) `cargo clippy --workspace
-D warnings` → (if `cargo-audit` installed) `cargo audit --deny warnings`.

> **Note:** the hook requires `Co-Authored-By:` in the commit message (a
> guardrails-template convention). Agent-driven commits include it automatically;
> for manual commits, add:
> `Co-Authored-By: Claude <noreply@anthropic.com>`

### CI
`.github/workflows/` runs on every PR:
- `guardrails.yml` — regression check, pattern scan, clippy, cargo audit.
- `secret-validation.yml` — `.env`/credential scan, hardcoded-secret patterns.
- `documentation-check.yml` — doc file-length (≤500) + link/section checks.
- `ci.yml` — build + `cargo test --workspace`.

## Run the checks yourself (before pushing)
```bash
node scripts/devgate/guardrails-scan.mjs                 # pattern scan
python scripts/devgate/regression_check.py --all --no-audit --no-settings
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

## File-size limits
| Location | Soft | Hard |
|----------|------|------|
| `crates/**/*.rs`, `engines/**/*.{gd,cs,cpp}` | 600 | 900 |
| `src/**` (TS/JS, if added) | 300 | 500 |
| `extensions/**` (TS/JS, if added) | 400 | 500 |
| `**/*.md` (docs) | — | 500 |

When you approach a soft limit, **split the file** (extract a module) rather
than squeezing toward the hard limit.

## Pattern rules — audited exceptions
All `error`-severity rules block CI and the pre-commit hook. If a violation is
genuinely correct, annotate the line inline:
```rust
let x = something().unwrap(); // guardrails-allow PREVENT-013: <why this is safe>
```
The `// guardrails-allow <RULE_ID>: <reason>` form (reason required) is the only
accepted escape hatch — it makes the exception visible and reviewed.

Notable Rust rules:
- `PREVENT-013` — `.unwrap()` in non-test code (use `?`/`expect()`).
- `PREVENT-RUST-001` — `unsafe` blocks need a `// SAFETY:` note + allow.
- `PREVENT-RUST-002` — `panic!()` in library code.
- `PREVENT-RUST-003` — `todo!()`/`unimplemented!()` shipped in committed code.
- `PREVENT-RUST-004` — unchecked derefs (`.unwrap_unchecked()`, …).
- `PREVENT-024` (hallucinated imports) intentionally **excludes** `*.rs`: a
  regex can't tell `use radredeye_core::X` from a fake crate. Rust import
  validity is enforced by `cargo build`/`cargo check` (compile error) and
  `cargo audit` (vulnerable crates) instead.

## Failure registry (lock-in)
When a guardrail prevents a class of failure, record it so it can't silently
return:
```bash
python scripts/devgate/log_failure.py --list
python scripts/devgate/log_failure.py --add \
  --category config --severity high \
  --message "..." --root-cause "..." \
  --files "a.rs" --regression-pattern "..." --prevention-rule "..."
```
The registry is append-only (`.guardrails/failure-registry.jsonl`).

## Documentation rules
- Every doc file ≤ **500 lines**. Split large docs (the `SPEC_AND_SPRINTS.md` /
  `ROADMAP_INTEGRATION.md` integration specs are the deliberate exceptions).
- Start with a `## Overview` and end with a `Last Updated:` footer.
- Internal links must resolve. No trailing whitespace.

## Commit & PR hygiene
- Keep commits focused; let the pre-commit hook gate quality.
- PRs should mention which guardrails they touch (if any).
- Don't bypass the hook with `--no-verify` — if a gate is wrong, fix the gate.
