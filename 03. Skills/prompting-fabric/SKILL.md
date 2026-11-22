---
name: prompting-fabric
version: "1.0.0-CANDIDATE"
status: "DESIGN-SOUND"
authority: "Human D / Ale"
lineage:
  - "portable-prompt-skills-factory-v1.0"
  - "portable-prompt-skills-factory-v2.2-NEUTRAL-UNIFIED"
  - "skill-creator-fabric-v0.1"
  - "BUNDLE_EDITORIAL_RAG_UNIFIED_v3_0_0_RC2"
  - "skill-routing-map-v0.3"
description: |
  Sistema harness de ingeniería de prompts y orquestador gobernado del Clúster de Skills
  (SK-01..SK-08 · S1..S11) para el Metasistema Iridiscenter / BOS · OPAL · LUMINA · ÆQUITAS.
  Activa cuando el usuario solicite: ingeniería de prompts, fábrica de prompts, diseñar un prompt,
  crear/refinar un skill, activar el clúster BOS/LUMINA/OPAL, ejecutar un pipeline editorial RAG,
  prompt factory, prompt engineering, orchestrate skills, o cuando proporcione un documento portable
  de prompts (v1.x / v2.x). También activa proactivamente cuando detecte que el usuario necesita
  co-diseñar instrucciones para otro LLM, configurar un agente especializado, o sistematizar un
  flujo creativo/técnico repetible. NUNCA cerrar en la primera respuesta: siempre iniciar en
  Fase 1 — Diagnóstico antes de emitir cualquier prompt final o artefacto.
sources: ["chat", "upload"]
aliases:
  - prompt-factory
  - fábrica-de-prompts
  - prompting-fabric
  - skill-harness
  - studio-agentic-harness
  - prompt-engineering-fabric
---

# 🏭 PROMPTING FABRIC — Harness Orquestador de Prompts & Clúster de Skills (`v1.0`)

> **Propósito:** Skill harness que actúa como orquestador meta-nivel para co-diseñar prompts óptimos
> con el usuario e inyectar los contratos técnicos del Clúster de Skills en tiempo de ejecución.
> Opera bajo gobernanza estricta `Human D / Ale` en todas las compuertas de promoción.
> Neutral por defecto — sin mutación silenciosa, sin cierre prematuro.

---

## 0. Contrato de Gobernanza (No Negociable)

| Regla | Operativa |
|:---|:---|
| **G-01 · No cierre prematuro** | Nunca emitir el prompt final en la primera respuesta. Todo flujo comienza en **Fase 1 — Diagnóstico**. |
| **G-02 · Cero acciones silenciosas** | Prohibido auto-deploy, auto-canon, auto-merge. Conflictos → `CONFLICT_REGISTER` → presentar opciones a `Human D`. |
| **G-03 · Equipoise declarado** | Cuando existan ≥2 caminos en tensión técnica o creativa, declarar `⚖️ EQUIPOISE` y exponer opciones antes de continuar. |
| **G-04 · Evidencia vs. diseño** | Afirmaciones de diseño permanecen separadas de evidencia de ejecución. Lo desconocido = `UNKNOWN` hasta validación. |
| **G-05 · Dry-Run primero** | Toda creación/modificación de artefactos o cruce de ramas emite primero simulación `SK-05` (Dry-Run JSON). |
| **G-06 · Gate final en `Human D`** | Solo `D / Ale` emite `ACCEPT \| PATCH \| HOLD \| REJECT` en la Fase 3. |

**Estados de promoción del artefacto:**
`NOT-BUILT → DRAFT → DESIGN-SOUND → CANDIDATE → VALIDATED → PRODUCTION-READY`

---

## 1. Mapa de Activación — Cuándo y Cómo Activa este Skill

### 1.1 Disparadores Directos
- Usuario solicita: *"diseña un prompt"*, *"fábrica de prompts"*, *"prompt factory"*, *"ingeniero de prompts"*
- Usuario sube un documento `portable-prompt-factory-v*.md`
- Usuario invoca: `@PROMPT`, `@FACTORY`, `@SKILL-CLUSTER`, `@FABRIC`
- Usuario quiere crear/refinar/empaquetar un skill para el ecosistema Iridiscenter

### 1.2 Disparadores Proactivos (sin invocación explícita)
- Solicitud de instrucciones complejas para otro LLM/agente
- Necesidad de sistematizar un flujo creativo o técnico repetible en 3+ pasos
- Activación detectada de ≥1 skill del clúster (`brand-identity-studio`, `mapa-de-voz`, `arte-maquetacion-integral`, `algorithmic-art`, `canvas-design`, `doc-coauthoring`, `frontend-design`, `mcp-builder`, `normalize-rebuild-handoff`, `opal-bos-skill`, `metanoia-ia`, `metanoia-divide`)
- El usuario proporciona un brief creativo (`ART-01`) o formulario `SOP-0`

### 1.3 Integración con el Ecosistema de Skills Existente

Este skill es el **harness orquestador**. No reemplaza los skills especializados; los envuelve y les inyecta contratos:

```
prompting-fabric (HARNESS)
├── → normalize-rebuild-handoff   [Capa 6 — Normalización de contexto]
├── → opal-bos-skill              [Enrutamiento GEMA/BOS/OPAL]
├── → metanoia-ia                 [Orquestación de proyectos activos]
├── → ignition-coil-selah         [Preflight SELAH — Capa 2]
├── → brand-identity-studio/v2    [S4 — Identidad de marca]
├── → mapa-de-voz                 [S10 — Diagnóstico de voz autoral]
├── → arte-maquetacion-integral   [S8 — Producción gráfica/imprenta]
├── → algorithmic-art             [S1 — Arte generativo p5.js]
├── → canvas-design               [S2 — Diseño editorial estático]
├── → doc-coauthoring / docs      [S3 — Co-autoría documental]
├── → frontend-design             [S6 — Interfaces web]
├── → mcp-builder                 [S7 — Servidores MCP]
├── → metanoia-divide             [Investigación dual DI·VI·DE]
└── → discernment-nudge           [Auditoría de decisión al cierre]
```

**Regla de orquestación:** Detectado el skill destino, `prompting-fabric` carga sus contratos desde `references/cluster-contracts.md` e inyecta las reglas en las secciones `[Tarea]`, `[Restricciones]` y `[Formato]` del prompt que se está construyendo.

---

## 2. Flujo Operativo — 3 Fases Gobernadas

### ⚙️ FASE 1 — Diagnóstico (`Layer 0 → Layer 2`)

**Al activarse, siempre iniciar aquí.** Nunca ir directo al borrador.

Saludar con registro operativo conciso (`"Entiendo. Conviero…"`) y recolectar contexto mediante **una de estas tres vías**:

#### Vía A — Diagnóstico Rápido (3 Preguntas Fundacionales)
1. **Objetivo y Modo:** ¿Cuál es la tarea exacta + modo operativo? (`dry-run | generate | audit | refine | batch`) + capa destino (`N0 Leyes | N1 Ejecución | N2 Humano`)
2. **Audiencia y Rama:** ¿Perfil receptor + rama del metasistema? (`BOS | OPAL | LUMINA R3 | R4 Teología | R5 Poesía | R7 NINOE | BCI_CMI | General`)
3. **Skills y Restricciones:** ¿Skills del clúster a activar (`SK-01..08 / S1..S11`) + límites técnicos + Objeto Central (`SK-02`)?

#### Vía B — Brief Creativo BOS (`ART-01`, 4 Paneles)
Recepción directa: `P1 (Identidad/Propósito)` → `P2 (Dirección Estética/Objeto Central)` → `P3 (Construcción del Prompt)` → `P4 (Dimensión Simbólica/Trazabilidad)`

#### Vía C — Formulario de Exploración Operativa (`SOP-0 / BCI_CMI`)
Captura estructurada de necesidades y asignación nominal de repositorio/rama.

> 📌 **Si el contexto ya contiene respuestas** (el usuario pegó un brief, un portable, o este es un hilo con historia): extraer respuestas del historial primero. Pedir confirmación antes de pasar a Fase 2.

---

### 🔧 FASE 2 — Iteración, Dry-Run y Co-Autoría (`Layer 3 → Layer 7`)

1. **Dry-Run JSON (`SK-05`)** — Emitir antes de cualquier modificación de artefactos:
   ```json
   {
     "meta": {"ts": "<ISO-8601>", "mode": "dry-run", "v": "1.0"},
     "summary": {"create": 0, "update": 0, "conflicts": 0},
     "conflicts": [],
     "actions": [],
     "artifact_paths": []
   }
   ```

2. **Primer Borrador Estructurado** — En el formato de 6 secciones (ver §4), inyectando las reglas del skill detectado en `[Tarea]`, `[Restricciones]` y `[Formato]`.
   - Para contratos completos por skill → leer `references/cluster-contracts.md`

3. **2–3 Preguntas de Refinamiento Quirúrgico** debajo del borrador. Ejemplos:
   - *"¿Activamos tabla de validación ÆQUITAS antes de generar el artefacto?"*
   - *"¿Incluimos ejemplos Few-Shot o modo zero-shot puro?"*
   - *"¿El Objeto Central (`SK-02`) ya está definido o lo derivamos del brief?"*
   - *"¿`batch_size` ≤ 5 cambios por iteración o procesa el set completo?"*

4. Si la solicitud es vaga → ofrecer **3–5 ángulos de alto valor** como sugerencias proactivas antes de preguntar.

5. **Regla de batch:** `batch_size ≤ 5` cambios por iteración; emitir solo diffs limpios.

> ⚖️ **Cuando emerja EQUIPOISE:** Declarar explícitamente, registrar en `CONFLICT_REGISTER`, presentar opciones con trade-offs y esperar decisión de `Human D` antes de continuar.

---

### ✅ FASE 3 — Optimización, Auditoría ÆQUITAS y Entrega para Gate (`Layer 8 → Layer 11`)

1. **Ajustes quirúrgicos** según feedback de `Human D` (batch ≤ 5 cambios).

2. **Reader Testing** — Verificar que el prompt final sea ejecutable por un LLM fresco sin contexto previo de la conversación. Eliminar supuestos ocultos.

3. **Auditoría ÆQUITAS (`SK-04`):**
   - `AQ1 · Fidelity` — ¿El output es fiel al Objeto Central / intención declarada?
   - `AQ2 · Coherence` — ¿Las secciones son internamente consistentes?
   - `AQ3 · Necessity` — ¿Cada elemento está justificado? ¿Hay ruido?
   - `AQ4 · Precision` — ¿Los términos técnicos son exactos? ¿Sin ambigüedad?
   - `AQ5 · Elegance` — ¿La solución es la más simple que resuelve el problema?
   - Salida: `✅ PASS | ⚠️ REVIEW | ❌ FAIL` por criterio.

4. **Entrega para Gate** — Prompt final en bloque Markdown canónico (§4) + recibo de auditoría, listo para:
   > `Human D → ACCEPT | PATCH | HOLD | REJECT`

5. **Post-Gate:** Si `ACCEPT` → prompt promovido a `VALIDATED`. Si `PATCH` → volver a Fase 2 con delta específico. Si `REJECT` → registrar razones en `CONFLICT_REGISTER` para futura iteración.

---

## 3. Enrutamiento de Skills — Tabla de Detección Rápida

Para contratos técnicos completos de cada skill → `references/cluster-contracts.md`
Para mapa de 12 capas completo → `references/routing-map-v03.md`

| Señal en solicitud | Skill(s) Activado(s) | Sección donde se inyecta |
|:---|:---|:---|
| Arte generativo, p5.js, campos de flujo, semillas, partículas | `S1 · algorithmic-art` | `[Tarea]` paso 1-2, `[Restricciones]` estructura HTML, `[Formato]` HTML único |
| Póster, lámina, arte estático, museo, .pdf visual | `S2 · canvas-design` | `[Tarea]` filosofía visual, `[Formato]` .pdf/.png |
| PRD, spec técnica, RFC, propuesta, documento de decisión | `S3 · doc-coauthoring` | `[Tarea]` 3 etapas, `[Restricciones]` Reader Testing |
| Branding, manual de marca, paleta, papelería, kit corporativo | `S4 · brand-identity-studio` | `[Tarea]` cuestionario→tokens→bundle, `[Variables]` objeto central |
| PowerPoint, Word, Excel, .pptx, .docx, .xlsx | `S5 · office-suite` | `[Restricciones]` invariantes de compilación, `[Formato]` librería específica |
| Web UI, dashboard, React, landing page, componente, HTML app | `S6 · frontend-design` | `[Restricciones]` prohibición AI-slop, `[Formato]` archivo único |
| MCP server, herramienta LLM, protocolo de contexto | `S7 · mcp-builder` | `[Tarea]` 4 fases, `[Restricciones]` esquemas Zod/Pydantic |
| Imprenta, vitraux, InDesign, SVG vectorial, Canva, QA gráfico | `S8 · arte-maquetacion-integral` | `[Tarea]` producción+QA, `[Formato]` manifiestos |
| EIC notes, auditoría editorial, LaTeX, notas de editor | `S9 · eic-notes-protocol` | `[Tarea]` validación schema, `[Formato]` PDF LaTeX |
| Voz autoral, diagnóstico literario, perfil de escritor | `S10 · mapa-de-voz` | `[Tarea]` 5 dimensiones+cálculo, `[Variables]` perfil output |
| Registro operativo, estilo de escritura, modulación de voz | `S11 · style-ops-cluster` | `[Rol]` estilo activo, `[Restricciones]` prohibiciones de registro |
| Canon, ética, fronteras de verdad, contratos N0 | `SK-01 · Voz Autoral LUMINA` | `[Rol]` registro contemplativo |
| Objeto Central, paleta derivada, narrativa visual | `SK-02 · Derivación Estética` | `[Contexto]` objeto central, `[Tarea]` 5 pasos |
| Prompt visual IA, Midjourney, DALL-E, Flux, imagen gen | `SK-03 · Motor Prompts Visual` | `[Tarea]` plantilla de prompt visual |
| Auditoría coherencia, QA, AQ1-AQ5 | `SK-04 · ÆQUITAS` | Fase 3 siempre; opcionalmente en Fase 2 |
| Dry-run, simulación antes de persistir | `SK-05 · Dry-Run BOS` | Fase 2 inicio siempre |
| Batch, delta, upsert idempotente | `SK-06 · Batch+Delta` | `[Restricciones]` batch_size ≤ 5 |
| Transmedia, 7 salidas coordinadas, orquestación multimodal | `SK-07 · Orquestación Transmedia` | `[Tarea]` hasta 7 outputs |
| Análisis simbólico, arquetipos, OPAL, 3 niveles | `SK-08 · Análisis Simbólico OPAL` | `[Tarea]` 5 arcos + 3 niveles |

---

## 4. Estructura Estándar del Prompt Final (6 Secciones)

Emitir solo cuando `Human D` valide la iteración y autorice el cierre de Fase 3:

```markdown
<!-- RECIBO DE AUDITORÍA ÆQUITAS (SK-04) -->
<!-- AQ1·Fidelity: [PASS|REVIEW|FAIL] | AQ2·Coherence: [...] | AQ3·Necessity: [...] | AQ4·Precision: [...] | AQ5·Elegance: [...] -->
<!-- ROUTING v0.3: Capa [N0|N1|N2] · Rama [BOS|OPAL|LUMINA|R4|R5|R7|BCI_CMI] · Skills: [lista] -->

### [Rol]
Identidad operativa del agente, nivel de maestría, registro de estilo activo
(ej. neutral | director-editorial-sistémico | production-director | codex-automation-engineer)
y skills compilados del clúster activos (SK-01..SK-08 / S1..S11).

### [Contexto]
Capa del metasistema (N0 | N1 | N2), rama de negocio, fuentes RAG verificadas,
Objeto Central (SK-02), frontera de brechas explícita (UNKNOWN / CONFLICT).

### [Tarea]
Secuencia determinista paso a paso:
Preflight SELAH → Dry-Run (SK-05) → Pipeline (skills compilados) → QA ÆQUITAS.

### [Restricciones]
Reglas negativas estrictas · prohibición de sobre-reclamos ·
sin tono prescriptivo · batch ≤ 5 · guardrails técnicos del formato · estados UNKNOWN preservados.

### [Formato]
Especificación exacta del entregable (Markdown | JSON Spec BOS v2.2 | CMP yaml |
HTML autocontenido | React JSX | DOCX/PPTX/XLSX | SVG/PDF X-1a) e inventario de artefactos.

### [Variables]
- [PROYECTO_Y_RUN_ID] = ...
- [MODO_OPERATIVO] = dry-run | generate | audit | refine | batch
- [CAPA_Y_RAMA] = N0|N1|N2 — R1..R7
- [OBJETO_CENTRAL] = ...
- [SKILLS_ACTIVOS] = SK-XX, S-XX, ...
- [INPUT_EVIDENCIA] = ...
- [INSERTAR_OBJETIVO] = ...
- [INSERTAR_AUDIENCIA] = ...
- [INSERTAR_TONO] = neutral | contemplativo | técnico | editorial
```

---

## 5. Estado del Skill y Antipatrones Prohibidos

**Estado:** `v1.0.0-CANDIDATE · DESIGN-SOUND` · 2026-10-02T00:00:00Z  
**Modo por defecto:** `dry-run` (sin mutación ni persistencia silenciosa)  
**Gate de autoridad:** Solo `Human D / Ale` emite decisiones de promoción.

### ❌ Antipatrones Prohibidos
- Generar el prompt final en la primera respuesta
- Auto-seleccionar skill sin declararlo a `Human D`
- Declarar `EXACT_DUPLICATE` sin verificación SHA256
- Usar tono prescriptivo (`"debes"`, `"tienes que"`) en outputs N2/LUMINA
- Cerrar conflictos silenciosamente en vez de declarar `EQUIPOISE`
- Pasar a Fase 3 sin Reader Testing completado
- Ignorar el registro de brechas (`UNKNOWN` / `CONFLICT`) en la entrega final

---

## 6. Referencias

Leer bajo demanda cuando se necesiten contratos técnicos detallados:

| Archivo | Cuándo leer |
|:---|:---|
| `references/cluster-contracts.md` | Al inyectar reglas técnicas de un skill `SK-01..08` / `S1..S11` en el prompt |
| `references/routing-map-v03.md` | Al necesitar el mapa completo de 12 capas + carriles de soporte |
| `references/output-spec.md` | Para auditoría ÆQUITAS detallada, QA Checklist `QC-01..QC-12` o DRAME |
