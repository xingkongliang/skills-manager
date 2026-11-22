# Changelog

## 3.0.0-rc.2 · 2026-07-23

### Patch type: factual_registry_patch

#### Literatura line correction
- Status corrected from `CURRENT_CANDIDATE (20 fichas)` to `LOAD_PARCIAL`.
- Audit confirmed 12/20 entries present: [1,2,3,4,5,6,7,8,17,18,19,20].
- Missing entries 9–16 (S2 Exploración) are registered as absent; not generated.
- `CUADERNILLO_LITERATURA_v1.0.md` not modified.

#### C-001 DMIE/MMII semantics update
- Status refined from `BLOCKED · master ausente` to `SOURCE_IDENTIFIED_NOT_MATERIALIZED_IN_RC1`.
- Structural pointer registered: `KOP_SRC_006`, sha256 `f84484ac…`, 69849 bytes.
- C-001 remains OPEN. HTML not created or reconstructed.

#### C-002 Comet runtime semantics update
- Status refined to `HTML_COMPLETE_RUNTIME_QA_PENDING`.
- Completeness sanity checks resolved (DOCTYPE, closing HTML, JS tail present).
- C-002 remains OPEN. Browser/print/export/pagination QA not claimed.

#### Preserved
- C-004, C-005, C-009 remain OPEN without change.
- Release decision: REVIEW. Canon status: NOT_FINAL_CANON.
- Runtime and deployment remain BLOCKED.

### Truth boundary
`DRAFT_FOR_APPROVAL`; no canon, deploy, publicación, push ni vector store.

# Changelog

## 3.0.0-rc.1 · 2026-07-22

### Added

- Arquitectura RAG por capas N0/N1/N2.
- Registro de fuentes, conflictos y límites de autoridad.
- Registro modular y manifiesto de skills.
- Adapters separados para DMIE/MMII y Comet Striker.
- Línea candidata Self-Care/Journey como spec sin contenido fabricado.
- Protocolos METANO_IA, Prometheus one-shot, SELAH y ÆQUITAS.
- CMP portable y handoff hacia D · QA / MERGE / RELEASE.
- Manifest y checksums no autorreferenciales.

### Integrated without promotion

- Literatura, Clínica y Espiritual desde BOS v2.
- Comet Striker Creative Journey v1.4 como candidato de revisión.
- EIC Notes Protocol v1.4 como módulo validado localmente en su upstream.

### Blocked

- Runtime DMIE/MMII: archivo maestro ausente.
- Publicación Comet: QA integral y aprobación pendientes.
- Self-Care/Journey v4: contenido autoral ausente.

### Truth boundary

`DRAFT_FOR_APPROVAL`; no canon, deploy, publicación, push, commit remoto ni vector store.
