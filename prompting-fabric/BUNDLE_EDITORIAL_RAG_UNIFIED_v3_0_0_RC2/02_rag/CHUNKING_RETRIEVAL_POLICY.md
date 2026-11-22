
# Chunking and retrieval policy

| Perfil | Unidad | Overlap | Uso |
|---|---:|---:|---|
| `contract_atomic` | heading + block | 0 | contratos y gates |
| `editorial_entry` | una entrada | 0 | cuadernillos |
| `schema_object` | objeto lógico | 0 | JSON/YAML |
| `protocol_step` | fase o gate | 1 bloque | operaciones |
| `evidence_window` | 600–900 tokens | 80–120 | upstream extenso |

Los chunks conservan `record_id`, `source_id`, `content_path`, `status`, `authority`, versiones y hash. La indexación no cambia el estado de verdad.

`top_k` sugerido: 6; máximo 2 chunks por `source_id` antes de rerank. Si un conflicto está enlazado, el join trae ambos lados aunque uno quede fuera del top-k.
