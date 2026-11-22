#!/usr/bin/env python3
"""Static validation for generated Apps Script projects."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

PLACEHOLDER_RE = re.compile(r"__[A-Z0-9_]+__|\bTODO\b")
FUNCTION_RE = re.compile(r"\bfunction\s+([A-Za-z_$][\w$]*)\s*\(")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("project", type=Path)
    args = parser.parse_args()
    required = ["appsscript.json", "Config.gs", "Main.gs", "Core.gs", "Domain.gs"]
    findings = []
    for name in required:
        if not (args.project / name).is_file():
            findings.append({"severity": "CRITICAL", "code": "MISSING_FILE", "file": name})

    manifest_path = args.project / "appsscript.json"
    if manifest_path.is_file():
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            if manifest.get("runtimeVersion") != "V8":
                findings.append({"severity": "HIGH", "code": "RUNTIME_NOT_V8"})
        except json.JSONDecodeError as exc:
            findings.append({"severity": "CRITICAL", "code": "INVALID_MANIFEST", "detail": str(exc)})

    sources = {}
    for path in args.project.glob("*.gs"):
        text = path.read_text(encoding="utf-8")
        sources[path.name] = text
        for match in PLACEHOLDER_RE.finditer(text):
            findings.append({
                "severity": "HIGH",
                "code": "PLACEHOLDER",
                "file": path.name,
                "token": match.group(0),
            })

    combined = "\n".join(sources.values())
    functions = set(FUNCTION_RE.findall(combined))
    for required_function in ("onOpen", "runSystemPreflight", "validateProtectedOutcomes"):
        if required_function not in functions:
            findings.append({
                "severity": "CRITICAL",
                "code": "MISSING_FUNCTION",
                "function": required_function,
            })

    if "LockService.getDocumentLock()" not in combined:
        findings.append({"severity": "HIGH", "code": "NO_DOCUMENT_LOCK"})
    if 'mode === "DRY_RUN"' not in combined:
        findings.append({"severity": "HIGH", "code": "NO_DRY_RUN_GUARD"})
    if 'report.mode === "DRY_RUN"' not in combined:
        findings.append({"severity": "HIGH", "code": "DRY_RUN_MAY_PERSIST_REPORT"})

    main_source = sources.get("Main.gs", "")
    if re.search(r"function\s+onEdit\s*\(", main_source):
        findings.append({
            "severity": "MEDIUM",
            "code": "SIMPLE_ON_EDIT_REQUIRES_REVIEW",
        })

    critical = sum(item["severity"] == "CRITICAL" for item in findings)
    high = sum(item["severity"] == "HIGH" for item in findings)
    result = {
        "status": "BLOCKED" if critical else "REVIEW" if high else "PASS",
        "critical": critical,
        "high": high,
        "findings": findings,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    raise SystemExit(1 if critical else 0)


if __name__ == "__main__":
    main()
