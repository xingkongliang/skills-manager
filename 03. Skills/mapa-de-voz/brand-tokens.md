# Mapa de Voz — Brand Tokens LUMINA
*Referencia canónica · Lazy-load en Fase 1 y Fase 3*
*Fuente: LUMINA canonical design system · Iridiscenter BOS*

---

## Paleta canónica LUMINA

| Token | Hex | Nombre | Uso en Mapa de Voz |
|-------|-----|--------|-------------------|
| `--oro-rb` | `#CC9C50` | Oro RB | TAG labels, CTA activo, barra dominante, HR accent |
| `--rose-gold` | `#C48B7A` | Rose Gold | Perfil 1 (Borde), score bars secundarios, hint text |
| `--crema` | `#F5F1EC` | Crema | Background principal (modo LUMINA) |
| `--carbon` | `#202428` | Carbón | Perfil 3 (Testigo), texto primario, fondo cards |

**Paleta extendida del componente original (mantener en modo non-LUMINA):**
```js
const G = {
  bg:        '#0C0B0F',                    // negro profundo
  card:      '#131218',                    // card background
  border:    '#252330',                    // bordes, barras inactivas
  gold:      '#D4A843',                    // accent original (≈ Oro RB)
  goldFaint: 'rgba(212,168,67,0.1)',       // selected option bg
  goldLight: '#E8C876',                    // selected option text
  text:      '#F2EDE4',                    // texto legible (≈ Crema)
  dim:       '#8A8490',                    // texto secundario
  accent:    '#3D3560',                    // barras no dominantes
}
```

---

## Tipografía canónica LUMINA

| Rol | Fuente | Peso | Uso |
|-----|--------|------|-----|
| Display / Headings | Playfair Display | 400, 700, italic | H1, nombre del perfil, títulos de sección |
| Body / UI | Montserrat | 400, 600 | Cuerpo, opciones, botones, labels |

**En el componente actual:**
- Cormorant Garamond (display) → puede migrarse a Playfair Display
- Lora (body/UI) → puede migrarse a Montserrat

**Regla de migración:**
```
Si brand_mode === "lumina_canonical":
  font_display = "Playfair Display"
  font_body    = "Montserrat"
Else:
  font_display = "Cormorant Garamond"  (original del componente)
  font_body    = "Lora"
```

---

## Mapping paleta perfil → LUMINA

| Perfil | Color original | Color LUMINA | Justificación |
|--------|--------------|--------------|---------------|
| Fuego Interior | `#E8836A` coral | `#CC9C50` Oro RB | Calidez, temperatura emocional, presencia |
| Voz del Borde | `#7B9EE0` azul | `#C48B7A` Rose Gold | Rareza, distinción, elegancia oblicua |
| Voz del Pulso | `#6EC4A8` teal | `#F5F1EC` Crema | Cadencia, respiración, flujo contenido |
| Testigo Íntimo | `#C4837A` rosa | `#202428` Carbón | Profundidad, peso, presencia silenciosa |
| Voz del Saltador | `#A889D4` violeta | `#CC9C50` Oro RB | Valentía, apuesta, brillo arriesgado |

---

## Tokens de tipografía por modo de entrega

### Modo artefacto React (brand_mode: lumina_canonical)
```js
// Google Fonts import string
"Playfair+Display:ital,wght@0,400;0,700;1,400&family=Montserrat:wght@400;600"

// Variables de uso
const FONT_DISPLAY = "'Playfair Display', Georgia, serif"
const FONT_BODY    = "'Montserrat', system-ui, sans-serif"

// Escalas tipográficas
TAG:  { font: FONT_DISPLAY, size: '10px', letterSpacing: '0.22em', color: '#CC9C50' }
H1:   { font: FONT_DISPLAY, size: 'clamp(2.1rem, 6vw, 2.9rem)', weight: 700 }
BODY: { font: FONT_BODY,    size: '0.88rem', lineHeight: 1.85 }
BTN:  { font: FONT_BODY,    size: '0.87rem', letterSpacing: '0.04em' }
```

### Modo documento (PDF/DOCX output)
```
Display: Playfair Display 18–28pt, regular/italic
Body:    Montserrat 11–13pt, regular
Labels:  Montserrat 8–9pt, bold, uppercase, letterSpacing 0.15em
Leading: 1.5
Margins: 3cm
Page:    A4
```

---

## Voice tokens por perfil (para brand_tokens output)

### Fuego Interior
```json
[
  { "token": "Temperatura antes que estructura",   "source": "Intensidad" },
  { "token": "Presencia emocional directa",        "source": "Intensidad" },
  { "token": "Mostrar sin explicar",               "source": "Intensidad" }
]
```

### Voz del Borde
```json
[
  { "token": "Ángulo no tomado",                   "source": "Originalidad" },
  { "token": "Mezcla de registros sin disculpa",   "source": "Originalidad" },
  { "token": "Territorio inexplorado como hogar",  "source": "Originalidad" }
]
```

### Voz del Pulso
```json
[
  { "token": "El ritmo es el argumento",           "source": "Ritmo" },
  { "token": "Cadencia antes que corrección",      "source": "Ritmo" },
  { "token": "El sonido precede al sentido",       "source": "Ritmo" }
]
```

### Testigo Íntimo
```json
[
  { "token": "Detalle concreto como acceso",       "source": "Perspectiva" },
  { "token": "Lo privado como puerta a lo universal", "source": "Perspectiva" },
  { "token": "Precisión interior sin adorno",      "source": "Perspectiva" }
]
```

### Voz del Saltador
```json
[
  { "token": "Valentía como método",               "source": "Riesgo" },
  { "token": "La incomodidad como señal correcta", "source": "Riesgo" },
  { "token": "Simplicidad como acto más difícil",  "source": "Riesgo" }
]
```

---

## QA visual (checklist Fase 1)

```
QV-01  Fuentes cargadas correctamente desde Google Fonts CDN
QV-02  Color dominante del perfil aplicado a radar stroke + score bar
QV-03  TAG labels en Oro RB (#CC9C50) o D4A843 según brand_mode
QV-04  Background legible: Crema (#F5F1EC) o near-black (#0C0B0F)
QV-05  Contraste texto/fondo ≥ WCAG AA (4.5:1 mínimo)
QV-06  RadarChart fillOpacity ≤ 0.15 (no oscurece el grid)
QV-07  Score bar dominante diferenciada visualmente de las demás
QV-08  Ejercicios con borde izquierdo en color del perfil
QV-09  Botones CTA activos visualmente distinguibles de deshabilitados
QV-10  Transition fade 220ms aplicada en cambio de fase
```
