# Skill Creator Action Plan: SKILL-BIBLE-CAND-001

## Propósito

Optimizar y producir `bible-studies-context-engine v0.1.0` mediante el workflow de skill-creator, integrando:
- Especificación funcional desde `/skills/aeq-memory-intake-handoff-v01`
- Constraints de gobernanza desde `/skills/opal-bos-skill`
- Validación ÆQUITAS (AQ1–AQ5)
- Entrega governada a Gate E → Gate D

---

## I. Candidatos Generados (Estado Actual)

| Artefacto | Ruta | Status | Propósito |
|-----------|------|--------|----------|
| **SKILL-BIBLE-STUDIES-CONTEXT-ENGINE-v0.1.md** | /home/claude/ | ✅ DRAFT | Markdown skill completo con triggers, workflow NORMALIZE→REBUILD→COMMIT |
| **skill-spec-bible-cand-001.yaml** | /home/claude/ | ✅ DRAFT | Especificación YAML de gobernanza + dominios + integración |
| **CMP-BIBLE-STUDIES-EPHESIANS-CANDIDATE.json** | /home/claude/ | ✅ DRAFT | Ejemplo ejecutable: Efesios 1:1–14 como proyecto piloto |
| **Este documento** | /home/claude/ | 📋 PLAN | Roadmap para skill-creator → v1.0.0 release |

---

## II. Workflow skill-creator (4 Fases)

### Fase 1: REVIEW (Ahora)

**Entrada**: 3 artefactos candidatos (MD + YAML + JSON)

**Decisión requerida**:
```yaml
questions:
  Q1: ¿Triggering es suficientemente específico? (Actual: 5 triggers)
      → Posible optimización: Agregre @BIBLE_VERIFY, @GEMA_MAP commands
      
  Q2: ¿Dominios de skill son mutuamente excluyentes?
      → Verificar: 01_LOGICAL_SYNTHESIS, 02_CONTEXT_MANAGEMENT, 03_GENERATIVE_SIMULATION
      → Parecen bien separados
      
  Q3: ¿Non-mutations rules son ejecutables sin manual de usuario?
      → Actual: 5 reglas + checklist de 8 secciones
      → RECOMENDACIÓN: Agregar código de validación automatizado (QC-01..10)
      
  Q4: ¿CMP structure de 8 secciones es estándar o derivada?
      → Derivada de normalize-rebuild-handoff v1.1
      → ✅ Validado contra spec oficial
      
  Q5: ¿Autoridad de Gate E/D está clara sin ambigüedad?
      → Actual: Authority matrix en Section 3
      → ✅ Claro pero necesita validación cruzada con opal-bos-skill oficial
```

**Output Fase 1**: REVIEW SUMMARY + recommendations

---

### Fase 2: OPTIMIZE (Propuesto)

**Entrada**: Feedback de Phase 1 + candidatos

**Acciones**:

#### 2.1 Refinar Triggering
Agregar comandos explícitos + patterns:

```markdown
# Nuevos triggers (propuesto)

### Explicit commands
- @BIBLE_VERIFY {versículos} — Verificar mapeo GEMA de versículos dados
- @GEMA_MAP {tribu} — Listar todos los versículos asignados a una tribu
- @HANDOFF_BIBLE — Generar CMP para transferencia inter-LLM
- @RECONSTRUCT_STATE — Ejecutar NORMALIZE→REBUILD→COMMIT completo

### Implicit patterns (mejorado)
- User: "reconstruct my Bible study from these 3 files" 
  → Detectar: reconstruct + Bible + multiple sources → AUTO-ACTIVATE
  
- User: "where's my Ephesians mapping status?"
  → Detectar: Bible reference + status query → AUTO-ACTIVATE
  
- User: "I moved my study to ChatGPT, bring it back"
  → Detectar: handoff + Bible context → AUTO-ACTIVATE aeq-memory-intake-handoff
```

#### 2.2 Agregar Validación Automatizada
Crear `validate_cmp.py` script (ejecutable antes de entrega Gate D):

```python
#!/usr/bin/env python3
# validate_cmp.py — QC-01..10 automated checker

import json
import sys

def validate_cmp(cmp_file):
    """
    Ejecutar QC-01 a QC-10 sobre un CMP candidato.
    Exit 0 si PASS; exit 1 si FAIL.
    """
    with open(cmp_file) as f:
        cmp = json.load(f)
    
    checks = {
        "QC-01_schema_validity": lambda c: validate_json_schema(c),
        "QC-02_duplicate_prevention": lambda c: check_duplicates(c),
        "QC-03_complete_mappings": lambda c: check_mappings_complete(c),
        "QC-04_token_clarity": lambda c: check_source_ids(c),
        "QC-05_changelog_log": lambda c: check_timeline(c),
        "QC-06_artifact_paths": lambda c: check_artifact_paths(c),
        "QC-07_batch_plan": lambda c: check_batch_sequencing(c),
        "QC-08_descriptors": lambda c: check_descriptors(c),
        "QC-09_script_sanity": lambda c: check_scripts(c),
        "QC-10_rollback_plan": lambda c: check_rollback(c),
    }
    
    results = {}
    for check_name, check_func in checks.items():
        try:
            results[check_name] = {"status": "PASS", "error": None}
            assert check_func(cmp), f"Assertion failed in {check_name}"
        except Exception as e:
            results[check_name] = {"status": "FAIL", "error": str(e)}
    
    return results

def validate_json_schema(cmp):
    """QC-01: JSON debe parsear sin error."""
    required_sections = [
        "metadata",
        "section_1_comprension_reconstruida",
        "section_2_estado_normalizado",
        "section_3_source_evidence_boundaries",
        "section_4_conflictos_y_unknown",
        "section_5_modulo_dueno",
        "section_6_siguiente_accion_segura",
        "section_7_cmp_handoff_copy_paste",
        "section_8_delta_commit_ready"
    ]
    return all(section in cmp for section in required_sections)

def check_duplicates(cmp):
    """QC-02: Detectar exactos duplicados vs. similares."""
    facts = cmp.get("section_2_estado_normalizado", {}).get("hechos_clasificados", [])
    ids = [f.get("id") for f in facts]
    return len(ids) == len(set(ids))  # No duplicates

def check_mappings_complete(cmp):
    """QC-03: Todos los versículos tienen tribu ↔ gema ↔ color."""
    facts = cmp.get("section_2_estado_normalizado", {}).get("hechos_clasificados", [])
    for fact in facts:
        if not all(k in fact for k in ["versiculos", "tribu_gema", "color_lumina"]):
            return False
    return True

def check_source_ids(cmp):
    """QC-04: Cada hecho tiene source_id; use SRC-001 pattern."""
    facts = cmp.get("section_2_estado_normalizado", {}).get("hechos_clasificados", [])
    for fact in facts:
        if "source_id" not in fact or not fact["source_id"].startswith("SRC-"):
            return False
    return True

def check_timeline(cmp):
    """QC-05: Timeline de decisiones existe y es trazable."""
    timeline = cmp.get("section_2_estado_normalizado", {}).get("timeline_decisiones", [])
    return len(timeline) > 0 and all("fecha" in e and "evento" in e for e in timeline)

def check_artifact_paths(cmp):
    """QC-06: Deliverables tienen rutas explícitas (no asumidas)."""
    section8 = cmp.get("section_8_delta_commit_ready", {})
    # Al menos debe existir section_8 con changeset
    return "changeset" in section8

def check_batch_sequencing(cmp):
    """QC-07: Si hay batch work, debe estar secuenciado en section_6."""
    section6 = cmp.get("section_6_siguiente_accion_segura", {})
    acciones = section6.get("acciones_recomendadas", [])
    # Cada acción debe tener "accion" y "quien"
    return all("accion" in a and "quien" in a for a in acciones)

def check_descriptors(cmp):
    """QC-08: Metadata completo antes de cualquier renderización."""
    metadata = cmp.get("metadata", {})
    required = ["document_id", "timestamp", "status", "target_gates"]
    return all(k in metadata for k in required)

def check_scripts(cmp):
    """QC-09: Si scripts incluidos, deben ser runnable (dry-run aquí)."""
    # Verificar que no hay scripts no-runnable
    # (Esto es una validación básica; en prod usarías linting)
    return True  # Placeholder

def check_rollback(cmp):
    """QC-10: Plan de rollback debe existir."""
    section8 = cmp.get("section_8_delta_commit_ready", {})
    # Debe tener "decision_options" que incluye "REJECT"
    options = section8.get("decision_options", {})
    return "REJECT" in options

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 validate_cmp.py <cmp.json|cmp.yaml>")
        sys.exit(1)
    
    cmp_file = sys.argv[1]
    results = validate_cmp(cmp_file)
    
    passed = sum(1 for r in results.values() if r["status"] == "PASS")
    failed = sum(1 for r in results.values() if r["status"] == "FAIL")
    
    print(f"\n=== QC VALIDATION RESULTS ===")
    print(f"PASS: {passed}/10 | FAIL: {failed}/10\n")
    
    for check, result in results.items():
        status_icon = "✅" if result["status"] == "PASS" else "❌"
        print(f"{status_icon} {check}: {result['status']}")
        if result["error"]:
            print(f"   Error: {result['error']}")
    
    sys.exit(0 if failed == 0 else 1)
```

#### 2.3 Integración con aeq-memory-intake-handoff-v01

**Actual**: aeq-memory-intake-handoff-v01 es generic (multi-domain)

**Propuesto**: Crear overlay especializado para Bible Studies

```yaml
# enhancement-proposal-aeq-bible-overlay.yaml

enhancement_type: "Domain-specific overlay for aeq-memory-intake-handoff-v01"
domain: "Bible Studies + GEMA Mappings"

additions:
  - intake_template_bible:
      fields:
        - versiculos_range: "e.g., Eph 1:1-14"
        - mapeos_gema_propuestos: "tribu, gema, color"
        - fuentes_fragmentadas: "list of SRC IDs"
        - conflictos_conocidos: "list of conflicts"
      output_format: "CMP 8-section standard"
      
  - sync_receipt_extensions:
      - packet_type: "BIBLE_STUDY_SYNC"
        required_fields: ["document_id", "versiculos_covered", "conflicts_count"]
      
  - memory_persistence:
      - key: "bible_study:{project_id}"
      - schema: "CMP JSON"
      - ttl: "indefinite (canonical)"

compatibility:
  - "No mutations to core aeq-memory-intake-handoff-v01"
  - "Overlay is additive only"
  - "Fallback to generic mode if Bible Studies domain not detected"
```

#### 2.4 ÆQUITAS Validation Automation

Agregar checklist ejecutable:

```json
{
  "aequitas_validation": {
    "AQ1_fidelity": {
      "criterion": "Cada hecho tiene source_id verificable",
      "automated_check": "validate_cmp.py QC-04_token_clarity",
      "manual_review": "Gate E verifica source links actuales"
    },
    "AQ2_coherence": {
      "criterion": "Contradicciones documentadas, no ocultadas",
      "automated_check": "validate_cmp.py QC-02_duplicate_prevention",
      "manual_review": "Gate E revisa section_4_conflictos"
    },
    "AQ3_necessity": {
      "criterion": "No scaffold inflado ni raw logs como validated",
      "automated_check": "Check classification (CANON vs PROVISIONAL vs UNKNOWN)",
      "manual_review": "Gate D valida que cada hecho tiene autoridad clara"
    },
    "AQ4_precision": {
      "criterion": "CMP ejecutable sin shared context",
      "automated_check": "validate_cmp.py check_descriptors + check_artifact_paths",
      "manual_review": "Gate D intenta executehandoff con solo JSON + este doc"
    },
    "AQ5_elegance": {
      "criterion": "Estructura clara, compacta, sin ruido",
      "automated_check": "Validar JSON schema + section sizes",
      "manual_review": "Gate D lee section 1 + 8; debe ser understandable en < 3 min"
    }
  }
}
```

---

### Fase 3: TEST (Propuesto)

**Entrada**: Candidato optimizado + validate_cmp.py + ejemplo JSON

**Test cases**:

```yaml
test_suite:
  - name: "TC-001: Parse CMP-BIBLE-STUDIES-EPHESIANS-CANDIDATE.json"
    script: "python3 validate_cmp.py CMP-BIBLE-STUDIES-EPHESIANS-CANDIDATE.json"
    expected: "QC-01..10 all PASS"
    
  - name: "TC-002: Trigger @BIBLE_VERIFY Eph 1:3"
    input: "User says '@BIBLE_VERIFY Eph 1:3'"
    expected: "Skill activates + identifies conflict CONF-001"
    
  - name: "TC-003: Trigger @HANDOFF_BIBLE"
    input: "User says '@HANDOFF_BIBLE after studying Eph 1:1-14'"
    expected: "Skill generates CMP + Sync Receipt + Packet ID"
    
  - name: "TC-004: Conflict detection (Eph 1:3)"
    input: "CMP has FACT-002 and FACT-004 (same verse, diff tribe)"
    expected: "section_4_conflictos lists CONF-001 + marks both UNKNOWN"
    
  - name: "TC-005: Authority chain"
    input: "Fact marked PROVISIONAL; user asks if can merge"
    expected: "Skill says 'Needs Gate E approval; not mergeable without it'"
    
  - name: "TC-006: Rollback scenario"
    input: "User says 'revert to SRC-001 baseline'"
    expected: "CMP section_8_delta shows decision_option REJECT; prepared for rollback"
```

**Output**: Test report + coverage matrix

---

### Fase 4: DELIVERY (Propuesto)

**Entrada**: Candidato validado + tests PASS

**Deliverables**:

1. **SKILL-BIBLE-STUDIES-CONTEXT-ENGINE-v1.0.0.md**
   - Versión final con feedback integrado
   - Ready for `/skills/user/` installation

2. **validate_cmp.py (executable)**
   - Integrable en CLI + CI/CD
   - Usable por Gate D para pre-merge validation

3. **bible-studies-overlay-aeq-memory-intake.yaml**
   - Enhancement para aeq-memory-intake-handoff-v01
   - Integrable sin mutations

4. **INSTALLATION.md**
   - Pasos para instalar skill
   - Integración con opal-bos-skill + LUMINA + ÆQUITAS
   - Comandos @BIBLE_* disponibles post-install

5. **GATE_E_GOVERNANCE_REVIEW.md**
   - Documento para Gate E review
   - Authority matrix + scope + non-mutations
   - Decision points: APPROVE / REQUEST_CHANGES

6. **GATE_D_QA_CHECKLIST.md**
   - Checklist QC-01..10 + ÆQUITAS AQ1–AQ5
   - Automated + manual review steps
   - Decision: ACCEPT / PATCH / REJECT

---

## III. Integración con Ecosystem (Noww)

### A. Dependencias de Lectura (Read-Only)

```
bible-studies-context-engine
  ↓ reads
  ├─ /skills/opal-bos-skill (governance + SELAH gating)
  ├─ /skills/aeq-memory-intake-handoff-v01 (continuidad)
  ├─ /projects/codex-gema (mapeos oficiales)
  ├─ /brand/lumina-color-palettes.json (colores autorizados)
  └─ /normalize-rebuild-handoff-v1.1 (reference framework)
```

### B. Slots de Integración (Write Slots)

```
bible-studies-context-engine produces:
  → CMP JSON (7 secciones + metadata)
  → Sync Receipt (para aeq-memory-intake)
  → Handoff packet (para inter-LLM transfer)
  → QC report (para Gate D)
  → Escalation to Gate E (si conflictos o UNKNOWN)
```

### C. Gating Requerido

| Gate | Cuándo | Qué | Quién autoriza |
|------|--------|-----|----------------|
| **E** | Antes de merge | ¿Autoridad? ¿Scope? ¿Canon seguro? | Capability Governance |
| **D** | Antes de release | QC pass? ÆQUITAS pass? Rollback plan? | QA / MERGE |

---

## IV. Timeline (Propuesto)

```
NOW (2026-08-11):
  ✅ Candidatos generados (MD + YAML + JSON)
  
WEEK 1 (skill-creator Phase 1–2):
  ☐ Review candidatos
  ☐ Refine triggering
  ☐ Optimize QC automation
  ☐ Integrate with aeq-memory-intake overlay
  
WEEK 2 (Phase 3):
  ☐ Test suite execution (TC-001..TC-006)
  ☐ Coverage matrix
  ☐ Bug fixes if needed
  
WEEK 3 (Phase 4):
  ☐ Final deliverables packaging
  ☐ Gate E review submission
  ☐ Gate D QA checkpoint
  
RELEASE:
  v1.0.0 if ACCEPT from both gates
```

---

## V. Risk & Constraints

| Risk | Mitigation |
|------|-----------|
| Gate E may reject scope | Escalation email to Gate E before Phase 3; iterate |
| LUMINA colors inaccesible | Mark as UNKNOWN; defer to brand-studio-v2 |
| Performance (large Bible data) | Test with full 66-book Bible; optimize parsing if needed |
| Non-mutation rules broke | Automated check: verify no changes to opal-bos-skill, Main, canon |

---

## VI. Success Criteria

✅ **Phase 1**: Review summary + recommendations delivered  
✅ **Phase 2**: Optimizations integrated; new triggers + validation script ready  
✅ **Phase 3**: All tests pass; coverage > 90%; documentation complete  
✅ **Phase 4**: Deliverables packaged; Gate E + Gate D checklists provided  
✅ **RELEASE**: v1.0.0 approved by Gate E + Gate D; installable via `/skills/user/`  

---

## VII. Next Commands

Para proceder con skill-creator:

```bash
# Phase 1: Review
skill-creator review \
  --spec /home/claude/skill-spec-bible-cand-001.yaml \
  --markdown /home/claude/SKILL-BIBLE-STUDIES-CONTEXT-ENGINE-v0.1.md \
  --example /home/claude/CMP-BIBLE-STUDIES-EPHESIANS-CANDIDATE.json \
  --output review-summary.md

# Phase 2: Optimize
skill-creator optimize \
  --feedback review-summary.md \
  --add-triggers @BIBLE_VERIFY,@GEMA_MAP,@HANDOFF_BIBLE \
  --add-validation validate_cmp.py \
  --add-overlay bible-studies-overlay-aeq-memory-intake.yaml \
  --output skill-v0.2-optimized.md

# Phase 3: Test
python3 validate_cmp.py CMP-BIBLE-STUDIES-EPHESIANS-CANDIDATE.json
skill-creator test \
  --suite test-cases.yaml \
  --output test-report.json

# Phase 4: Deliver
skill-creator package \
  --version v1.0.0 \
  --include validate_cmp.py \
  --include INSTALLATION.md \
  --include GATE_E_REVIEW.md \
  --include GATE_D_CHECKLIST.md \
  --output bible-studies-context-engine-v1.0.0.zip
```

---

**Document**: SKILL-CREATOR-ACTION-PLAN-BIBLE-CAND-001  
**Status**: Ready for skill-creator Phase 1 (REVIEW)  
**Next**: Submit 3 candidates to skill-creator for automated review  
**Awaiting**: Gate E + Gate D approval after Phase 4 completion
