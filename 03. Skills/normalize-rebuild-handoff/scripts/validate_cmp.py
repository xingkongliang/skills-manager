#!/usr/bin/env python3
"""Validate core safety and routing invariants for a CMP JSON/YAML file."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


REQUIRED_CMP_KEYS = {
    "meta",
    "understanding",
    "state",
    "sources",
    "conflicts_unknown",
    "routing",
    "safe_next_action",
    "handoff_prompt",
    "delta",
    "d_acceptance",
}


def load_packet(path: Path) -> Any:
    text = path.read_text(encoding="utf-8")
    if path.suffix.lower() == ".json":
        return json.loads(text)
    if path.suffix.lower() in {".yaml", ".yml"}:
        try:
            import yaml  # type: ignore
        except ImportError as exc:
            raise RuntimeError("YAML requires PyYAML; use JSON or install PyYAML") from exc
        return yaml.safe_load(text)
    raise ValueError("Expected .json, .yaml, or .yml")


def validate(data: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(data, dict) or not isinstance(data.get("cmp"), dict):
        return ["root must contain object 'cmp'"]

    cmp = data["cmp"]
    missing = sorted(REQUIRED_CMP_KEYS - set(cmp))
    if missing:
        errors.append(f"missing CMP keys: {', '.join(missing)}")

    meta = cmp.get("meta", {})
    if meta.get("mode") != "NORMALIZE_REBUILD_COMMIT":
        errors.append("meta.mode must equal NORMALIZE_REBUILD_COMMIT")
    if meta.get("status") != "DRAFT_FOR_APPROVAL":
        errors.append("meta.status must equal DRAFT_FOR_APPROVAL")
    if meta.get("destination") != "D · QA / MERGE / RELEASE HANDOFF":
        errors.append("meta.destination must equal D · QA / MERGE / RELEASE HANDOFF")

    routing = cmp.get("routing", {})
    if routing.get("mutation_performed") is not False:
        errors.append("routing.mutation_performed must be false")
    if not routing.get("owner_module"):
        errors.append("routing.owner_module is required")

    acceptance = cmp.get("d_acceptance", {})
    if acceptance.get("decision_status") != "PENDING":
        errors.append("d_acceptance.decision_status must equal PENDING")
    if acceptance.get("canon_declared") is not False:
        errors.append("d_acceptance.canon_declared must be false")
    allowed = acceptance.get("allowed_decisions")
    if allowed != ["ACCEPT", "PATCH", "REJECT"]:
        errors.append("d_acceptance.allowed_decisions must be [ACCEPT, PATCH, REJECT]")

    if not isinstance(cmp.get("handoff_prompt"), str) or not cmp["handoff_prompt"].strip():
        errors.append("handoff_prompt must be a non-empty string")

    return errors


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate_cmp.py <packet.json|packet.yaml>", file=sys.stderr)
        return 2
    path = Path(sys.argv[1])
    try:
        data = load_packet(path)
        errors = validate(data)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"INVALID: {exc}", file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print("VALID: CMP core invariants pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
