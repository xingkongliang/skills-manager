# Skill Routing Map v0.3 — 12 Capas + Carriles de Soporte
# Fuente: portable-prompt-factory-v2.2 · BUNDLE_RAG_v3_0_0_RC2
# Leer desde SKILL.md §6 · Solo cuando se requiera el mapa completo

---

## Capas Secuenciales del Pipeline (Layer 0 → Layer 11)

| Capa | Capacidad / Responsable | Uso en el Flujo | Output | Mutación / Autoridad |
|:---|:---|:---|:---|:---|
| **0 · Human intent** | **D / Ale** | Define propósito, prioridad, alcance y objetivo de decisión | Meta humana / Gate decision | **Solo Humano** |
| **1 · Discernment** | `METANO_IA` + `discernment-nudge` | Traduce solicitud literal en intención operativa acotada; expone supuestos | Brief de intención / Scope | Ninguna |
| **2 · Preflight** | `ignition-coil-selah` | Congela alcance, fuentes, prohibiciones y condiciones de parada | Preflight SELAH | Ninguna |
| **3 · Discovery** | GPT_02 / escaneo de fuentes | Localiza archivos, exportaciones, versiones candidatas y procedencia | Inventario de fuentes (SOURCE_INDEX) | Ninguna |
| **4 · Raw adapter** | Evidence Compiler (candidato) | Parsing determinista de estructura cruda; extracción semántica candidata | Índice de turnos / Candidatos | Ninguna |
| **5 · Lineage** | PROMETHEUS / GPT_02 | Parentesco, ramas, derivaciones, reclamos de reemplazo, origen de cuenta/sesión | Grafo de linaje de artefactos | Ninguna |
| **6 · Normalize / Rebuild** | `normalize-rebuild-handoff v1.1` | Clasifica evidencia / reclamo / conflicto / UNKNOWN → estado portable | CMP / Delta / Handoff | Ninguna |
| **7 · Skill fabrication** | `prompting-fabric` / `skill-creator-fabric` | Empaqueta métodos acotados cuando requisitos y contrato de evaluación son explícitos | Candidato de Skill + Plan de eval | Solo generación de candidato |
| **8 · Peer audit** | CLD_00 / Carril ÆQUITAS | Revisión adversarial; cuestiona sobre-reclamos, linaje y cobertura | Recibo de auditoría / Lista de parches | Ninguna |
| **9 · Architecture QA** | CLD_04 | Revisa modularidad, acoplamiento y frontera implementación/pruebas | Delta de arquitectura | Ninguna |
| **10 · Gate prep** | Reconciliación GPT_02 | Reconcilia deltas de auditoría sin resolver conflictos silenciosamente | CMP listo para Gate | Ninguna |
| **11 · Gate** | **D / Ale** | Disposición final: `ACCEPT \| PATCH \| HOLD \| REJECT` | Decisión ejecutiva | **Solo Humano** |

---

## Carriles Condicionales de Soporte / Witness (no secuenciales)

| Carril | Actor | Disparador / Propósito | Output Permitido | Frontera Operativa |
|:---|:---|:---|:---|:---|
| **S1 · Factibilidad** | E·Motus | Evaluar skills/workers, factibilidad de secuencia, viabilidad de procesador y condiciones de parada | Delta de factibilidad / Recomendación de worker | Sin canon, sin despliegue, sin mutación de estado, sin auto-aprobación |
| **S2 · Cronología** | Isacar | Reconstruir cronología, linaje documental, paradero, recibos y continuidad de handoff | Delta de cronología / paradero / recibos | Solo testigo documental; sin promoción de verdad |
| **S3 · Testigo Histórico** | GPT_01 | Recuperar evidencia fuente local o histórica ante brecha concreta de linaje | Delta de cuenta | Sin reconciliación global ni selección de rama |
| **S4 · Testigo Drive** | Gemini / Drive witness | Verificar presencia en Drive, IDs, hashes o deltas vinculados a fuente cuando está autorizado | Recibo de Drive / fuente | Sin reclamo de sync, merge, canon ni normalización semántica |

---

## 12 Reglas Canónicas de Enrutamiento y Evidencia (v0.3)

1. Un registro crudo se enruta al Evidence Compiler **antes** de cualquier reconciliación semántica.
2. El Evidence Compiler es un **adaptador hacia el pipeline de procedencia/linaje**, no fuente de verdad independiente.
3. `skill-creator-fabric` / `prompting-fabric` empaqueta métodos acotados; no decide verdad ni se valida a sí mismo.
4. El mismo número de versión en diferentes cuentas no implica el mismo artefacto ni el mismo linaje.
5. `EXACT_DUPLICATE` requiere igualdad exacta de hash SHA256; similitud permanece como `POSSIBLE_DERIVATIVE`.
6. La existencia de un skill no prueba su integración, ejecución en runtime ni despliegue.
7. `normalize-rebuild-handoff` puede producir documentación lista para commit, pero no puede persistirla ni promoverla.
8. CLD_00 audita; CLD_04 revisa arquitectura; GPT_02 compila/reconcilia; D / Ale decide.
9. E·Motus, Isacar, GPT_01 y Gemini se invocan **únicamente cuando su carril de evidencia es necesario**.
10. Gemini actúa exclusivamente como testigo vinculado a fuente/Drive; visibilidad en Drive no establece sync global.
11. `UNKNOWN` y `CONFLICT` sobreviven a través de todas las capas hasta que evidencia empírica o D los resuelva.
12. Entradas vacías o faltantes **nunca** implican autorización ni estado `GO`.

---

## Diagrama de Flujo del Pipeline

```
D intent (Layer 0)
  ↓
Discernment (L1) → Preflight SELAH (L2) → Discovery (L3)
                                           ├─ archivos estándar ──────────────→ PROMETHEUS / Lineage (L5)
                                           └─ logs crudos de conversación ──→ Evidence Compiler (L4) → PROMETHEUS/Lineage (L5)
                                                                                                                ↓
                                                                                               Normalize / Rebuild (L6)
                                                                                                                ↓
                                                                                   Skill fabrication / prompting-fabric (L7)
                                                                                                                ↓
                                                                                                CLD_00 Peer Audit (L8)
                                                                                                                ↓
                                                                                               CLD_04 Arch QA (L9)
                                                                                                                ↓
                                                                                             GPT_02 Reconciliation (L10)
                                                                                                                ↓
                                                                                                           D Gate (L11)
```

---

## Topología del Metasistema (N0 → N1 → N2)

| Capa | Definición | Metáfora (Ez 37) | Artefactos Verificados |
|:---|:---|:---|:---|
| **N0 · Leyes** | Canon, ética, fronteras de verdad, contratos y tokens | Huesos | `CORE_CONTRACT.md`, `Codex GEMA`, `ÆQUITAS`, `SELAH_LOCKS.md`, `domain_tokens` |
| **N1 · Ejecución** | Schemas, compilación, motores, pipelines, RAG, QA | Tendones / Carne | `BUNDLE_RAG_v3_0_0_RC2`, `OPAL`, `DRAME`, `validate_bundle.py` |
| **N2 · Humano** | Experiencia directa del usuario final ("No se evalúa. Se siente.") | Espíritu | Workbooks (Aleph→Tav, NINOE_369), Cuadernillos (Lit·Clín·Esp), Mapa de Voz, Handbook LUMINA |
