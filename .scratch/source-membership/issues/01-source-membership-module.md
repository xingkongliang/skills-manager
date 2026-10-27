# 01: SourceMembership 模块

**What to build:** 一个能回答"这条已存技能在当前 checkout 的哪里"的模块，返回 At / Moved（带置信度与候选）/ Gone。不接任何调用方，纯新增。

**Blocked by:** None（可立即开始）

**Status:** done

- [x] `locate(stored_skill, checkout) -> Placement` 存在，三结局齐全
- [x] 高置信：目录内容哈希等于 `content_hash`
- [x] 低置信：目录 basename 或 SKILL.md frontmatter name 命中
- [x] 多个低置信候选全部返回，不替调用方挑选
- [x] 只在 checkout 内扫描，不跨仓库
- [x] 表驱动测试，无网络、无真实 git

## 落地形态（给 02）

入口是自由函数 `core::source_membership::locate(skill: &SkillRecord, checkout: &Path) -> Placement`（无状态，不需要 struct）。返回类型：

```rust
pub enum Placement {
    At { subpath: String },
    Moved { candidates: Vec<MoveCandidate> },
    Gone,
}

pub struct MoveCandidate {
    pub subpath: String,          // 相对 checkout，`/` 分隔，空串 = checkout 根
    pub confidence: Confidence,   // High | Low
    pub reason: MoveReason,       // ContentHash | DirectoryName | FrontmatterName
}
```

`Moved` 带列表而不是单个候选；`candidates[0]` 是最强的一个，排序规则：理由强度（ContentHash → DirectoryName → FrontmatterName）然后按路径字典序，稳定可复现。

规则明细：

- 有高置信候选时只返回高置信候选（内容一致已经确定答案，名字巧合只是噪音）；没有高置信时返回全部低置信候选。
- `At` 要求记录路径上**本身**是技能目录（有 SKILL.md），仅有该目录不算——上游把它变成分类目录时不报 At。
- 三类型均实现 `Serialize`（`Placement` 带 `placement` tag），02 想要序列化无需再加 derive。
