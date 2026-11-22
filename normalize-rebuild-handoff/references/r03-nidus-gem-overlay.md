# R03 · NIDUS_GEM overlay

Apply only when project scope is `R03/NIDUS_GEM`.

## Routing map

| Module | Owns | Does not own |
|---|---|---|
| `A · Visual Production` | Generate/review visual candidates, art-direction assets, contact sheets, visual classifications. | Prompt-engine architecture, gallery infrastructure, canon/release. |
| `B · Production Engine` | Prompt packs, batch kernels, rulesets, corrective patches/variants, production snapshots, engine handoffs. | Final visual curation, gallery promotion, canon/release. |
| `C · Visual Gallery` | Curate, classify, archive, display, lightbox/collection infrastructure, gallery-ready compilations. | Generate production assets, rewrite engine, canon/release. |
| `D · QA / Merge / Release` | Verify claims, reconcile deltas, accept/patch/reject, merge/release decisions, explicit canon authority. | Unreviewed production mutation. |
| `E · MOTUS Skill Governance` | Skill architecture, triggers, instructions, evaluations, version governance. | A/B/C production work or canon declaration. |

## Routing rules

1. Select primary owner by requested deliverable/mutation, not vocabulary alone.
2. List secondary modules as dependencies only.
3. Route mixed work to module owning next atomic deliverable.
4. Route unresolved ownership, conflicting release claims, or canon questions to D.
5. Return every packet to `D · QA / MERGE / RELEASE HANDOFF`, regardless of primary owner.
6. Set `mutation_performed: false`; this skill routes but does not execute A/B/C/E work.
7. For E-bound work, propose skill change only. Do not install, update, merge, or integrate skills.

## R03 invariants

- Generated image text is never canon.
- Visual candidates require explicit promotion; production output does not enter C automatically.
- `ERROR_USEFUL` preserves drift evidence; it is neither preferred asset nor canon.
- A scaffold, shell, registry placeholder, or empty collection is not a completed artifact.
- Module-local success does not equal D acceptance.
- D acceptance must identify artifact/delta, version/scope, evidence, and decision.
