
# Core Contract

## Objetivo

Mantener un solo contenedor editorial consultable por RAG sin colapsar las fronteras entre evidencia, especificación, artefacto, candidato y canon.

## Estados permitidos

| Estado | Significado | Generación directa |
|---|---|---|
| `EVIDENCE` | bytes o registro inspeccionado | no |
| `VALIDATED_RECORD` | dato validado contra fuente | con filtro |
| `PROVISIONAL` | normalización reversible | con aprobación |
| `DRAFT_FOR_APPROVAL` | salida construida, no aceptada | no publicación |
| `ACTIVE_CONTROLLED` | autorizado para operación acotada | según gate |
| `CANON` | solo declaración explícita de Dirección | sí, según alcance |
| `UNKNOWN` | no verificable | no |
| `BLOCKED` | condición material pendiente | no |

## Reglas invariantes

1. Un nombre de archivo, un prefijo o una palabra como “final” no otorga autoridad.
2. Cada afirmación material debe resolver a `source_id` o `UNKNOWN`.
3. Los logs crudos sirven para descubrimiento, no para generación directa.
4. `package_version` y `line_version` son namespaces distintos.
5. Todo módulo recibe un contrato y devuelve delta, evidencia y estado.
6. Toda mutación externa requiere preflight, aprobación y rollback.
7. La decisión final pertenece a Dirección y vuelve a `D · QA / MERGE / RELEASE HANDOFF`.
