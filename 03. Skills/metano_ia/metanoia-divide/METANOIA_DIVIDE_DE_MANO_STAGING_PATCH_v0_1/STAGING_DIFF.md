# D · Staging diff · IRIDISCENTER hierarchy patch

Decision: `D-IRIDISCENTER-HIERARCHY-PATCH-2026-07-28-01`  
Result: `PATCH APPLIED · READY FOR D REVIEW`  
Canon: `false`  
Production writes: `0`

## Outcome

The architecture overlay was applied to staging metadata without changing the
DIVIDE ingestion records or the source spreadsheet. The original ingestion ZIP
is preserved byte-for-byte inside this handoff.

## Claim delta

| Claim | Before D patch | Patched staging |
|---|---|---|
| System root | Conflicted | `BOS` |
| Prometheus / METANO_IA | Incorrectly promoted above BOS | Transversal agents hosted by `AEQUITAS` |
| IRIDISCENTER | Downstream ecosystem | Ecosystem with seven sibling branches |
| R06 / R07 | Misread as parent container | Sibling branches: `STORE` / `PERFUMERY` |
| DE MANO | Misread as child subsystem | `INTERBRANCH_MINI_BRIDGE` connecting R06 and R07 |
| DIVIDE capability | Operational but misplaced | Active capability of `METANO_IA` |
| GEMA | Incorrectly placed above BOS | Existence preserved; exact placement `UNKNOWN` |
| VI. M& | Incorrectly promoted to macro authority | Exact placement `UNKNOWN`; authority in this patch `NONE` |
| Physical ZIP claim | External claim / pending | Physically revalidated in the current runtime |

## Physical validation resolved

- ZIP present and readable.
- SHA-256:
  `cf6cfc503f8ca548a1c801ed9d2a4d1b0041bb02fe1d0513e15f0511c7b99310`.
- Compressed-data integrity test: `PASS`.
- Extracted archive matches the mounted source directory: `PASS`.
- DIVIDE deterministic validator: `PASS`.
- Candidate target rows: `20` (`2 DI + 4 VI + 12 EV + 1 DE + 1 RUN`).

The previous unqualified `PHYSICALLY_VERIFIED` claim is not reused. This handoff
records the narrower, evidenced state
`PHYSICALLY_REVALIDATED_IN_CURRENT_RUNTIME`.

## Payload preservation

No changes were made to:

- `divide_ingestion_v1.json`;
- the five `MI_*` CSV tables;
- candidate records or raw evidence;
- the Google Sheet;
- market or supplier research;
- commercial gates.

`PROPOSAL_ELIGIBLE ≠ SALE_APPROVED` remains in force.

## Scope boundary

The “no reinvestigation” instruction applies only to this architecture metadata
patch. Future price, stock, supplier, QA, regulatory, compatibility and
commercial validation remain required when the workflow reaches those gates.

## Unresolved

- `GEMA_EXACT_PLACEMENT`
- `VI_M_AND_EXACT_PLACEMENT`
- `R01_R05_DEFINITIONS`

These unknowns do not block DIVIDE/DE MANO staging.

## D return

Requested decision: `ACCEPT PATCHED STAGING` or issue a new targeted patch.

