#!/usr/bin/env python3
from __future__ import annotations
import argparse, json, subprocess, sys
from pathlib import Path
from jsonschema import validate

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "source/editorial_protocols/eic_notes/schema/editor_in_chief_notes.schema.json"
TEMPLATE = ROOT / "templates/eic_notes/print_notes_layout.md"
PREAMBLE = ROOT / "templates/eic_notes/preamble.tex"

def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))

def score(data, key):
    return str(data["audit_scoring"].get(key, 0))

def render_md(data: dict, tribe: str, out_md: Path):
    text = TEMPLATE.read_text(encoding="utf-8")
    chief = data["editor_notes"].get("chief_instructions", [])
    chief_md = "\n".join(f"- {item}" for item in chief) or "- Sin instrucciones registradas."
    replacements = {
        "{{TRIBU}}": tribe,
        "{{VERSION}}": data["meta"].get("version", "1.4.0"),
        "{{DATE}}": data["approval_seal"].get("date", ""),
        "{{AEQUITAS_STATUS}}": data["approval_seal"].get("aequitas_status", "TENSION"),
        "{{CERTEZA_LEVEL}}": data["meta"].get("status", "in_review"),
        "{{FIDELITY}}": score(data, "fidelity"),
        "{{COHERENCE}}": score(data, "coherence"),
        "{{NECESSITY}}": score(data, "necessity"),
        "{{PRECISION}}": score(data, "precision"),
        "{{ELEGANCE}}": score(data, "elegance"),
        "{{FIDELITY_NOTES}}": "Revisar contra fuentes primarias.",
        "{{COHERENCE_NOTES}}": "Verificar continuidad de rama.",
        "{{NECESSITY_NOTES}}": "Confirmar necesidad editorial.",
        "{{PRECISION_NOTES}}": "Validar términos técnicos.",
        "{{ELEGANCE_NOTES}}": "Mantener contención visual.",
        "{{STRONG_ID}}": data["editor_notes"]["lexicographical_verification"].get("strong_id", "TBD"),
        "{{BDB_PAGE}}": str(data["editor_notes"]["lexicographical_verification"].get("bdb_page") or "PENDING"),
        "{{TENSION_ID}}": data["editor_notes"]["lexicographical_verification"].get("tension_notes", ""),
        "{{CHIEF_INSTRUCTIONS}}": chief_md,
    }
    for k, v in replacements.items():
        text = text.replace(k, v)
    out_md.parent.mkdir(parents=True, exist_ok=True)
    out_md.write_text(text, encoding="utf-8")


def compile_pdf(md: Path, pdf: Path):
    pdf.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "pandoc", str(md), "-o", str(pdf),
        "--pdf-engine=xelatex",
        "--include-in-header", str(PREAMBLE),
        "--variable", "mainfont=Tinos",
        "--variable", "sansfont=Arimo",
    ]
    return subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("input_json", type=Path)
    ap.add_argument("--tribe", default="LUMINA")
    ap.add_argument("--out-md", type=Path, default=ROOT / "work/eic_notes/EIC_Notes_LUMINA.md")
    ap.add_argument("--out-pdf", type=Path, default=ROOT / "print/eic_notes/EIC_Notes_LUMINA_Print.pdf")
    ap.add_argument("--no-pdf", action="store_true")
    args = ap.parse_args()
    data = load_json(args.input_json)
    schema = load_json(SCHEMA)
    validate(data, schema)
    render_md(data, args.tribe, args.out_md)
    print(f"[BOS] Markdown generated: {args.out_md}")
    if not args.no_pdf:
        result = compile_pdf(args.out_md, args.out_pdf)
        if result.returncode:
            print(result.stderr, file=sys.stderr)
            raise SystemExit(result.returncode)
        print(f"[ÆQUITAS] PDF generated: {args.out_pdf}")

if __name__ == "__main__":
    main()
