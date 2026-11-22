---
name: using-hiway-kit
description: Session-start meta-skill (hook-injected). Kit workflow chain and plan file rules.
disable-model-invocation: true
---

# Using hiway-kit

**작업이 스킬의 사용 시점에 해당하면 구현 전에 그 스킬을 먼저 invoke한다.** 같은 규율을 주는
다른 플러그인이 있으면 그쪽을 따라도 된다 — 킷은 그 존재를 가정하지도, 충돌하지도 않는다.

## Workflow Chain — 크기에 맞춰 탄다

```
Large 새 기능:  brainstorming → plan-task → auto-dev
Medium:         plan-task → auto-dev
Small·버그:     바로 구현 (완료 조건 명령으로 검증)
```

크기 기준과 Small 경로는 `skills/plan-task/references/elicitation.md` §6 이 소유한다. 계획 파일은 Medium/Large 만
`docs/plans/<날짜>-<slug>/plan.md` 에 둔다(규약 `skills/plan-task/references/plan-format.md`).
