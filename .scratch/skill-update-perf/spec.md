# Spec: 让 skill 更新按 Source 计数

追踪器：本仓库未运行 `/setup-matt-pocock-skills`，沿用本地 markdown 约定（`.scratch/<feature>/`）。标签：`ready-for-agent`。

## Problem Statement

用户从 GitHub 通过「安装 Skills → Git 安装」导入了几十个 Skill，它们来自十几个仓库，之后靠「检查全部 / 全部更新」维持最新。

「全部更新」要等几分钟，界面上只有一个忙状态，看不出进行到哪一步。库越大越慢，而技能数增长不等于仓库数增长——同一个仓库往往装着十几个技能。

慢的原因不是网络本身：更新阶段按**技能**去访问远端，而不是按 **Source**。每个技能各自解析远端 head、各自刷新仓库缓存、各自把整棵缓存复制到临时目录、各自全树哈希两次、各自重写一遍全库元数据。40 个技能、12 个 Source 的库，做的是 40 次该做的事，而理论下限是 12 次。

## Solution

更新改为按 Source 组织：每个 Source 只解析一次、只取一次代码，该 Source 下的所有技能共用这一次的结果。同一批更新里，元数据整库重写只发生一次，而不是每个技能一次。

对用户的结果：同样的库，「全部更新」的墙钟时间显著下降；检查与更新的行为、审批语义、错误提示不变。

## User Stories

1. As a 技能库用户, I want 「全部更新」的耗时随 Source 数量增长而不是随技能数量增长, so that 我从少数几个仓库装了很多技能时不必等几倍的时间.
2. As a 技能库用户, I want 同一仓库的多个技能在一次更新里只下载一次代码, so that 我的网络流量与磁盘写入与实际变化量相称.
3. As a 技能库用户, I want 更新失败时仍按技能逐个报告, so that 我能分辨是哪个技能、哪个仓库出了问题.
4. As a 技能库用户, I want 某个 Source 取代码失败时其它 Source 的技能继续更新, so that 一个坏仓库不会让整批停摆.
5. As a 技能库用户, I want 「需要移除文件」的审批语义保持不变, so that 更新永远不会悄悄删掉我库里的东西.
6. As a 技能库用户, I want 更新过程中的「仓库忙」提示不再频繁出现, so that 并行的后台检查不会把我的手动更新挤掉.
7. As a 技能库用户, I want 更新后我的 Agent 目标与项目副本仍然同步, so that 我不需要手动再点一次同步.
8. As a 技能库用户, I want 检查与更新对「同一个仓库」的判断口径一致, so that 检查说有的更新，更新一定能拿到.
9. As a 技能库维护者, I want 「这是哪个远端」只有一个类型, so that 判断两个技能是否同源时不必翻三个类型和一条注释.
10. As a 技能库维护者, I want 远端身份的判定有表驱动测试, so that SSH/HTTPS/scp/大小写/`.git`/分支后缀这些写法不会悄悄分裂成两个 Source.
11. As a 技能库维护者, I want 「每个 Source 只解析一次」由模块保证而不是由调用方记得, so that 新增一条更新入口不会重复引入 N 次网络往返.
12. As a 技能库维护者, I want 更新工序接受一个已就绪的 checkout, so that 我可以用一个假目录测试安装/审批/替换三条易错分支，而不必连网.
13. As a 技能库维护者, I want 全库元数据重写在批量边界发生一次, so that 大库不会退化成正比于技能数的平方.
14. As a 技能库维护者, I want 后台轮次、托盘检查、前台批量共用同一套编排, so that 并发与让锁策略只有一处实现.
15. As a 技能库维护者, I want 行为不变的纯重构先落地, so that 后续的性能改动有一个可回滚的基线.

## Implementation Decisions

**远端身份（候选 7，先落地，纯重构）**

- 合并三个类型为一个 `RemoteRef`：解析 URL 得到的来源、从技能记录重建的来源、用于去重的身份键。
- 两个构造器：来自一个 URL（安装路径）、来自一条技能记录（更新/检查路径）。原始 URL 只在第一条路径需要，作为可选字段。
- 身份是一个只含 clone_url 与 branch 的小值，`same_remote(a, b)` 定义为身份相等。删掉"比较时忽略 subpath"的方法与解释它的注释——忽略 subpath 由身份类型的字段直接表达。
- subpath 构造时只做"空串 → 无"，不做 trim、不换分隔符：unix 下尾随空格与反斜杠都是合法目录名，改写会让窄克隆选错目录（既有实现已就此写过警告）。
- skillssh 的兜底定位 id 收进 `RemoteRef`：它确实描述"这个 Source 指向目录里的哪一项"，且解析技能目录时需要它。
- **不**跨语言合并前端已有的 URL 归一化表；只补 Rust 侧表驱动测试。
- 比较键**不折叠大小写**：主机名不区分大小写，但路径在部分主机上区分；合并只差大小写的两个路径会把一个仓库的 head 写给另一个仓库的技能。
- 修正（实现时发现）：比较键只折叠 git 自己视为同一仓库的书写差异（尾随 `.git`、尾随斜杠、大小写），**不把 ssh 与 https 归一**。前端那种归一用于显示分组，后端若照做会把一个只能走 ssh 凭据的私有仓库丢给 https 去取。因此 `RemoteIdentity` 保留真实 `clone_url` 供 git 使用，只在相等性与哈希上用比较键。

**按 Source 取代码（候选 1+2）**

- 新增一个深模块 `RemoteBatch`，接口两步：
  - `prepare(skills) -> Prepared`：按 `RemoteRef` 身份分组，每 Source 解析 head 一次、取一次 checkout；有并发上限；**全程不持有 Central Repo 写锁**。
  - `apply(&Prepared, skill) -> Outcome`：持锁完成本地工序（暂存、审批、替换、写库、同步副本），不触网。
- `prepare` 的并发上限取 4（取代码比只读解析重），与检查阶段既有的 8 分开配置。
- 同一个 Source 的技能由仓库缓存锁串行——并发收益来自**跨** Source，这一点写进模块文档与测试断言。
- 更新工序的接口从"接受技能 id、自己取代码"改为"接受已就绪的 checkout 与技能"，`Prepared` 是唯一来源。
- 失败按 Source 展开成每个技能的失败条目，保持现有的"逐技能报告失败"契约，前端无需改动。
- 每个 Source 一个临时 checkout，`apply` 阶段结束（含失败路径）后删除。

**元数据写（候选 3）**

- 全库元数据重写从"每技能一次"改为"每批一次"：全部 `apply` 结束后一次，且在 Central Repo 锁内（该函数假定调用方持锁）。
- 单技能路径（手动更新一个、安装一个）保留一次全量重写，不引入新的窄写接口。
- 该模块已有的窄接口（用于每 `(skill, agent)` 都跑一次的命令）是这次改动的先例，沿用其注释里给出的理由。

**边界**

- 不改任何用户可见文案与错误文本。
- 不改检查阶段的语义与 TTL。
- 不改 Schema：不引入 repository 表、不引入 repository id；分组仍然从 `(clone_url, branch)` 重建。

## Testing Decisions

好测试的标准：只断言外部行为——给一批技能和一个假远端，断言**网络调用次数、每个技能的结局、库里的文件**；不断言内部函数被调用了几次（`resolve_concurrent` 的注入式测试是既有先例，沿用同一种写法）。

Seam（首选最高处，尽量只有一个）：

- **远端传输 seam**：把"解析 head / 取 checkout"收在一个可注入的 trait 后面（生产实现是 git 子进程 + 缓存；测试实现记录调用并返回假 revision 与临时目录）。`RemoteBatch::prepare` 取它作为依赖，因此"每 Source 只解析一次"、"锁不跨网络"、"并发上限"三条不变量都能在无网络下断言。
- **文件系统 seam**：`apply` 只接受一个目录，测试直接在临时目录上跑，覆盖暂存/审批/替换，沿用安装器测试现有的临时目录先例。
- 不新增第三个 seam：前端不改，命令签名尽量不变（`check_all_skill_updates` 与 `batch_update_skills` 保持原样）。

将被测试的模块：

- `RemoteRef`：表驱动 `same_remote`——`.git`、尾随斜杠、大小写折叠为同一仓库；不同分支、不同访问方式（ssh vs https）保持不同。
- `RemoteBatch::prepare`：给定 N 个技能、M 个 Source，断言解析次数 == M；一个 Source 解析失败只影响它下面的技能；并发不超过上限。
- `RemoteBatch::apply`：内容未变时不写库；内容变化时替换、写库、同步副本；`pending_removals` 非空时扣下并返回审批令牌。
- 元数据：一批 40 个技能的更新中，全库重写恰好发生 1 次（批量边界），单技能更新仍是每次 1 次。

既有的 `resolve_concurrent` 注入式测试、安装器临时目录测试、`check_all_skill_updates` 的阶段化测试是同类先例。

## Out of Scope

- （已完成，保留原记录）进度事件与取消。
- 把 `ls-remote` 与 `fetch` 合并成一次远端往返（候选 4）：会改变"分支不存在 / 仓库不存在"的失败分类与提示。
- 统一前台批量与后台轮次成一个 `UpdateRound`（候选 5）。
- 前端任何改动，包括错误字符串的分解与逐技能结果展示。
- Schema 变更、repository 表、跨设备共享缓存。

## Further Notes

- 需要一个可复现的测量基线：同一批技能（≥40 个、跨 ≥8 个 Source）在改动前后各跑一次「全部更新」，记录墙钟时间与 git 调用次数。目标是把"git 调用次数 / Source 数"压到约 1，而不是约技能数。基线数字目前缺失。
- 检查阶段已经在做按 Source 去重与并发；这次是让更新阶段跟上，两处最终共用同一个远端身份类型。
- 仓库缓存锁（按 URL）意味着同一 Source 内的技能必然串行；任何"看起来没变快"的反馈先看这一点：用户的库是否集中在少数几个大仓库上。

---

## 实现状态

- 候选 7（`RemoteRef`）：`ed6d2b5` 完成。
- 候选 1+2+3（`prepare` / `apply` 拆分、按 Source 分组、元数据每批一次）：`4859ebe` 完成，评审后修正于其后一个提交。
- 测试：`cargo test --lib` 631 passed（基线 619，新增 12）。
- 候选 1 的可用性缺口已补：`core/remote_batch.rs` 提供 `RemoteTransport` seam、每 Source 一次解析（并发上限 4）、按槽的取代码计划，4 条测试覆盖解析次数/失败隔离/并发上限/槽计划（`1deeeae`）。
- 取代码并发已生效：先串行刷新槽、再并发复制兄弟（`f4ab423`）。
- `apply_prepared_update` 三条分支已有直接测试（内容未变不写、变更替换、扣下待批准），临时目录 fixture，无网络；"全库重写恰好一次"有计数器断言（`apply` 内部不写、批量边界写一次）。
- 仍无断言：锁不跨网络（需要真实 git 仓库的 prepare 才能构造）。
- 候选 5 已完成：前台批量与后台自动更新共用 `run_update_round(store, ids, UpdatePolicy)`；后台用 `yield_to_foreground` 在技能之间让锁。托盘检查仍只用检查路径（不应用更新）。
- 候选 6 已完成：`UpdatePolicy` 多了 `cancel` 与 `progress`；批量命令发 `skill-update-progress` 事件、提供 `cancel_batch_update`；技能库按钮显示「更新中 n/total」并可取消；取消在解析远端之前生效，不会先跑一轮网络。
- 候选 4 已完成：更新路径不再单独 `ls-remote`，revision 取自取回后的 checkout（`git rev-parse HEAD`）；远端每次更新只被访问一次。检查阶段仍用 `ls-remote`（它不需要代码）。
- 尚未测量：需要用户提供改动前后的墙钟时间与 git 调用次数（见 Further Notes）。
