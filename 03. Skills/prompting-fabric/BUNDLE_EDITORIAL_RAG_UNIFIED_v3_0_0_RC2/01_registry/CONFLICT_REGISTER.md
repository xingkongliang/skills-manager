# Conflict Register · RC2

No se resolvió silenciosamente ninguna colisión.

| ID | Severidad | Categoría | Estado | Tratamiento RC2 |
|---|---|---|---|---|
| C-001 | BLOCKER | missing_runtime | OPEN | `SOURCE_IDENTIFIED_NOT_MATERIALIZED_IN_RC1` · Pointer registrado: KOP_SRC_006, 69849 bytes, sha256 f84484ac… · HTML no creado ni reconstruido. |
| C-002 | BLOCKER | runtime_qa | OPEN | `HTML_COMPLETE_RUNTIME_QA_PENDING` · Sanidad de completitud resuelta (DOCTYPE, closing HTML, JS tail). QA browser/print/export/pagination pendiente. |
| C-003 | HIGH | integrity_manifest | MITIGATED_IN_RC1 | Manifest no autorreferencial generado en RC1. Sin cambio en RC2. |
| C-004 | HIGH | editorial_pattern | OPEN | Patrón odd=contenido / even=reflexión PROVISIONAL. Sin cambio en RC2. Requiere decisión Director. |
| C-005 | HIGH | brand_token_drift | PROVISIONAL | Token clínico `#7B9EA6` provisional. Sin cambio en RC2. Requiere decisión Director. |
| C-006 | MEDIUM | version_namespace | MITIGATED_IN_RC1 | `package_version` y `line_version` separados. Sin cambio en RC2. |
| C-007 | MEDIUM | project_boundary | MITIGATED_IN_RC1 | Solo reutilización de gobernanza. Sin cambio en RC2. |
| C-008 | MEDIUM | historical_overclaim | MITIGATED_IN_RC1 | Solo patrones de protocolo; claims runtime no autoritativos. Sin cambio en RC2. |
| C-009 | MEDIUM | missing_content | OPEN | Self-Care/Journey: spec-only, sin contenido autoral generado. Sin cambio en RC2. Requiere brief autoral. |
| C-010 | LOW | source_discovery | CLOSED_AS_NEGATIVE_RESULT | No se infirió ausencia ni se inventó fuente Drive. Sin cambio en RC2. |

Los detalles y `source_ids` están en `CONFLICT_REGISTER.json`.

**Conflictos que requieren decisión o evidencia adicional:** C-001, C-002, C-004, C-005, C-009.

**RC2 patch scope:** solo C-001 y C-002 refinados semánticamente. C-004, C-005, C-009 preservados sin cambio.
