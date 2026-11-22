# 킷 도구 탐색 + Task 도구 부재 시 대체 경로 — 단일 소스

> `brainstorming` · `plan-task` · `auto-dev` · `skill-forge` 가 **공통으로 참조**한다.
> 아래 두 절(§A 도구 탐색, §B Task 폴백)을 각 스킬에 복제하지 않는다 — 복제된 계약은 반드시
> 드리프트한다. 경로는 플러그인 루트 기준이다(소비자 프로젝트 cwd 기준이 아니다).

---

## §A. 킷 도구 탐색 규약

스킬이 부르는 도구(`tools/checklist.py` · `tools/feedback_ledger.py` · `hooks/stop-validator.py`
…)는 **플러그인 안**에 있다. 소비자 프로젝트에는 `scripts/` 가 없고, 플러그인이 어디에 풀리는지는
하네스마다 다르다. 그래서 **플러그인 루트**를 아래 순서로 찾는다(앞이 정확하고 뒤가 추정이다):

| 순서 | 출처 | 비고 |
| --- | --- | --- |
| ① | `$CLAUDE_PLUGIN_ROOT` | 비어 있거나 `tools/` 가 없으면 건너뛴다. 스킬의 Bash 컨텍스트에는 이 변수가 안 실리는 하네스가 있다(Codex 쉘에 없음) |
| ② | SKILL.md 기준 상대 경로 `../../` | 스킬이 로드될 때 하네스가 알려 준 그 SKILL.md 의 디렉토리를 `SKILL_DIR` 로 둔다. 플러그인 루트 = `$SKILL_DIR/../..` |
| ③ | `~/.claude/plugins/cache/*/hiway-kit/*/` | Claude Code 설치본. 여러 버전이면 **버전 디렉토리 이름**을 `sort -V` 한 마지막(마켓플레이스 이름이 아니다 — 경로 전체를 정렬하면 옛 버전이 이긴다, 실측) |
| ④ | `~/.codex/plugins/cache/*/hiway-kit/*/` | Codex 설치본. 같은 규칙 |
| ⑤ | 위가 모두 실패 | **멈추고 사용자에게 플러그인 루트 경로를 묻는다.** 알게 되면 `python3 <루트>/tools/<도구>` 로 직접 부른다. `export_harness.py` 는 `--plugin-root <루트>` 로 받는다 |

③④는 "설치된 것 중 최신"이지 "지금 돌고 있는 것"이 아니다 — 여러 버전이 깔려 있으면 ①②가
맞을 때 ①②가 이긴다. **탐색 실패를 조용히 넘기지 않는다**: 못 찾았으면 "도구를 못 찾아 X 를
건너뛴다"고 한 줄 고지한다(건너뛰어도 되는 도구인지는 호출한 스킬이 정한다).

한 곳에만 두는 구현 — 쓰는 쪽은 이 함수를 그대로 붙여 쓰고 `kit_root` 만 호출한다:

```bash
# SKILL_DIR: 이 도구를 부르는 SKILL.md 가 있는 디렉토리(예: <루트>/skills/plan-task). 모르면 비워 둔다.
kit_root() {
  local r p
  for r in "${CLAUDE_PLUGIN_ROOT:-}" "${SKILL_DIR:+$SKILL_DIR/../..}"; do        # ① ②
    [ -n "$r" ] && [ -d "$r/tools" ] && { (cd "$r" && pwd); return 0; }
  done
  for p in .claude .codex; do                                                    # ③ ④
    # 마켓플레이스 디렉토리 이름이 아니라 버전 디렉토리 이름(끝 성분)으로 정렬한다.
    r=$(for d in "$HOME/$p"/plugins/cache/*/hiway-kit/*/tools; do [ -d "$d" ] && echo "${d%/tools}"; done 2>/dev/null \
          | awk -F/ '{print $NF "\t" $0}' | sort -V -k1,1 | tail -1 | cut -f2)
    [ -n "$r" ] && { echo "$r"; return 0; }
  done
  echo "kit_root: 플러그인 루트를 찾지 못했다 — 사용자에게 경로를 물어라 (--plugin-root)" >&2   # ⑤
  return 1
}
# 사용:  KR=$(kit_root) && python3 "$KR/tools/checklist.py" show <plan_dir>
```

**각 Bash 호출은 새 셸이다** — `kit_root` 정의를 쓰는 호출마다 같은 블록에 붙인다(앞 호출에서 정의한
함수는 다음 호출에 남지 않는다).

`KR=$(kit_root)` 가 rc 1 이면 `&&` 뒤가 실행되지 않는다 — 실패를 빈 문자열 경로(`/tools/…`)로
흘리지 않는다.

---

## §B. Task 도구 부재 시 대체 경로 (fail-open)

### 문제

세 스킬 모두 진입 직후 호스트 태스크 도구(Claude Code: `ToolSearch("select:TaskCreate,TaskUpdate,TaskList")`)를
로드한다. 그런데 **Task 계열 도구는 모든 호스트/세션에 있는 것이 아니다**(실측 2026-08-22:
`ToolSearch` 가 `No matching deferred tools found`). 존재를 전제하는 필수 단계는 consumer-first
위반이다 — 이 킷은 설치되는 플러그인이지 이 레포 전용 도구가 아니다.

### 규율

**Task 도구 로드는 시도하되, 부재를 실패로 취급하지 않는다.** 도구가 있으면 원래 절차대로
쓰고, 없으면 사용자에게 한 줄 고지한 뒤 **아래 대체 경로로 계속 진행**한다.

### 대체 경로: 단계마다 추적 수단이 다르다

`checklist.py` 는 항목마다 `id · description · acceptance · verify` 네 필드가 **필수**이고
`verify` 는 비어 있지 않은 **셸 명령**이다(`_REQUIRED_FIELDS`; 빈 verify 는 `init` 이 rc 2 로
거부한다). 의존 순서(`blockedBy`)를 담을 필드는 없고, 정의 밖의 키는 `init` 이 버린다.
그래서 **verify 명령으로 증명할 수 있는 단계만 checklist 에 건다.** 증명할 명령이 없는 단계를
억지로 넣으면 `true` 같은 가짜 verify(게이트 착시)가 된다.

| 단계 | Task 도구가 있을 때 | **없을 때** |
| --- | --- | --- |
| `[Brainstorm]` · `[Planning]` (brainstorming · plan-task) | `TaskCreate/Update` | **대화창 진행표** + 산출물은 `docs/specs/…` · `plan.md` 의 절 |
| `[Dev]` (auto-dev) | `TaskCreate` + `addBlockedBy` | **checklist** (`## 완료 조건` 명령 = `verify`) · 순서는 아래 |
| `[Validation]` T-spec · T-review · T-security · T-merge | `TaskCreate/Update` | **대화창 진행표** + 결과(명령·rc)는 `plan.md` `## 검증 결과` |
| 마감 시 `TaskList` 잔존 확인 | 잔존 in_progress/pending 정리 | **해당 없음** — 진행표 마지막 상태를 보고에 싣는다 |

**대화창 진행표** — 한 번 만들고 단계가 바뀔 때마다 같은 형식으로 갱신해 다시 출력한다.
완료 칸에는 판단이 아니라 **근거(실행한 명령과 rc, 또는 산출 파일 경로)** 를 적는다:

```
| 단계 | 상태 | 선행 | 근거 |
| --- | --- | --- | --- |
| [Planning] 요구사항 | done | - | plan.md `## 요구사항` 작성 |
| [Planning] 구현 계획 | in-progress | 요구사항 | - |
```

**영속은 진행표가 아니라 파일이 한다.** 진행표는 세션을 넘지 못하므로 재개 지점은 항상
`plan.md` 의 `status`·채워진 절과 `checklist.json` 에서 읽는다(`auto-dev` 의 "재개 위치").
Validation 은 checklist 에 항목이 없으므로, `## 검증 결과` 가 비어 있으면 **T-spec 부터 다시**
돌린다(멱등).

**`[Dev]` 항목의 checklist 화** (계획 디렉토리 `<plan_dir>` = `docs/plans/<YYYY-MM-DD>-<slug>/`):

- `verify` 는 `## 완료 조건` 의 명령에서 **파생**한다(실행자가 새로 지어내지 않는다). 항목에
  걸 완료 조건 명령이 없으면 그 항목은 checklist 에 넣지 말고 진행표에서 근거와 함께 추적한다.
- **선행 관계**는 필드가 없으므로 (a) `id` 를 실행 순서대로(`D1`, `D2` …) 붙이고 (b) 선행이
  있으면 `description` 첫머리에 `(선행: D1)` 로 적는다. 의존 순서의 정본은 `plan.md` `## 구현 계획`
  의 번호다. 선행이 미완이면 후행 항목의 `complete` 를 부르지 않는다(도구가 강제하지 않는다 —
  호출하는 쪽의 규율이다).
- **완료 게이트 자체를 항목으로 넣지 않는다** — 아래 금지 절.

```bash
# kit_root 는 §A 의 함수. 실패(rc 1)하면 CL 이 비므로 아래 명령을 실행하지 않고 진행표로 내려간다.
CL=""; KR=$(kit_root) && CL="$KR/tools/checklist.py"
python3 "$CL" init <plan_dir> '[{"id":"D1","description":"...","acceptance":"...","verify":"<셸 명령>"},
                                {"id":"D2","description":"(선행: D1) ...","acceptance":"...","verify":"<셸 명령>"}]'
python3 "$CL" show     <plan_dir>   # 현황
python3 "$CL" complete <plan_dir> D1  # verify 실행 → exit 0 일 때만 완료 전환 (아니면 rc 1)
python3 "$CL" status   <plan_dir>   # 0=전부 완료 1=미완·손상 3=원장 없음
python3 "$CL" verify   <plan_dir>   # 전 항목 재증명(opt-in)
```

`verify` 는 **셸 명령**이어야 한다. "확인했다"는 완료 전환의 근거가 아니다(definition-of-done:
완료는 판단이 아니라 명령의 출력). `checklist.py` 를 못 찾으면(§A ⑤) 위 표의 "대화창 진행표"로
내려가되, `[Dev]` 항목도 근거 칸에 명령과 rc 를 남긴다.

### 계획 파일도 없는 경우 (Small)

Small 작업은 계획 디렉토리가 없어 checklist 도 걸 곳이 없다. **대화창 진행표만** 유지하고, 각
항목의 완료 근거로 실행한 명령과 그 출력을 남긴다(`auto-dev` Step 4). 추적 수단이 없다는 이유로
게이트를 면제하지 않는다.

### 금지

- Task 도구 부재를 이유로 파이프라인을 중단하는 것
- 부재를 조용히 무시하고 **아무 추적 없이** 진행하는 것 (둘 다 결함이다)
- checklist 항목의 `verify` 에 `true` 같은 무조건 통과 명령을 넣는 것 — 게이트 착시. Planning ·
  Brainstorm · Validation 단계를 checklist 에 넣으려고 만든 가짜 verify 가 대표적이다
- **완료 게이트 자체를 항목으로 넣는 것** — 프로젝트의 완료 게이트가 "활성 계획의 checklist
  전항목 완료"를 요구하면(이 킷 레포의 게이트가 그렇다), 게이트를 `verify` 로 둔 항목은 자기참조
  데드락이 된다(게이트가 그 항목을 기다리고, 그 항목이 게이트를 기다린다). 게이트는
  checklist **밖**의 마지막 단계다.
