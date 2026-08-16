#!/bin/bash
# Pre-Commit Hook - Runs before git commit
# Validates: AI attribution, no secrets, scope

set -euo pipefail

echo "[GUARDRAILS] Pre-commit validation running..."

# NOTE: git's pre-commit hook receives NO arguments; the commit-message file
# is only passed to commit-msg / prepare-commit-msg. The Co-Authored-By check
# therefore lives in .claude/hooks/commit-msg, not here.

# Check for secrets in staged files using trufflehog if available
if command -v trufflehog &> /dev/null; then
    if ! trufflehog git file://. --since-commit HEAD --only-verified --fail 2>/dev/null; then
        echo "[ERROR] Potential secrets detected in staged files"
        exit 1
    fi
fi

# Rudimentary secret detection (basic patterns)
STAGED_FILES=$(git diff --cached --name-only)
if echo "$STAGED_FILES" | grep -q '\.env'; then
    echo "[ERROR] .env file is staged. Add to .gitignore or use environment variables."
    exit 1
fi

# --- DevGate guardrails integration (radredeye) -------------------------
# Pattern scan across Rust (crates/), Godot (engines/), and TS/JS sources;
# file-size headroom; and the Rust safety gates. Each is guarded so a missing
# tool simply skips its step instead of failing the commit.
if command -v node &> /dev/null; then
    echo "[GUARDRAILS] Pattern scan (guardrails-scan.mjs)..."
    node scripts/devgate/guardrails-scan.mjs || exit 1
fi

if command -v python3 &> /dev/null; then
    echo "[GUARDRAILS] Regression + file-size headroom (regression_check.py)..."
    python3 scripts/devgate/regression_check.py --all --no-audit --no-settings || exit 1
fi

# Rust-only gates: only when Rust sources or manifests actually changed.
STAGED_RUST=$(git diff --cached --name-only --diff-filter=ACM | grep -E '\.rs$|Cargo\.toml|Cargo\.lock' || true)
if [ -n "$STAGED_RUST" ] && command -v cargo &> /dev/null; then
    echo "[GUARDRAILS] cargo clippy..."
    cargo clippy --workspace -- -D warnings || exit 1
    if command -v cargo-audit &> /dev/null; then
        echo "[GUARDRAILS] cargo audit..."
        cargo audit --deny warnings || exit 1
    fi
fi

echo "[GUARDRAILS] Pre-commit validation passed"
