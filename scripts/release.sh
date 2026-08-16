#!/usr/bin/env bash
#
# scripts/release.sh — radredeye release wrapper.
#
# Verifies the working tree is clean, runs the full gate (test + clippy), then
# tags the release and (optionally) publishes the crates to crates.io.
#
# Usage:
#   scripts/release.sh                # clean check + gates + create v$VERSION tag
#   scripts/release.sh --dry-run      # run gates only; echo tag/publish/push, do nothing destructive
#   scripts/release.sh --publish      # also run `cargo publish` in dependency order
#   scripts/release.sh --push        # also run `git push --follow-tags`
#   scripts/release.sh --dry-run --publish --push   # preview everything
#
# Safety:
#   - `git push` is NEVER run without `--push`.
#   - `cargo publish` is NEVER run without `--publish`.
#   - With `--dry-run`, no tag, publish, or push occurs (gates still run).
#   - The script does not modify Cargo.toml; the version is read from the
#     workspace manifest.
#
# Exit codes:
#   0  success (or dry-run preview completed)
#   1  working tree dirty, gate failure, or tag already exists

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

DRY_RUN=0
DO_PUBLISH=0
DO_PUSH=0

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit 1
}

for arg in "$@"; do
  case "$arg" in
    --dry-run)  DRY_RUN=1 ;;
    --publish)  DO_PUBLISH=1 ;;
    --push)     DO_PUSH=1 ;;
    -h|--help)  usage ;;
    *) echo "error: unknown flag: $arg" >&2; usage ;;
  esac
done

# Crates in dependency (publish) order. radredeye-webxr is excluded from
# the workspace build (wasm32 target) and intentionally not published here.
PUBLISH_ORDER=(
  radredeye-core
  radredeye-bevy
  radredeye-mcp
  radredeye-unreal
  radredeye-unity
)

# ---------------------------------------------------------------------------
# Step 0 — read the workspace version.
# ---------------------------------------------------------------------------
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read version from Cargo.toml" >&2
  exit 1
fi
echo "==> radredeye release: v${VERSION}"

# ---------------------------------------------------------------------------
# Step 1 — verify the working tree is clean.
# ---------------------------------------------------------------------------
echo "==> [1/4] checking working tree is clean…"
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "error: working tree has uncommitted changes — commit or stash before releasing." >&2
  git status --short >&2
  exit 1
fi
echo "    clean."

# ---------------------------------------------------------------------------
# Step 2 — run the gate: cargo test + cargo clippy.
# ---------------------------------------------------------------------------
echo "==> [2/4] running cargo test --workspace…"
cargo test --workspace
echo "==> [2/4] running cargo clippy --workspace -- -D warnings…"
cargo clippy --workspace -- -D warnings
echo "    gates passed."

# ---------------------------------------------------------------------------
# Step 3 — create the v$VERSION git tag (skipped in dry-run).
# ---------------------------------------------------------------------------
TAG="v${VERSION}"
echo "==> [3/4] git tag ${TAG}…"
if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "error: tag ${TAG} already exists." >&2
  exit 1
fi
if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "    (dry-run) would run: git tag ${TAG}"
else
  git tag "$TAG"
  echo "    created tag ${TAG}."
fi

# ---------------------------------------------------------------------------
# Step 4 — publish crates (dependency order), guarded by --publish.
# ---------------------------------------------------------------------------
echo "==> [4/4] cargo publish…"
for crate in "${PUBLISH_ORDER[@]}"; do
  if [[ "$DRY_RUN" -eq 1 ]] || [[ "$DO_PUBLISH" -eq 0 ]]; then
    echo "    (would run) cargo publish -p ${crate}"
  else
    echo "    publishing ${crate}…"
    cargo publish -p "$crate"
  fi
done

# ---------------------------------------------------------------------------
# Optional — git push (guarded by --push).
# ---------------------------------------------------------------------------
if [[ "$DO_PUSH" -eq 1 ]] && [[ "$DRY_RUN" -eq 0 ]]; then
  echo "==> git push --follow-tags…"
  git push --follow-tags
else
  echo "==> (skipped) git push --follow-tags  # pass --push to push the tag"
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "==> dry-run complete — no tag, publish, or push performed."
else
  echo "==> release ${TAG} ready. (publish/push require their explicit flags.)"
fi
