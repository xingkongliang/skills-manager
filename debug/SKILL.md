---
name: debug
description: Investigate a failure whose cause is not evident from the error itself - flaky or intermittent failures, failures that only happen in CI or one environment, or errors with several plausible causes across files - by reproducing, isolating, fixing and verifying. Skip it when the message already shows the fix (a missing variable, a typo, a wrong import); just fix that directly.
---

# 디버깅 실행

> 4-Phase Debugging
> Reproduce → Isolate → Fix → Verify

호출되면 바로 파이프라인을 시작한다. 단계가 바뀔 때 한 줄로 진행을 알린다.

에러 정보: $ARGUMENTS

---

## 파이프라인 구조

```
Phase 1 Reproduce → Phase 2 Isolate → Phase 3 Fix → (수정 반영) → Phase 4 Verify
└─ fix-bugs (진단 전용) ─┘            fix-bugs (격리)               verify-code
```

---

## 위임 수단이 없는 하네스 — 강등 경로

아래 `subagent_type:` 블록은 운송 수단일 뿐이고, 불변식은 블록의 `prompt:` 계약(검사 항목·출력
형식·완료 선언)이다. 쓸 수 있는 수단을 호스트에 확인하고 위에서부터 한 칸씩 내려간다:
**① 네이티브 서브에이전트**(블록 그대로) → **② 호스트의 다른 격리 위임 수단**(에이전트 정의
본문 `<플러그인 루트>/agents/<이름>.md` 를 **요약 없이** 넘기고 뒤에 `prompt:` 를
붙인다 — 플러그인 루트 탐색은 `skills/plan-task/references/task-tools-fallback.md` §A. 회신 요약이 아니라 계약
산출물로 판정한다) → **③ 이 세션에서 같은 계약을 직접 수행.** 수단이 없다고 단계를 건너뛰거나
하지 않은 위임을 했다고 보고하지 않는다. ③은 격리가 없다 — 판정을 서술이 아니라 재현 명령의 실제 출력으로 하고, 기각한 대안 원인을 최소 1개 적는다.

## Phase 1-2: 재현 및 격리 (Reproduce + Isolate)

> **진단 전용 호출은 격리 이득이 없다.** 읽기·재현 명령만 하고 파일을 고치지 않으므로
> (`rules/parallel-worktree.md` — 읽기 전용 작업자에 격리를 걸지 않는다) 프롬프트에 "수정 금지"를
> 명시한다. `fix-bugs` 정의에 격리가 걸려 있어 호출 단위로 끌 수 없는 호스트면 그것은 감수한다 —
> 이 단계에서 생긴 수정이 있으면 반영하지 않고 버린다.

```
Task tool 사용:
subagent_type: fix-bugs
model: opus
prompt: |
  (진단 전용 — 이 단계에서는 수정하지 말고 원인·재현 명령·근거만 보고)
  다음 에러를 진단해주세요:
  $ARGUMENTS

  ### Phase 1: Reproduce (재현)
  - 에러를 재현하는 최소 단계 식별
  - 재현 조건 및 환경 확인
  - 에러 발생 빈도 (항상/간헐적)

  ### Phase 2: Isolate (격리)
  - 에러 유형 (문법/타입/런타임/빌드/외부API)
  - 파일 위치 및 라인 번호
  - 스택 트레이스 분석
  - 루트 원인 파악
  - 영향 범위 (단일 함수/모듈/시스템 전체)

  ### 수정 방안 제안
  - 옵션 A: [빠른 수정]
  - 옵션 B: [근본 해결]
```

---

## Phase 3: 수정 (Fix)

```
Task tool 사용:
subagent_type: fix-bugs
model: sonnet
prompt: |
  [Phase 1-2 진단 결과 포함]

  진단 결과를 바탕으로 버그를 수정해주세요.

  수정 원칙:
  - 최소 변경 원칙
  - exc_info=True 포함 (Python 로깅)
  - 에러 핸들링 추가 (필요시)
  - 같은 패턴의 버그가 다른 곳에 없는지 확인
```

**수정 반영 [건너뛰기 금지]:** `fix-bugs` 는 `isolation: worktree` 라 수정이 **격리 트리에** 남는다.
Phase 4 로 가기 전에 그 수정(diff/브랜치)을 **이 세션의 트리에 반영**한다(방법: `rules/parallel-worktree.md`
의 반환·통합 절). 반영하지 않고 `verify-code` 를 부르면 옛 코드를 검증해 "해결됨"으로 오판한다.
반영하지 못했으면 Phase 4 를 건너뛰고 "수정 미반영"으로 보고한다.

**재시도 상한:** Phase 3→4 를 **최대 3회**까지만 돈다. 3회째에도 재현이 남거나, **무진전 2회**(연속
두 번의 검증에서 실패 항목이 줄지 않음)이면 멈추고 현재까지의 원인 가설·시도·남은 실패를 사용자에게
보고한다. 같은 수정안을 반복해 시도하지 않는다.

---

## Phase 4: 검증 (Verify)

```
Task tool 사용:
subagent_type: verify-code
model: haiku
prompt: |
  수정된 코드를 검증해주세요:
  [변경된 파일 목록]

  검증 항목:
  1. 에러 재현 → 해결 확인
  2. 빌드/컴파일 성공
  3. 타입 체크 통과
  4. 관련 테스트 통과
  5. 엣지 케이스 확인
```

---

## 출력 형식

### Phase 1-2: 재현 및 격리

| 항목      | 내용                       |
| --------- | -------------------------- |
| 유형      | [타입/런타임/빌드/외부API] |
| 위치      | [파일:라인]                |
| 재현 조건 | [최소 재현 단계]           |
| 루트 원인 | [원인 설명]                |
| 영향 범위 | [함수/모듈/시스템]         |

### Phase 3: 수정

[변경사항 diff 또는 설명]

### Phase 4: 검증

[테스트/빌드 통과 여부]

### 예방 권장

[재발 방지를 위한 제안]

- 테스트 추가
- 타입 강화
- 에러 핸들링 개선
