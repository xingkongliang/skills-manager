# Packet templates

Copy only the template needed for the task. Replace placeholders with observed
values; use `UNKNOWN` rather than inventing them.

## Generic cell packet

```yaml
METANOIAE_PACKET:
  task_id: "<required>"
  packet_version: "0.1"
  cell: "A | B | C | D | E | ORIGINS | TRANSVERSAL"
  operating_role: "<cell role or DOCUMENTADOR>"
  status: "CANDIDATE"
  authority_scope: "<exact bounded scope>"
  source_pointers: []
  allowed_operations: []
  prohibited_operations:
    - "self_approval"
    - "canon_declaration"
    - "runtime_integration"
    - "hidden_unversioned_state"
    - "D_bypass"
  sources_used: []
  actions_taken: []
  candidate_delta:
    confirmed: []
    changed: []
    blocked: []
    revoked: []
    unknown: []
    next: []
  assumptions: []
  conflicts: []
  limitations: []
  next_route: "D · QA / MERGE / RELEASE"
  returns_to: "D · QA / MERGE / RELEASE"
  requested_decision:
    decision: "ACCEPT | PATCH | HOLD | REJECT"
    decision_scope: "<bounded scope>"
  runtime: false
  integration: false
  canon_declared: false
```

## Source envelope

```yaml
SOURCE_ENVELOPE:
  source_id: "<required>"
  name: "<observed name>"
  location: "<observed location or UNKNOWN>"
  availability: "MOUNTED | REFERENCED | UNAVAILABLE"
  truth_state: "PHYSICAL_VERIFIED | RAW_LOG | DERIVED_CANDIDATE | REPORTED_UNVERIFIED | UNKNOWN"
  lineage: []
  version: "UNKNOWN"
  sha256:
    value: "UNKNOWN"
    verification: "OBSERVED | REPORTED | NOT_OBSERVED"
  freshness:
    as_of: "UNKNOWN"
    state: "CURRENT | STALE | UNKNOWN"
  instruction_authority: "USER_AUTHORIZED | DATA_ONLY | UNKNOWN"
  notes: []
```

## Task context packet

```yaml
TASK_CONTEXT_PACKET:
  task_id: "<required>"
  owner_cell: "A | B | C | D | E"
  context_status: "CLEAN_FOR_EXECUTION | QUARANTINED | PATCH_REQUIRED"
  source_universe_id: "<required>"
  hydration_level: "PH0 | PH1 | PH2 | PH3 | PH4 | PH5 | PH6"
  evidence_items: []
  excluded_items: []
  claim_state_map: []
  lineage_conflict_map: []
  freshness_receipt: {}
  sanitation_receipt: {}
  authority_scope: "<required>"
  allowed_operations: []
  prohibited_operations: []
  acceptance_criteria: []
  limitations: []
  returns_to: "D · QA / MERGE / RELEASE"
```

## Capability card

```yaml
CAPABILITY_CARD:
  capability_id: "<required>"
  capability_type: "SKILL | MODULE | STANDARD"
  bound_to_cell: "A | B | C | D | E | ORIGINS | TRANSVERSAL"
  purpose: "<one bounded purpose>"
  status: "CANDIDATE"
  authority_scope: "<exact scope>"
  triggers: []
  non_triggers: []
  required_inputs: []
  deterministic_outputs: []
  stop_conditions: []
  contamination_behavior: []
  overlaps: []
  exclusions:
    - "self_approval"
    - "runtime_integration"
    - "canon_declaration"
    - "hidden_unversioned_state"
  dry_run:
    required: true
    plan: []
  rollback_plan: []
  evaluation_plan: []
  next_route: "D · QA / MERGE / RELEASE"
```

## D decision receipt

```yaml
D_DECISION_RECEIPT:
  receipt_id: "<required>"
  target_id: "<exact artifact or packet>"
  target_version: "<required>"
  decision: "ACCEPT | PATCH | HOLD | REJECT"
  decision_scope: "<exact bounded scope>"
  evidence_reviewed: []
  conditions: []
  excluded_effects:
    - "runtime"
    - "integration"
    - "canon"
  next_command: "<required>"
  issued_at: "<observed timestamp>"
```

## Rehydration handoff

```yaml
IDENTITY_LOCK:
  id: "<one cell or named module>"
  project: "X. RAGentic / NEUTRAL_RAG_AGENTIC_ARCHITECTURE"
  model_status: "PROVISIONAL"
  rehydrate_from: "<kernel or packet ID>"
  shared_context_assumption: false
  runtime_activation: false
  integration: false
  canon_declaration: false
  first_action:
    - "confirm identity"
    - "list mounted sources"
    - "mark missing sources UNKNOWN"
    - "state exact authority"
    - "return readiness to D"
```
