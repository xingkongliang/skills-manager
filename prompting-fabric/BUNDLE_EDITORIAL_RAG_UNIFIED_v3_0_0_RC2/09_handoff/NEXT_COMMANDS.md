# Next commands · RC2

## 1 · Validate RC2

```bash
python tools/validate_bundle.py .
```

## 2 · Deliver missing Literatura entries

Provide authored content for entries [9,10,11,12,13,14,15,16] (S2 Exploración).
Do not generate synthetically. Verify odd/even pattern against C-004 decision.

## 3 · Materialize DMIE/MMII

Provide exactly `DMIE_MMII_INTERACTIVE_WORKBOOK_v0_3_2_COMET_DRAME_PRINTFIX_PAGINATION.html`.
Verify: size `69849`, sha256 `f84484ac9e6e546c8b44f46f64d506458437d07fe9aaf0baebe45166d17c999f`.
Reject any mismatch.

## 4 · Execute Comet runtime QA

Run browser interaction, localStorage save/load/reset, export flow, print layout, and pagination QA.
Document results before promoting C-002 from OPEN.

## 5 · Director decision

Return one structured decision: `ACCEPT`, `PATCH` with exact files/fields, or `REJECT` with conflict IDs.
Do not issue a global "final" or "deploy" command while blockers remain.
