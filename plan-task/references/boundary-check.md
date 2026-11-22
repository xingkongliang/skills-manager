# 아키텍처 경계를 프로젝트 도구로 강제한다

계획에서 "domain 은 infra 를 import 하지 않는다"를 정해도, 그 문장이 **문서에만** 있으면 다음
변경에서 조용히 깨진다. 이 문서는 그 결정을 **프로젝트가 이미 가진 도구의 설정과 검사 명령**으로
옮겨, 계획의 `## 완료 조건` 에 태우는 절차를 소유한다. 그러면 `auto-dev` 의 기존 경로(완료 조건
명령 → checklist `verify` → exit 0 으로만 통과)가 그대로 실행한다 — 새 게이트·훅·스킬은 없다.

이 문서 안의 경로는 이 스킬 디렉토리 기준이다(소비자 프로젝트 cwd 기준이 아니다).

## 적용 조건 (이 밖에서는 아무것도 추가하지 않는다)

> **계획의 기술 결정이 모듈·레이어 경계나 의존 방향을 정하거나 바꿀 때, 또는 프로젝트에 경계 검사
> 설정이 이미 있고 이번 변경이 그 설정이 다루는 패키지 사이의 import 를 추가·이동할 때** 적용한다.

경계와 무관한 버그 수정·Small 작업은 대상이 아니다 — 상시 적용되는 절차는 죽는다. Small(계획 파일
없음)에서는 프로젝트의 평소 lint/test 가 그 도구를 이미 돌린다면 그 경로로 잡힌다.

## 탐지 — 프로젝트가 이미 가진 것만 쓴다

킷은 도구를 설치하지도 가정하지도 않는다. 순서:

1. **프로젝트 자신의 진입점** — 패키지 스크립트(`package.json` scripts)·Makefile/justfile 타깃·
   tox/nox·pre-commit·CI 설정에서 아래 표의 도구를 부르는 곳. 있으면 그 명령을 그대로 쓴다.
2. **도구 설정 파일 + 프로젝트 로컬 실행 파일** — 진입점은 없지만 설정 파일이 있고 실행 파일이
   프로젝트 안에 설치돼 있을 때(예: `node_modules/.bin/` 아래, 가상환경 안).

**`npx`·`uvx`·`pipx run`·전역 설치 호출로 검사 명령을 만들지 않는다.** 레지스트리를 두드리거나
소비자 환경에 없는 것을 있다고 가정하는 것이다(auto-format 이 `npx` 로 레지스트리를 조회하던 결함과
같은 클래스). Glob/Grep 으로 설정 파일·스크립트를 찾는다 — 못 찾았으면 아래 "도구가 없을 때"다.

### 탐지표

행마다 **설정 위치 · 실행 명령 · 잡는 것 · 위반 시 비0 종료 근거**를 공식 자료로 확인했다.
`위반 시 비0 종료`가 자료로 확인되지 않은 도구는 표에 넣지 않았다 — 판정이 stdout 에만 있는 검사는
게이트가 아니다(`SKILL.md` Step 2-4).

| 도구 | 설정 위치 | 실행 명령 | 잡는 것 | 위반 시 비0 종료 근거 |
| --- | --- | --- | --- | --- |
| Python **import-linter** | `.importlinter` · `setup.cfg`(INI) · `pyproject.toml`(`[tool.importlinter]`) — [researched: https://import-linter.readthedocs.io/en/stable/get_started/configure/] | `lint-imports` (`--config <path>` 로 설정 지정) — [researched: https://import-linter.readthedocs.io/en/stable/get_started/run/] | 계약 유형 `forbidden`·`protected`·`layers`·`independence`·`acyclic_siblings` — [researched: https://import-linter.readthedocs.io/en/stable/] | 계약이 깨지면 종료코드 1. 산문 문서가 아니라 공식 저장소 소스의 `EXIT_STATUS_ERROR = 1` 이다 — [researched: https://raw.githubusercontent.com/seddonym/import-linter/main/src/importlinter/cli.py] |
| Python **Tach** | `tach.toml`(프로젝트 루트), 모듈 옆 `tach.domain.toml` — [researched: https://docs.gauge.sh/usage/configuration] | `tach check` — [researched: https://docs.gauge.sh/usage/commands] | 모듈 간 의존(`depends_on`·`cannot_depend_on`), `layers`, 공개 인터페이스, `forbid_circular_dependencies = true` 일 때 순환 — [researched: https://docs.gauge.sh/usage/configuration] | "When an error is detected, `tach check` will exit with a non-zero code." — [researched: https://github.com/gauge-sh/tach] |
| JS/TS **dependency-cruiser** | `.dependency-cruiser.js`(그 외 `.cjs`·`.mjs`·`.json` 등) — [researched: https://github.com/sverweij/dependency-cruiser/blob/main/doc/cli.md] | `dependency-cruiser [options] <파일-또는-디렉토리>` — [researched: https://github.com/sverweij/dependency-cruiser/blob/main/doc/cli.md] | `forbidden`(금지 의존)·`allowed`(허용 목록)·`required`, `"circular": true` 로 순환 — [researched: https://github.com/sverweij/dependency-cruiser/blob/main/doc/rules-reference.md] | 규칙 severity 가 `error` 인 위반 개수가 종료코드다. **기본 출력형(`err`)일 때만** 그렇다 — `dot` 같은 그래프 출력형은 종료코드를 만들지 않는다. severity `warn` 은 판정하지 않는다 — [researched: https://github.com/sverweij/dependency-cruiser/blob/main/doc/cli.md] |
| JS/TS **ESLint `import/no-cycle`** (eslint-plugin-import) | ESLint 설정의 `rules` — [researched: https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-cycle.md] | 프로젝트의 ESLint 실행 진입점(`eslint [options] [file\|dir\|glob]*`) — [researched: https://eslint.org/docs/latest/use/command-line-interface] | 모듈로 되돌아오는 import 경로(순환). 깊이는 `maxDepth` 로 제한 — [researched: https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-cycle.md] | 규칙이 `error`(2)여야 한다: "exit code is 1 when triggered". `warn` 은 종료코드를 바꾸지 않는다 — [researched: https://eslint.org/docs/latest/use/configure/rules] · 종료코드 1 = "at least one linting error" — [researched: https://eslint.org/docs/latest/use/command-line-interface] |
| JS/TS **eslint-plugin-boundaries** | ESLint 설정의 `settings`(`boundaries/elements`·`boundaries/files`)와 `rules`(`boundaries/dependencies`) — [researched: https://github.com/javierbrea/eslint-plugin-boundaries] | 위 ESLint 실행 진입점과 같다 | 요소(레이어) 타입 기반 허용 import(`default: "disallow"` + 허용 정책). **순환은 이 플러그인의 범위로 확인되지 않았다** — 순환이 필요하면 `import/no-cycle` 이나 dependency-cruiser 를 쓴다 | ESLint 와 같다: 규칙을 `error`(2)로 둔다 — [researched: https://eslint.org/docs/latest/use/configure/rules] |
| Nx **`@nx/enforce-module-boundaries`** | ESLint 설정의 `rules`(flat config 예시는 `eslint.config.mjs`), `depConstraints` 의 태그 — [researched: https://nx.dev/docs/features/enforce-module-boundaries] | 프로젝트의 ESLint 실행 진입점 | 태그 기반 허용 import(`sourceTag` → `onlyDependOnLibsWithTags`) — [researched: https://nx.dev/docs/features/enforce-module-boundaries] | ESLint 와 같다: 예시대로 `'error'` 심각도 — [researched: https://eslint.org/docs/latest/use/configure/rules] |
| Go **golangci-lint `depguard`** | `.golangci.yml`(그 외 `.yaml`·`.toml`·`.json`)의 `linters.settings.depguard`, 린터 활성화는 `linters.enable` — [researched: https://golangci-lint.run/docs/configuration/file/] | `golangci-lint run` — [researched: https://golangci-lint.run/docs/welcome/quick-start/] | import 경로가 허용/금지 목록에 맞는지(`allow`·`deny`, `files` 글롭으로 범위 지정). 순환·레이어 검사를 제공한다는 문서는 없다 — [researched: https://golangci-lint.run/docs/linters/configuration/] | `--issues-exit-code`: "Exit code when issues were found (default 1)" — [researched: https://golangci-lint.run/docs/configuration/cli/] |

**표에 넣지 않은 것** — Java ArchUnit(JUnit 테스트로 실행)과 Go 의 컴파일러 수준 규칙(import 순환·
`internal/`)은 위반이 빌드/테스트 실패로 이어진다고 추정할 수는 있지만, **비0 종료를 명시한 공식
문서를 확인하지 못했다.** 프로젝트가 이미 그 테스트/빌드를 완료 조건에 갖고 있으면 그것을 쓰되,
이 표가 보증하는 것은 아니다.

ESLint 계열 세 행은 도구 자체가 아니라 **ESLint 의 종료코드 규약**에 기대고 있다 — 규칙 심각도가
`warn` 이면 아무리 위반해도 rc 0 이므로, 설정을 읽을 때 심각도를 먼저 본다.

## 계획에 넣는 네 가지

경계를 정하거나 바꾸는 계획은 다음을 **빠짐없이** 담는다(자리는 `plan-format.md` 의 절 이름):

1. **경계 결정 문장** — `## 결정` 에 *"A 는 B 를 import 하지 않는다"* 처럼 검사할 수 있는 문장으로.
   이어서 **강제 수단** 한 줄: `경계 강제: <도구> — 설정 <경로> — 검사 <명령>`.
2. **설정 갱신 항목** — `## 구현 계획` 의 한 배치로: 결정을 도구의 설정 파일로 옮기는 변경(새 계약·
   태그·규칙). 코드 변경보다 **먼저** 또는 같은 배치에서 한다.
3. **검사 명령** — `## 완료 조건` 에 탐지한 명령을 그대로. 종료코드로 판정되고(파이프로 rc 를 죽이지
   않는다), 위반 시 비0 이어야 한다. 표의 "비0 종료 근거" 열을 확인한 뒤 쓴다.
4. **양성 대조** (새 계약을 추가하거나 기존 계약을 **확장**한 계획만) — 검증 단계에서 *경계를 어기는
   import 1줄을 넣는다 → 검사 명령이 rc≠0 → 그 줄을 되돌린다 → rc 0* 을 한 번 기록한다.
   한 번도 red 가 나지 않은 검사는 검사가 아니다. 기존 계약을 그대로 쓰기만 하면 생략한다.

## 도구가 없을 때 (fail-open — 조용히 넘어가지도, 조용히 설치하지도 않는다)

탐지에서 아무것도 못 찾았으면 계획 `## 결정` 에 다음을 적는다:

```
경계 강제: 없음 — <이유: 프로젝트에 표의 도구도 그 설정도 없다 / 생태계 표 밖이다 등>
```

그리고 그 생태계의 대표 도구 도입을 **P1 결정**으로 사용자에게 제안한다(등급 SSOT: 규범
`planning-protocol`). **기본값은 도입하지 않음**이다 — 사용자가 확인했을 때만 도입을 계획에 넣는다.
의존성 추가는 소비자가 정할 일이지 킷이 강요할 일이 아니다.

## 완화 금지 — 검사를 통과시키려고 경계 설정을 고치지 않는다

green 을 만드는 가장 싼 길은 코드가 아니라 **설정을 고치는 것**이다. 다음은 모두 완화다:

- 허용 목록·예외 추가(`ignore_imports`·`allow`·`onlyDependOnLibsWithTags` 확대, 규칙 `off`/`warn` 강등)
- 계약·규칙 삭제
- 검사 범위 축소(대상 패키지·`files` 글롭·`--exclude` 축소)

구현은 이를 하지 않고 `ARCHITECTURE_LIMIT` 로 멈춰 보고한다. 리뷰는 **계획 `## 결정` 에 근거 없는**
완화를 HIGH 로 판정한다. 계획이 경계 변경을 **결정**했다면(근거가 `## 결정` 에 있다면) 그것은
완화가 아니라 경계의 개정이다 — 이때는 위 "네 가지"를 다시 채운다.

## 흔한 실패

- 훈련 기억으로 도구 이름·설정 파일명·명령을 적는다 → 이 표와 프로젝트의 실제 설정을 대조한다.
- 검사 명령이 **경계 설정을 읽지 못하는 위치**에서 돈다(cwd·`--config` 누락) → 위반이 있어도 rc 0.
  양성 대조가 이것을 잡는다.
- 심각도 `warn` 으로 설정돼 있는데 게이트라고 믿는다 → 표 아래 주의 참고.
- `cmd | tail` 처럼 파이프로 검사 명령의 rc 를 삼킨다 → 파일로 받거나 `pipefail`.
- 경계 결정을 계획에 적었지만 설정 갱신 배치가 없다 → 다음 변경에서 조용히 깨지는 원래 결함 그대로.
