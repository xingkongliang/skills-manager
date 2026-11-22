# Bundle Editorial RAG Unificado · RC2

**Paquete:** `BUNDLE_EDITORIAL_RAG_UNIFIED_v3_0_0_RC2`
**Versión:** `3.0.0-rc.2`
**Patch:** `factual_registry_patch` sobre RC1
**Estado:** `DRAFT_FOR_APPROVAL`
**Release decision:** `REVIEW`
**Canon:** no declarado · **runtime:** no desplegado · **vector store:** no construido

## Qué cambió respecto a RC1

1. **Literatura** corregida a `LOAD_PARCIAL`: 12/20 fichas presentes [1–8, 17–20]; fichas 9–16 (S2 Exploración) ausentes y registradas. `CUADERNILLO_LITERATURA_v1.0.md` no modificado.
2. **C-001** refinado a `SOURCE_IDENTIFIED_NOT_MATERIALIZED_IN_RC1`: pointer `KOP_SRC_006` registrado. HTML no creado.
3. **C-002** refinado a `HTML_COMPLETE_RUNTIME_QA_PENDING`: sanidad de completitud resuelta. QA browser/print pendiente.
4. **C-004, C-005, C-009** preservados sin cambio para decisión posterior.

## Qué puede hacerse con este paquete

- revisar fuentes, conflictos, líneas, módulos y rutas de ejecución;
- ingerir el índice RAG en un runtime externo después de aprobación;
- abrir el HTML de Comet como candidato de revisión;
- ejecutar la validación local: `python tools/validate_bundle.py .`;
- rehidratar la sesión desde `09_handoff/CMP.yaml`.

## Qué no declara

- no declara canon, merge remoto, publicación ni despliegue;
- no reconstruye el HTML ausente de DMIE/MMII;
- no genera las fichas 9–16 de Literatura;
- no promueve logs crudos o documentos históricos a fuente vigente;
- no genera contenido para Self-Care/Journey ni para líneas propuestas sin fuente autoral.

## Entrada operativa

1. Leer `00_contract/CORE_CONTRACT.md`.
2. Revisar `01_registry/CONFLICT_REGISTER.md`.
3. Confirmar el gate en `08_qa/QA_GATE_MATRIX.md`.
4. Ejecutar el handoff de `09_handoff/NEXT_COMMANDS.md`.

La arquitectura completa está en [ARCHITECTURE.md](ARCHITECTURE.md).
