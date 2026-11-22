---
name: plan-task
description: Structured task planning into a plan file. Use for any new feature, bug fix, or project that needs a task breakdown before implementation.
---

# Plan-Task 스킬

## 사용법

```
/plan-task 로그인 기능 추가                       # 새 요청
/plan-task fix the auth bug                      # 새 요청 (description 형식)
/plan-task docs/plans/2026-09-28-login-lockout   # 기존 계획 재개 (디렉토리 또는 그 plan.md)
```

## Phase Gate

`brainstorming`(설계 승인)은 **설계가 필요한 Large 새 기능**에만 선행한다 — 크기 기준은
`skills/plan-task/references/elicitation.md` §6 이 소유한다. 그런 작업인데 `docs/specs/`에 관련
스펙이 없으면 먼저 `brainstorming` 을 invoke 한다. 버그 수정·Small·Medium 은 바로 진행한다(건너뛴
사실만 한 줄 적는다).

---

요구사항을 명확히 하고 구현 계획을 세운다. Medium/Large 면 결과를 **계획 파일**에 남긴다 —
규약(위치·frontmatter·절 순서) SSOT: `skills/plan-task/references/plan-format.md`.

**영속 상태는 `plan.md`(와 실행 단계의 checklist)다.** 호스트 태스크 도구는 진행 추적에만
쓴다 — 세션 스코프이거나(Claude Code) 아예 없는(Codex 등) 하네스가 있어서 세션을 넘지 못한다.

---

## Step 0: 진입 [건너뛰기 금지]

1. 호스트 태스크 도구가 있으면 로드한다(Claude Code: `ToolSearch("select:TaskCreate,TaskUpdate,TaskList")`).

   > **Task 도구가 없으면 멈추지 말고 대체 경로로 간다** — `skills/plan-task/references/task-tools-fallback.md`
   > §B 가 소유한다. `[Planning]` 단계의 대체는 **대화창 진행표 + `plan.md`** 다 — checklist 에는
   > 걸지 않는다(걸 `verify` 명령이 없다).

   이미 `[Planning]` Task 가 있으면 생성을 건너뛴다. 없으면(도구가 있을 때만):

   ```
   T1: [Planning] 요구사항 명확화
   T2: [Planning] 구현 계획 수립   ← blockedBy T1
   ```

2. **기존 계획 경로가 주어졌으면** 그 `plan.md` 를 읽고 `status` 와 비어 있는 절을 보고
   중단된 지점부터 재개한다. `status: done` 이면 재개하지 않고 사용자에게 알린다.

3. **새 요청이면** 아직 파일을 만들지 않는다 — 규모 판정(Step 1-1) 후에 만든다.

---

## Step 1: T1 — 요구사항 명확화

`clarify-requirements` 에이전트에 위임하거나 직접 진행.

> **절차 SSOT: `skills/plan-task/references/elicitation.md`** — 규모가 Medium 이상이면 **읽고 시작한다.**
> 등급 체계만으로는 *"딱히 모호한 게 없었다"* 로 끝나고, 못 찾은 모호함은 사라지지 않고
> 구현 중 추측으로 메워진다.

1. **규모 판단** — 기준(Small/Medium/Large 임계값)과 규모별 완료 조건은
   `skills/plan-task/references/elicitation.md` §6 이 소유한다. 여기에 옮겨 적지 않는다.

   **Medium/Large 면 지금 계획 파일을 만든다**: `docs/plans/<오늘 YYYY-MM-DD>-<slug>/plan.md`
   (이름이 있으면 `-2`, `-3`). frontmatter 는 `status: planning` 과 판정한 `size`,
   본문은 `skills/plan-task/references/plan-format.md` 의 절 머리만 둔다(`## 검증 결과` 제외). **Small 은 파일 없이** 진행한다.

2. **자격 인벤토리** (Large 필수 · Medium 권장 — `elicitation.md` §0):
   확보된 권한을 세고 그 경계를 **이번 범위선**으로 삼는다. 미확보 항목은 기다리지 말고
   mock/adapter 로 처리한다. 이 인벤토리가 그대로 **위임 브리프의 금지사항**이 된다.

3. **모호함 탐색** — 감지되기를 기다리지 않고 네 자리를 훑는다 (`elicitation.md` §2):
   상태 전이가 갈라지는 지점 · 두 규칙이 동시 적용되는 지점 · 부등호의 등호 포함 여부 ·
   실패 후 롤백 범위. **하나도 안 나왔다면 훑지 않은 것이다** — 어디를 봤는지 기록한다.

   **구현 결과를 바꾸는 것만 묻는다.** 조사로 채울 수 있는 값은 묻지 말고
   `[researched: 출처, n]` 으로 채운 뒤 검토만 요청한다. 한 번에 5개 이하로 묶는다.

4. **P0 모호함 해결**: P0 발견 시 즉시 중단 → 사용자에게 질문.
   P1~P3 는 등급대로 처리하고 멈추지 않는다. P0 의 답은 근거와 함께 `## 결정` 에 적는다.

   ```
   맥락: [상황]   질문: [구체적 질문]
   옵션: 1. [A]   2. [B]
   ```

5. **요구사항 정리**: 핵심 요구사항, 영향 범위, 리스크.
   **모든 값에 출처 태그**(`[confirmed]` / `[researched: 출처, n]` / `[unresolved]`)를
   붙인다. 상충은 고르지 말고 상충 사실을 기록해 설정값으로 위임한다.
   정책값(임계값·상한·기간·비율)은 산문에서 빼내 한 곳에 모은다 — **산문에 숫자가 남아
   있으면 미완성**이다.

6. **기록** (계획 파일이 있을 때): 결과를 `## 요구사항`, P0 결정을 `## 결정` 에 쓴다.
   brainstorming 스펙(`docs/specs/…`)이 있으면 `## 요구사항` 첫 줄에 그 경로를 적는다.

7. T1 완료 마킹 — 도구가 있으면 `TaskUpdate(T1, status="completed")`, 없으면 진행표의 근거 칸을 채운다

---

## Step 2: T2 — 구현 계획 수립

`plan-implementation` 에이전트에 위임하거나 직접 진행:

1. T1 결과(`## 요구사항`·`## 결정`) 기반으로 구현 계획 작성
2. 규모별 추가 단계 (`skills/plan-task/references/elicitation.md` §6):
   - Medium+: 사용자 여정 설계 포함
   - Large+: 비즈니스 로직 정의 포함
3. 구현 순서, 의존성, 예상 범위 명시

4. **완료 조건을 실행 가능한 명령으로 쓴다** [건너뛰기 금지] (`skills/plan-task/references/elicitation.md` §5):
   각 단계의 완료 조건은 **종료코드로 판정되는 명령**이어야 한다.

   ```
   ✗ 로그인이 정상 동작한다
   ✓ pytest tests/test_auth.py -q     (exit 0)
   ```

   판정이 stdout 에 있는 검증(조회 성공만으로 exit 0 이 되는 명령)은 게이트가 아니다.
   비결정적 산출물이 섞이면 조건을 둘로 나눈다 — 결정적 부분만 통과/실패로, 나머지는
   고정 평가셋과 임계값으로. **금지 행위 위반은 임계값이 아니라 0이다.**

5. **경계 검사** (해당할 때만): 계획이 모듈·레이어 경계나 의존 방향을 정하거나 바꾸거나, 프로젝트에
   경계 검사 설정이 이미 있고 이번 변경이 그 설정이 다루는 패키지 사이의 import 를 건드리면
   `skills/plan-task/references/boundary-check.md` 를 읽고 그 검사 명령을 4의 완료 조건에 반영한다. 해당하지 않으면 건너뛴다.

6. **기록** (계획 파일이 있을 때): `## 구현 계획` 과 `## 완료 조건` 을 채운다.

7. T2 완료 마킹 (T1 과 같다 — 도구가 있으면 `TaskUpdate`, 없으면 진행표)

---

## Step 3: Planning 완료 처리

0. **[건너뛰기 금지]** auto-dev invoke(핸드오프) **전에** 이 계획의 `[Planning]`/`[Brainstorm]` 중
   "끝났는데 마킹 안 된" 태스크를 completed로 정리한다 — 호스트 태스크 도구가 있을 때만
   (규율 SSOT: `rules/definition-of-done.md#task-마감-규율`). 도구가 없으면 진행표를 마지막 상태로
   한 번 더 출력하는 것이 이 단계다.
1. 계획 파일이 있으면 네 절이 채워졌는지 확인하고 frontmatter `status: in-progress` 로 바꾼다.
   Small 이면 요구사항·계획·완료 조건을 대화창에 출력한다.
2. 다음 단계:

Planning이 완료되었습니다. 바로 개발을 시작하겠습니다.

`auto-dev` 스킬을 즉시 invoke합니다 — 계획 디렉토리를 인자로 넘긴다. 사용자가 "나중에" 또는
"직접 실행"을 원하면 아래 명령을 안내하고 invoke를 건너뜁니다.

**Small 은 계획 파일이 없다.** Small 경로의 정본은 `elicitation.md` §6 이고(바로 구현 + 완료 조건
명령으로 검증), plan-task 가 Small 로 판정하면 대화창의 계획을 인자 없이 `auto-dev` 에 넘겨 그
Step 4(가벼운 경로)로 실행하거나, 사용자가 원하면 `auto-dev` 없이 직접 구현한다. 어느 쪽이든
Medium 이상의 절차(계획 파일·checklist·Validation 3단계)를 Small 에 씌우지 않는다.

```
/auto-dev docs/plans/<YYYY-MM-DD>-<slug>
```

---

## 참고 문서

플러그인 루트 기준 경로다(소비자 프로젝트 cwd 기준이 아니다):
계획 파일 규약 `skills/plan-task/references/plan-format.md` · 요구사항 정련 절차
`skills/plan-task/references/elicitation.md` · 경계 검사 절차
`skills/plan-task/references/boundary-check.md` · Task 도구 폴백·킷 도구 탐색
`skills/plan-task/references/task-tools-fallback.md` · Planning 프로토콜은 규범
`planning-protocol`(세션 주입).
