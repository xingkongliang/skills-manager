
# DMIE/MMII adapter

## Runtime identity

- expected master: `DMIE_MMII_INTERACTIVE_WORKBOOK_v0_3_2_COMET_DRAME_PRINTFIX_PAGINATION.html`
- expected bytes: `69849`
- expected sha256: `f84484ac9e6e546c8b44f46f64d506458437d07fe9aaf0baebe45166d17c999f`
- source: `SRC-DMIE-REGISTRY`
- state: `MISSING_NOT_MOUNTED`

`PRINTFIX` is the canonical label. The phrase `PrintFix Pagination` is preserved only as source terminology.

## Adapter input

An approved `line_contract`, tokens, twenty authored entries, persistence namespace, print mode and provenance.

## Adapter output

Interactive workbook candidate, printable view, export delta and QA evidence.

## Hard gates

1. Exact master bytes must be mounted and hash-matched.
2. Patch fragments, lowercase variants and `(1)/(2)` files remain reference-only.
3. No homonymous merge by title.
4. Runtime, browser and print QA are required before promotion.
5. This RC1 does not reconstruct or include the missing master.
