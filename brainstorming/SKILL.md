---
name: brainstorming
description: Design and spec a Large new feature or system change before planning - explores requirements, proposes approaches, gets design approval, then chains to plan-task. Skip for bug fixes and Small/Medium work; go straight to plan-task or just do it.
---

# Brainstorming

## 크기 게이트 — 먼저 판정한다

이 스킬은 **설계가 필요한 Large 새 기능·시스템 변경**에만 쓴다(Large 의 기준 SSOT:
`skills/plan-task/references/elicitation.md` §6).

- **버그 수정 · Small · Medium** → 이 스킬을 건너뛴다. Medium 은 바로 `plan-task`, Small 은
  그냥 구현한다. 건너뛴 사실을 한 줄 적고 진행한다 — 사용자에게 설계 승인을 받으려고 멈추지 않는다.
- 판정이 애매하면 Medium 으로 보고 `plan-task` 로 간다(규모가 드러나면 거기서 되돌아온다).

아래는 게이트를 통과한(Large) 작업에만 적용된다.

<HARD-GATE>
이 스킬이 invoke된 작업은 사용자가 설계를 승인하기 전까지 plan-task 호출·코드 작성·파일 생성을 하지 않는다 — 승인 없는 구현은 되돌리는 비용이 크다.
</HARD-GATE>

## 체크리스트

진입 즉시 ToolSearch("select:TaskCreate,TaskUpdate,TaskList")로 Task 도구를 로드한 후,
아래 항목 각각에 대해 TaskCreate를 실행하세요.

> **Task 도구가 없으면 멈추지 말고 대체 경로로 간다** — `skills/plan-task/references/task-tools-fallback.md`
> §B: `[Brainstorm]` 단계는 걸 `verify` 명령이 없으므로 checklist 가 아니라 **대화창 진행표**로
> 추적한다(산출물은 `docs/specs/…` 스펙 파일). 아래 항목을 진행표의 행으로 쓴다.

**Task 네이밍 규약:** `[Brainstorm] {항목명}` (plan-task의 `[Planning]` Task와 구분)
**완료 마킹 규약 [건너뛰기 금지]:** 각 항목을 마치면 **즉시** `TaskUpdate(status="completed")`
(도구가 없으면 진행표의 근거 칸을 채운다), 마지막 항목(plan-task invoke)은 **invoke 직전에**
마킹 (규율 SSOT: `rules/definition-of-done.md#task-마감-규율`).

1. 프로젝트 컨텍스트 파악 (파일, 최근 커밋, docs)
2. 명확화 질문 (한 번에 하나씩)
3. 2-3가지 접근법 + 추천
4. 설계 제시 및 사용자 승인
5. 스펙 문서 작성 (`docs/specs/YYYY-MM-DD-{topic}.md`)
6. 스펙 자가 검토 (플레이스홀더, 모순, 모호성)
7. 사용자 스펙 검토 대기
8. `plan-task` invoke

## 프로세스

### 1단계: 컨텍스트 파악

- 관련 파일, README, 최근 커밋 확인
- 규모 판단: 복수의 독립 서브시스템이면 분해 먼저 제안

### 2단계: 명확화 질문

- 한 번에 하나의 질문만
- 목적, 제약, 성공 기준에 집중
- 가능하면 객관식 선택지 제공

### 3단계: 접근법 제안

- 2-3가지 접근법과 트레이드오프
- 추천안과 이유를 먼저 제시

### 4단계: 설계 제시

- 각 섹션을 순서대로 제시하고 승인 확인
- 아키텍처, 컴포넌트, 데이터 흐름, 에러 처리, 테스트 전략 포함
- 수정 요청 시 해당 섹션 재작성

### 5단계: 스펙 문서 작성

**저장 경로:** `docs/specs/YYYY-MM-DD-{topic}.md`
(계획 파일은 plan-task 가 만든다 — 그 `## 요구사항` 에서 이 스펙 경로를 가리킨다)

스펙 문서 구조:
```markdown
# {Topic} 설계

**Goal:** [한 문장]
**Architecture:** [2-3 문장]

## 요구사항
## 접근 방식
## 컴포넌트 구조
## 데이터 흐름
## 에러 처리
## 테스트 전략
## 범위 외
```

### 6단계: 자가 검토

1. **플레이스홀더 스캔:** TBD, TODO, 미완성 섹션 → 즉시 수정
2. **일관성:** 섹션 간 모순 없는지 확인
3. **범위:** 단일 plan으로 구현 가능한가?
4. **모호성:** 두 가지로 해석 가능한 요구사항 → 명확화

### 7단계: 사용자 검토 대기

> 세션 경계 주의: 태스크 도구의 태스크는 세션을 넘지 못한다 — 이 단계에서 세션이 끝나면 대기
> 태스크는 사라진다. 재개 여부는 `docs/specs/` 스펙 파일이 있는지로 판단한다(스펙이 있고
> 승인이 기록돼 있으면 8단계부터, 없으면 이 단계부터 다시).

```
스펙을 `{path}`에 저장했습니다. 검토 후 수정 사항이 있으면 말씀해 주세요.
승인되면 plan-task로 넘어갑니다.
```

수정 요청 시 → 수정 후 자가 검토 재실행
승인 시 → 8단계

### 8단계: plan-task invoke

```
설계 승인 완료. plan-task를 시작합니다.
```

`plan-task` 스킬을 invoke합니다. (Skill 도구 사용)
