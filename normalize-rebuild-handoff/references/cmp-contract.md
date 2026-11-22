# Context Management Packet contract

Return exactly eight numbered sections. Use concise prose/tables for sections 1–6. Put portable packet in section 7. Put current-run delta and D decision request in section 8.

## Contents

1. Comprensión reconstruida
2. Estado normalizado
3. Source/evidence boundaries
4. Conflictos y UNKNOWN
5. Módulo dueño
6. Siguiente acción segura
7. CMP/handoff copy-paste
8. Delta commit-ready para D

## 1. Comprensión reconstruida

Need, desired outcome, scope, exclusions, deliverables. Label unsupported interpretation `ASSUMPTION`.

## 2. Estado normalizado

Checkpoint, objective, CSF, OKR, KPI, decisions, blockers. Use `UNKNOWN` when absent.

## 3. Source/evidence boundaries

List authorized/inspected/unavailable sources with IDs. State what each source proves and does not prove.

## 4. Conflictos y UNKNOWN

Exact duplicates, possible duplicates, contradictions, drift, revoked claims, missing artifacts, unresolved facts.

## 5. Módulo dueño

Primary owner, secondary dependencies, routing rationale, `mutation_performed: false`.

## 6. Siguiente acción segura

One atomic action, executor, required inputs, approval gate, completion condition.

## 7. CMP/handoff copy-paste

Use this portable structure:

```yaml
cmp:
  meta:
    packet_id: CMP-YYYYMMDD-...
    generated_at: ISO-8601
    project: neutral|project_id
    mode: NORMALIZE_REBUILD_COMMIT
    status: DRAFT_FOR_APPROVAL
    destination: D · QA / MERGE / RELEASE HANDOFF
  understanding:
    need: "..."
    outcome: "..."
    scope: "..."
    exclusions: []
  state:
    checkpoint: UNKNOWN
    objective: UNKNOWN
    csf: []
    okr: []
    kpi: []
    decisions: []
    blockers: []
  sources:
    inspected: []
    unavailable: []
    boundaries: []
  conflicts_unknown:
    exact_duplicates: []
    possible_duplicates: []
    contradictions: []
    drift: []
    revoked: []
    missing: []
    unknown: []
  routing:
    owner_module: "..."
    dependencies: []
    rationale: "..."
    mutation_performed: false
  safe_next_action:
    action: "..."
    executor: "..."
    required_inputs: []
    approval_required: true
    completion_condition: "..."
  handoff_prompt: |-
    REHYDRATE FROM THIS CMP ONLY.
    Role: ...
    Scope: ...
    Inputs: ...
    Evidence boundaries: ...
    Task: ...
    Do not: invent; assume shared context; promote raw logs/scaffold; declare canon.
    Deliver: ...
    Return to: D · QA / MERGE / RELEASE HANDOFF.
  delta:
    confirmed: []
    changed: []
    blocked: []
    revoked: []
    next: []
  d_acceptance:
    decision_status: PENDING
    allowed_decisions: [ACCEPT, PATCH, REJECT]
    canon_declared: false
```

Every material item uses `source_ids`. Omit repeated prose only when meaning remains unambiguous.

## 8. Delta commit-ready para D

Report only current-run changes:

- `confirmed`;
- `changed`;
- `blocked`;
- `revoked`;
- `next`;
- `commit_ready: true|false` + reason;
- `D decision: PENDING — ACCEPT | PATCH | REJECT`;
- `canon_declared: false`.
