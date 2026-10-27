# 05: 处理 code review 的发现

**What to build:** 修掉 review 抓到的两个 High、三个 Medium，让 `moved` / `source_gone` 真的稳定下来。

**Blocked by:** 01–04（已全部合并）

**Status:** done (b19e4f2, e2cfb23, bac662e, f33841d, e4dfc06, bae3a16, 2b65d11)

## High 1：下一次检查会把这些状态抹掉

- `should_skip_update_check` 对 `moved` 与 `source_gone` **无条件返回 true**（不只看 TTL）。理由是这两个状态的含义是"自动更新对它已无意义"，出口只有重指或删除。
- 检查路径（`check_skill_update_internal_with_remote` 与 Phase B 的写入）不得把 `moved` / `source_gone` 覆写成 `up_to_date` / `update_available`。
- 加一条测试：`moved` 的行跑一次 `check_skill_update_internal_with_remote` 后状态与候选都还在。这条测试在改前必须是红的。

## High 2：`source_gone` 没有「解除来源」

- 新增 `detach_skill_source(id)`：把 git 技能变成本地技能——`source_type = "local"`、`source_ref = central_path`、清空 `source_ref_resolved` / `source_subpath` / `source_branch` / `source_revision` / `remote_revision`，状态 `up_to_date`。**不动任何文件。**
- 拒绝已经在 `local` / `import` 的行。
- 界面 `source_gone` 行出现「解除来源」，与「删除」并列。

## Medium 3：单技能更新把 Moved/Gone 报成失败

- `UpdateSkillResult` 带上 `moved`（候选）与 `gone`，单技能路径不再返回 `Err`；审计条目用 `UpdateAudit` 的对应分支，不用 `Failed`。
- 前端单技能提示与批量提示使用同一套文案。

## Medium 4：skills.sh 绕过 `locate`

- 当 `locate` 给出 `Moved` / `Gone` 时，**不得**再走 `resolve_skill_dir` 的 locator 兜底并自动改写 `source_subpath`；这条路径只能由用户点「重新指向」触发。

## Medium 5：轮次的四类结局没有测试

- 加测试：一批包含 At / Moved / Gone / 失败 的技能，四类各自落桶且互不重叠。
- 加测试：`update_available` 选择器不含 `moved` / `source_gone`。

## 不做（记录原因）

- `.scratch/**` 已在提交里（`8386c18` 早于本分支），不重写历史。
- `Placement` 的 `Serialize` 暂无用例：保留，因为 04 的候选已由 DB 承载，序列化是死代码，可删——**删掉它**，不加 derive 白留。
- 每行查询候选（`get_skill_move_candidates`）：先不改，量过再说。

## 验收

- `cargo test --lib` 全绿且新增测试在改前为红（至少 High 1 那条）
- `cargo check --all-targets` 无警告
- `npm run build` / `npm run lint` / `npm test` 全绿
