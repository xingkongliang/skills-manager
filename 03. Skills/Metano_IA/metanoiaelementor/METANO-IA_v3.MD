## 1. Identidad y modo

`TRANSVERSAL · METANOIA CAPABILITY ARCHITECT`

Modo: `EVALUATE_CAPABILITY · VERSIONING_READINESS`

**Dictamen:** sí, Aleja. Los cinco componentes que señalaste forman el núcleo versionable de METANOIA, pero deben conservar **versiones independientes** y coordinarse mediante un manifiesto común de compatibilidad.

## 2. Comprensión reconstruida

La arquitectura correcta es:

| Capa           | Capacidad                          | Función estable                                                 |
| -------------- | ---------------------------------- | --------------------------------------------------------------- |
| Constitución   | `metanoia-ia`                      | Intención, autoridad humana, continuidad y canon constitucional |
| Gobernanza     | `metanoiae`                        | Rehidratación, saneamiento, ruteo A–E y paquetes para D         |
| Inteligencia   | `metanoia-divide`                  | Escaneo DI–VI–DE y evidencia para decisiones                    |
| Automatización | `metano-ia-codexlsx-auto-engineer` | Traducción de hojas a sistemas auditables y Apps Script         |
| Presentación   | `metanoiaelementor-style`          | Voz técnico-editorial y densidad de salida                      |

Flujo funcional:

`METANO_IA → METANOIAE → DIVIDE → CodexLSX`

`metanoiaelementor-style` opera transversalmente sobre la presentación; no debe controlar evidencia, estado ni automatización.

## 3. Fuentes y límites

Se inspeccionaron físicamente:

* Los cinco paquetes completos.
* Sus referencias, schemas, scripts, assets y evaluaciones disponibles.
* [Modelo-candidato-de-trabajo-RAG-agentic.txt](sandbox:/workspace/scratch/eaa1ec895b98/project_sources/01-Modelo-candidato-de-trabajo-RAG-agentic.txt).
* La declaración constitucional de `METANO_IA v1.0` y su implementación `metanoia-ia 1.1.0`.

Validación ejecutada:

* **6/6 paquetes estructuralmente válidos**, incluyendo `codex-automation-engineer-style`.
* **7/7 scripts Python compilables**.
* Ninguna modificación realizada.
* Esto demuestra integridad estructural, no integración funcional ni runtime.

## 4. Conflictos / UNKNOWN

| Componente                         |           Versión observada | Brecha                                                                          |
| ---------------------------------- | --------------------------: | ------------------------------------------------------------------------------- |
| `metanoia-ia`                      |                     `1.1.0` | Correctamente separada de su canon constitucional `v1.0`                        |
| `metanoiae`                        |                     `0.1.0` | Ya versionada; requiere compatibilidad explícita con METANO_IA canónica         |
| `metanoia-divide`                  |        Sin versión de skill | Su schema sí declara `metanoia-divide/1.0`                                      |
| `metano-ia-codexlsx-auto-engineer` |        Sin versión de skill | Templates y scripts existen, pero falta identidad SemVer                        |
| `metanoiaelementor-style`          |                 Sin versión | Falta baseline y pruebas de consistencia                                        |
| `codex-automation-engineer-style`  | “V2.1” dentro del contenido | Está ligado a Claude Sonnet/Gema BOS; no equivale a una versión formal de skill |

Además, el modelo A–E adjunto clasifica `METANO_IA` como `PROVISIONAL`. Ese estado quedó **supersedido**, sin borrar el registro histórico, por tu declaración canónica del 3 de agosto de 2026:

`METANO_IA v1.0 = USER_DECLARED_CANONICAL_CONSTITUTION`

Eso no promueve automáticamente A–E, DIVIDE, CodexLSX, integración ni runtime.

## 5. Célula responsable

* **E:** evaluar compatibilidad, solapamientos, triggers y SemVer.
* **D:** aceptar o devolver el baseline.
* **C:** materializar versiones aceptadas.
* **Aleja:** única autoridad para cambiar el canon constitucional de METANO_IA.

## 6. Acción propuesta

Crear un primer release train:

`METANOIA_CAPABILITY_FAMILY_v0.1.0-rc.1`

Sin forzar una versión única:

| Miembro                            | Baseline propuesto |
| ---------------------------------- | -----------------: |
| `metanoia-ia`                      |  conservar `1.1.0` |
| `metanoiae`                        |  conservar `0.1.0` |
| `metanoia-divide`                  |   declarar `0.1.0` |
| `metano-ia-codexlsx-auto-engineer` |   declarar `0.1.0` |
| `metanoiaelementor-style`          |   declarar `0.1.0` |
| Schema DIVIDE                      |    conservar `1.0` |
| Canon METANO_IA                    |   conservar `v1.0` |

`codex-automation-engineer-style` conviene registrarlo como **perfil auxiliar heredado**, sujeto a neutralización de proveedor antes de incorporarlo al release train.

## 7. CMP / handoff copy-paste

```yaml
METANOIA_CAPABILITY_FAMILY_PROPOSAL:
  family_id: "METANOIA_CAPABILITY_FAMILY"
  proposed_release: "0.1.0-rc.1"
  status: "VERSIONING_CANDIDATE"

  constitutional_base:
    component: "metanoia-ia"
    canon: "METANO_IA v1.0"
    skill_version: "1.1.0"
    canon_authority: "Aleja"

  members:
    metanoiae:
      version: "0.1.0"
      role: "governed rehydration and A-E routing"

    metanoia-divide:
      proposed_version: "0.1.0"
      schema_version: "1.0"
      role: "DI-VI-DE evidence intelligence"

    metano-ia-codexlsx-auto-engineer:
      proposed_version: "0.1.0"
      role: "spreadsheet automation and Apps Script"

    metanoiaelementor-style:
      proposed_version: "0.1.0"
      role: "technical-editorial presentation profile"

  auxiliary:
    codex-automation-engineer-style:
      state: "LEGACY_PROFILE_COMPATIBILITY_REVIEW"
      issue: "provider and Gema BOS V2.1 coupling"

  invariants:
    independent_semver: true
    shared_compatibility_manifest: true
    schema_versions_independent: true
    raw_sources_preserved: true
    self_approval: false
    runtime: false
    integration: false

  next_owner: "E · Capability Governance"
  returns_to: "D · QA / MERGE / RELEASE"
```

## 8. Delta para D

```yaml
delta:
  confirmed:
    - "The five named capabilities form a coherent versionable family."
    - "All inspected packages pass structural validation."
    - "METANO_IA v1.0 remains the user-declared constitutional canon."
    - "Skill, schema, adapter and family versions must remain distinct."

  changed: []

  blocked:
    - "Formal SemVer declaration for DIVIDE, CodexLSX and metanoiaelementor."
    - "Cross-capability compatibility tests."
    - "D acceptance and C materialization."

  next:
    - "Build the family manifest and patch the three missing version declarations."
    - "Run E compatibility evaluation."
    - "Return one bounded release candidate to D."

D_REQUEST:
  decision: "ACCEPT"
  decision_scope: "METANOIA_CAPABILITY_FAMILY_VERSIONING_ARCHITECTURE_ONLY"
  runtime: false
  integration: false
  canon_declared: false
```
