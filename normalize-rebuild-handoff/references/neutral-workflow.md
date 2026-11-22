# Neutral workflow

Use this engine independently of any project overlay.

## NORMALIZE

Build working record:

```yaml
request: {need, outcome, deliverables, exclusions}
scope: {project, branch, time_boundary, approval_gate}
management: {objective, csf, okr, kpi}
state: {checkpoint, decisions, blockers, next_movement}
sources: [{id, role, type, location, available, hash, authority_scope}]
```

- Preserve user's natural-language intention; translate jargon without changing meaning.
- Separate requested future action from claimed past state.
- Use `UNKNOWN` for absent fields.
- Record assumptions explicitly; never blend them into facts.

## REBUILD

For each material assertion, record:

```yaml
claim: "..."
state: EVIDENCE_PHYSICAL_VERIFIED|VALIDATED_RECORD|RAW_LOG|PROVISIONAL_CLAIM|SCAFFOLD_ONLY|CANON_REPORTED|UNKNOWN
status: ACTIVE|SUPERSEDED|REVOKED|CONFLICTED|MISSING
source_ids: [SRC-...]
scope: "..."
confidence: high|medium|low|not_applicable
```

Reconcile in order:

1. source availability and boundaries;
2. exact duplicate hashes;
3. derivation/version lineage;
4. explicit decisions and revocations;
5. contradictions and drift;
6. missing dependencies;
7. owner routing;
8. smallest safe next action.

Drift types: `instruction`, `terminology`, `schema`, `version`, `status`, `ownership`, `visual`, `scope`.

Do not average conflicts. Do not award confidence numerically without defined method.

## COMMIT

Generate proposed packet only:

- status: `DRAFT_FOR_APPROVAL`;
- mutation: `false`;
- canon declaration: `false`;
- human-readable 8-section result;
- copy-paste handoff;
- delta limited to current run;
- D acceptance request.

`commit-ready` means sufficiently structured for D review. It does not mean committed, merged, approved, released, or canonical.

## Blocking-question rule

Ask one question only if all safe useful progress depends on it. Bundle smallest decision:

- identify missing source or desired target;
- offer 2–3 mutually exclusive choices when possible;
- state default safe fallback.

If partial work remains possible, proceed and mark dependency `UNKNOWN` or `BLOCKED`.
