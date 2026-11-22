# UCARE-XP · Niveles de concreción cognitivo-operativa

| Nivel | Bloom/Marzano analogue | Pregunta de la IA | Operación | Artefacto |
|---|---|---|---|---|
| L0 | Self-system / propósito | ¿Qué quiere lograr D y qué NO debo asumir? | Traducir intención | INTENT_BRIEF |
| L1 | Retrieval / Remember | ¿Qué tengo realmente? | Recibir, localizar, inventariar | SOURCE_INVENTORY |
| L2 | Comprehension / Understand | ¿Qué significa este contexto? | Orientar y contextualizar | CONTEXT_MAP |
| L3 | Analysis | ¿Qué evidencia sostiene cada cosa? | Verificar, trazar, separar | EVIDENCE_TRACE |
| L4 | Evaluate / Reconcile | ¿Qué coincide, diverge o está obsoleto? | Comparar y reconciliar | RECONCILIATION_DELTA |
| L5 | Create / Reconstruct | ¿Cuál es el mínimo estado coherente? | Rehidratar/reconstruir | CMP / NORMALIZED_STATE |
| L6 | Metacognitive alignment | ¿Estoy usando la misma semántica y límites? | Realinear/sintonizar | ALIGNMENT_MAP |
| L7 | Knowledge utilization | ¿Qué trabajo puedo hacer con este estado? | Integrar vista derivada | ACCOUNT_DELTA / WORKING_VIEW |
| L8 | Metacognitive receipt | ¿Qué entendí y qué sigue separado? | ACK verificable | REHYDRATION_RECEIPT |
| L9 | Governance | ¿Requiere promoción/decisión humana? | SELAH + D Gate | ACCEPT/PATCH/HOLD/REJECT |
| L10 | Continuity | ¿Cómo reanuda el siguiente peer/chat sin rebuild? | Cerrar y transferir | SESSION_CMP + NEXT_LOADER |

## Regla de aprendizaje continuo

Cada ciclo debe dejar:
- evidencia nueva;
- delta de comprensión;
- corrección de error o precisión semántica;
- estado rehidratable;
- punto de reanudación;
- una sola capa de mayor abstracción si está justificada.

La siguiente IA no "memoriza" el sistema por decreto: **reaprende desde evidencia condensada y receipts**.
