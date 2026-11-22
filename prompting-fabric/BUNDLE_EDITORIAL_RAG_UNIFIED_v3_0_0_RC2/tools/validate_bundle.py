#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

try:
    import yaml
except ImportError as exc:
    raise SystemExit("Install requirements-qa.txt: " + str(exc))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_schema(instance, schema, path="$"):
    type_map = {
        "object": lambda value: isinstance(value, dict),
        "array": lambda value: isinstance(value, list),
        "string": lambda value: isinstance(value, str),
        "integer": lambda value: isinstance(value, int) and not isinstance(value, bool),
        "boolean": lambda value: isinstance(value, bool),
        "null": lambda value: value is None,
    }
    expected = schema.get("type")
    if expected:
        choices = expected if isinstance(expected, list) else [expected]
        if not any(type_map[item](instance) for item in choices):
            raise ValueError(f"{path}: invalid type")
    if "enum" in schema and instance not in schema["enum"]:
        raise ValueError(f"{path}: value not in enum")
    if isinstance(instance, str) and schema.get("pattern") and not re.search(schema["pattern"], instance):
        raise ValueError(f"{path}: pattern mismatch")
    if isinstance(instance, dict):
        for key in schema.get("required", []):
            if key not in instance:
                raise ValueError(f"{path}: missing {key}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            extra = set(instance) - set(properties)
            if extra:
                raise ValueError(f"{path}: unexpected {sorted(extra)}")
        for key, value in instance.items():
            if key in properties:
                validate_schema(value, properties[key], f"{path}.{key}")
    if isinstance(instance, list) and schema.get("items"):
        for index, value in enumerate(instance):
            validate_schema(value, schema["items"], f"{path}[{index}]")


root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
errors = []

for path in root.rglob("*.json"):
    try:
        json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        errors.append(f"JSON {path.relative_to(root)}: {exc}")

for path in list(root.rglob("*.yaml")) + list(root.rglob("*.yml")):
    try:
        yaml.safe_load(path.read_text(encoding="utf-8"))
    except Exception as exc:
        errors.append(f"YAML {path.relative_to(root)}: {exc}")

try:
    schema = json.loads((root / "05_lines/LINE_CONTRACT.schema.json").read_text(encoding="utf-8"))
    data = json.loads((root / "05_lines/LINE_REGISTRY.json").read_text(encoding="utf-8"))
    validate_schema(data, schema)
except Exception as exc:
    errors.append(f"LINE_SCHEMA: {exc}")

checksum_path = root / "10_release/SHA256SUMS.txt"
if checksum_path.exists():
    for line in checksum_path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        expected, rel = line.split("  ", 1)
        path = root / rel
        if not path.is_file():
            errors.append(f"MISSING {rel}")
        elif sha256(path) != expected:
            errors.append(f"HASH {rel}")

print(json.dumps({
    "package": root.name,
    "status": "PASS" if not errors else "FAIL",
    "errors": errors,
}, ensure_ascii=False, indent=2))
raise SystemExit(1 if errors else 0)
