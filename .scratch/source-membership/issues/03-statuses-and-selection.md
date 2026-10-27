# 03: 两个新状态进入状态机

**What to build:** `update_status` 增加 `moved` 与 `source_gone`；两者在 TTL 内被跳过检查；「全部更新」的选择器只含 `update_available`，因此它们不再被反复处理。界面用徽标区分两者。

**Blocked by:** 02

**Status:** done (7bb59bf)

- [x] `moved` / `source_gone` 写入后，TTL 内 `should_skip_update_check` 返回 true
- [x] 批量选择器不包含两者
- [x] 列表徽标区分“已改名”与“上游已删除”
- [x] 前端类型与 i18n 同步（en / zh / zh-TW）
