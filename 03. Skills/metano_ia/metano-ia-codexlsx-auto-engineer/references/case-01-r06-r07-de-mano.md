# Case 01 · R06 × R07 · DE MANO

## Evidence

- Workbook: `R06_R07_DE_MANO_Pilot_Control_v0_2_UNIFICADO.xlsx`
- SHA-256: `5e6e3504f0417752e6b9192d005ad68cc37019d54b81048de9dda9a5c14cbf3a`
- Sheets inspected: 29
- State: `CANDIDATE_UNIFIED · NOT_FINAL_CANON`
- Current sale approvals: `0`
- Supplemental raw conversation export: `Pasted text.txt`
- Supplemental SHA-256: `2e11c7e44c43c18ab6df832a39378a7109c2553d244bc2c28fea9d8840d2556b`
- Supplemental status: `RAW_LOG_CANDIDATE · NOT_CANON`

## System flow

`BD / BACKLOG → BRIDGE_PIR_BD → PIR_PRODUCTO → RPR_CENTRAL → LAY_LAYERING → META_KIT_LAYERING / META_PROPUESTA_PORTATIL → PILOTO_3x3x3 → 01_PANEL`

Supporting controls: `META_ENVASE_COSTO`, `META_QA_ENVASE`, `META_LOTE_REENVASADO`, `META_TRAZABILIDAD_REENVASE`, `LISTAS`, `SRC_REGISTRY`, `BD_CONTROL`, and `03_CMP_HANDOFF`.

## Protected rules

1. `PROPOSAL_ELIGIBLE ≠ SALE_APPROVED`.
2. Never set `Venta_Aprobada = SI` unless compatibility is `PASS`, regulation is `APROBADO`, QA is `PASS`, and price is present.
3. Preserve `107 María María concepts / 106 catalog references` as `WARN`; do not manufacture the missing record.
4. Historical 2025 price or availability is not current 2026 evidence.
5. `BRIDGE_PIR_BD` selection links evidence to the pilot; it does not approve RPR or sale.
6. Formula columns remain formula-owned.

## Supplemental conversation decisions

Accept as user intent:

- skill identity: `metano-ia_codexlsx-auto-engineer` (normalized skill name: `metano-ia-codexlsx-auto-engineer`);
- recognize yellow inputs, blue formulas, IDs, and multi-tab gates;
- keep future modules for RAG/JSON export, Flash Sale automation, and ÆQUITAS data-quality checks.

Reject unless independently verified:

- “100% of the workbook is automatable”;
- `ReferenceError: Drive is not defined` as an observed incident;
- deleting existing Apps Script code as a default installation step;
- hardcoded column letters or numeric indexes;
- automatic `SALE_APPROVED` promotion from only cost and QA;
- treating `03_CMP_HANDOFF` as a list already ready for customers.

## Recommended automation

Safe: structural preflight, duplicate and orphan checks, controlled values, gate-bypass detection, formula-presence checks, decision summary, audit log, and menu.

Conditional: propagate a selected bridge candidate, create a PIR/RPR row from a reviewed template, or repair a forbidden approval value. Require a dry-run delta and explicit confirmation.

Blocked: sale approval, regulatory approval, canon promotion, mass-production authorization, and deletion/deduplication of source evidence.

## Pilot success criteria

- dry-run makes zero data writes;
- all 29 required sheets are recognized;
- primary IDs are unique;
- references resolve or are reported;
- sale approvals remain zero unless every gate is verified;
- repeated validation returns the same findings;
- audit entries do not duplicate for a retried run ID.
