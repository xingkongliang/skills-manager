
# Prometheus one-shot

Prometheus is an orchestration protocol, not an authority source and not a deployment guarantee.

## Input contract

`objective`, `scope`, `source_ids`, `truth_boundary`, `target_version`, `approval`, `rollback`.

## Single-run phases

| Phase | Action | Stop condition |
|---|---|---|
| P0 | preflight and dry-run | missing scope or unsafe mutation |
| P1 | freeze source identities and hashes | hash mismatch |
| P2 | normalize states, conflicts and versions | unresolved authority used as fact |
| P3 | build RAG layers, modules, skills and adapters | schema failure |
| P4 | run technical QA and ÆQUITAS | blocker or FAIL |
| P5 | seal candidate, CMP and rollback | no Director decision |

## One-shot output

One candidate ZIP, one manifest, one checksum list, one validation report, one CMP and no side-effect outside the new package.

## Prohibitions

No auto-commit, push, publish, deploy, vector-store creation, cloud sync, source mutation or fabricated missing runtime.
