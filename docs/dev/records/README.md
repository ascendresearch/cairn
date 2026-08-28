# Development record index

- 状态：历史记录索引
- 日期：2026-08-28

记录只说明一次 slice 曾如何被设计和审查，不自动成为 current product dependency 或实施前置条件。当前状态
以 [`SLICE_CATALOG.md`](../SLICE_CATALOG.md) 和 [`CURRENT_BASELINE.md`](../CURRENT_BASELINE.md) 为准。

| Slice | Current status | Record | 当前用途 |
| --- | --- | --- | --- |
| DEV-001 | Accepted | [`DEV-001.md`](DEV-001.md)、[`historical private review`](DEV-001-PRIVATE-REVIEW.md) | evaluator fixture history；expected/private answer 不进入 runtime context |
| DEV-002 | Superseded | [`DEV-002.md`](DEV-002.md) | 只记录 D-040 qualification framework 被删除的原因；详细旧评审在 Git history |
| DEV-003 | Accepted | [`DEV-003.md`](DEV-003.md) | current fixture provenance/sanitation foundation |
| DEV-004 | Accepted | [`implementation note`](DEV-004-IMPLEMENTATION.md) | recorded/full CI与用户授权的live DeepSeek均闭合；只证明generic proposal workflow接通 |
| DEV-005 | Accepted | [`cross-task/value evaluation`](DEV-005-EVALUATION.md) | atomic compaction复用同一路径；proposal改变Oracle order决策；CP0 Go |

DEV-004 是 proposal-only value proof，不因普通 fixture 或内部模块强制创建 DCR。若 implementation note 触及
authority、restricted/secret visibility、external effect、public API 或 persisted/wire contract，再按
[`DESIGN_CONFORMANCE_RECORD_TEMPLATE.md`](../DESIGN_CONFORMANCE_RECORD_TEMPLATE.md) 做风险分级记录。
