# 02: 更新轮次按 Placement 分流

**What to build:** 更新轮次用 `locate` 的答案决定每个技能的结局：At 正常更新，Moved 记为“已改名”并带候选，Gone 记为“上游已删除”，其余仍是失败。轮次汇总里 `missing_sources` 拆成 `moved_sources` 与 `gone_sources`。

**Blocked by:** 01

**Status:** done (2e307a0)

- [x] At 路径行为不变（现有 641 个测试仍绿）
- [x] Moved 进入 `moved_sources`，携带候选 subpath 与置信度
- [x] Gone 进入 `gone_sources`
- [x] 真正的失败仍进 `failed`，四类互不重叠
- [x] 轮次日志一行含四个计数
