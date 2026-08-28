# Cairn 开发计划

- 状态：规范性开发计划；runtime-model value first
- 日期：2026-08-28
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 上位规范：[`../SYSTEM_REQUIREMENTS.md`](../SYSTEM_REQUIREMENTS.md)、
  [`../DECISIONS.md`](../DECISIONS.md)、[`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md)
- 软件架构：[`../design/README.md`](../design/README.md)

## 1. 当前目的

Cairn 是一个基于 Agent 的迁移应用。Repository coding agent 负责构建通用运行时、边界和评测；DeepSeek
等 configured runtime model 才是面对每个未知迁移任务、阅读代码并提出高阶意图假设的 actor。

当前先回答一个产品问题：SIR 相比 source-preserving 或用户直接声明 intent，是否真的改善后续迁移决策。
在这个问题有证据之前，不建设完整 Admission、Oracle、Candidate、qualification 或多 Agent 拓扑。

## 2. 文档地图

| 文档 | 用途 |
| --- | --- |
| [`../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md`](../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md) | SIR 的规范性目标、authority 边界与当前建设路线 |
| [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) | 当前代码事实、保留/删除边界和近期起点 |
| [`DEVELOPMENT_MODEL.md`](DEVELOPMENT_MODEL.md) | 如何按产品证据而不是架构清单切片 |
| [`ROADMAP.md`](ROADMAP.md) | runtime SIR value 的近期 critical path |
| [`SLICE_CATALOG.md`](SLICE_CATALOG.md) | DEV-001..005 的当前状态和边界 |
| [`QUALITY_GATES.md`](QUALITY_GATES.md) | 风险分级 gate 与实际 workflow 证据 |
| [`WORKSTREAMS.md`](WORKSTREAMS.md) | 当前协作和代码 ownership |
| [`records/README.md`](records/README.md) | 仍有意义的历史 slice 记录 |

## 3. 固定边界

- coding agent 不替 DeepSeek 解 fixture，也不把已知答案写入 prompt、product type 或 policy；
- fixture 是 evaluator input，不是产品知识或架构；
- production crate 不依赖 `cairn-testkit`；expected/private material 不进入 proposal episode；
- 第一个 case 只证明接线，第二个实质不同的 task 才检验通用路径；
- Proposal 没有 Admission、execution、hidden evidence 或 verdict authority；
- pre-release V1 直接替换，删除 superseded code/tests/data，不建兼容路径；
- 没有当前 consumer 的 crate、registry、role、fixture taxonomy 和 review ceremony 不实施。

## 4. 当前状态

- DEV-001：Accepted，保留为 reduction evaluation fixture；
- DEV-003：Accepted，保留最小 fixture provenance/sanitation 基础；
- DEV-002：Superseded；D-040 的预建 qualification bundle、实现、测试、公开/私有材料从 current tree 删除；
- DEV-004：Accepted，task-generic recorded/live DeepSeek SIR proposal episode 已闭合；
- DEV-005：Accepted，第二个atomic compaction task复用同一路径，并产生可观察Oracle utility。
- DEV-006：Accepted，完整typed recovery input/proposal contract通过recorded、full CI与真实DeepSeek
  strict-repair/restart。

CP0结论是`Go`：SIR继续留在当前建设路径。完整`IntentRecoveryInputV1`与
`IntentHypothesisSetProposalV1`已经闭合；下一条纵向链以第一个正式consumer约束建设范围，接入最小claim-scoped Intent Admission与
`NeedsUserDecision`，再让一个真实Oracle决策消费`MigrationIntentContract`。这不授权一次性建设完整
CP1，也不恢复DEV-002式第三人fixture review、通用Admission/qualification框架或固定多Agent拓扑。
