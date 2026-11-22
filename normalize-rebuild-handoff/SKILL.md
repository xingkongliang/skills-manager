---
name: normalize-rebuild-handoff
description: Reconstruct trustworthy project state from messy or incomplete natural-language requests, current conversation, attachments, and explicitly authorized sources. Classify evidence, validated records, raw logs, provisional claims, scaffold-only material, canon, and UNKNOWN; detect duplicates, contradictions, drift, revocations, missing files, and authority limits; recover objectives, CSF, OKR, KPI, decisions, blockers, and safest next action; route work to correct module; and produce compact Context Management Packet, copy-paste handoff, and daily delta. Use when user asks to rehydrate, normalize, rebuild, resume, consolidate, audit, route, transfer work between chats/LLMs, prepare approval/release handoff, recover lost context, or correct AI misunderstanding/hallucination. For R03/NIDUS_GEM, route among A/B/C/D/E and return to D · QA / MERGE / RELEASE HANDOFF. Never mutate source modules, integrate skills, or declare canon.
---

# Normalize · Rebuild · Handoff

## Result

Translate human, fragmented input into a compact, traceable, execution-ready state without making the user reconstruct the architecture or repeat large chat histories.

Operate read-only. Produce a `DRAFT_FOR_APPROVAL`; never perform the routed module's work.

## Load references

Before execution:

1. Read `references/truth-state-model.md` to classify sources and claims.
2. Read `references/neutral-workflow.md` to run `NORMALIZE → REBUILD → COMMIT`.
3. Read `references/cmp-contract.md` before composing the final response.
4. Read `references/r03-nidus-gem-overlay.md` only when scope is R03/NIDUS_GEM or its A/B/C/D/E modules.

## Authority boundary

- Use only current conversation, accessible attachments, and sources explicitly authorized for this task.
- Treat prior memory, summaries, filenames, and cross-chat references as discovery hints unless source-backed in current accessible scope.
- Distinguish user instruction from factual evidence: instruction grants task authority; it does not verify historical claims.
- Never imply access to another chat, LLM, drive, connector, file, or runtime that was not actually inspected.
- Mark inaccessible, missing, unsupported, or materially conflicting facts `UNKNOWN`.
- Ask at most one question, only when its answer blocks every safe useful output. Otherwise proceed with explicit assumptions and `UNKNOWN`.
- Do not ask user to paste giant turn stacks. Request only smallest missing source boundary when essential.

## Non-mutation rules

- Do not mutate A/B/C/E or any routed owner module.
- Do not merge, install, update, or integrate skills.
- Do not declare, promote, freeze, lock, or release canon.
- Do not convert raw logs into validated records.
- Do not convert file presence or scaffold into a completed artifact.
- Do not resolve contradictions by silently selecting preferred wording.
- Do not delete near-duplicates. Prove exact duplicates before recommending removal.
- Return every result to D · QA / MERGE / RELEASE HANDOFF for `ACCEPT`, `PATCH`, or `REJECT`.

## Workflow

### 1. NORMALIZE

Extract without embellishment:

- actual request and desired outcome;
- project, scope, exclusions, deliverables, and approval gate;
- current state and last verified checkpoint;
- objectives, CSF, OKR, KPI, decisions, blockers, and next movement;
- available sources, unavailable sources, source roles, and authority limits.

Create compact source IDs such as `SRC-001`. Record file path/name, source type, availability, and hash when useful and available. File existence proves existence only.

If CSF, OKR, KPI, decision, or checkpoint is absent, write `UNKNOWN`; do not manufacture management structure.

### 2. REBUILD

Classify assertions—not merely whole files—using the truth-state model.

Reconcile:

- exact duplicates versus similar or derivative artifacts;
- version lineage, supersession, and active candidate;
- contradictions and unresolved collisions;
- terminology, instruction, schema, visual, status, or ownership drift;
- explicit revocations and claims made obsolete;
- missing files, incomplete source boundaries, and unverifiable external references.

Attach source IDs to material facts. Preserve competing claims when authority cannot resolve them. Use `UNKNOWN` for unresolved state.

Choose one primary owner module from configured overlay. List secondary dependencies without assigning mutations. If routing ambiguity blocks safe execution, route resolution to D.

### 3. COMMIT

Interpret `COMMIT` as creation of a commit-ready proposed record—not persistence, merge, canonization, or release.

Produce exactly the eight sections defined in `references/cmp-contract.md`:

1. comprensión reconstruida;
2. estado normalizado;
3. source/evidence boundaries;
4. conflictos y UNKNOWN;
5. módulo dueño;
6. siguiente acción segura;
7. CMP/handoff copy-paste;
8. delta commit-ready para D.

Set packet status `DRAFT_FOR_APPROVAL`. End inside section 8 with pending D decision and allowed responses: `ACCEPT`, `PATCH`, `REJECT`.

## Handoff rules

- Make handoff self-contained but compact.
- Include exact task, owner role, scope, inputs, evidence boundaries, prohibitions, expected deliverable, and return destination.
- Never say “use shared context.” Identify attached source IDs or list exact required attachments.
- Do not reproduce full raw logs. Reference source IDs, relevant turn boundaries, hashes, or filenames.
- Include only delta: confirmed, changed, blocked, revoked, next.
- Preserve `UNKNOWN`; do not hide it to make packet look complete.

## Quality gate

Before returning, verify:

- every material fact has source ID or `UNKNOWN`;
- self-declared `PASS`, `VERIFIED`, `FINAL`, or `CANON` was not accepted without authority evidence;
- scaffold was not counted as completed artifact;
- raw logs remain evidence only;
- exact and semantic duplicates remain distinct;
- routed owner and D destination are both present;
- `mutation_performed: false` and `canon_declared: false`;
- handoff is executable without assumed shared context;
- delta contains only current-run change;
- no more than one blocking question was asked.

When saving a CMP as JSON or YAML, run:

```bash
python3 scripts/validate_cmp.py <packet.json|packet.yaml>
```

Fix every validation error before delivery.
