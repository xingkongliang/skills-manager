# METANOIA DIVIDE · DE MANO · Ingestión v1

Estado: `READY_FOR_REVIEW · DRY_RUN`

Run: `RUN-DEMANO-DIVIDE-V1-20260728T030459-0600`

Escrituras en producción: **0**

## Decisión apoyada

Preparar la capa candidata DI · VI · DE para validar el formato hero de perfume DE MANO de 5 ml sin modificar la hoja viva, sustituir placeholders ni aprobar ventas.

## Baseline verificado

| Control | Estado observado |
|---|---:|
| Pestañas actuales | 30 |
| Pestañas DIVIDE existentes | 0 |
| Proveedores reales en la hoja | 0 |
| Proveedores-placeholder | 3 |
| Cotizaciones completas | 0 de 27 |
| QA físico completo | 0 de 27 |
| Aceptación registrada | 0 de 27 |
| Candidatos con venta aprobada | 0 |

## Contenido del paquete

| Tabla destino | Crear | Actualizar | Omitir | Conflicto | Rechazar |
|---|---:|---:|---:|---:|---:|
| `MI_AUDIENCE_SIGNAL` | 2 | 0 | 0 | 0 | 0 |
| `MI_SUPPLIER` | 4 | 0 | 0 | 0 | 0 |
| `MI_EVIDENCE` | 12 | 0 | 0 | 0 | 0 |
| `MI_OPPORTUNITY_BRIDGE` | 1 | 0 | 0 | 2 | 0 |
| `MI_RUN_LOG` | 1 | 0 | 0 | 0 | 0 |
| **Total de registros candidatos** | **20** | **0** | **0** | **2** | **0** |

Los dos conflictos son de gobernanza y mapeo; no invalidan el paquete, pero bloquean cualquier promoción comercial.

## Oportunidad principal

| Candidato | Evidencia DI | Evidencia VI | Estado DE | Próxima prueba |
|---|---|---|---|---|
| Perfume DE MANO 5 ml | Dos señales transaccionales independientes | Xena, TEGSA, EMPAX y Envaplastix | `TEST_NEXT` | Muestras/cotizaciones + fuga 72 h + 20 atomizaciones + 5 usuarias |

## Reglas de aplicación

1. Crear las cinco tablas DIVIDE como una capa independiente.
2. No sobrescribir `ENV-PROV-A/B/C`.
3. No modificar `PIR_PRODUCTO`, `RPR_CENTRAL`, `PILOTO_3x3x3` ni sus gates.
4. Conservar `RPR.Propuesta_Elegible`; registrar `DIVIDE.Decision_State=TEST_NEXT` aparte.
5. No emitir ni derivar `SALE_APPROVED`.
6. Aplicar cambios únicamente después de autorización explícita y un nuevo readback del destino.

## Archivos

- `divide_ingestion_v1.json`: paquete canónico validable.
- `MI_AUDIENCE_SIGNAL.csv`: 2 señales DI.
- `MI_SUPPLIER.csv`: 4 candidatos VI.
- `MI_EVIDENCE.csv`: 12 registros de evidencia.
- `MI_OPPORTUNITY_BRIDGE.csv`: 1 oportunidad DE.
- `MI_RUN_LOG.csv`: 1 registro de ejecución.
- `VALIDATION.txt`: recibo de validación y controles de seguridad.

## Validación

- Schema `metanoia-divide/1.0`: `PASS`.
- JSON UTF-8 y sintaxis: `PASS`.
- IDs estables y únicos: `PASS`.
- Referencias DI–VI–EV–DE: `PASS`.
- Anchos de las cinco tablas CSV: `PASS`.
- Registros candidatos esperados: `20`.
- `SALE_APPROVED` emitido: `0`.
- Escrituras en producción: `0`.

## Gate siguiente

`REVIEW_AND_AUTHORIZE_STAGING`

Autorizar staging no equivale a autorizar compra, reenvasado, catálogo o venta.
