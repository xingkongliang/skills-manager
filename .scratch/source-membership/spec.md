# Spec: Git 来源的技能，上游改名 / 搬家 / 删除时怎么办

## Problem Statement

从 Git 安装的技能，`source_subpath` 是安装当时的目录。上游一旦改名、搬目录，或干脆删掉这个技能，那次更新就只会得到一句 `Path 'skills/x' does not exist in the repository`，被算作"失败"。用户看到的是"15 个失败"，点重试永远失败，而实际它们只是换了位置；真正被删掉的也同样躺着不动。

2026-10-26 实测：hyperframes 把技能从 `skills/<name>` 搬到 `.agents/skills/<name>` 并改了名（`embedded-captions` → `captions-overlay` 等），15 个技能全部变成同一个"失败"；而 `story` 是目录还在、上游删了 3 个文件，走的是"会删文件"的扣下路径。两类完全不同的处境，用户无法区分。

## Solution

把"这个技能在上游还在不在、在哪"变成一个明确的问题，并给出三个结局：

- **At**：还在原路径（正常更新）
- **Moved**：换了路径，带置信度（高＝目录内容哈希与库里一致；低＝目录名或 `SKILL.md` frontmatter name 命中）
- **Gone**：整个仓库里找不到候选

`Moved` 只改来源指针，不动库里任何文件，由用户点一次确认（不自动）。`Gone` 不再催更新，提供「解除来源」与「删除」两个动作。

## User Stories

1. As a 从 Git 安装技能的用户, I want 上游改名时被告知"它搬到了 X", so that 我不用自己翻仓库对比路径.
2. As a 用户, I want 搬家且内容一致时用一个动作完成重指, so that 15 个技能不需要 15 次手工重装.
3. As a 用户, I want 重指不改变我库里的任何文件, so that 我不担心"更新"顺手删东西.
4. As a 用户, I want 低置信候选先给我看路径和理由, so that 我不会把技能指到错的地方.
5. As a 用户, I want 上游真的删掉时不再反复提示更新, so that 列表里没有永远失败的项.
6. As a 用户, I want 被删除的技能可以选择"解除来源"变成普通本地技能, so that 我已改过的副本不丢.
7. As a 用户, I want 「全部更新」只处理真正有新版本的技能, so that 搬家/删除的不再拖慢每一轮.
8. As a 维护者, I want 判定逻辑在一个模块里, so that 更新、单技能更新、将来的重指行为保持一致.
9. As a 维护者, I want 三个结局可测且无需网络, so that 置信度规则不会靠手工点界面验证.
10. As a 维护者, I want 新状态进入既有状态机, so that TTL 与筛选不需要特例.

## Implementation Decisions

**Module：`SourceMembership`**

Interface 一个问题、三个答案：

```
locate(stored_skill, checkout) -> Placement

Placement = At { subpath }
          | Moved { subpath, confidence, reason }
          | Gone
```

- 候选取自 checkout，三条策略按置信度从高到低：目录内容哈希等于 `content_hash` → 高；目录 basename 或 `SKILL.md` frontmatter `name` 等于 `skills.name` → 低。
- 多个低置信候选时返回全部候选，由界面呈现，不替用户挑。
- 只在被扫的 checkout 内找，不跨仓库。
- Implementation 私有；对外只有 `locate`。

**Adapters**：生产实现读真实 checkout；测试实现给一张「路径 → 内容哈希」表。两个 adapter，seam 为真。

**更新轮次接入**

- `prepare_update` 用 `locate` 取代裸 `resolve_skill_dir` 的"在不在"判断。
- 三结局分别落到汇总：`moved`（带候选）、`gone`、其余仍为 `failed`。
- 现状的 `missing_sources` 字段拆成 `moved_sources` 与 `gone_sources`。

**状态机**

- `update_status` 新增 `moved` 与 `source_gone`。
- 两者都进 `should_skip_update_check` 的稳定状态集合：TTL 内不再重复检查。
- 「全部更新」只挑 `update_available`，两个新状态自然排除。

**动作**

- `repoint_skill_source(id, subpath)`：只写 `source_subpath`（与 `source_ref_resolved` 保持），不复制、不删除、不触碰 `central_path`。
- `source_gone` 走既有「解除来源」与「删除」路径，不新增删除语义。

**不做**

- 不自动重指：改来源指针属于用户决定。
- 不跨仓库搜索。
- 不改安装流程。

## Testing Decisions

好测试只断言外部行为：给一个假的 checkout 描述（路径 → 子树摘要）与一条已存技能，断言 `Placement` 与置信度；不断言内部扫描顺序。

- `SourceMembership::locate`：At / 高置信 Moved / 低置信 Moved（多候选）/ Gone / 空 checkout / 同名候选但哈希不同。
- 更新轮次：三结局分别进入 `moved_sources`、`gone_sources`、`failed`，且互不重叠。
- 状态机：`moved` 与 `source_gone` 在 TTL 内被 `should_skip_update_check` 跳过；「全部更新」的选择器不含它们。
- 重指：只改 `source_subpath`，`content_hash`、`central_path`、库内文件不变。

既有先例：`remote_batch` 的记录式 transport 测试（注入即可无网络）、`apply_prepared_update` 的临时目录测试。

## Out of Scope

- 跨仓库/跨 Source 搬家检测。
- 自动合并上游对同一个技能的多次改名历史。
- 删除策略的任何改变（扣下语义保持 #256 的行为）。

## Further Notes

现状证据：`~/.skills-manager/skills-manager.db` 的 `audit_log` 里 2026-10-26 的 15 条 `Path ... does not exist`（hyperframes）与 20 条 `held back`（story-*、short-drama-*）；`15 missing upstream / 0 failed` 的轮次汇总在应用日志里。

用户已确认：**不自动重指，只提供一键**。
