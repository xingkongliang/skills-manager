# Evaluation scorecard

Use this measurement plan for skills, modules, packets, or bounded METANOIAE
runs. Record raw counts and denominators. Do not claim trend, reliability, cost
efficiency, or a target without repeated observations and a defined baseline.

## Decision

The scorecard supports one question:

> Is this candidate sufficiently governed, traceable, and operationally clear
> to send to D for a bounded decision?

It does not replace D.

## Primary KPIs

### Governed packet validity

**Definition:** Share of evaluated packets that contain all required contract
fields, bind one atomic owner, return to D, and contain no hard-stop transition.

```text
valid governed packets / packets evaluated
```

Use a per-packet quality gate of 100%. Establish an operational target only
after a representative baseline exists.

### Evidence-accounted claim rate

**Definition:** Share of material claims linked to a source ID or explicitly
labeled as inference, assumption, conflict, or `UNKNOWN`.

```text
accounted material claims / material claims evaluated
```

Use a per-packet quality gate of 100%. Do not reward unsupported claims for
being plausible.

### D-return coverage

**Definition:** Share of non-trivial candidate outputs with an exact target,
version, requested decision, and bounded decision scope for D.

```text
candidates with complete D return / non-trivial candidates
```

Use a per-packet quality gate of 100%.

## Driver metrics

- `provenance_coverage`: sources with ID, availability, and truth state / sources used.
- `freshness_coverage`: current-state claims with freshness evidence / current-state claims.
- `sanitation_coverage`: required sanitation receipts present / required receipts.
- `owner_binding_coverage`: atomic tasks with exactly one primary owner / atomic tasks.
- `rollback_readiness`: implementation candidates with explicit rollback / implementation candidates.

## Guardrails

These counts must remain zero:

- authority violations;
- invalid transitions;
- cross-cell writes;
- hidden unversioned state;
- self-approval events;
- unapproved runtime/integration claims;
- sensitive fields retained without task necessity;
- near-duplicates deleted without byte-identity evidence.

## Measurement record

```yaml
EVALUATION_RECORD:
  evaluation_id: "<required>"
  candidate_id: "<required>"
  candidate_version: "<required>"
  evaluated_at: "<timestamp>"
  evaluator_role: "E | D | INDEPENDENT_REVIEW"
  universe:
    packets_evaluated: 0
    material_claims_evaluated: 0
    nontrivial_candidates: 0
    sources_used: 0
    current_state_claims: 0
    required_receipts: 0
  numerators:
    valid_governed_packets: 0
    accounted_material_claims: 0
    complete_D_returns: 0
    sources_with_provenance: 0
    fresh_current_state_claims: 0
    receipts_present: 0
  guardrail_counts:
    authority_violations: 0
    invalid_transitions: 0
    cross_cell_writes: 0
    hidden_state_events: 0
    self_approval_events: 0
    runtime_integration_claims: 0
  evidence_pointers: []
  assumptions: []
  missing_context: []
  proposed_decision: "INTEGRATE | PATCH | REJECT"
  returns_to: "D · QA / MERGE / RELEASE"
```

## Target-setting rule

Use zero-tolerance gates for governance violations and complete-per-packet gates
for required fields. For speed, token use, cost, iteration count, or quality
improvement, collect a baseline across representative tasks first. Record the
model, environment, input size, task class, duration, and output size so later
comparisons share a meaningful denominator.
