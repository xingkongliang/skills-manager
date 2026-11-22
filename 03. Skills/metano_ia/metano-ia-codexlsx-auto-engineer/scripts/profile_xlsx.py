#!/usr/bin/env python3
"""Create a compact structural profile of an XLSX workbook using stdlib only."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

MAIN_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
NS = {"m": MAIN_NS, "r": REL_NS, "p": PKG_REL_NS}
CELL_REF_RE = re.compile(r"([A-Z]+)(\d+)")
SHEET_REF_RE = re.compile(r"(?:'([^']+)'|([A-Za-z0-9_]+))!")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def column_index(cell_ref: str) -> int:
    match = CELL_REF_RE.match(cell_ref)
    if not match:
        return 0
    value = 0
    for char in match.group(1):
        value = value * 26 + ord(char) - 64
    return value - 1


def shared_strings(archive: zipfile.ZipFile) -> list[str]:
    if "xl/sharedStrings.xml" not in archive.namelist():
        return []
    root = ET.fromstring(archive.read("xl/sharedStrings.xml"))
    values = []
    for item in root.findall("m:si", NS):
        values.append("".join(node.text or "" for node in item.iter(f"{{{MAIN_NS}}}t")))
    return values


def sheet_paths(archive: zipfile.ZipFile) -> list[tuple[str, str]]:
    workbook = ET.fromstring(archive.read("xl/workbook.xml"))
    rels = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
    targets = {
        rel.attrib["Id"]: rel.attrib["Target"]
        for rel in rels.findall("p:Relationship", NS)
    }
    result = []
    for sheet in workbook.find("m:sheets", NS):
        rel_id = sheet.attrib[f"{{{REL_NS}}}id"]
        target = targets[rel_id].lstrip("/")
        if not target.startswith("xl/"):
            target = f"xl/{target}"
        result.append((sheet.attrib["name"], target))
    return result


def cell_value(cell: ET.Element, strings: list[str]) -> object:
    cell_type = cell.attrib.get("t")
    value = cell.find("m:v", NS)
    if cell_type == "inlineStr":
        node = cell.find("m:is", NS)
        return "" if node is None else "".join(
            child.text or "" for child in node.iter(f"{{{MAIN_NS}}}t")
        )
    if value is None or value.text is None:
        return ""
    if cell_type == "s":
        index = int(value.text)
        return strings[index] if 0 <= index < len(strings) else ""
    if cell_type in {"str", "e"}:
        return value.text
    try:
        number = float(value.text)
        return int(number) if number.is_integer() else number
    except ValueError:
        return value.text


def likely_header(rows: list[list[object]]) -> tuple[int | None, list[str]]:
    best_index = None
    best_score = -1.0
    best_headers: list[str] = []
    for index, row in enumerate(rows[:12], start=1):
        headers = [str(value).strip() if value is not None else "" for value in row]
        filled = [value for value in headers if value]
        if len(filled) < 2:
            continue
        unique_ratio = len(set(filled)) / len(filled)
        text_ratio = sum(not value.replace(".", "", 1).isdigit() for value in filled) / len(filled)
        score = len(filled) + unique_ratio + text_ratio
        if score > best_score:
            best_index, best_score, best_headers = index, score, headers
    return best_index, best_headers


def profile_sheet(
    archive: zipfile.ZipFile,
    name: str,
    path: str,
    strings: list[str],
) -> dict[str, object]:
    root = ET.fromstring(archive.read(path))
    dimension = root.find("m:dimension", NS)
    row_maps: dict[int, dict[int, object]] = {}
    formulas = []
    max_column = 0
    sheet_data = root.find("m:sheetData", NS)
    if sheet_data is not None:
        for row in sheet_data.findall("m:row", NS):
            physical_row = int(row.attrib.get("r", "0") or "0")
            row_values: dict[int, object] = {}
            for cell in row.findall("m:c", NS):
                ref = cell.attrib.get("r", "A1")
                col = column_index(ref)
                max_column = max(max_column, col + 1)
                row_values[col] = cell_value(cell, strings)
                formula = cell.find("m:f", NS)
                if formula is not None and formula.text:
                    refs = sorted({
                        match.group(1) or match.group(2)
                        for match in SHEET_REF_RE.finditer(formula.text)
                    })
                    formulas.append({"cell": ref, "formula": formula.text, "sheetRefs": refs})
            if 1 <= physical_row <= 12:
                row_maps[physical_row] = row_values
    width = max(
        max_column,
        max((max(values.keys(), default=-1) + 1 for values in row_maps.values()), default=0),
    )
    rows = [
        [row_maps.get(physical_row, {}).get(index, "") for index in range(width)]
        for physical_row in range(1, 13)
    ]
    header_row, headers = likely_header(rows)
    validations = root.find("m:dataValidations", NS)
    merged = root.find("m:mergeCells", NS)
    return {
        "name": name,
        "path": path,
        "dimension": dimension.attrib.get("ref") if dimension is not None else None,
        "sampleRows": rows,
        "likelyHeaderRow": header_row,
        "likelyHeaders": headers,
        "formulaCount": len(formulas),
        "formulas": formulas,
        "dataValidationCount": int(validations.attrib.get("count", 0)) if validations is not None else 0,
        "mergedCellCount": int(merged.attrib.get("count", 0)) if merged is not None else 0,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with zipfile.ZipFile(args.input) as archive:
        strings = shared_strings(archive)
        sheets = [
            profile_sheet(archive, name, path, strings)
            for name, path in sheet_paths(archive)
        ]
    payload = {
        "schemaVersion": "1.0",
        "workbook": {
            "filename": args.input.name,
            "bytes": args.input.stat().st_size,
            "sha256": sha256(args.input),
            "sheetCount": len(sheets),
        },
        "sheets": sheets,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
