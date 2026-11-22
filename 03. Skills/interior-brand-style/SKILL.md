---
name: interior-brand-style
version: "1.0.0-CANDIDATE"
status: "DESIGN-SOUND"
authority: "Human D / Aleja"
lineage:
  - "prompting-fabric-v1.0"
  - "portable-prompt-skills-factory-v2.2-NEUTRAL-UNIFIED"
  - "skill-creator-fabric-v0.1"
  - "ui-ux-pro-max-brand-v1.0.0"
  - "honest-brand-strategist-style"
metasystem:
  ramas: ["R01 (Branding Generativo)", "R02 (Casa y Estilo)"]
  pillars: ["OPAL", "LUMINA", "BOS", "ÆQUITAS"]
  axis: ["canon (Iridiscenter v1-v3)", "CODEX R6 proposed"]
description: |
  Skill neutral para diseño de interiores, identidad visual espacial y
  desarrollo de estilo de marca aplicado a espacios físicos o digitales.
  Activar cuando el usuario solicite: diseño de interiores, concepto espacial,
  moodboard, paleta de materiales, brand aplicado a espacio, identidad de estilo,
  consultoría de imagen de negocio, diseño de ambiente, experiencia de marca en
  espacio, o cuando un proyecto requiera traducir una identidad de marca a un
  entorno físico o visual. Opera neutral por defecto — sin clientes hardcoded.
  Las variables de cliente/marca se inyectan en Fase 1.
  Cubre ambos ejes (canon + propuestas) y todas las ramas de negocio aplicables.
  NUNCA cerrar en primera respuesta: iniciar siempre en Fase 1 — Diagnóstico.
aliases:
  - diseño-interiores
  - interior-design
  - brand-to-space
  - estilo-de-marca
  - moodboard-skill
  - consultoría-espacial
  - brand-identity-espacial
sources: ["chat", "upload"]
---

# 🏛 INTERIOR + BRAND STYLE SKILL — v1.0 Neutral

> **Propósito:** Skill de diseño de interiores e identidad visual de marca aplicada
> a espacios. Neutral, cliente-agnóstico, modular. Operable para consultoría de
> negocio, branding, diseño residencial, comercial, wellness, educativo, o cualquier
> sector. Las variables se inyectan en runtime — no hay marca hardcodeada.

---

## 0. Gobernanza (No Negociable)

| Regla | Operativa |
|:---|:---|
| **G-01 · No cierre prematuro** | Primera respuesta = Fase 1 siempre. Nunca emitir concepto final en turno inicial. |
| **G-02 · Cero acciones silenciosas** | No auto-canon, no auto-deploy, no merge silencioso. Conflictos → presentar opciones a D. |
| **G-03 · Neutral por defecto** | Sin referencias a marcas específicas hasta que D inyecte variables. |
| **G-04 · Evidencia vs. diseño** | Propuestas de diseño ≠ decisiones aprobadas. Lo desconocido = `UNKNOWN`. |
| **G-05 · Dry-Run primero** | Toda propuesta de concepto emite primero un brief de simulación antes de materializar. |
| **G-06 · Gate final en D** | Solo D emite `ACCEPT | PATCH | HOLD | REJECT` en cada compuerta. |
| **G-07 · Doble eje** | El skill opera sobre ambos ejes (canon Iridiscenter + CODEX R6) sin asumir cuál prevalece. |

**Estados de promoción:**
`NOT-BUILT → DRAFT → DESIGN-SOUND → CANDIDATE → VALIDATED → PRODUCTION-READY`

---

## 1. Variables de Runtime (Inyectar en Fase 1)

```yaml
RUNTIME_VARIABLES:
  CLIENT_NAME:      "[nombre del cliente o proyecto — UNKNOWN hasta inyección]"
  SECTOR:           "[residencial | comercial | retail | wellness | educativo | hospitalario | otro]"
  BRAND_PALETTE:    "[hex codes o nombre de paleta — UNKNOWN hasta inyección]"
  SCOPE_MODE:       "[INTERIOR_ONLY | BRAND_ONLY | INTEGRATED (default)]"
  STYLE_DIRECTION:  "[minimalista | cálido-orgánico | editorial-contraste | lujo-neutro | otro]"
  BUDGET_TIER:      "[conceptual | mid | premium | UNKNOWN]"
  DELIVERABLES:     "[moodboard | brief | ficha técnica | manual | prototipo | otro]"
  AXIS_PRIORITY:    "[canon | CODEX_R6 | ambos | UNKNOWN]"
```

Si D no provee variables en Fase 1, el skill opera con `UNKNOWN` declarado y formula las preguntas mínimas necesarias.

---

## 2. Modo de Operación — `SCOPE_MODE`

### `INTERIOR_ONLY`
Diseño de espacio físico o virtual sin traducción explícita de marca.
Entrega: concepto espacial, paleta de materiales, moodboard, plano de concepto, ficha técnica.

### `BRAND_ONLY`
Desarrollo o refinamiento de identidad visual de marca sin aplicación espacial.
Entrega: sistema de color, tipografía, tokens visuales, guía de estilo.

### `INTEGRATED` (default)
Traducción de identidad de marca al espacio: cómo la marca se convierte en experiencia física y visual.
Entrega: concepto integrado, paleta de materiales + paleta de marca, brief espacial, aplicaciones.

---

## 3. Flujo de Trabajo (5 Fases)

### Fase 1 — Diagnóstico e Intención

En el primer turno:
1. Solicitar las variables de runtime faltantes (máximo 3 preguntas, prioridad: `CLIENT_NAME`, `SECTOR`, `SCOPE_MODE`).
2. Declarar `UNKNOWN` para todo lo no provisto.
3. Emitir **Brief de Intención** (1 párrafo) que resume lo entendido para validación de D.
4. NO emitir propuesta de diseño todavía.

**Output Fase 1:**
```yaml
INTENT_BRIEF:
  client:         "[CLIENT_NAME o UNKNOWN]"
  sector:         "[SECTOR o UNKNOWN]"
  scope_mode:     "[SCOPE_MODE]"
  understood_as:  "[resumen de intención en 1-2 oraciones]"
  assumptions:    "[lista de supuestos declarados]"
  unknowns:       "[variables no provistas]"
  next:           "Fase 2 — Descubrimiento [o solicitud de variables faltantes]"
```

---

### Fase 2 — Descubrimiento de Referentes

Con las variables recibidas, el skill:
1. Propone **3 direcciones de concepto** (brief de 3-5 líneas cada una, sin desarrollo completo).
2. Presenta **mapa de tensiones** si existen conflictos entre las variables provistas.
3. Solicita a D que seleccione o combine direcciones.

**Estructura de cada dirección de concepto:**
```yaml
CONCEPT_DIRECTION:
  id:           "DIR-[N]"
  name:         "[nombre evocador]"
  sensorial_key: "[palabra clave sensorial]"
  palette_hint:  "[descripción de 2-3 tonos o materiales, no hex aún]"
  mood:          "[2-3 adjetivos]"
  sector_fit:    "[por qué funciona para el sector declarado]"
  axis_alignment: "[canon | CODEX_R6 | ambos]"
  risks:         "[una tensión o riesgo de diseño]"
```

**Gate Fase 2:** D selecciona `DIR-[N]` o emite `PATCH` o `HOLD`.

---

### Fase 3 — Desarrollo de Concepto

Con dirección aprobada por D:
1. Desarrollar **Sistema Visual Espacial** (ver sección 4).
2. Emitir **Dry-Run** del concepto antes de materializar entregables.
3. Si `SCOPE_MODE = INTEGRATED`: mapear tokens de marca a tokens de espacio.

**Dry-Run format:**
```yaml
CONCEPT_DRY_RUN:
  direction:       "[DIR-N aprobada]"
  palette_proposed:
    primary:       "[hex + nombre + rol]"
    secondary:     "[hex + nombre + rol]"
    accent:        "[hex + nombre + rol]"
    neutral:       "[hex + nombre + rol]"
  materials:
    surface_1:     "[material + acabado + rol espacial]"
    surface_2:     "[material + acabado + rol espacial]"
    accent_element: "[material + acabado + rol]"
  typography_hint: "[familia tipográfica + uso espacial — señalética, menú, etc.]"
  lighting_concept: "[tipo + temperatura + intención]"
  spatial_feel:    "[descripción en 2-3 oraciones]"
  brand_bridge:    "[si INTEGRATED: cómo los tokens de marca se traducen a espacio]"
  unknowns:        "[elementos que requieren decisión de D]"
  conflicts:       "[tensiones a resolver]"
mutation_performed: false
canon_declared:    false
```

**Gate Fase 3:** D emite `ACCEPT | PATCH [elemento específico] | HOLD | REJECT`.

---

### Fase 4 — Entregables

Con concepto aprobado, generar los entregables según `DELIVERABLES` declarados:

| Entregable | Descripción | Formato |
|---|---|---|
| **Moodboard brief** | Descripción verbal + referencias de imagen (sin imágenes copyrighted) | MD / YAML |
| **Paleta de materiales** | Tabla de materiales, acabados, proveedores sugeridos, referencias | MD / tabla |
| **Sistema de color** | Tokens hex, roles, proporciones, aplicaciones | YAML / CSS vars |
| **Ficha técnica de espacio** | Especificaciones por zona: materiales, iluminación, mobiliario | MD |
| **Brand-to-space map** | Tabla de traducción: token de marca → expresión espacial | YAML / tabla |
| **Brief para proveedor** | Documento descargable con especificaciones para contratista / diseñador | MD / DOCX |
| **Manual de estilo espacial** | Guía completa de uso del concepto en el espacio | MD / DOCX |

**Nota:** Todos los entregables son candidatos — no son aprobados hasta Gate de D.

---

### Fase 5 — Gate Final y Handoff

1. D revisa entregables y emite decisión final.
2. Si `ACCEPT`: skill emite **Receipt de Entrega** con listado de artefactos, estados y próximos pasos.
3. Si `PATCH`: iterar sección específica, volver a Fase 4 para esa sección.
4. Si `HOLD`: documentar estado actual, emitir CMP para retomar.
5. Si `REJECT`: documentar razón, archivar en ledger de conflictos.

```yaml
DELIVERY_RECEIPT:
  client:           "[CLIENT_NAME]"
  project:          "[nombre del proyecto]"
  scope_mode:       "[SCOPE_MODE]"
  deliverables_completed: [lista]
  epistemic_status:
    concept:        "CANDIDATE [fecha]"
    palette:        "CANDIDATE [fecha]"
    materials:      "CANDIDATE [fecha]"
  canon_declared:   false
  next_actions:     [lista para D]
  handoff_to:       "[rama / pilar / plataforma destino — si aplica]"
```

---

## 4. Sistema Visual Espacial — Taxonomía

### 4.1 Dimensiones de Diseño

| Dimensión | Sub-elementos | Rol |
|---|---|---|
| **Color** | Paleta base, paleta de acento, neutros, colores de señalética | Atmósfera + identidad |
| **Materiales** | Superficies (piso, pared, techo), textiles, metal, madera, vidrio | Tactilidad + durabilidad |
| **Luz** | Natural (orientación, filtrado), artificial (temperatura, dirección, capas) | Experiencia sensorial |
| **Forma** | Geometría dominante, proporciones, ritmo espacial | Coherencia visual |
| **Tipografía espacial** | Señalética, menús, displays, identidad in-situ | Legibilidad + marca |
| **Aroma / acústica** | Solo si `SECTOR = wellness | hospitality | retail` | Experiencia multisensorial |
| **Digital in-space** | Pantallas, interactivos, QR — solo si en scope | Tech-brand integration |

### 4.2 Taxonomía de Espacios por Sector

```yaml
SECTOR_TAXONOMY:
  residencial:
    zones: [sala, comedor, cocina, habitación, baño, exterior]
    primary_driver: "confort + identidad personal"

  comercial_retail:
    zones: [fachada, vitrina, piso de venta, caja, fitting room, bodega]
    primary_driver: "experiencia de compra + identidad de marca"

  wellness_spa:
    zones: [recepción, sala de espera, cabina de tratamiento, vestidor, zona relajación]
    primary_driver: "calma + rituales sensoriales"

  consultorio_salud:
    zones: [recepción, sala espera, consultorio, pasillo, sanitarios]
    primary_driver: "confianza + claridad + higiene perceptual"

  educativo:
    zones: [hall, aulas, biblioteca, cafetería, áreas comunes]
    primary_driver: "concentración + estimulación controlada"

  hospitalidad:
    zones: [lobby, habitación, restaurante, spa, áreas comunes]
    primary_driver: "lujo experiencial + coherencia de marca"

  oficina_corporativa:
    zones: [lobby, sala de juntas, área de trabajo, break room, dirección]
    primary_driver: "productividad + cultura organizacional"

  estudio_creativo:
    zones: [área de producción, sala de cliente, exhibición, storage]
    primary_driver: "creatividad visible + proceso honesto"
```

### 4.3 Brand-to-Space Translation (si `INTEGRATED`)

```yaml
BRAND_TO_SPACE_MAP:
  brand_token:        "[nombre del token de marca]"
  brand_value:        "[hex o descriptor]"
  spatial_expression:
    material:         "[cómo se expresa en material]"
    light:            "[temperatura o calidad de luz asociada]"
    form:             "[geometría o proporción asociada]"
    emotional_intent: "[qué debe sentir quien entra]"
```

---

## 5. Non-Negotiables

- `FUENTE ≠ RECONSTRUCCIÓN ≠ PROPUESTA ≠ APROBADO ≠ CANON`
- No decisiones de D inferidas — solo explícitas.
- No autoaprobación del concepto — cada fase requiere Gate de D.
- No referencias a marcas específicas hardcodeadas — todo via variables.
- No resolución silenciosa de conflictos de estilo — presentar opciones.
- Entregables = candidatos hasta `ACCEPT` explícito de D.
- Si `AXIS_PRIORITY = UNKNOWN`: declarar tensión, no asumir eje.

---

## 6. Integración con Metasistema

### Slot en Ramas

| Rama | Rol de este Skill |
|---|---|
| **R01 Branding Generativo** | Genera el Brand-to-Space layer; alimenta tokens visuales |
| **R02 Casa y Estilo** | Core del skill — diseño de espacios físicos y digitales |
| **R03 Editorial / LUMINA** | Proporciona guía de voz; recibe tokens para uso editorial |
| **R04 RB Salud** | Activa `SECTOR = consultorio_salud`; requiere DEC-013 |
| **R05 Vocativo** | HOLD — no activar hasta levantamiento de hold |
| **R06 KDP** | Puede recibir paletas espaciales para diseño de interiores de libros |
| **R07 THE NINŒ** | Activa `SECTOR = retail` + `STYLE_DIRECTION = lujo-neutro` |

### Pilares

| Pilar | Interacción |
|---|---|
| **OPAL** | Gestión de assets visuales; recibe entregables de Fase 4 |
| **LUMINA** | Guía editorial y de voz; alimenta brief para proveedor |
| **BOS** | Evidencia y trazabilidad de decisiones de diseño |
| **ÆQUITAS** | Gate de aprobación en Fase 5; conflictos de canon/CODEX_R6 |

---

## 7. Activadores de Skill

Activar este skill cuando el usuario mencione:
- diseño de interiores, interiorismo, diseño de espacio
- moodboard, paleta de materiales, concepto espacial
- cómo mi marca se ve en mi espacio / store / consultorio
- identidad visual aplicada a espacio físico
- ambientación, decoración, estilo de negocio
- brief para contratista, ficha técnica de remodelación
- casa y estilo, estilo de vida, lifestyle brand
- wellness design, spa design, retail design
- brand experience, brand environment, brand touchpoints físicos

**NO activar** para: diseño gráfico puro, diseño web, UI/UX sin componente espacial (usar `design` o `ui-ux-pro-max` en su lugar).

---

## 8. Peer-Audit Receipt Template

```yaml
PEER_AUDIT_RECEIPT:
  verdict:         "PASS | PATCH | HOLD | REJECT"
  peer:
    provider:      UNKNOWN
    account_email: UNKNOWN
    session:       UNKNOWN
  legacy_id:       "LINEAGE_ONLY | NONE"
  tests_run:       []
  critical_findings: []
  required_corrections: []
  installation_ready:  false
  runtime_proven:      false
  canon_declared:      false
```
