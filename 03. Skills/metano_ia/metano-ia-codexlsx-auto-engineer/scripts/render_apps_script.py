#!/usr/bin/env python3
"""Render a Google Apps Script project from an automation blueprint."""

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path


def validate_blueprint(payload: dict) -> None:
    for key in ("schemaVersion", "project", "workbook", "runtime", "sheets", "rules"):
        if key not in payload:
            raise ValueError(f"Missing blueprint key: {key}")
    if payload["runtime"].get("defaultMode") != "DRY_RUN":
        raise ValueError("runtime.defaultMode must be DRY_RUN")
    names = [sheet["name"] for sheet in payload["sheets"]]
    if len(names) != len(set(names)):
        raise ValueError("Duplicate sheet names in blueprint")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("blueprint", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    payload = json.loads(args.blueprint.read_text(encoding="utf-8"))
    validate_blueprint(payload)

    base = Path(__file__).resolve().parent.parent / "assets" / "apps-script-base"
    args.output.mkdir(parents=True, exist_ok=True)
    for source in base.iterdir():
        if source.is_file():
            shutil.copy2(source, args.output / source.name)

    manifest = args.output / "appsscript.json"
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            "__TIMEZONE__", payload["project"].get("timezone", "Etc/UTC")
        ),
        encoding="utf-8",
    )
    config = {
        "schemaVersion": payload["schemaVersion"],
        "projectName": payload["project"]["name"],
        "projectVersion": payload["project"]["version"],
        "menuName": payload["project"].get("menuName", "Automatización"),
        "defaultMode": "DRY_RUN",
        "auditSheet": payload["runtime"].get("auditSheet", "_AUTOMATION_LOG"),
        "sheets": payload["sheets"],
        "rules": payload["rules"],
    }
    (args.output / "Config.gs").write_text(
        "const AUTOMATION_CONFIG = Object.freeze("
        + json.dumps(config, ensure_ascii=False, indent=2)
        + ");\n",
        encoding="utf-8",
    )
    (args.output / "automation-blueprint.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
