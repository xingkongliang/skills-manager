---
name: brand-identity-studio
description: >
  Crea sistemas completos de identidad de marca: paleta de colores oficial, tipografía,
  manual de marca, hoja membretada, tarjeta de presentación, firma de email, social media
  kit, brand guidelines y más — todo desde un cuestionario interactivo. ACTIVAR SIEMPRE
  cuando el usuario mencione: branding, identidad visual, manual de marca, papelería
  corporativa, colores oficiales, logo, guía de estilo, kit de marca, diseño corporativo,
  o cuando cualquier artefacto necesite aplicar el look-and-feel de una organización.
  También activar si el usuario pide "algo que se vea profesional / con mi branding /
  con los colores de mi empresa". Interacción principal en español latino mexicano.
---

# Brand Identity Studio

Genera el sistema completo de identidad visual a través de un cuestionario interactivo,
seguido de la producción automática del bundle de entregables.

```
CUESTIONARIO → BRAND TOKENS → GENERACIÓN → ENTREGA
```

---

## FASE 1 — Cuestionario Interactivo

Lee `references/questionnaire.md` para el flujo completo de preguntas.

**Principios:**
- Máximo 5–7 preguntas por bloque; aceptar respuestas cortas e inferir el resto
- Si el usuario ya tiene logo, colores o tipografía → respetarlos como base, no reinventar
- Si omite una respuesta → usar el default del esquema (`references/brand-tokens-schema.md`)
- Confirmar resumen en tabla antes de generar (ver BLOQUE 6 del cuestionario)

**Bloques:**
1. Datos de la organización (nombre, industria, misión, país/región, logo existente)
2. Audiencia y posicionamiento (tipo de cliente, tono, valores)
3. Identidad visual (colores existentes o por inferir, tipografía, referencias)
4. Información de contacto para papelería
5. Checklist de entregables requeridos

---

## FASE 2 — Brand Tokens

Lee `references/brand-tokens-schema.md` para la estructura de datos.

Procesar las respuestas del cuestionario y construir el objeto de tokens que alimentará
todos los entregables: colores primario / secundario / acento / neutro con HEX + RGB +
CMYK, par tipográfico, datos de contacto, personalidad y valores de marca.

---

## FASE 3 — Bundle de Entregables

Lee `references/deliverables-catalog.md` para las especificaciones técnicas de cada pieza.

### Entregables estándar (siempre generados)

| Entregable | Formato |
|---|---|
| Brand Kit One-Pager | PDF / HTML |
| Manual de Identidad de Marca | DOCX / PDF |
| Brand Guidelines | HTML / MD |
| Hoja Membretada (carta + A4) | DOCX / PDF |
| Tarjeta de Presentación (frente + reverso) | PDF / HTML |
| Paleta de Color Oficial (HEX, RGB, CMYK) | HTML / PDF |
| Guía Tipográfica con specimens | HTML / PDF |
| Plantilla de Documento Institucional | DOCX |
| Firma de Correo Electrónico | HTML |
| Social Media Kit (banners + avatar) | HTML |

### Entregables opcionales (preguntar al usuario)

- Presentación corporativa master (PPTX, 10 slides)
- Plan de proyecto PMBOK — leer `references/pmbok-brand-plan.md`
- Swimlane diagram del proceso (siempre incluido como bonus)
- Resumen ejecutivo de la encuesta (siempre incluido)

---

## FASE 4 — Generación

Ejecutar `scripts/generate_bundle.py` para instanciar los brand tokens y producir
cada entregable.

Para entregables que requieren skills complementarias:

| Formato | Skill a leer |
|---|---|
| PPTX | `/mnt/skills/public/pptx/SKILL.md` |
| DOCX | `/mnt/skills/public/docx/SKILL.md` |
| PDF | `/mnt/skills/public/pdf/SKILL.md` |
| Diseño visual / canvas | `/mnt/skills/examples/canvas-design/SKILL.md` |
| HTML / componentes | `/mnt/skills/public/frontend-design/SKILL.md` |

---

## FASE 5 — Entrega Final

Presentar en este orden:

1. **Resumen ejecutivo** — tabla con todas las decisiones capturadas
2. **Swimlane diagram** — visualización del proceso (Visualizer / Mermaid)
3. **Bundle de archivos** — `present_files` con todos los entregables
4. **Informe de marca** — justificación breve de las decisiones creativas clave
5. **Next steps / PMBOK** — solo si el usuario solicitó gestión formal del proyecto

---

## Notas de implementación

- **Idioma:** Cuestionario en español. Entregables en el idioma que el cliente defina.
- **Inferencia inteligente:** Industria → paleta estándar del sector. Sin preferencia tipográfica → sugerir par Google Fonts según personalidad. Ver tablas en `references/questionnaire.md`.
- **No duplicar trabajo:** Si el usuario ya tiene elementos definidos, construir sobre ellos.

---

## Archivos de referencia

| Archivo | Cuándo leerlo |
|---|---|
| `references/questionnaire.md` | Al iniciar — preguntas, lógica de flujo e inferencias |
| `references/brand-tokens-schema.md` | Al procesar respuestas — estructura de datos de marca |
| `references/deliverables-catalog.md` | Al generar — specs técnicas de cada entregable |
| `references/pmbok-brand-plan.md` | Solo si el usuario solicita plan de proyecto formal |
| `scripts/generate_bundle.py` | Script principal de generación del bundle |
