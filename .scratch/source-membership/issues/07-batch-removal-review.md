# 07: 批量审阅「会删文件」的技能

**What to build:** 一次更新结束后，被扣下的技能不止报个数字。给用户一屏可展开的清单：每个技能会删哪些路径、删在库目录还是 Agent 副本里，勾选后按技能逐个批准。

**Blocked by:** 无（ticket 05/06 已合并，审批模型已存在）

**Status:** done (bd56121, 7ea2971)

## 现状

`apply_prepared_update` 在锁内算出 `pending_removals`，非空就返回 `Held{pending, approval}`，轮次只把技能名收进 `held_back`，**路径和令牌都被丢掉**。用户只能一个个点开技能单独更新，让那一次重新计算、再弹一次确认。

2026-10-27 实测：20 个技能、175 个路径，逐个点需要 20 轮。

## 设计决定

**1 · 落库而不是重算**

新增表 `skill_pending_removals(skill_id, revision, location, path)`（与 `skill_move_candidates` 同族，迁移版本顺延）。

- `apply_prepared_update` 在返回 `Held` 之前写入，附带计算令牌用的 `revision`。
- `location` 保留现有语义：`library`（`LIBRARY_LOCATION`）或某个 Agent 的 key。用户需要知道去哪个目录抢救文件。
- 一个技能批准或状态离开 `held back` 时删除其行。轮次里"这次没被扣下"的技能也清空。

**2 · 批准仍然是按技能、仍然用令牌**

- 面板的批准动作对每个勾选的技能调用现有单技能更新路径，带上该技能存储的令牌。
- **不做"无令牌批量强删"**：令牌是"这份清单被看过"的证据，批量不能绕过它。
- 令牌失效（revision 变了）时，更新会重新计算并重新扣下 —— 面板显示"清单已更新，请再看一次"，不静默应用。

**3 · 不做跨技能原子性**

10 个批准里 8 个成功 2 个失败，就如实报 8/2；不搞全成功或全回滚。删文件是按技能的事。

**4 · 面板形态**

- 每个技能一行：名字、会删多少、来源仓库。
- 展开后按 `location` 分组列出路径（库目录在前）。
- 底部：全选 / 取消全选（复用 `lib/newSkillSelection.ts` 的手势规则）、`批准所选（N）`。
- 默认**全不选**。

## 接口

```
list_pending_removals() -> Vec<PendingRemovalGroup>
  { skill_id, skill_name, source, revision, location, paths[] }
approve_removals(skill_ids: Vec<String>) -> { approved, failed: Vec<{name, message}> }
```

`approve_removals` 内部逐个走既有单技能更新，不做并发（删除是破坏性动作，串行更容易解释失败）。

## 测试

- 写库：一次 `Held` 之后，`list_pending_removals` 能读到该技能的全部路径与 revision。
- 清理：批准成功后该技能的行消失；轮次里变成 `up_to_date` 的技能行也消失。
- 令牌过期：改掉存储的 revision，再批准 → 不得删除任何文件，状态回到 `held back` 或重新扣下。
- 批量：3 个批准里 1 个令牌失效 → 返回 `approved: 2, failed: 1`，且失败那个的文件仍在。
- 选择器复用：`newSkillSelection` 的规则不变（已有测试）。

## 不做

- 自动批准、按路径模式的批量白名单。
- 把删除做成可撤销（这需要另一套快照，另开）。
- 跨技能事务。

## 验收

- `cargo test --lib` 全绿（改前 674）；`cargo check --all-targets` 无警告。
- `npm run build` / `npm run lint` / `npm test` 全绿。
- 手工：拿 `media-use`（70 个路径，属于目录重组）验证——面板列出 70 条 `.mjs`，批准后技能仍可打开，`skills/<name>` 目录内容与上游一致。
