---
name: review
description: Review current changes or specified files - ruff lint for Python files, an adversarial review-code pass, and a security scan only when security-sensitive files changed.
---

# 코드 리뷰 실행

호출되면 바로 파이프라인을 시작한다. 단계가 바뀔 때 한 줄로 진행을 알린다.

## 파이프라인 구조

```
대상 확정 (0단계) → 정적 분석 ruff (0.2) → 복잡도 flag (0.5) → 입력 준비 (1) → 범위 판단 (1.5)
  → review-code (2) → security-scan (3, 보안 관련 파일이 있을 때만) → 요약 (4)
```

---

## 0단계: 리뷰 대상 확정 [가장 먼저]

**대상이 정해지기 전에 정적 분석을 돌리지 않는다** — 대상 없이 도는 ruff 는 아무것도 검사하지 않고도
"통과"로 보고된다.

$ARGUMENTS에 파일/디렉토리가 있으면 그것이 대상이다. 없으면 git 변경분이 대상이다:

```bash
# 비교 기준: 작업 트리(스테이징 포함)에 변경이 있으면 HEAD, 없으면 마지막 커밋(HEAD~1)
BASE=HEAD
[ -z "$(git diff HEAD --name-only)" ] && BASE=HEAD~1
git diff "$BASE" --name-only --diff-filter=d     # 읽을 대상 (삭제된 파일 제외)
git diff "$BASE" --name-only --diff-filter=D     # 삭제된 파일 — 읽을 수 없으니 diff 로만 본다
```

`--diff-filter=d` 는 삭제된 파일을 목록에서 뺀다(없는 파일을 ruff·Read 에 넘기지 않기 위해).
`2>/dev/null` 로 stderr 를 숨기지 않는다 — git 오류(저장소 아님, `HEAD~1` 없음)가 "대상 없음"으로
둔갑한다. 두 목록이 모두 비면 **"리뷰 대상 없음"을 출력하고 멈춘다**(통과가 아니다).

**형식:**

```
## 0단계: 리뷰 대상
- 기준: [인자 지정 | git diff HEAD | git diff HEAD~1 (작업 트리 변경 없음)]
- 대상: [N]개 ([.py N개 …])  삭제: [N]개
```

---

## 0.2단계: 정적 분석 (Python 파일 포함 시)

**0단계 대상에 `.py` 파일이 있으면 그 목록으로 ruff check를 실행합니다.**

```bash
# 0단계 목록의 .py 만. ruff 종료코드를 그대로 읽는다 — xargs 로 이으면 1 이 123 으로 바뀐다.
# 목록은 `sh -c` 로 넘긴다: 호스트의 쉘이 zsh 면 따옴표 없는 $PYS 가 단어 분리되지 않아
# 줄바꿈 낀 한 인자가 되고, ruff 가 "No such file" 을 낸다(실측). 공백 낀 경로는 파일을 하나씩 넘긴다.
PYS=$(git diff "$BASE" --name-only --diff-filter=d -- '*.py')   # 인자를 지정했으면 그 대상 중 .py
if [ -z "$PYS" ]; then echo "Python 파일 없음"
elif ! command -v ruff >/dev/null; then echo "스킵됨 (ruff 미설치)"
else PYS="$PYS" sh -c 'ruff check $PYS'; echo "ruff rc=$?"; fi
```

**결과 처리** (ruff 종료코드 그대로 판독한다: 0 통과 · 1 발견 · 2 ruff 자체 오류):

- 1: 출력을 리뷰 컨텍스트에 포함하여 review-code 에이전트에 전달
- 0: "정적 분석: 통과" 메시지만 출력
- 2: "ruff 오류 — 정적 분석 미수행"과 stderr 를 그대로 보고한다(통과로 쓰지 않는다)
- ruff가 설치되지 않은 경우: "스킵됨 (ruff 미설치)" 메시지 출력

**형식:**

```
## 0.2단계: 정적 분석 (ruff)
- 대상: [파일 목록 또는 "없음 (Python 파일 없음)"]
- 결과: [통과 | N건 발견 | 오류(미수행) | 스킵(ruff 미설치)]
- 발견된 이슈: [있을 때만 출력]
```

---

## 0.5단계: 복잡도 flag — 하이브리드 (advisory 전용, Python 파일 포함 시)

**결정론 flag → LLM 판단** 2단 하이브리드입니다. 복잡도 수치는 **리젝 사유가 아니라
리뷰 초점 신호**입니다.

```bash
# mccabe 복잡도 flag (ruff 내장, 기본 임계 10)
ruff check --select C901 [0.2단계와 동일한 대상 파일]
```

**결과 처리:**

- flag된 함수가 있으면: 목록을 review-code 에이전트 컨텍스트에 다음 지시와 함께 전달 —
  > 아래 함수들은 복잡도 임계를 넘었다(결정론 flag). **줄수·복잡도 수치 자체를 리젝
  > 사유로 삼지 말고**, 다음 관점으로만 분리 타당성을 판단하라: ① 모듈/함수 경계가
  > 단일 책임인가 ② IN/OUT 계약(인자·반환·부수효과)이 명확히 분리 가능한가
  > ③ 분리 시 계약이 단순해지는가(아니면 그대로가 응집적인가). 응집적이면 flag를
  > 기각하고 그 근거를 명시하라.
- flag 없으면: "복잡도: 통과" 한 줄만.
- **절대 blocking 아님**: 이 단계는 어떤 경우에도 리뷰를 실패시키지 않는다
  (전역 lint(E,F)·stop-validator와 무관 — advisory 전용).
- ruff 미설치: "스킵됨 (ruff 미설치)" — fail-open.

**형식:**

```
## 0.5단계: 복잡도 flag (advisory)
- flag: [없음 | 함수 N건 (파일:라인, 복잡도)]
- 처리: [review-code 판단 위임 | 통과]
```

---

## 1단계: 리뷰 입력 준비

0단계에서 확정한 대상의 **내용**을 모은다:

- $ARGUMENTS가 있으면: 해당 파일/디렉토리를 읽는다
- $ARGUMENTS가 없으면: 0단계의 `BASE` 로 `git diff "$BASE"` 를 뽑는다(삭제된 파일은 이 diff 로만 보인다)

---

## 1.5단계: 자동 범위 판단 (Auto-Scope)

**변경 파일을 분석하여 리뷰 범위를 자동으로 결정합니다.**

### 수동 오버라이드 확인

$ARGUMENTS에 `--quick`이 포함되어 있으면:

- scope = 'quick' (사용자 명시 오버라이드)
- security_scan_needed = false
- "수동으로 quick 모드가 지정되었습니다" 메시지 출력
- 1.5단계 나머지 스킵하고 2단계로 진행

### 자동 범위 판단

변경 파일 목록을 분석하여 scope를 자동 결정:

#### 보안 관련 파일 감지

다음 경로 패턴에 매칭되는 파일이 **1개라도 있으면**:

- `**/auth/**`, `**/security/**`, `**/payment/**`
- `**/middleware/**`, `**/*secret*`, `**/*token*`
- `**/config.py`

프로젝트에 보안 경로 규약이 따로 있으면(AGENTS.md·CLAUDE.md 등) 그 경로도 이 목록에 더한다 — 이
스킬은 소비자 프로젝트에도 설치되므로 특정 레포의 디렉토리 이름을 박아 두지 않는다.

→ **scope = 'adversarial', security_scan_needed = true**

#### Trivial 파일 판정

다음 파일 유형이 **전체의 80% 이상**이면:

- `*.md`, `*.txt`, `docs/**`
- `*.json`, `*.yml`, `*.yaml` (예외: `settings.json`, `project.yaml`)

→ **scope = 'quick', security_scan_needed = false**

#### 기본 (일반 코드)

위 두 조건에 해당하지 않으면(소스·스크립트·에이전트/스킬 정의 등 모든 코드):

→ **scope = 'adversarial', security_scan_needed = false**

### 판정 결과 출력

```
🔍 Auto-Scope 판정 결과:
- 변경 파일: [N]개
- 보안 관련: [N]개
- Trivial: [N]개
- **Scope: [adversarial | quick]**
- Security Scan: [필요 | 불필요]
```

---

## 1.7단계: 사용자 의견 처리 원칙 (조건부)

**트리거**: $ARGUMENTS에 파일 경로·옵션(`--quick` 등)이 아닌 **자연어 문장이 포함된 경우**에만 실행.

예시:
- `/review src/auth.py 보안이 걱정돼` → **실행** (자연어 의견 포함)
- `/review src/auth.py` → 실행 안 함 (경로만)
- `/review --quick` → 실행 안 함 (옵션만)

### 처리 절차

```
1. Restate  — 의견을 기술적으로 재술 (오해 없게)
2. Verify   — Read/Grep 도구만으로 해당 코드를 직접 확인 (이 단계에서는 읽기 전용 — Bash 실행 없이)
3. Evaluate — 기술적 타당성 판단
4. Respond  — 동의 or 근거 있는 반박을 review-code 프롬프트에 반영
```

### 금지

- "맞습니다!" / "좋은 지적이에요!" (검증 없는 동의)
- Verify 없이 의견을 바로 리뷰 방향에 반영

### 처리 예시

```
의견: "이 함수는 성능 문제가 있을 것 같아"
Verify: [Read로 함수 확인 → 중첩 루프 발견]
Evaluate: 타당함 — O(n²) 확인됨
→ review-code 프롬프트에 포함: "사용자 지적 — 성능 이슈 가능성, 검증됨"

의견: "이 부분은 타입 체크가 없어"
Verify: [Grep으로 상위 검증 로직 확인 → strict mode 확인]
Evaluate: 상위에서 처리됨 — 중복 불필요
→ review-code 프롬프트에 반영 안 함. 사용자에게 이유 명시.
```

---

## 위임 수단이 없는 하네스 — 강등 경로

아래 `subagent_type:` 블록은 운송 수단일 뿐이고, 불변식은 블록의 `prompt:` 계약(검사 항목·출력
형식·완료 선언)이다. 쓸 수 있는 수단을 호스트에 확인하고 위에서부터 한 칸씩 내려간다:
**① 네이티브 서브에이전트**(블록 그대로) → **② 호스트의 다른 격리 위임 수단**(에이전트 정의
본문 `<플러그인 루트>/agents/<이름>.md` 를 **요약 없이** 넘기고 뒤에 `prompt:` 를
붙인다 — 플러그인 루트 탐색은 `skills/plan-task/references/task-tools-fallback.md` §A. 회신 요약이 아니라 계약
산출물로 판정한다) → **③ 이 세션에서 같은 계약을 직접 수행.** 수단이 없다고 단계를 건너뛰거나
하지 않은 위임을 했다고 보고하지 않는다. ③은 격리가 없다 — 직접 수행한 리뷰는 강등된 리뷰이므로 리포트에 `## Scope Used: [adversarial | quick] (직접 수행 — 격리 없음)` 을 적고, `## 완료:` 종료 선언은 그대로 요구된다.

## 2단계: 코드 리뷰 실행

> **`review-code`에는 Bash가 없다** (`tools: Read/Glob/Grep`, `disallowedTools`에 Bash).
> 따라서 프롬프트에 "`git diff`로 확인하라"고 **위임하면 안 된다** — 에이전트가 실행하지
> 못하고 빈 리뷰를 반환한다(2026-08-23 실측). 1단계에서 **main이 diff를 뽑아 프롬프트에
> 인라인으로 붙이거나**, diff가 크면 **읽을 파일 경로 목록**을 명시해서 넘겨라.
> (`security-scan`은 Bash가 있으므로 이 제약이 없다 — 두 에이전트를 같은 방식으로
> 호출하지 말 것.)
>
> 또한 **에이전트의 마지막 메시지가 곧 반환값**이다. "리포트 형식 그대로, 서두 없이,
> 마지막 메시지로 출력하라"를 프롬프트에 명시하지 않으면 진행 상황 서술이 반환될 수 있다.
>
> **한 번에 넘기는 파일 수를 제한하라.** 두 에이전트 모두 `maxTurns` 가 유한하다. 읽을
> 파일을 10여 개 한꺼번에 넘기면 읽기만으로 턴이 소진되고 **리포트 없이 종료**한다 —
> 호출자에게는 빈 결과가 돌아오고, 그것은 "결함 0건"과 구별되지 않는다(2026-09-09
> 실측: 저장소 규모 대상을 한 번에 넘겨 17회 도구 호출로 턴을 소진, 두 에이전트 모두
> 빈 반환). **대상이 6개를 넘으면 배치로 쪼개 여러 번 호출하라.** 각 배치의 결과를
> main이 모아서 4단계 요약을 만든다. 부분 리포트는 실패가 아니다 — **빈 반환이 실패다.**
>
> **그리고 리포트가 `## 완료:` 줄로 끝나지 않으면 잘린 것으로 취급하라.** *"비었으면
> 실패"* 만으로는 부족하다 — **절단된 출력은 비어 있지 않으므로 그 검사를 통과한다.**
> 의도한 부분 보고와 중간에 끊긴 출력을 가르는 것은 **선언**뿐이다. 잘린 것으로
> 판정되면 그 배치를 더 작게 쪼개 다시 호출하라.


```
Task tool 사용:
subagent_type: review-code
model: opus
prompt: |
  다음 코드를 리뷰해주세요:
  [1단계에서 확인한 코드/diff]

  **정적 분석 결과 (0.2단계):**
  [0.2단계 ruff check 결과 - 있으면 포함, 없으면 "정적 분석 통과" 또는 "Python 파일 없음"]

  **Auto-Scope 설정:**
  - scope: [1.5단계에서 결정된 scope]
  - security_scan_needed: [1.5단계 결과]

  scope가 'quick'이면:
  - CRITICAL, HIGH 등급만 스캔
  - MEDIUM, LOW는 필터링
  - 예상 시간: 1-2분

  scope가 'adversarial'이면:
  - 4개 페르소나 전체 공격
  - 모든 등급 보고
  - 예상 시간: 3-5분

  리포트는 review-code 정의의 출력 형식(## 판정 … ## 완료:)을 그대로 따른다.
```

---

## 3단계: 보안 검토 (조건부)

**1.5단계에서 security_scan_needed = true인 경우에만 실행**

코드에 보안 관련 파일이 포함되면 security-scan을 실행:

```
Task tool 사용:
subagent_type: security-scan
model: sonnet
prompt: |
  다음 코드의 보안 취약점을 검사해주세요:
  [대상 코드]

  검사 항목:
  - OWASP Top 10 취약점
  - 인증/인가 로직
  - 입력 검증
  - SQL 인젝션
  - XSS
```

**security_scan_needed = false인 경우**:

- 이 단계를 스킵하고 4단계로 진행

---

## 4단계: 결과 요약

리뷰 결과를 사용자에게 요약 보고

`review-code` 가 내는 형식 그대로 옮긴다(자체 등급 체계를 새로 만들지 않는다 — 판정·등급 정의는
`agents/review-code.md` 의 "판정 기준"이 소유한다):

### 리뷰 결과

| 항목      | 결과                                     |
| --------- | ---------------------------------------- |
| 판정      | [REJECT / CONDITIONAL / ACCEPT]          |
| Scope     | [adversarial / quick] (직접 수행이면 그렇게 표기) |
| CRITICAL  | [N개]                                    |
| HIGH      | [N개]                                    |
| MEDIUM    | [N개] (quick 에서는 0)                   |
| LOW       | [N개] (quick 에서는 0)                   |
| 완료 선언 | [`## 완료: 전체` / `부분` / 없음(잘림)]  |
| 보안 이슈 | [N개 / 스캔 안 함]                       |

### 수정 필요 사항

[CRITICAL/HIGH 항목 목록 — 필요하면 MEDIUM 까지]
