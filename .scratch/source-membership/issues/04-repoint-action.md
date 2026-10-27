# 04: 一键重新指向

**What to build:** `repoint_skill_source(id, subpath)` 只改 `source_subpath`，不动库内文件；界面上「已改名」的技能显示候选路径与理由，点一次完成重指。低置信多候选时先让用户选。

**Blocked by:** 03

**Status:** done (b7f5b98)

- [x] 重指后 `central_path`、`content_hash`、库内文件均不变
- [x] 重指后状态回到 `update_available`（下一次检查决定）
- [x] 多候选必须先选再重指
- [x] 测试断言只改指针
