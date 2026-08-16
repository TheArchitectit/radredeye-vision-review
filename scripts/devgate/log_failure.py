#!/usr/bin/env python3
"""Minimal failure-registry logger for the DevGate guardrails integration.

Appends JSONL entries to .guardrails/failure-registry.jsonl and can list them.
This is a slim, dependency-free stand-in for the upstream DevGate log_failure.py
(which is not shipped in the framework) so the documented command works:

    python scripts/devgate/log_failure.py --list
    python scripts/devgate/log_failure.py --add \
        --category config --severity high \
        --message "..." --root-cause "..." \
        --files "a.rs,b.rs" --regression-pattern "..." \
        --prevention-rule "..." --fix-commit <sha>

--list / --add are top-level flags (not subcommands) so the documented
invocation works without a subcommand verb.

Entries are append-only (DO NOT edit existing lines). The registry is the
"lock-in" companion to pattern-rules.json: when a guardrail prevents a class of
failure, record it here so the same mistake cannot silently return.
"""
from __future__ import annotations

import argparse
import json
import os
import uuid
from datetime import datetime, timezone
from pathlib import Path

REGISTRY = Path(os.getenv("FAILURE_REGISTRY_PATH", ".guardrails/failure-registry.jsonl"))


def _now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def cmd_list() -> int:
    if not REGISTRY.exists():
        print("(no registry at %s)" % REGISTRY)
        return 0
    shown = 0
    for line in REGISTRY.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            print(json.dumps(json.loads(line)))
            shown += 1
        except json.JSONDecodeError:
            print("# (skipped unparseable line)")
    print("--- %d entr%s ---" % (shown, "y" if shown == 1 else "ies"))
    return 0


def cmd_add(args: argparse.Namespace) -> int:
    REGISTRY.parent.mkdir(parents=True, exist_ok=True)
    entry = {
        "failure_id": "FAIL-" + uuid.uuid4().hex[:8],
        "timestamp": _now(),
        "category": args.category,
        "severity": args.severity,
        "error_message": args.message,
        "root_cause": args.root_cause,
        "affected_files": [f for f in args.files.split(",") if f],
        "fix_commit": args.fix_commit,
        "regression_pattern": args.regression_pattern,
        "prevention_rule": args.prevention_rule,
        "status": "active",
    }
    with REGISTRY.open("a") as fh:
        fh.write(json.dumps(entry) + "\n")
    print("Appended %s" % entry["failure_id"])
    return 0


def main() -> int:
    p = argparse.ArgumentParser(
        description="Append/list guardrails failure-registry entries."
    )
    p.add_argument("--list", action="store_true", help="List existing failure entries")
    p.add_argument("--add", action="store_true", help="Append a new failure entry")
    p.add_argument("--category", default="config")
    p.add_argument("--severity", default="medium")
    p.add_argument("--message", default="")
    p.add_argument("--root-cause", default="")
    p.add_argument("--files", default="", help="Comma-separated affected files")
    p.add_argument("--fix-commit", default="")
    p.add_argument("--regression-pattern", default="")
    p.add_argument("--prevention-rule", default="")
    args = p.parse_args()
    if args.add:
        return cmd_add(args)
    # default: list
    return cmd_list()


if __name__ == "__main__":
    raise SystemExit(main())
