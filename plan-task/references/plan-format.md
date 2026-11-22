# 계획 파일 규약 — 단일 소스

> `plan-task` 가 만들고 `auto-dev` 가 읽는다. 두 스킬과 session-start 훅이 이 규약 하나를
> 따른다 — 키 이름·절 순서를 다른 곳에서 다시 정의하지 않는다.

## 위치

```
docs/plans/<YYYY-MM-DD>-<slug>/
├── plan.md          # 필수
└── checklist.json   # 선택 — 실행 단계에서 tools/checklist.py 가 만든다
```

- **식별자 = 디렉토리 이름.** 발급기가 없다. 같은 이름이 이미 있으면 `-2`, `-3` 을 붙인다.
- 만드는 시점: `plan-task` 가 **Medium/Large** 로 판정한 작업. Small 은 파일 없이 대화 안에서 끝낸다.
- `docs/plans/` 를 트래킹할지는 프로젝트가 정한다. 킷은 gitignore 하지 않는다 — 병렬 워커가
  계획 원문을 봐야 한다.

## frontmatter — 키 이름 고정

```yaml
---
title: "<한 줄 제목>"
status: planning        # planning | in-progress | done
created: 2026-09-28     # YYYY-MM-DD
size: medium            # small | medium | large
---
```

| `status` | 누가 바꾸는가 |
| --- | --- |
| `planning` | `plan-task` 가 파일을 만들 때 |
| `in-progress` | `plan-task` 완료 시 (auto-dev 로 넘기기 직전) |
| `done` | `auto-dev` 가 검증 통과 후 |

**활성 계획** = `docs/plans/*/plan.md` 중 `status` 가 `done` 이 아닌 것.
`status`·`checklist.json` 쓰기는 **메인 세션만** 한다(`rules/parallel-worktree.md` — 상태는 단일 writer).

## 본문 절 — 이 순서

`## 요구사항` · `## 결정` · `## 구현 계획` · `## 완료 조건` · `## 검증 결과`

- `## 결정` — P0 결정과 그 근거. 사용자가 답한 것은 답 그대로 인용한다.
- `## 완료 조건` — **실행 가능한 명령**(종료코드로 판정). 절차 SSOT: `elicitation.md` §5.
- `## 검증 결과` — `auto-dev` 가 끝에 추가한다. 계획 단계에서는 만들지 않는다.

루프는 요약이 아니라 **이 파일 원문**을 다시 읽는다(`rules/loop-engineering.md` 재앵커).

## 예시

```markdown
---
title: "로그인 실패 5회 시 계정 잠금"
status: in-progress
created: 2026-09-28
size: medium
---

## 요구사항

- 연속 실패 5회 시 15분 잠금 `[confirmed: 사용자]`
- 성공 로그인 시 실패 카운터 초기화 `[researched: OWASP ASVS 2.2.1, 1]`

## 결정

- P0 잠금 중 올바른 비밀번호 입력 → 거부하고 카운터는 늘리지 않는다
  (사용자 답: "잠금 중엔 무조건 거부")

## 구현 계획

1. `auth/lockout.py` — 카운터·잠금 만료 (정책값은 `config/auth.toml`)
2. `auth/login.py` — 잠금 확인을 비밀번호 검증 앞에 둔다
3. `tests/test_lockout.py`

## 완료 조건

- `pytest tests/test_lockout.py -q` (exit 0)
- `ruff check auth/` (exit 0)
```
