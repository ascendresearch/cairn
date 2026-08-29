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
| DEV-006 | Accepted | [`typed recovery contract`](DEV-006-IMPLEMENTATION.md) | caller/source分离的current-V1 input/proposal contract；recorded、full CI和live strict repair闭合 |
| DEV-007 | Accepted | [`typed decision request`](DEV-007-IMPLEMENTATION.md) | model-free process生成scoped request；用户选择由DEV-008继续promotion |
| DEV-008 | Accepted | [`first admitted intent`](DEV-008-IMPLEMENTATION.md) | exact用户决策经独立Admission机械promotion；restricted commit后由contract-only collection Oracle policy消费 |
| DEV-009 | Accepted | [`contract-bound collection observation`](DEV-009-IMPLEMENTATION.md) | 首个contract policy已约束真实双ABI output、authoritative receipt与comparison evidence |
| DEV-010 | Accepted | [`first qualified local Oracle claim`](DEV-010-IMPLEMENTATION.md) | 用actual honest/fault implementation receipts资格化首个局部Oracle capability；Controller发布后Candidate才可消费 |
| DEV-011 | Accepted | [`publish local Oracle and open Candidate input`](DEV-011-IMPLEMENTATION.md) | restricted commit后发布exact local claim，并立即由local-only Candidate search input consumer消费 |
| DEV-012 | Accepted | [`first bounded Candidate proposal episode`](DEV-012-IMPLEMENTATION.md) | 真实DeepSeek Candidate actor消费answer-free input并提交可重建typed source proposal；不含build/run/verdict |
| DEV-013 | Accepted | [`first exact Candidate remote Ascend build`](DEV-013-IMPLEMENTATION.md) | exact proposal经Controller/scheduler进入remote no-device build lane；真实`SubjectFailed` receipt发现`acl/acl.h` include divergence |
| DEV-014 | Accepted | [`first receipt-bound Candidate revision`](DEV-014-IMPLEMENTATION.md) | 新isolated DeepSeek episode消费exact receipt-bound diagnostic并提交parent-linked full-source revision |
| DEV-015 | Accepted | [`first exact Candidate revision remote build`](DEV-015-IMPLEMENTATION.md) | DEV-014 exact revision取得remote `Succeeded` receipt，同时暴露当前gate未强制native Ascend compile |
| DEV-016 | Accepted | [`product-owned native Ascend Candidate gate`](DEV-016-IMPLEMENTATION.md) | exact revision primary source经固定ASC harness进入`bisheng`并取得真实`SubjectFailed` diagnostic |
| DEV-017 | Accepted | [`first native-feedback Candidate follow-up`](DEV-017-IMPLEMENTATION.md) | 新isolated DeepSeek episode消费exact native receipt-bound diagnostic并提交previous-revision-linked full source |
| DEV-018 | Accepted | [`first native-feedback follow-up remote ASC build`](DEV-018-IMPLEMENTATION.md) | DEV-017 exact follow-up经同一product-owned native ASC gate取得真实`SubjectFailed` receipt |
| DEV-019 | Accepted | [`explicit repeatable native repair episode`](DEV-019-IMPLEMENTATION.md) | DEV-018 exact linker receipt进入新的isolated DeepSeek episode并提交typed repair；没有自动续轮或build |
| DEV-020 | In progress | [`exact native repair remote ASC build`](DEV-020-IMPLEMENTATION.md) | DEV-019 exact repair重新进入同一product-owned native ASC gate |

DEV-004 是 proposal-only value proof，不因普通 fixture 或内部模块强制创建 DCR。若 implementation note 触及
authority、restricted/secret visibility、external effect、public API 或 persisted/wire contract，再按
[`DESIGN_CONFORMANCE_RECORD_TEMPLATE.md`](../DESIGN_CONFORMANCE_RECORD_TEMPLATE.md) 做风险分级记录。
