# Guía de Estilo Editorial — Sistema BOS · Rama LUMINA/Editorial
**Versión:** 2.0 · Iridiscenter  
**Rama:** LUMINA → Pilar Editorial  
**Estado:** Canónico · Operativo  

---

## 1. Fundamento — De dónde viene este estilo

El estilo editorial BOS no es una elección estética arbitraria.  
Se deriva directamente de la arquitectura del metasistema:

| Dominio BOS | Función | Aporte al estilo editorial |
|-------------|---------|---------------------------|
| **BOS** (núcleo) | Integración metaanalítica | Rigor estructural, coherencia interna |
| **OPAL** | Símbolos y narrativa | Lenguaje simbólico, arquetipos como anclas |
| **LUMINA** | Percepción estética | Paleta, tipografía, respiración visual |
| **ÆQUITAS** | Ética y filosofía | Tono invitacional, no prescriptivo |

Todo producto editorial es una instancia de LUMINA aplicada.  
Toda pieza debe poder auditarse contra los cuatro dominios.

---

## 2. Voz editorial — Tono unificado

### Principios de voz

La voz BOS/LUMINA es **unitaria**: no cambia entre líneas (Literatura, Clínica, Espiritual).  
Lo que cambia es el vocabulario de cada dominio — no el registro.

**El registro es siempre:**
- Contemplativo sin ser pasivo
- Riguroso sin ser académico
- Poético sin ser abstracto
- Invitacional: pregunta, no ordena
- Encarnado: lo espiritual es siempre somático, concreto

**Se evita:**
- Lenguaje prescriptivo ("debes", "tienes que")
- Tono terapéutico condescendiente
- Abstracción sin anclaje ("el universo te llama")
- Urgencia comercial
- Saturación de texto en una sola pieza

### Patrones de construcción de frase

| Patrón | Ejemplo |
|--------|---------|
| Pregunta abierta | *¿Qué permanece cuando todo lo demás cambia?* |
| Afirmación breve + espacio | *El silencio también es respuesta.* [línea] |
| Cita + contexto mínimo | *"En el principio..." — Génesis 1:1 · El punto de partida.* |
| Instrucción suave | *Escribe sin filtro. Borra si quieres. Vuelve si necesitas.* |
| Puente simbólico | *El árbol no fuerza su crecimiento. Tú tampoco tienes que.* |

---

## 3. Sistema cromático LUMINA — Paleta canónica v2

### Paleta base (todas las líneas)

| Token | Hex | Uso |
|-------|-----|-----|
| `--lum-primary` | `#2C1810` | Texto principal, titulares |
| `--lum-bg` | `#F5F0EC` | Fondo, papel |
| `--lum-accent` | `#8A8680` | Citas, footers, elementos secundarios |
| `--lum-bridge` | `#D7DEE8` | Transiciones, elementos de puente BOS |

### Acentos por línea editorial (v2 — ajustados)

| Línea | Token | Hex | Evocación |
|-------|-------|-----|-----------|
| **Literatura** | `--lum-lit` | `#CC9C50` | Pergamino, manuscrito, oro antiguo |
| **Clínica** | `--lum-cli` | `#7B9EA6` | Agua quieta, claridad, contención |
| **Espiritual** | `--lum-esp` | `#1E7D63` | Naturaleza, umbral, lo sagrado encarnado |

> **Ajuste v2 — Clínica:** el fucsia `#E31A7E` original se reemplaza por `#7B9EA6`.  
> El fucsia tiene carga de marketing. El azul-gris evoca contención clínica auténtica, alineada con LUMINA.

### Reglas de uso cromático

1. Fondo siempre `#F5F0EC`
2. Texto principal `#2C1810`
3. Acento de línea solo en: título de sección, símbolo decorativo, separador
4. Nunca: gradientes, sombras, fondos de color en bloques de texto

---

## 4. Tipografía — Sistema completo

| Rol | Familia | Peso | Tamaño | Uso |
|-----|---------|------|--------|-----|
| Titular principal | Playfair Display | 700 | 22–24pt | Nombre del cuadernillo |
| Subtítulo sección | Playfair Display | 400 italic | 16–18pt | Encabezados de sección |
| Encabezado entry | Playfair Display | 600 | 14pt | Título de cada entry |
| Cuerpo | Montserrat | 400 | 11pt | Contenido, instrucciones |
| Citas / referencias | Georgia | 400 italic | 10pt | Textos sagrados, citas |
| Footer / metadata | Montserrat | 300 | 8pt | Número de página, versión |
| Prompts de reflexión | Montserrat | 500 italic | 11pt | Preguntas al lector |

### Interlineado y respiración

- Párrafo a párrafo: 1.4 (generoso)
- Sección a sección: espacio equivalente a 2.5 líneas vacías
- Espacios de escritura: mínimo 6 líneas por bloque, máximo 12

---

## 5. Arquitectura de página — Estructura visual canónica

```
┌─────────────────────────────────────────────────────────────┐
│ HEADER (15%)                                                 │
│ Línea · Sección # · Entry # · Semana                        │
├──────────────────────────┬──────────────────────────────────┤
│ COLUMNA IZQUIERDA (45%)  │ COLUMNA DERECHA (55%)            │
│                          │                                   │
│ Título del entry         │ ________________________         │
│ Texto / Referencia       │ ________________________         │
│ Contexto simbólico       │                                   │
│                          │ [zona escritura libre]           │
│ › Prompt reflexivo       │                                   │
│   (en cursiva)           │ ________________________         │
│                          │                                   │
│ ⊙                        │ [zona dibujo — solo Espiritual]  │
├──────────────────────────┴──────────────────────────────────┤
│ FOOTER (5%)                                                  │
│ Página # | Cuadernillo — [Línea] | Iridiscenter · LUMINA   │
└─────────────────────────────────────────────────────────────┘
```

---

## 6. Iconografía mínima

| Token | Símbolo | Uso |
|-------|---------|-----|
| Inicio de sección | `▪` | Antes de título de sección |
| Separador | `—` | Em-dash centrado entre entries |
| Indicador de prompt | `›` | Antes de la pregunta reflexiva |
| Cierre de reflexión | `⊙` | Al final de cada entry |
| Zona de escritura | `________________________` | Líneas para escribir |

---

## 7. Niveles de interactividad

| Nivel | Nombre | Líneas | Elementos |
|-------|--------|--------|-----------|
| 1 | Escritura | Todas | Líneas de escritura, espacios abiertos |
| 2 | Visual | Espiritual (primario), Literatura (secundario) | Zonas para dibujar, símbolos |
| 3 | Wreck It Journal | Todas (opcional) | Tachar, doblar, romper — instrucción explícita |

Las instrucciones WIJ se marcan con `[WIJ]` en el contenido.

---

## 8. Criterios de auditoría ÆQUITAS

| Criterio | Pregunta | Resultado |
|----------|----------|-----------|
| **Fidelity** | ¿El tono es fiel a la voz LUMINA? | PASS / REVISIÓN / FAIL |
| **Coherence** | ¿Paleta, tipo y estructura son consistentes? | PASS / REVISIÓN / FAIL |
| **Necessity** | ¿Cada elemento tiene función? | PASS / REVISIÓN / FAIL |
| **Precision** | ¿Las preguntas son abiertas pero específicas? | PASS / REVISIÓN / FAIL |
| **Elegance** | ¿La página respira? ¿Hay saturación? | PASS / REVISIÓN / FAIL |

---

*Guía de Estilo Editorial BOS · LUMINA/Editorial v2.0 · Iridiscenter · 2026*
