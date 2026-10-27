# 06: 修评审发现（新技能提示）

**What to build:** 让"来源新增了技能"这件事真的能被发现、且不会误把已有技能当新技能。当前实现有一个 Critical：扫描的是**被 sparse 裁剪过的 checkout**，所以永远只能看到那一个技能自己的目录。

**Blocked by:** 01–05（已合并）

**Status:** done (997291c, 49ffe7a, cea857b, 4d0d93e)

## Critical 1：扫描必须读整棵树，不是 worktree

- `scanned` 是首个成员的 checkout，它的 sparse cone 只包含该技能的 subpath。`discover` 走文件系统 → 永远只看到一个目录，然后被当成"已安装"过滤掉。
- 改成从 **git 树**枚举：`git -C <checkout> ls-tree -r --name-only HEAD`，取以 `/SKILL.md` 结尾（或根 `SKILL.md`）的目录，深度 ≤ 6，排除 `.git`。blobless 克隆里树对象是完整的，所以**离线可用**，不需要为读 frontmatter 去拉 blob。
- 名称用目录 basename（不读文件内容）。`discover` 的输入从"checkout 目录"改为"checkout 目录（用于定位 .git）+ 已安装 subpath 列表"，实现改为树枚举；`locate` 保持现有 walk 不变。

## High 2：身份键必须用既有的归一化

- 现在 `installed` 用 `skill.source_ref_resolved` 原字符串、`discover` 用 `RemoteRef::clone_url`；`.git`、尾斜杠、大小写差异会让"已装的"被当成"新的"→ 装上去会覆盖库里活着的目录。
- 一律走 `RemoteRef::identity()`（等价于 `comparison_key`）。拒绝记录的 key 也要带 branch。

## High 3：跨组去重 + 带上 branch

- 组按 `(clone_url, branch)` 分，两个分支会各扫一次；`discover` 的名字去重只在单次调用内生效。
- 轮次内跨组去重：同一个名字（小写）+ 同一 Source 身份只出现一次。
- 前端 dialog 的 key 用 Rust 给的稳定 key（带 branch），不要在前端拼。

## High 4：安装不要在前端拼 tree URL

- 现状：`<clone_url>/tree/<branch>/<subpath>` 拼接再解析——branch 为 NULL 时会被当成分支名 `skills`，`git@` / GitLab 源根本不匹配 `parse_github_tree_url_path`，安装直接失败。
- 新增命令 `install_new_skill(source_ref_resolved, branch, subpath)`：内部构造 `RemoteRef`，复用现有安装工序。**优先复用缓存**（`CacheRefresh::Reuse`，缓存刚被这一轮热过；缺失时自然回落到克隆）。

## High 5：改名不能被当成"新增"

- 同一轮里被判为 `Moved` 的技能，其候选 subpath 不得出现在 `new_skills` 里；否则改名会同时得到"已改名"和"装一个新技能"两个提示，而安装那条会改写已有记录的 `source_subpath`（等于自动重指，违反既定规则）。

## Medium 6：一处摆放两个树的处理

- `DiscoveredSkill.also_at` 目前被丢弃。拒绝时把**所有**树的 subpath 一起记下；提示里的 key 用名字，避免"拒了 `.claude/` 那条、下一轮又拿 `.agents/` 那条来问"。

## Medium 7：拒绝记录会永久增长

- 现在注释声称"来源删掉再添加时失效"，实际不会失效。要么实现剪枝（每轮只保留本轮仍存在的 key），要么删掉这句话并把上限写死（例如 500 条，超出丢最旧）。二选一并写进注释。

## Nit 8

- `held` 与 `tally.held_back` 语义无关 → 改名（如 `known_subpaths`）。
- `if let Ok(skills)` 把数据库读失败当成"没有已装技能" → 会一次性把所有技能当新的。改为失败即整轮跳过发现（记录 warn）。

## 验收

- 新增测试：库里有 `skills/alpha`、树上新增 `skills/beta` → `beta` 被报出（这条在改前为红）。
- 新增测试：改名（`locate` 判 Moved）不产生 `new_skills`。
- 新增测试：`.git` 与尾斜杠差异的同源 URL 不产生重复提示。
- `cargo test --lib` 全绿、`cargo check --all-targets` 无警告、`npm run build` / `npm run lint` / `npm test` 全绿。
