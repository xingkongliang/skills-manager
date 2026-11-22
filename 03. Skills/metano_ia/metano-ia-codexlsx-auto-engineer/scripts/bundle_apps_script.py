#!/usr/bin/env python3
"""Bundle a multi-file Apps Script project into one copy-paste Code.gs."""

from __future__ import annotations

import argparse
from pathlib import Path

ORDER = ("Config.gs", "Core.gs", "Domain.gs", "Main.gs")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("project", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = args.output or args.project / "Code.gs"
    sections = []
    for name in ORDER:
        path = args.project / name
        if not path.is_file():
            raise FileNotFoundError(path)
        sections.append(f"// ===== {name} =====\n{path.read_text(encoding='utf-8').rstrip()}\n")
    output.write_text("\n".join(sections), encoding="utf-8")


if __name__ == "__main__":
    main()
