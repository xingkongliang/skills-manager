#!/usr/bin/env python3
"""Validate a METANOIA DIVIDE ingestion packet without external dependencies."""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path
from typing import Any


CLAIM_TYPES = {"OBSERVED", "CALCULATED", "INFERRED", "HYPOTHESIS", "PLACEHOLDER"}
VERIFICATION_STATES = {
    "UNCHECKED",
    "SOURCE_CAPTURED",
    "SOURCE_VERIFIED",
    "CROSS_CHECKED",
    "STALE",
    "CONFLICTED",
}
DECISION_STATES = {
    "OPEN",
    "TEST_NEXT",
    "PROPOSAL_ELIGIBLE",
    "HOLD",
    "REJECTED_BY_EVIDENCE",
}
MODES = {
    "scan-demand",
    "scan-vendors",
    "duo-scan",
    "audit-evidence",
    "prepare-ingestion",
    "design-adapter",
}
ID_PATTERNS = {
    "demand_signals": re.compile(r"^DI-[A-Za-z0-9._-]+$"),
    "vendors": re.compile(r"^VI-[A-Za-z0-9._-]+$"),
    "evidence": re.compile(r"^EV-[A-Za-z0-9._-]+$"),
    "opportunities": re.compile(r"^DE-[A-Za-z0-9._-]+$"),
}
ID_FIELDS = {
    "demand_signals": "signal_id",
    "vendors": "vendor_id",
    "evidence": "evidence_id",
    "opportunities": "opportunity_id",
}


def _is_list(value: Any) -> bool:
    return isinstance(value, list)


def _check_enum(
    errors: list[str], path: str, value: Any, allowed: set[str], required: bool = True
) -> None:
    if value is None and not required:
        return
    if value not in allowed:
        errors.append(f"{path}: expected one of {sorted(allowed)}, got {value!r}")


def _check_refs(
    errors: list[str], path: str, refs: Any, known: set[str], allow_empty: bool = True
) -> None:
    if not _is_list(refs):
        errors.append(f"{path}: expected an array")
        return
    if not allow_empty and not refs:
        errors.append(f"{path}: must contain at least one reference")
    for ref in refs:
        if ref not in known:
            errors.append(f"{path}: unknown reference {ref!r}")


def _check_unique_ids(
    errors: list[str], packet: dict[str, Any], collection: str
) -> set[str]:
    rows = packet.get(collection)
    if not _is_list(rows):
        errors.append(f"{collection}: expected an array")
        return set()

    field = ID_FIELDS[collection]
    pattern = ID_PATTERNS[collection]
    ids: list[str] = []
    for index, row in enumerate(rows):
        path = f"{collection}[{index}]"
        if not isinstance(row, dict):
            errors.append(f"{path}: expected an object")
            continue
        value = row.get(field)
        if not isinstance(value, str) or not pattern.fullmatch(value):
            errors.append(f"{path}.{field}: invalid or missing prefixed ID")
            continue
        ids.append(value)

    duplicates = [item for item, count in Counter(ids).items() if count > 1]
    for item in duplicates:
        errors.append(f"{collection}: duplicate ID {item!r}")
    return set(ids)


def validate(packet: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(packet, dict):
        return ["packet: expected a JSON object"]

    if packet.get("schema_version") != "metanoia-divide/1.0":
        errors.append("schema_version: expected 'metanoia-divide/1.0'")

    run = packet.get("run")
    if not isinstance(run, dict):
        errors.append("run: expected an object")
        run = {}
    run_id = run.get("run_id")
    if not isinstance(run_id, str) or not re.fullmatch(r"RUN-[A-Za-z0-9._-]+", run_id):
        errors.append("run.run_id: invalid or missing RUN- ID")
    if not run.get("project_id"):
        errors.append("run.project_id: required")
    _check_enum(errors, "run.mode", run.get("mode"), MODES)

    currency = run.get("currency")
    if currency is not None and (
        not isinstance(currency, str) or not re.fullmatch(r"[A-Z]{3}", currency)
    ):
        errors.append("run.currency: expected ISO 4217 code or null")

    for collection in (
        "demand_signals",
        "vendors",
        "evidence",
        "opportunities",
        "conflicts",
        "next_actions",
    ):
        if not _is_list(packet.get(collection)):
            errors.append(f"{collection}: expected an array")

    di_ids = _check_unique_ids(errors, packet, "demand_signals")
    vi_ids = _check_unique_ids(errors, packet, "vendors")
    ev_ids = _check_unique_ids(errors, packet, "evidence")
    _check_unique_ids(errors, packet, "opportunities")

    for collection in ("demand_signals", "vendors", "evidence"):
        rows = packet.get(collection, [])
        if not _is_list(rows):
            continue
        for index, row in enumerate(rows):
            if not isinstance(row, dict):
                continue
            path = f"{collection}[{index}]"
            _check_enum(errors, f"{path}.claim_type", row.get("claim_type"), CLAIM_TYPES)
            _check_enum(
                errors,
                f"{path}.verification_state",
                row.get("verification_state"),
                VERIFICATION_STATES,
            )
            if collection != "evidence":
                _check_refs(
                    errors,
                    f"{path}.evidence_refs",
                    row.get("evidence_refs"),
                    ev_ids,
                    allow_empty=False,
                )

    vendors = packet.get("vendors", [])
    if _is_list(vendors):
        for index, row in enumerate(vendors):
            if not isinstance(row, dict):
                continue
            price = row.get("price")
            currency = row.get("currency")
            if price is not None and not isinstance(price, (int, float)):
                errors.append(f"vendors[{index}].price: expected number or null")
            if price is not None and (
                not isinstance(currency, str) or not re.fullmatch(r"[A-Z]{3}", currency)
            ):
                errors.append(
                    f"vendors[{index}].currency: required ISO 4217 code when price exists"
                )

    opportunities = packet.get("opportunities", [])
    if _is_list(opportunities):
        for index, row in enumerate(opportunities):
            if not isinstance(row, dict):
                continue
            path = f"opportunities[{index}]"
            _check_enum(
                errors,
                f"{path}.decision_state",
                row.get("decision_state"),
                DECISION_STATES,
            )
            _check_refs(
                errors,
                f"{path}.demand_signal_refs",
                row.get("demand_signal_refs"),
                di_ids,
                allow_empty=False,
            )
            _check_refs(
                errors,
                f"{path}.vendor_refs",
                row.get("vendor_refs"),
                vi_ids,
                allow_empty=True,
            )
            _check_refs(
                errors,
                f"{path}.evidence_refs",
                row.get("evidence_refs"),
                ev_ids,
                allow_empty=False,
            )
            if row.get("decision_state") == "SALE_APPROVED":
                errors.append(f"{path}.decision_state: SALE_APPROVED is not allowed")
            if not row.get("vendor_refs") and not row.get("supply_gap"):
                errors.append(
                    f"{path}: empty vendor_refs requires a non-empty supply_gap"
                )
            if not row.get("next_action"):
                errors.append(f"{path}.next_action: required")

    return errors


def _self_test() -> int:
    valid_packet = {
        "schema_version": "metanoia-divide/1.0",
        "run": {
            "run_id": "RUN-self-test",
            "project_id": "PROJECT-self-test",
            "mode": "duo-scan",
            "currency": "MXN",
        },
        "demand_signals": [
            {
                "signal_id": "DI-1",
                "claim_type": "OBSERVED",
                "verification_state": "SOURCE_VERIFIED",
                "evidence_refs": ["EV-1"],
            }
        ],
        "vendors": [
            {
                "vendor_id": "VI-1",
                "price": 10.0,
                "currency": "MXN",
                "claim_type": "OBSERVED",
                "verification_state": "SOURCE_VERIFIED",
                "evidence_refs": ["EV-1"],
            }
        ],
        "evidence": [
            {
                "evidence_id": "EV-1",
                "claim_type": "OBSERVED",
                "verification_state": "SOURCE_VERIFIED",
            }
        ],
        "opportunities": [
            {
                "opportunity_id": "DE-1",
                "decision_state": "TEST_NEXT",
                "demand_signal_refs": ["DI-1"],
                "vendor_refs": ["VI-1"],
                "evidence_refs": ["EV-1"],
                "next_action": "Request a sample",
            }
        ],
        "conflicts": [],
        "next_actions": [],
    }
    if validate(valid_packet):
        print("SELF_TEST_FAILED: valid fixture was rejected", file=sys.stderr)
        return 1

    invalid_packet = json.loads(json.dumps(valid_packet))
    invalid_packet["opportunities"][0]["vendor_refs"] = ["VI-missing"]
    invalid_packet["vendors"][0]["currency"] = None
    errors = validate(invalid_packet)
    if len(errors) < 2:
        print("SELF_TEST_FAILED: invalid fixture was not rejected", file=sys.stderr)
        return 1

    print("SELF_TEST_PASS")
    return 0


def _load_packet(path: str) -> Any:
    if path == "-":
        return json.load(sys.stdin)
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packet", nargs="?", help="JSON packet path, or - for stdin")
    parser.add_argument(
        "--self-test", action="store_true", help="run built-in valid and invalid fixtures"
    )
    args = parser.parse_args()

    if args.self_test:
        return _self_test()
    if not args.packet:
        parser.error("packet is required unless --self-test is used")

    try:
        packet = _load_packet(args.packet)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"INVALID: unable to read JSON: {exc}", file=sys.stderr)
        return 1

    errors = validate(packet)
    if errors:
        print(f"INVALID: {len(errors)} error(s)", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    counts = {
        key: len(packet[key])
        for key in ("demand_signals", "vendors", "evidence", "opportunities")
    }
    print(f"VALID metanoia-divide/1.0 {json.dumps(counts, sort_keys=True)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
