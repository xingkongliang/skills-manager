---
name: agent-creator
description: Create a Claude Code sub-agent definition for this project or user. Use when the user asks to create a new custom agent or a specialized sub-agent for a recurring task.
---

# Agent Creator

이 프로젝트(또는 사용자)용 Claude Code 서브에이전트 정의 파일을 만든다.

> **서브에이전트가 없는 하네스**(Codex 등)에서는 에이전트 파일이 인식되지 않는다 — 그 경우
> 같은 절차를 스킬이나 진입점 문서(`AGENTS.md`)의 절로 만드는 편이 맞는지 사용자에게 먼저 묻는다.

## 생성 워크플로우

1. **요구사항 수집** — 목적, 언제 위임할지(구체 조건), 필요한 도구, 파일 수정 여부.
2. **위치 결정** — 프로젝트 공유면 `.claude/agents/<name>.md`, 개인용이면
   `~/.claude/agents/<name>.md`. 설치된 플러그인 캐시 안에는 쓰지 않는다(업데이트 때 사라진다).
   같은 이름이 이미 있으면 덮어쓰지 말고 사용자에게 알린다.
3. **설정 결정** — 도구는 필요한 최소만 허용한다. 읽기 전용 역할에는 `Edit`·`Write` 를 주지
   않는다. 모델·effort 는 생략하면 호스트 기본값을 따른다 — 지정은 이유가 있을 때만.
4. **파일 생성 후 검증** — frontmatter 가 파싱되는지(`name`·`description` 필수, `name` 은
   파일명과 같은 kebab-case), 도구 이름이 호스트에 실제로 있는지 확인한다. 없는 MCP 도구를
   allowlist 에 넣으면 그 서버가 없는 환경에서 에이전트가 환각한다.

## description 쓰는 법

위임은 description 으로 결정된다. **무엇이 주어졌을 때 쓰는지**를 구체적으로 쓴다.
"~해줘"·"확인"·"오류" 같은 일상 표현 단독 트리거는 대화형 요청까지 끌어가 느린 경로로
빼돌린다 — 다른 에이전트와 트리거가 겹치지 않는지도 본다.

## 출력 형식

```markdown
---
name: [소문자-하이픈]
description: [역할]. Use when [구체 조건 — 무엇이 주어졌을 때].
tools:
  - [도구 목록]
---

[시스템 프롬프트 — 역할, 절차, 출력 형식, 끝났음을 알리는 마지막 줄]
```

## 예제

```markdown
---
name: migration-checker
description: Database migration reviewer. Use when a new migration file under db/migrations/ is added or changed and needs a safety review before merge.
tools:
  - Read
  - Grep
  - Glob
---

You review database migrations for safety.

1. Read the changed migration files
2. Flag locking operations, missing down-migrations, and destructive column changes
3. Report findings as a list, then end with `## 완료: <N> findings`
```
