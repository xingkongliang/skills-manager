# Adaptador candidato · R06 × R07 · DE MANO

Estado: `CANDIDATE_ADAPTER · NON_CANON · REQUIRES_LIVE_SCHEMA_CHECK`

## Alcance

Adaptar DIVIDE al sistema:

> **DE MANO · Sistema portátil de perfume y cuidado personal**

Usar como ocasión secundaria Viaje, no como identidad principal.

## Contexto confirmado

- Proyecto: `R06_R07_DE_MANO`
- Casos de uso: uso diario, bolso/mochila, oficina, evento, regalo, emergencia personal y viaje.
- Objetos operativos confirmados en el contexto: PIR y RPR; mapear otros objetos únicamente después de inspeccionar el schema vivo.
- Regla protegida: `PROPOSAL_ELIGIBLE ≠ SALE_APPROVED`.
- Los hallazgos DIVIDE entran como candidatos; no modifican ni aprueban automáticamente PIR, RPR u objetos comerciales.

## Mapeo técnico candidato

| Registro DIVIDE | Pestaña propuesta |
|---|---|
| `DI_SIGNAL` | `MI_AUDIENCE_SIGNAL` |
| `VI_VENDOR` | `MI_SUPPLIER` |
| `EVIDENCE_RECORD` | `MI_EVIDENCE` |
| `DE_OPPORTUNITY` | `MI_OPPORTUNITY_BRIDGE` |
| `RUN_LOG` | `MI_RUN_LOG` |

Verificar nombres, columnas y protecciones contra la hoja viva antes de crear o modificar pestañas.

## Extensiones DI

Agregar, cuando existan:

- ocasión DE MANO;
- necesidad portátil;
- contexto de aplicación;
- sensibilidad a derrames, tamaño, discreción y recarga;
- formato preferido;
- intención: uso propio, obsequio, evento/recuerdo o consultora;
- evidencia de lenguaje real del usuario.

## Extensiones VI

Agregar, cuando existan:

- tipo de envase o producto;
- material y volumen;
- atomizador, roll-on, gotero u otro mecanismo;
- compatibilidad con fórmula;
- hermeticidad y fuga;
- MOQ y escalas de precio;
- muestra;
- lead time;
- personalización;
- etiquetado y cumplimiento;
- costo unitario puesto en destino;
- evidencia física/QA requerida.

## Gate DE MANO

Mantener una oportunidad en `TEST_NEXT` o `HOLD` mientras falte cualquiera de estos elementos críticos aplicables:

- proveedor real verificable;
- precio, unidad y moneda comparables;
- MOQ;
- muestra o evidencia física;
- compatibilidad envase–contenido;
- lead time;
- costo puesto en destino;
- QA físico;
- aprobación humana.

`PROPOSAL_ELIGIBLE` permite diseñar una propuesta o piloto. No habilita catálogo, compra, reenvasado, cumplimiento regulatorio ni venta.

## Integración con CodexLSX y Apps Script

DIVIDE prepara records y evidencia. METANO_IA · CodexLSX Auto Engineer:

1. inspecciona el workbook;
2. genera el mapeo exacto;
3. propone `DRY_RUN`;
4. construye importación, deduplicación, logs, locks y exportación RAG;
5. aplica cambios solo con solicitud explícita.

Apps Script actúa como plano de control. No realiza el escaneo web principal ni decide la promoción de estados.
