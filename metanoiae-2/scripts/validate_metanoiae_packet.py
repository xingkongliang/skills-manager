#!/usr/bin/env python3
"""Validate the minimum structural contract of a METANOIAE packet."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


REQUIRED_FIELDS = {
    "task_id",
    "cell",
    "status",
    "authority_scope",
    "source_pointers",
    "allowed_operations",
    "prohibited_operations",
    "sources_used",
    "actions_taken",
    "candidate_delta",
    "assumptions",
    "conflicts",
    "limitations",
    "next_route",
    "returns_to",
}

ALLOWED_CELLS = {
    "A",
    "B",
    "C",
    "D",
    "E",
    "ORIGINS",
    "TRANSVERSAL",
    "IGNITION_COIL",
    "COMET_VAULT",
}

FORBIDDEN_TRUE_KEYS = {
    "self_approved",
    "canon_declared",
    "runtime_integrated",
    "runtime_activation",
    "hidden_unversioned_state",
    "cross_write",
    "D_bypassed",
}

REQUIRED_PROHIBITIONS = {
    "self_approval",
    "canon_declaration",
    "runtime_integration",
    "hidden_unversioned_state",
    "D_bypass",
}


def load_document(path: Path) -> Any:
    text = path.read_text(encoding="utf-8")
    if path.suffix.lower() == ".json":
        return json.loads(text)
    try:
        import yaml  # type: ignore
    except ImportError as exc:
        raise RuntimeError(
            "YAML input requires PyYAML; use JSON or install PyYAML."
        ) from exc
    return yaml.safe_load(text)


def unwrap(document: Any) -> dict[str, Any]:
    if not isinstance(document, dict):
        raise ValueError("The document root must be a mapping/object.")
    if "METANOIAE_PACKET" in document:
        packet = document["METANOIAE_PACKET"]
        if not isinstance(packet, dict):
            raise ValueError("METANOIAE_PACKET must contain a mapping/object.")
        return packet
    return document


def walk_forbidden(value: Any, path: str = "$") -> list[str]:
    errors: list[str] = []
    if isinstance(value, dict):
        for key, child in value.items():
            child_path = f"{path}.{key}"
            if key in FORBIDDEN_TRUE_KEYS and child is True:
                errors.append(f"{child_path} must not be true")
            errors.extend(walk_forbidden(child, child_path))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            errors.extend(walk_forbidden(child, f"{path}[{index}]"))
    return errors


def validate(packet: dict[str, Any]) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []

    missing = sorted(REQUIRED_FIELDS - packet.keys())
    if missing:
        errors.append("Missing required fields: " + ", ".join(missing))

    cell = packet.get("cell")
    if cell not in ALLOWED_CELLS:
        errors.append(
            f"cell must be one of {sorted(ALLOWED_CELLS)}; observed {cell!r}"
        )

    for field in (
        "source_pointers",
        "allowed_operations",
        "prohibited_operations",
        "sources_used",
        "actions_taken",
        "assumptions",
        "conflicts",
        "limitations",
    ):
        if field in packet and not isinstance(packet[field], list):
            errors.append(f"{field} must be a list")

    prohibitions = packet.get("prohibited_operations", [])
    if isinstance(prohibitions, list):
        missing_prohibitions = sorted(REQUIRED_PROHIBITIONS - set(prohibitions))
        if missing_prohibitions:
            errors.append(
                "Missing mandatory prohibitions: "
                + ", ".join(missing_prohibitions)
            )

    for route_field in ("next_route", "returns_to"):
        route = str(packet.get(route_field, ""))
        if "D" not in route or "QA" not in route:
            errors.append(f"{route_field} must route to D · QA / MERGE / RELEASE")

    delta = packet.get("candidate_delta")
    if "candidate_delta" in packet and not isinstance(delta, dict):
        errors.append("candidate_delta must be a mapping/object")
    elif isinstance(delta, dict):
        for field in ("confirmed", "changed", "blocked", "unknown", "next"):
            if field not in delta:
                warnings.append(f"candidate_delta.{field} is absent")

    errors.extend(walk_forbidden(packet))

    if not packet.get("source_pointers"):
        warnings.append("source_pointers is empty; evidence coverage may be UNKNOWN")
    if not packet.get("authority_scope"):
        errors.append("authority_scope must not be empty")

    return errors, warnings


def self_test() -> int:
    packet = {
        "task_id": "SELF-TEST",
        "cell": "TRANSVERSAL",
        "status": "CANDIDATE",
        "authority_scope": "STRUCTURE_TEST_ONLY",
        "source_pointers": ["SRC-TEST"],
        "allowed_operations": ["validate"],
        "prohibited_operations": sorted(REQUIRED_PROHIBITIONS),
        "sources_used": ["SRC-TEST"],
        "actions_taken": ["validated"],
        "candidate_delta": {
            "confirmed": [],
            "changed": [],
            "blocked": [],
            "unknown": [],
            "next": [],
        },
        "assumptions": [],
        "conflicts": [],
        "limitations": [],
        "next_route": "D · QA / MERGE / RELEASE",
        "returns_to": "D · QA / MERGE / RELEASE",
    }
    errors, warnings = validate(packet)
    print(json.dumps({"valid": not errors, "errors": errors, "warnings": warnings}))
    return 0 if not errors else 1


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate a METANOIAE generic cell packet."
    )
    parser.add_argument("packet", nargs="?", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.packet is None:
        parser.error("provide a JSON/YAML packet path or use --self-test")

    try:
        packet = unwrap(load_document(args.packet))
        errors, warnings = validate(packet)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        print(json.dumps({"valid": False, "errors": [str(exc)], "warnings": []}))
        return 2

    print(
        json.dumps(
            {"valid": not errors, "errors": errors, "warnings": warnings},
            ensure_ascii=False,
            indent=2,
        )
    )
    return 0 if not errors else 1


if __name__ == "__main__":
    sys.exit(main())
