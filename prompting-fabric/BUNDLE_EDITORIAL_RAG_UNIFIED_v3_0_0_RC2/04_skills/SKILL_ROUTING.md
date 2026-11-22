
# Skill routing

| Intent | Owner | Skill | Return |
|---|---|---|---|
| Rehydrate or reconcile | M01/M11 | `normalize-rebuild-handoff` | CMP to D |
| Maintain continuity | M12 | `metanoia-ia` | memory delta |
| Route BOS/LUMINA | M03 | `opal-bos-skill` | module contract |
| Build editorial object | M08 | `crafter-for-editorial-artefacts` | dry-run → artifact |
| Automate build | M11 | `codex-automation-engineer-style` | manifest + QA |
| Derive identity | M08 | `brand-identity-studio` | tokens, if requested |
| Implement UI | M06/M07 | `frontend-design` | runtime candidate |

## Contract

Every skill receives: `task`, `scope`, `source_ids`, `mode`, `constraints`, `expected_output`, `return_destination`. Every skill returns: `delta`, `assumptions`, `conflicts`, `artifact_paths`, `qa`, `next_gate`.
