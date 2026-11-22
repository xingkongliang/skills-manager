# EIC Notes Protocol Rehydration · v1.4

## Source
`Protocolo y Maquetación de Notas de Dirección Editorial.docx`

## Operational interpretation
The protocol adds an Editor-in-Chief Notes module to the LUMINA unified package.

## Rehydrated components
- JSON Schema: `source/editorial_protocols/eic_notes/schema/editor_in_chief_notes.schema.json`
- Example record: `source/editorial_protocols/eic_notes/editor_in_chief_notes.example.json`
- Markdown template: `templates/eic_notes/print_notes_layout.md`
- LaTeX preamble: `templates/eic_notes/preamble.tex`
- Generator: `scripts/eic_notes/generate_eic_notes.py`
- Compile helper: `scripts/eic_notes/compile_eic_notes.sh`
- Sample output: `work/eic_notes/EIC_Notes_LUMINA.md`
- Sample PDF: `print/eic_notes/EIC_Notes_LUMINA_Print.pdf`

## Key tension resolved
The source document declares Half Letter / Media Carta output at 5.5 x 8.5 in, but its bash snippet used `papersize=letter`.
This package resolves the tension by keeping the physical output as Half Letter using geometry:

```yaml
paperwidth: 5.5in
paperheight: 8.5in
margin: 0.65in
```

## BOS / ÆQUITAS / LUMINA / OPAL mapping
- BOS: orchestrates generation and batch execution.
- ÆQUITAS: carries audit status, scoring, certainty, and tension notes.
- LUMINA: preserves editorial-mineral visual direction.
- OPAL: enforces print/export discipline.
- IRIDISCENTER: preserves ecosystem signature and editorial continuity.

## Status
Integrated as `v1.4_EIC_NOTES_PROTOCOL` and left as commit candidate pending local Mac execution review.
