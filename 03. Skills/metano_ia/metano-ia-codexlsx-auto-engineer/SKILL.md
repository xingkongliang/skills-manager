---
name: metano-ia-codexlsx-auto-engineer
description: Audit an Excel workbook or Google Sheet as one connected system and generate a personalized, deployable Google Apps Script project. Use whenever a user asks to automate a spreadsheet, create Apps Script for a workbook, integrate multiple tabs, add spreadsheet menus/triggers/QA/gates, migrate Excel workflows to Google Sheets, generate a RAG/JSON export from spreadsheet data, automate spreadsheet-based pricing or campaigns, run ÆQUITAS-style data-quality controls, or determine what should and should not be automated—even if they only say “automate this sheet.”
---

# METANO_IA · CodexLSX Auto Engineer

Treat the workbook as a small information system, not as isolated tabs.

## Core contract

1. Preserve formulas, identifiers, source boundaries, and existing business gates.
2. Separate diagnosis from mutation: `DRY_RUN` inspects; `APPLY` writes only after an explicit user action.
3. Prefer stable header names and IDs over fixed column letters.
4. Make every write idempotent and record it in an audit sheet.
5. Never auto-promote `candidate`, `eligible`, `approved`, `canon`, or equivalent states without verified workbook conditions.
6. Flag Excel-to-Sheets compatibility for formulas, macros, validations, named ranges, external links, pivots, and charts instead of assuming parity.
7. Treat pasted AI conversations and prior generated code as candidate evidence. Verify logs, sheet structure, and business rules before adopting their claims.

## Workflow

### 1. Rehydrate the workbook

- Resolve the exact workbook and preserve its hash/version.
- Use the Spreadsheets workflow to inspect every sheet, used range, formulas, validation, formatting signals, tables, charts, and defined names.
- Run `scripts/profile_xlsx.py INPUT.xlsx --output profile.json` when local `.xlsx` bytes are available.
- Detect title rows and the most likely header row; verify ambiguous headers manually.
- Build an inter-sheet dependency graph from formulas, IDs, lookups, and business flow.

### 2. Classify sheets and rules

Assign roles: `CONTROL`, `DASHBOARD`, `MASTER_DATA`, `TRANSACTION`, `BRIDGE`, `QA`, `AUDIT`, `REFERENCE`, `SOURCE_EVIDENCE`, or `HANDOFF`.

Classify candidate automations:

- `SAFE`: validation, duplicate checks, referential-integrity checks, menu actions, audit logs, and dry-run reports.
- `CONDITIONAL`: row creation, bridge synchronization, formula propagation, or status updates requiring confirmation.
- `BLOCKED`: irreversible deletion, regulatory approval, canonization, sale approval, or writes without sufficient evidence.

Apply Data Analytics checks for completeness, uniqueness, validity, consistency, integrity, timeliness, and grain.

Never promise “100% automation.” State the automated scope and the human or regulatory decisions that remain.

### 3. Write the blueprint

Create `automation-blueprint.json` using `references/blueprint-schema.md`. Include workbook identity, sheets, headers, IDs, references, formula/input ownership, controlled vocabularies, protected outcomes, triggers, tests, and rollback.

Read `references/apps-script-patterns.md` before generating code.

### 4. Generate and personalize

```bash
python3 scripts/render_apps_script.py \
  automation-blueprint.json \
  --output apps-script-project
```

Then personalize `Domain.gs`:

- implement workbook-specific cross-sheet checks;
- resolve columns by header text;
- keep simple `onEdit(e)` work under 30 seconds and limited to relevant cells;
- reserve multi-sheet or authorized operations for menus or installable triggers;
- use `LockService` around writes;
- batch `getValues()` and `setValues()`;
- never store credentials in code or cells.

For a beginner-friendly one-file handoff, run:

```bash
python3 scripts/bundle_apps_script.py apps-script-project
```

### 5. Validate

Run `scripts/validate_apps_script.py apps-script-project`.

Verify:

- configured sheets and required headers exist;
- referenced functions exist and no placeholder tokens remain;
- `DRY_RUN` contains no setters;
- no unsafe automatic approval path exists;
- repeated `APPLY` does not duplicate records;
- audit logging and locks are present;
- `appsscript.json` is valid.

When possible, test against a disposable imported copy, never the only live workbook.

### 6. Deliver

Return the project folder, blueprint, concise deployment note, and one status: `READY_FOR_IMPORT`, `READY_FOR_DRY_RUN`, `BLOCKED`, or `DEPLOYED_VERIFIED`.

Do not claim deployment merely because code was generated.

## Case reference

For R06 × R07 · DE MANO, read `references/case-01-r06-r07-de-mano.md` and preserve `PROPOSAL_ELIGIBLE ≠ SALE_APPROVED`.

Future workbook-specific modules may include RAG/JSON export, campaign or Flash Sale calculations, and ÆQUITAS data-quality gates. Generate each only after profiling its own workbook; do not inherit column letters or rules from another case.
