---
name: auto-dev
description: Automated development pipeline. Runs a completed plan file through Development and Validation phases end-to-end.
---

# Auto-Dev 스킬

계획 파일(`docs/plans/<YYYY-MM-DD>-<slug>/plan.md` — 규약 SSOT: `skills/plan-task/references/plan-format.md`)을
입력으로 Development → Validation 파이프라인을 자동 실행합니다.

```
/auto-dev docs/plans/2026-09-28-login-lockout          # 계획 디렉토리
/auto-dev docs/plans/2026-09-28-login-lockout/plan.md  # 또는 그 plan.md
```

**진행 추적**: 호스트 태스크 도구(있으면) + checklist(`verify` 명령 exit 0 으로만 통과). 태스크 도구가
없으면 `[Dev]` 는 checklist, Validation 은 대화창 진행표 + `plan.md` `## 검증 결과`로 추적한다 —
대체 경로 SSOT: `skills/plan-task/references/task-tools-fallback.md` §B.
**영속 상태**: `plan.md` 의 `status`·`## 검증 결과` 와 `checklist.json` — 태스크 도구는 세션을 넘지 못한다.

---

## Step 0: 계획 확인 + 추적 초기화 [건너뛰기 금지]

### 진입 확인 [건너뛰기 금지]

- **계획 경로가 주어졌으면** `plan.md` 를 읽는다. 파일이 없거나, `status: planning` 이거나,
  `## 구현 계획`·`## 완료 조건` 이 비어 있으면 → *"계획이 완성되지 않았습니다. `/plan-task <경로>` 를 먼저 실행하세요."* 출력 후 중단.
  `status: done` 이면 재실행하지 않고 사용자에게 알린다.
- **새 요청으로 진입했으면**(`/auto-dev 로그인 기능 추가`) → **즉시 `plan-task` 로 보낸다.**
  계획 없이 Step 1 진행 금지 — `brainstorming → plan-task → auto-dev` 체인 준수.
- **plan-task 가 Small 로 판정해 인자 없이 넘겼으면** 대화창의 계획으로 Step 4(Small 경로)를 실행한다.

**재개 위치** — 계획 디렉토리에 checklist 가 없으면 Step 1, 미완 항목이 있으면 Step 2, 전부 통과면 Step 3.

### 추적 초기화 [건너뛰기 금지]

1. 호스트 태스크 도구가 있으면 로드한다(Claude Code: `ToolSearch("select:TaskCreate,TaskUpdate,TaskList")`).

> **Task 도구가 없으면 멈추지 말고 대체 경로로 간다** — `skills/plan-task/references/task-tools-fallback.md`
> §B: `[Dev]` 는 durable checklist, `[Validation]` 은 대화창 진행표 + `plan.md` (킷 도구 탐색은 §A).
2. **도구가 있으면** `TaskList` 로 이 계획의 `[Dev]`/`[Validation]` Task 가 있는지 보고 상태 확인 후
   재개 (재생성 스킵). **도구가 없으면** 위 "재개 위치"대로 `checklist.json` 과 `plan.md` 에서 읽는다.
3. 없으면 → Step 1

---

## Step 1: Development Tasks 생성

`plan.md` 의 `## 구현 계획` 항목 분석 + **소스코드 직접 탐색**:

1. 구현 계획 항목 목록화
2. 코드베이스 직접 탐색 (에이전트 위임 아님):
   - 이미 구현된 파일/함수 확인
   - 해당 항목은 ✅ 완료로 마킹 (Task 생성 스킵)
3. 미완료 항목만 Task 로 만든다(도구가 있으면 TaskCreate, 없으면 아래 checklist 항목):

**분리 기준:**

- 독립적으로 구현 가능한 항목 → 별도 Task (병렬 실행)
- 다른 항목에 의존하는 항목 → `addBlockedBy` 설정 (도구가 없으면 id 순서 + `description` 의
  `(선행: …)` — 규약은 fallback §B)

**Task 네이밍:** `[Dev] {구현 항목명}`

**예시 구조:**

```
T-dev-1: [Dev] 데이터 모델 정의       ← 독립 (병렬)
T-dev-2: [Dev] API 엔드포인트 구현    ← blockedBy: T-dev-1
T-dev-3: [Dev] 프론트엔드 컴포넌트   ← blockedBy: T-dev-1
T-dev-4: [Dev] 테스트 작성            ← blockedBy: T-dev-2, T-dev-3
```

**checklist 생성** (계획 파일이 있을 때): `## 완료 조건` 의 명령을 항목의 `verify` 로 삼아
계획 디렉토리에 `init` 한다(호출·도구 탐색: `skills/plan-task/references/task-tools-fallback.md` §A·§B).
`verify` 는 계획에서 **파생**한다 — 실행자가 새로 지어내지 않는다. 걸 명령이 없는 항목은
checklist 에 넣지 않는다(가짜 verify 금지).

---

## Step 2: Development 실행

각 Task의 구현 역할은 `implement-code` 계약을 따른다. 먼저 현재 호스트의 위임·격리 기능과
프로젝트가 선택한 운송을 확인한다. 네이티브 에이전트가 없으면 허용된 외부 위임 수단에
역할 계약을 전달한다. 수정 worker는 격리하며, 명시된 운송/계보를 다른 수단으로 대체하지 않는다.
위임이 불가능하고 직접 수행이 허용되면 부모가 같은 계약으로 순차 실행한다. 명시적 운송 요구나
활성 작업 권한 때문에 대체할 수 없으면 차단 사유를 보고한다. 상세: `control-loop`의 운송 절.
아래 네이티브 호출 예시는 그 기능이 제공되는 환경에서만 사용한다.

**병렬 실행 원칙 (스케일별):**

blockedBy 없는 Task가 2개 이상이면 동일 응답에서 동시 dispatch:

```
Agent(task_A) ─┐
Agent(task_B) ─┼─ 동일 응답에서 동시 dispatch
Agent(task_C) ─┘
```

**dispatch 전 파일 소유권 확인 [건너뛰기 금지]:** 병렬 청크의 수정 대상 파일이 disjoint 해야
한다 — 겹치면 순차 dispatch 로 강등하고, 공유 파일(설정·배럴 export·등록부)은 마지막에 메인
세션이 단독 수정한다. 반환·통합·충돌 에스컬레이션(임의 ours/theirs 금지, 사용자에게
ours/theirs/manual 선택지를 호스트 수단으로 묻는다)은 규범 `parallel-worktree`(참조 등급 — 세션 주입의
참고 목록이 경로를 준다)를 따른다.
같은 지점 충돌 2회 반복 = 청크 분해 오류 → 남은 dispatch 중단, 계획 단계로 돌아간다.

**Large 라우팅:** 계획 `size: large`이고 unblocked 병렬 청크가 **10개 이상**이면,
스킬 주도 dispatch는 main 컨텍스트에 부담이 큽니다. 이 경우 청크 목록을 정리해
사용자에게 네이티브 `ultracode`(dynamic workflow) 트리거를 안내하세요
(`ultracode`는 대화형 전용 — 스킬에서 자동 트리거 불가). 10개 미만이면 현행 dispatch.

**Task 완료 시 매번 의무:**

1. 대응 checklist 항목이 있으면 `complete <id>` — verify exit 0 일 때만 통과
2. 완료 마킹(호스트 태스크 도구가 **있으면** `TaskUpdate(id, status="completed")` — 없으면 1의
   `complete` 가 곧 마킹이다)
3. unblocked Task 확인 → 즉시 실행

**Durable executor 규율 (장기·다세션 실행):**

- **미완 1항목/iteration**: 한 iteration은 checklist 미완 항목 **하나**만 목표로 한다
  (한 번에 다수 항목을 "완료"로 몰아 찍지 않는다 — 검증 없는 일괄 통과 방지).
- **verify 통과 전 passes 금지**: `checklist.json`의 `passes:true`는 오직
  checklist `complete <id>`(경로는 `skills/plan-task/references/task-tools-fallback.md` §A 의 `kit_root`)가
  항목의 `verify` 명령을 **실제 실행해 exit 0**일 때만 전환된다.
  모델 판단으로 completed를 self-mark하지 않는다.
- **상태 쓰기는 메인 세션 소유**: `checklist.json`·`plan.md` 쓰기는 **메인 세션**만
  수행한다. worktree subagent는 코드만 변경하고 상태 파일은 건드리지 않는다
  (규범 `parallel-worktree` 의 공유 상태 파일 절 — 상태는 단일 writer).

모든 Dev Task 완료 후 Step 3 으로 간다.

---

## Step 3: Validation Tasks 생성 및 실행

Dev 완료 직후 Validation을 3단계로 실행합니다. 아래 역할 이름은 특정 도구의 존재를
보장하지 않는다. Step 2의 기능·운송 확인을 동일하게 적용한다. 독립 리뷰 수단이 없으면
자가 점검으로 대체했다는 한계를 보고하고, 필수 독립 리뷰를 통과한 것으로 마킹하지 않는다:

```
T-spec   (스펙 준수 확인, 단독 선행)
  ↓ 통과 후
T-review + T-security (병렬)
  ↓ 완료 후
T-merge  (결과 통합)
```

### T-spec: 스펙 준수 확인 (선행 실행)

Task 생성(도구가 있을 때 — 없으면 대화창 진행표의 한 행):
```
T-spec: [Validation/C] 스펙 준수 확인   — blockedBy: 모든 Dev Tasks
```

**T-spec 실행 내용:**
1. `plan.md` 의 `## 구현 계획` 항목 목록화
2. 실제 변경된 코드(`git diff` 또는 worktree 변경 파일)와 대조:
   - 모든 항목 구현 확인 → T-review + T-security 병렬 실행으로 진행
   - 누락/불일치 항목 발견 → **T-spec 실패 흐름** 실행

**T-spec 실패 흐름:**
1. 누락/불일치 항목 목록을 사용자에게 보고
2. 누락 항목에 대한 추가 Dev Task만 생성 (기존 완료 Task 유지)
3. 추가 Dev Task 완료 후 T-spec 재실행
4. **최대 2회 재시도** — 초과 시 사용자에게 판단 요청 후 파이프라인 중단

T-spec 통과 후 아래 T-review, T-security를 동시 생성(도구가 없으면 진행표에 두 행 추가) 후 **병렬 실행**:

```
T-review:   [Validation/A] 코드 리뷰   — blockedBy: T-spec
T-security: [Validation/B] 보안 스캔   — blockedBy: T-spec
```

**병렬 실행:**

- `T-review` → `review-code` 에이전트. **호출 규칙은 `skills/review/SKILL.md` 2단계가 소유한다**
  (`review-code` 에는 Bash 가 없어 diff 를 프롬프트에 **인라인으로 붙이거나** 읽을 파일 경로를
  명시해야 하고, 대상이 **6개를 넘으면 배치로 쪼개며**, 리포트가 `## 완료:` 줄로 끝나지 않으면
  잘린 것으로 취급해 더 작게 다시 부른다). 여기에 복제하지 않는다 — 안 지키면 빈 리뷰가 "결함 0건"
  으로 읽혀 T-merge 를 통과한다.
- `T-security` → `security-scan` 에이전트 (Bash 가 있어 위 제약이 없다)

두 Task 모두 완료 후 결과 통합 Task 생성(도구가 없으면 진행표에 행 추가):

```
T-merge: [Validation] 결과 통합   — blockedBy: T-review, T-security
```

### Feedback ledger 캡처 [건너뛰기 금지]

review-code/security-scan 결과에 **발견된 결함이 있으면(pass·fail 무관)** 각 결함을 정규화해 ledger에 upsert합니다. 같은 실수를 다음 작업에서 사전 차단하는 학습 루프입니다.

```bash
# category ∈ {lint, security, architecture, test, convention}
# severity ∈ {critical, high, medium, low}
# kit_root: skills/plan-task/references/task-tools-fallback.md §A (못 찾으면 "ledger 건너뜀"을 보고에 적는다)
KR=$(kit_root) && python3 "$KR/tools/feedback_ledger.py" upsert <category> <severity> "<결함 요지>"
```

- 발견된 결함만 기록 (통과 시 회피 패턴은 노이즈라 기록 안 함)
- ledger 로직(상한·중복제거·감쇠)은 헬퍼가 보장 — 직접 테이블 편집 금지
- 다음 세션 session-start가 상위 빈도 교훈을 `=== LESSONS ===`로 주입

### T-merge 판정 기준 [건너뛰기 금지]

#### Iron Law — 완료 주장 전 필수 실행

```
완료를 주장하기 전:
1. 어떤 명령이 이 주장을 증명하는가?
2. 지금 그 명령을 실행한다 (fresh run)
3. 출력 전체를 읽는다
4. 출력이 주장을 확인하는가?
   → NO: 실제 상태를 증거와 함께 보고
   → YES: 증거와 함께 주장
명령을 실행하지 않고 완료를 주장하는 것은 오류다.
```

T-review, T-security 결과를 구조적으로 검증:
- `## 완료 조건` 명령을 **지금** 전부 실행했고 전부 rc 0 인가(경계 검사 명령 포함 — 파이프로 rc 를 삼키지 않는다)?
- review-code 리포트의 `## 판정:` 이 `[ACCEPT]` 인가 (CRITICAL·HIGH 0건)?
- 리포트가 `## 완료:` 줄로 끝나는가?
- security-scan 결과에 CRITICAL/HIGH == 0 인가?

→ 모두 충족 시에만 `T-merge` 실행 ("이슈 없음" 판정)
→ 하나라도 미충족 시 이슈 목록과 권고사항을 사용자에게 보고, 파이프라인 중단

`T-merge` 실행:

0. **[Guard]** 판정 기준 미충족 시: 미충족 항목 + 이슈 목록 + 권고사항을 사용자에게 보고하고 파이프라인을 중단한다. 태스크 도구가 있으면 `TaskUpdate(T-merge, status="failed")`, 없으면 진행표의 T-merge 행에 실패와 사유를 적는다. 아래 단계를 실행하지 않는다.
1. **검증 마커 생성** — Stop hook 이중 검증 방지 (Claude Code 전용 — 이 훅이 없는 하네스는 건너뛴다).
   지문·마커 경로 계산은 `stop-validator.py` 모듈이 단일 소스다. 경로는
   `skills/plan-task/references/task-tools-fallback.md` §A 의 `kit_root` 로 찾는다:
   ```bash
   SV=""; KR=$(kit_root) && SV="$KR/hooks/stop-validator.py"
   [ -f "$SV" ] && SV="$SV" python3 - <<'PY'
   import importlib.util, os, tempfile
   spec = importlib.util.spec_from_file_location("stop_validator", os.environ["SV"])
   sv = importlib.util.module_from_spec(spec); spec.loader.exec_module(sv)
   state = sv._worktree_state_hash()
   marker = str(sv.VALIDATED_MARKER)
   d = os.path.dirname(marker)
   os.makedirs(d, mode=0o700, exist_ok=True)
   fd, tmp = tempfile.mkstemp(dir=d)
   with os.fdopen(fd, "w") as fh:
       fh.write(state)
   os.replace(tmp, marker)  # rename은 심링크 자체를 교체(CWE-59)
   PY
   ```
   > 마커에는 검증 스코프 지문(HEAD + 검증 대상 .py 내용 sha256)을 기록한다. Stop hook은
   > 이름이 아니라 이 내용으로 판정한다. 모듈을 못 찾으면 마커 없이 진행한다(fail-open —
   > Stop hook 이 한 번 더 검증할 뿐이다).
2. T-review, T-security 결과와 `## 완료 조건` 명령의 실행 결과(명령·rc)를 `plan.md` 끝의
   `## 검증 결과` 절에 추가하고 frontmatter `status: done` 으로 바꾼다
   (Small 이면 대화창에 출력)
3. **태스크 도구가 있으면** `TaskUpdate(T-merge, status="completed")` + `TaskList`로 이번 계획의 잔존
   in_progress/pending 태스크가 없는지 확인해 정리한다. **없으면** 진행표를 마지막 상태(전 행 done +
   근거)로 한 번 출력하는 것이 이 단계다 — `TaskList` 확인은 해당 없다.
   (T-merge = 검증 **결과 통합** 태스크 — 브랜치 머지가 아니다. 머지 여부는 옵션
   선택의 몫이다 — 계획 `status` 와 무관하다.)
   **반드시 사용자 보고(다음 단계)보다 먼저** — 단발 실행에선 보고 후 턴이 사용자
   입력 대기로 끝나 마킹이 증발한다(태스크 잔존 버그의 근원). 배치 모드(Step 5)에선
   턴이 안 끝나지만 마킹-우선 순서는 동일하게 적용한다.
   마킹 규율의 SSOT는 `rules/definition-of-done.md#task-마감-규율`.
4. 사용자에게 완료 보고 후 브랜치 처리 옵션 제시:

   ```
   Validation 통과. 다음 단계를 선택하세요:
   1. 로컬 머지 (현재 브랜치 → main)
   2. PR 생성
   3. 코드 리뷰 먼저 (`/review`)
   4. 브랜치 유지 (나중에 처리)
   ```

   옵션 3(`/review`)은 결과에 따라 옵션 1 또는 2 로 이어진다.

---

## Step 4: Small — 계획 파일 없이

plan-task 가 Small 로 판정해 계획 파일이 없는 경우, 태스크·상태 파일·위임 없이 가볍게 실행한다
(Small 의 기준·완료 조건 정본: `skills/plan-task/references/elicitation.md` §6):

1. **구현**: 대화창의 계획대로 직접 코드 작성 (위임은 선택 — 별도 컨텍스트가 이득일 때만)
2. **테스트**: 동작이 바뀌었으면 테스트를 추가·수정한다
3. **검증**: 완료 조건 명령을 실행하고 변경분을 스스로 리뷰한다. 보안 스캔은 인증·입력
   처리·시크릿·권한처럼 **보안에 닿는 변경일 때만** 돌린다
4. **보고**: 결과(명령·rc)를 대화창에 출력

---

## Step 5: 배치 모드 — 자율 완주 [opt-in]

여러 계획이 배치로 실행 요청된 경우(예: `/auto-dev docs/plans/A docs/plans/B`,
또는 "활성 계획 전부 진행"), **설계 게이트 통과 후에는 P0·완료·가드 전까지 멈추지
않고** 자율 완주합니다. (Loop Engineering — `rules/loop-engineering.md`)

활성 계획 = `docs/plans/*/plan.md` 중 `status` 가 `done` 이 아닌 것(`plan-format.md`).

```
배치 드라이버 (검증된 Task 시스템 + 스킬 루프, 자체 데몬 없음):
  while (활성 계획 존재):
    1. 요청된 순서대로 다음 활성 계획 선택 (선행 계획이 적혀 있으면 그것이 done 인 것만)
    2. 그 plan.md 원문을 다시 읽고 Step 0~3 (Development → Validation) 실행
    3. validation 통과 → `## 검증 결과` 추가 + `status: done`
    4. 종료 가드 점검 → 다음 계획으로 사람 확인 없이 전진
  → 배치 완료 보고
```

**종료 가드 (필수):**
- P0 모호함 → 즉시 사용자에게 질문(호스트 수단으로), 루프 탈출
- validation 실패가 Stop 훅 block 재시도 후에도 잔존 → 보고 후 해당 계획 정지
- 동일 계획에서 진전 없는 반복(2회+) → 에스컬레이션
- 배치 전체 완료 → 종합 보고

**단발 실행**(계획 하나)은 기존 동작 유지 — 배치 드라이버 미발동.

---

## 참고 문서

플러그인 안의 경로다(소비자 프로젝트 cwd 기준이 아니다):
계획 파일 규약 `skills/plan-task/references/plan-format.md` · Planning 프로토콜 규범 `planning-protocol`.
