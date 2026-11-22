# Truth and authority model

Classify at assertion level. One file may contain verified evidence, provisional claims, scaffold, and UNKNOWN simultaneously.

## Source roles

| Role | Meaning |
|---|---|
| `INSTRUCTION` | User-authorized task, scope, constraint, or decision request. |
| `EVIDENCE` | Directly inspected material capable of supporting a fact. |
| `RECORD` | Structured statement of prior validation or decision. |
| `RAW_LOG` | Unnormalized event/conversation record; evidence only. |
| `CONTEXT_HINT` | Memory, summary, filename, or pointer used only to locate evidence. |

## Assertion states

| State | Minimum basis | Constraint |
|---|---|---|
| `EVIDENCE_PHYSICAL_VERIFIED` | Direct observation of accessible physical/digital artifact; identify source. | Proves observed property only. File presence does not prove contents true. |
| `VALIDATED_RECORD` | Traceable validation record, validator/authority, scope, result, date/version. | Validation is limited to recorded scope; not automatic canon. |
| `RAW_LOG` | Unmodified or source-faithful log with identifiable boundary. | Evidence input; never canon by itself. |
| `PROVISIONAL_CLAIM` | Unsupported, derived, AI-generated, or pending assertion. | Keep candidate/draft label. |
| `SCAFFOLD_ONLY` | Folder, placeholder, schema, shell, template, or empty/minimal artifact. | Never report as implemented or complete. |
| `CANON_REPORTED` | Explicit D-authorized canon decision artifact with scope/version/date and traceable source. | Skill may report existing canon; it cannot declare new canon. |
| `UNKNOWN` | Missing, inaccessible, unsupported, or materially unresolved. | Preserve until evidence resolves it. |

## Status flags

- `ACTIVE`: currently applicable within verified scope.
- `SUPERSEDED`: replaced by traceable later version.
- `REVOKED`: explicitly withdrawn; cite revocation.
- `CONFLICTED`: incompatible supported assertions remain unresolved.
- `MISSING`: required artifact absent from inspected boundary.

## Decision rules

1. Treat labels such as `PASS`, `VERIFIED`, `FINAL`, `COMPLETE`, `CANON`, or `100%` inside an artifact as claims until validation evidence is present.
2. Prefer explicit applicable D decision over lower-authority summaries. Never infer D approval from file naming, directory placement, or polished formatting.
3. Prefer direct source observation over derived summary for observed facts.
4. Keep later timestamp separate from higher authority. Newer does not automatically mean valid or canonical.
5. Compute SHA-256 when proving exact file duplication. Same name, size, or topic is insufficient.
6. Label semantic/near duplicates `POSSIBLE_DUPLICATE`; preserve both until reviewed.
7. Record contradictions as separate claims with source IDs. Resolve only with traceable authority; otherwise `CONFLICTED` + `UNKNOWN`.
8. Mark revoked claim and revocation source; do not erase history.
9. Treat inaccessible external links, other chats, and unattached files as `UNKNOWN`, not absent facts.
10. Never promote a claim because multiple derived files repeat it; shared ancestry is not independent corroboration.
