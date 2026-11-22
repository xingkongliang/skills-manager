---
name: bible-studies-context-engine
description: "Reconstrucción y gestión de contexto para estudios bíblicos dentro del ecosistema ÆQUITAS/Iridiscenter. Normaliza fragmentos, valida autoridad, ejecuta handoff sin mutación de canonicidad. Candidato v0.1 - UNVERIFIED."
status: "CANDIDATE_UNVERIFIED"
version: "0.1.0-draft"
document_id: "SKILL-BIBLE-CAND-001"
timestamp: "2026-08-11T09:06:48-06:00"
---

# Bible Studies Context Engine v0.1.0-draft

## Propósito

Reconstruir estado fragmentado de estudios bíblicos (versículos, referencias, gemas, tribus) desde mensajes naturales, archivos adjuntos y fuentes autorizadas, sin mutar datos canónicos. Produce **Context Management Packets (CMP)** compactos, trazables y ejecutables para entrega a Gate D (QA/MERGE) vía Gate E (Governance).

**Alineación**: Pilar III · ÆQUITAS (Equity + Integrity + Auditable Transformation)

---

## I. Cuándo Activar Este Skill

### Triggers Explícitos (high confidence)
- Usuario invoca `@INVESTIGAR`, `@PREFLIGHT`, `@STATUS` + contexto bíblico
- Usuario dice: "reconstruct my Bible study", "normalize estos versículos", "rebuild the study state"
- Usuario trasfiere trabajo entre chats/LLMs con contenido bíblico + pide continuidad
- Usuario carga archivos bíblicos y solicita estructuración, auditoría o handoff

### Triggers Contextuales (medium confidence)
- Usuario menciona Codex GEMA + preguntas sobre versículos, tribus, gemas, mapeos
- Usuario describe fragmentos de estudio bíblico esparcidos en múltiples fuentes
- Usuario dice "what's the current state of my Bible project"
- Usuario solicita `aeq-memory-intake-handoff-v01` con scope bíblico
- Usuario pide validación ÆQUITAS (AQ1–AQ5) de contenido exegético

### Boundaries (no activar)
- Preguntas teológicas abiertas sin contexto de project state → responder directamente
- Traducción de Biblia aislada sin reconstructión de proyecto → usar herramientas de traducción
- Interpretación exegética no solicitada → rechazar amablemente
- Acceso a datos crudos KOP sin autorización → UNKNOWN + escalate

---

## II. Modo de Operación: NORMALIZE → REBUILD → COMMIT

### A. NORMALIZE (Extracción sin adornar)

**Input**: Mensaje natural + archivos + autorización explícita  
**Output**: Estructuras limpias con source IDs

Extraer sin embellecimiento:
- **Solicitud actual** y resultado deseado  
- **Alcance del proyecto**: límites, exclusiones, entregables  
- **Estado actual** y último checkpoint verificado  
- **Objetivos, CSF, OKR, KPI**: si existen; else → `UNKNOWN`  
- **Decisiones, bloqueadores, próximo movimiento**  
- **Fuentes disponibles** vs. inaccesibles (con rol y límites de autoridad)  

Crear IDs compactos como `SRC-001`, `VER-042`, `GEMA-012`.  
Si CSF/OKR/checkpoint falta → escribir `UNKNOWN`, no fabricar.

**Ejemplo**:
```
REQUEST: "Reconstruir el estado de mi estudio de Efesios capítulos 1–3"
SCOPE: Tres versículos + mapeos a tribus GEMA + colores de LUMINA
STATE_CURRENT: Fragmentos en 4 chats diferentes + Notion doc (SRC-001)
LAST_CHECKPOINT: "2026-08-10 - draft de mapeos Efesios 1:3–14 ↔ Asher + Topacio"
AVAILABLE_SOURCES: SRC-001 (Notion), SRC-002 (email), SRC-003 (current chat)
UNKNOWN: Autorización oficial para mutar colores de LUMINA
```

---

### B. REBUILD (Clasificación y Reconciliación)

**Input**: Hechos normalizados + source IDs  
**Output**: Estado reconstruido, conflictos documentados, autoridad verificada

Clasificar afirmaciones (no archivos completos) usando:

```
TRUTH_STATE_MODEL:
  - CANON: Verificado, autorizado, inmutable (ej. versículos oficiales)
  - VALIDATED: Verificado pero revisable (ej. mapeos GEMA aprobados por Gate E)
  - PROVISIONAL: Sugerido, pendiente aprobación (ej. draft de colores)
  - SCAFFOLD: Andamiaje solo para exploración (ej. esquema de trabajo)
  - RAW_LOG: Evidencia, no proceso validated (ej. chat history)
  - UNKNOWN: Inaccesible, sin autoridad, conflictivo
```

Reconciliar:
- **Duplicados exactos** vs. derivados vs. similares  
- **Lineage de versión**: qué supera a qué  
- **Contradicciones**: si "Efesios 1:3 → Asher" vs. "Efesios 1:3 → Judá", documentar ambas + marcar como conflicto  
- **Drift**: ¿cambió la interpretación de trib ↔ versículo desde el último checkpoint?  
- **Revocaciones explícitas**: "Ignore el mapeo anterior de..."  
- **Archivos faltantes**: "Esperaba SRC-004.json pero no está disponible"  

Asignar fuente a cada hecho material. Preservar claims en conflicto cuando la autoridad no puede resolverlos.

**Ejemplo**:
```
FACT: Efesios 1:3 ↔ Asher + Topacio
  CLASS: PROVISIONAL (draft propuesto por usuario; aprobación de Gate E pendiente)
  SOURCE: SRC-002 (email del 2026-08-09)
  CONFLICT: None
  LINEAGE: Supercede "Efesios 1:3 ↔ Judá" (SRC-001, obsoleto marcado 2026-08-08)

FACT: Color de LUMINA para Asher (Topacio) = #D4AF37
  CLASS: UNKNOWN (no se encontró autorización de LUMINA Brand Kit)
  SOURCE: None
  CONFLICT: Hex color posible pero no validado contra LUMINA canon
  ACTION: Escalate a Gate E para aprobación de color
```

---

### C. COMMIT (Creación de Paquete Commit-Ready)

**Input**: Estado reconstruido + clasificaciones + autoridad verificada  
**Output**: CMP (Context Management Packet) en 8 secciones, status `DRAFT_FOR_APPROVAL`

**CMP Structure**:

```
1. COMPRENSIÓN RECONSTRUIDA
   - Resumen ejecutivo del estado del proyecto
   - Alcance, objetivos, últimas decisiones
   - Línea de base: qué es conocido vs. UNKNOWN

2. ESTADO NORMALIZADO  
   - Hechos clasificados (CANON / VALIDATED / PROVISIONAL / UNKNOWN)
   - Mapeos bíblicos (versículos ↔ tribus ↔ gemas ↔ colores)
   - Timeline de decisiones

3. SOURCE/EVIDENCE BOUNDARIES
   - Tabla de fuentes (SRC-001…SRC-N con path, tipo, hash si aplica)
   - Autoridad de cada fuente (usuario, Gate E, sistema, UNKNOWN)
   - Accesibilidad (disponible / inaccesible / de solo lectura)

4. CONFLICTOS Y UNKNOWN
   - Lista de contradicciones sin resolver
   - Hechos clasificados UNKNOWN + bloqueadores
   - Escalaciones necesarias a Gate E/D

5. MÓDULO DUEÑO
   - Enrutamiento primario: opal-bos-skill (Bible Domain)
   - Dependencias secundarias: 
     * aeq-memory-intake-handoff-v01 (continuidad de contexto)
     * production-director-style (ejecución)
     * name-brand-identity-studio-v2 (LUMINA colors)

6. SIGUIENTE ACCIÓN SEGURA
   - Próximo paso sin ambigüedad
   - Quién ejecuta (usuario vs. Gate E vs. Gate D)
   - Bloqueadores y cómo resolverlos

7. CMP/HANDOFF COPY-PASTE
   - Paquete compacto, ejecutable sin contexto compartido
   - JSON o YAML estructurado
   - Hashsum para integridad

8. DELTA COMMIT-READY PARA GATE D
   - Cambios de esta sesión (vs. SRC-001 baseline)
   - Qué es nuevo, qué cambió, qué se revocó
   - Status: DRAFT_FOR_APPROVAL
   - Decisiones permitidas: ACCEPT / PATCH / REJECT
```

---

## III. Reglas de No-Mutación

**CRÍTICO**: Este skill **nunca mutará**:

1. **opal-bos-skill** — Leer solo; no editar ni integrar  
2. **Datos canónicos bíblicos** — Versículos, referencias  
3. **LUMINA Brand Canon** — Colores, tipografía, paleta oficial  
4. **Governance Gates** — No declarar canon sin Gate E; no mergear sin Gate D  
5. **Memory sources** — Citar, no reescribir  

Si detectas que una acción mutatoria es necesaria:
- Marca como `mutation_required: true`  
- Especifica **quién debe autorizar** (Gate E / Gate D / usuario)  
- Proporciona el handoff a ese módulo dueño  
- No proceder sin aprobación explícita  

---

## IV. Validación ÆQUITAS + QC

### ÆQUITAS (AQ1–AQ5) Checklist

Antes de entrega, validar:

| AQ | Criterio | Check |
|-----|----------|-------|
| AQ1 | **Fidelity** | ¿Cada hecho material tiene source ID verificable? |
| AQ2 | **Coherence** | ¿Contradicciones están documentadas o resueltas? |
| AQ3 | **Necessity** | ¿No hay scaffold inflado ni raw logs presentados como validados? |
| AQ4 | **Precision** | ¿CMP es ejecutable sin shared context? ¿Qué falta? |
| AQ5 | **Elegance** | ¿Estructura es clara, compacta, sin ruido? |

Si ALGÚN AQ falla → marcar `PASS: false` + explicar bloqueador + escalar.

### QC-01 a QC-10

```
QC-01 Schema Validity      → JSON/YAML parse sin error
QC-02 Duplicate Prevention  → Exactos vs. similares documentados
QC-03 Complete Mappings     → Versículos ↔ tribus ↔ gemas ↔ colores trazables
QC-04 Token Clarity         → SRC-IDs, GEMA IDs, versículo refs explícitos
QC-05 Changelog/Log         → Timeline de decisiones trazable
QC-06 Artifact Paths        → Dónde viven los deliverables (no asumidos)
QC-07 Batch Plan            → Trabajo secuenciado si es multiturno
QC-08 Descriptors Before    → Metadatos completos antes de renderización
QC-09 Script Sanity         → Si scripts incluidos, verificar localmente primero
QC-10 Rollback Plan         → Cómo revertir si falla commit
```

Si cualquier QC falla → `PASS: false` + fijar antes de entrega a Gate D.

---

## V. Autoridad de Decisión

| Decisión | Quién autoriza | Evidencia requerida |
|----------|----------------|-------------------|
| Cambiar mapeo versículo ↔ tribu | Gate E (Governance) | RFC + revisión exegética |
| Actualizar color LUMINA | Gate E + name-brand-identity-studio-v2 | Brand Kit oficial |
| Mergear cambios a Main | Gate D (QA/MERGE) | CMP + ÆQUITAS PASS + QC-01..10 PASS |
| Completar estudio (marcar CANON) | Gate E + opal-bos-skill | Todas las secciones validadas + auditoría final |

---

## VI. Ejemplo Breve: Flujo Completo

**User Input**:
> "Tengo fragmentos de mi estudio de Efesios 1:1–14 en 3 chats diferentes y un Notion doc. Quiero que lo reconstruyas y me hagas un paquete de entrega limpio para Gate D."

**NORMALIZE**:
```
REQUEST: Reconstruct Ephesians 1:1–14 study state
SCOPE: 3 chat histories + 1 Notion doc = SRC-001, SRC-002, SRC-003, SRC-004
TARGET: Clean CMP + Gate D handoff
AVAILABLE: All SRC files accessible
UNKNOWN: Authority to modify LUMINA colors if needed
```

**REBUILD**:
```
FOUND FACTS:
  - Eph 1:3 ↔ Asher + Topacio [CLASS: PROVISIONAL, SRC-002]
  - Eph 1:1 ↔ Judá + Diamante [CLASS: CANON, SRC-001 (Nota: verificado en Gate E aprobado 2026-08-01)]
  - Eph 1:14 ↔ Benjamin [CLASS: UNKNOWN, SRC-003, necesita verification]

CONFLICTS:
  - SRC-004 menciona Efesios 1:3 ↔ Judá (contradice SRC-002); revisor humano debe resolver

LINEAGE:
  - SRC-001 es baseline oficial (2026-08-01, Gate E approved)
  - SRC-002..004 son drafts post-baseline (pendiente reconciliation)
```

**COMMIT** → Generar CMP de 8 secciones:
```
1. COMPRENSIÓN: Estudio de Eph 1:1–14, mapeos GEMA x 3 versículos, 2 de CANON, 1 PROVISIONAL, 1 conflicto
2. ESTADO: Tabla de hechos clasificados + lineage
3. SOURCES: 4 archivos, 1 Gate E official, 3 drafts
4. CONFLICTOS: Eph 1:3 tribu ambigua, Eph 1:14 necesita verificación
5. MÓDULO DUEÑO: opal-bos-skill
6. SIGUIENTE ACCIÓN: Resolver conflicto Eph 1:3 con Gate E; verificar Eph 1:14
7. CMP JSON/YAML compacto + hash
8. DELTA: 3 cambios (2 nuevos mapeos, 1 conflicto detectado) vs. SRC-001 baseline
   STATUS: DRAFT_FOR_APPROVAL
   DECIDE: ACCEPT / PATCH / REJECT
```

**Entrega a Gate D**: "Candidato listo para review. Resolver 1 conflicto antes de merge. AQ1–AQ5 PASS. QC-01..10 PASS."

---

## VII. Integración con Ecosystem

### Leer Primero (Fuentes Canónicas)
- `/skills/opal-bos-skill` (authority de governance)
- `/skills/aeq-memory-intake-handoff-v01` (continuidad)
- `/projects/codex-gema` (mapeos oficiales tribu ↔ gema)
- `/brand/lumina-color-palettes.json` (colores LUMINA autorizados)

### Escribir Nunca Directamente A
- Main rama  
- Datos canónicos  
- Brand kit oficial  

### Delegar A
- **Gate E** (Capability Governance): decisiones de scope, autoridad, canonicidad  
- **Gate D** (QA/MERGE): validación final, merge, release  
- **production-director-style**: ejecución de artefactos si handoff se acepta  

---

## VIII. Known Limitations & Deferments

| Limitación | Por qué | Workaround |
|-----------|--------|-----------|
| No edita archivos en Main | Protege canon | Proporciona patch draftable → espera Gate D |
| No interpreta teología | Riesgo de desviación | Proporciona struct de datos → usuario interpreta |
| No accede KOP crudo | Cuarentena de datos | Usa MINIMIZED_EXTRACT con source IDs claros |
| No autodeploy a producción | Governance | Entrega CMP + espera aprobación Gate D |

---

## IX. Success Criteria

Skill ha ejecutado exitosamente si:

✅ User recibe CMP compacto, trazable, sin shared context requerido  
✅ Cada hecho tiene source ID o está marcado UNKNOWN  
✅ Conflictos documentados, no ocultados  
✅ ÆQUITAS AQ1–AQ5 pasan  
✅ QC-01..10 pasan  
✅ Próximo paso es claro (Gate E decision / Gate D merge)  
✅ No mutations a código, canon, ni governance gates  
✅ User puede handoff a otro LLM con CMP y ejecutar sin loss de contexto  

---

## X. Próximos Pasos

- [ ] Gate E review de autoridades y boundaries  
- [ ] Gate D validación QC completo  
- [ ] skill-creator: Optimizar triggering + performance  
- [ ] Production release (v1.0.0) si aprobado  
- [ ] Integración con CLI opal-bos-skill para activation automática  

---

**Document ID**: SKILL-BIBLE-CAND-001  
**Status**: CANDIDATE_UNVERIFIED  
**Ready for**: Gate E → Gate D review  
**Last updated**: 2026-08-11T09:06:48-06:00
