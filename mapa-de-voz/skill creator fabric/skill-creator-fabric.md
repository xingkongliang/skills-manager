---
name: skill-creator-fabric
description: Upstream fabrication and validation workflow for governed skills.
version: "0.1"
status: "DESIGN-SOUND / UNVALIDATED"
authority: "Human D"
---

# Skill Creator Fabric v0.1

## Purpose
Repeatable pipeline:
capture intent → draft SKILL.md → static review → test cases → baseline → eval → iterate → package.

This fabric creates artifacts. It does NOT deploy, install, canonize, merge, or authorize runtime behavior.

## Non-negotiables
1. Human D retains approval authority at promotion gates.
2. No auto-deploy, auto-canon, auto-merge, or silent conflict resolution.
3. Design claims remain distinct from execution evidence.
4. UNKNOWN remains UNKNOWN until evidence changes its status.
5. Passing design review is not runtime validation.
6. Evaluation artifacts are versioned and reproducible.

## Lifecycle
0. **Intent Capture** — capability, triggers, exclusions, authority, inputs/outputs, dependencies, acceptance criteria.
1. **Draft** — SKILL.md, workflow, boundaries, outputs, failure behavior, evaluation hooks.
2. **Static Review** — authority ambiguity, contradictions, unsupported claims, hidden auto-actions, missing failure states, lineage/collision handling.
3. **Test Design** — happy path, edge, adversarial, authority-boundary, collision, UNKNOWN/abstention cases.
4. **Baseline** — fixed eval set, evaluator version, skill version, results, failures, timestamp.
5. **Evaluation** — explicit grading criteria; never infer validation from prose quality.
6. **Iteration** — change only what failures justify; preserve prior baselines.
7. **Package** — SKILL.md, evals.json, grading.json, baseline result, changelog, manifest.

## Promotion states
NOT-BUILT → DRAFT → DESIGN-SOUND → CANDIDATE → VALIDATED → PRODUCTION-READY

Promotion requires evidence appropriate to the target state and explicit D authorization where required.

## Output contract
Return artifact inventory, status per artifact, known gaps, evidence references, and recommended next gate.

Never label an artifact deployed unless deployment evidence exists.