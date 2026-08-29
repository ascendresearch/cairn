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

CP0已经回答第一个产品问题：SIR相对source-preserving路径确实改变了atomic compaction的下游
output-order决策；DEV-008又证明该结果可经用户authority和独立Admission进入contract-only comparator
policy。DEV-009已让该policy约束真实child process和materialized comparison，DEV-010已资格化一个local-only
claim，DEV-011又在restricted commit后发布它并生成首个answer-free Candidate search input。DEV-012已经让
真实DeepSeek Candidate通过bounded source reads提交首个typed Ascend C proposal。当前目标是选择最短的真实
target/toolchain build consumer，而不是因此建设完整Admission、Oracle portfolio、qualification或多Agent拓扑。

## 2. 文档地图

| 文档 | 用途 |
| --- | --- |
| [`../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md`](../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md) | SIR 的规范性目标、authority 边界与当前建设路线 |
| [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) | 当前代码事实、保留/删除边界和近期起点 |
| [`CURRENT_IMPLEMENTATION_WALKTHROUGH.md`](CURRENT_IMPLEMENTATION_WALKTHROUGH.md) | 用atomic compaction样例逐步解释当前SIR→Admission→Oracle→Candidate proposal实现 |
| [`DEVELOPMENT_MODEL.md`](DEVELOPMENT_MODEL.md) | 如何按产品证据而不是架构清单切片 |
| [`ROADMAP.md`](ROADMAP.md) | runtime SIR value 的近期 critical path |
| [`SLICE_CATALOG.md`](SLICE_CATALOG.md) | DEV-001..012 的当前状态和边界 |
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
- DEV-007：Accepted，model-free process从exact live proposal生成scoped output-order request；实际任务
  authority选择unordered-set hypothesis。
- DEV-008：Accepted，exact typed decision经独立Admission process机械promotion、restricted commit，并只由
  `MigrationIntentContractV1`驱动首个collection-output Oracle comparator policy。
- DEV-009：Accepted，contract policy已约束actual child output、generic authoritative receipt与可信
  materialization/comparison evidence。
- DEV-010：Accepted，已用两个独立actual implementations资格化首个local-only Oracle claim；构造
  admitted type不等于Controller已经发布authority。
- DEV-011：Accepted，restricted artifacts先commit，再返回exact public outcome并机械生成local-only Candidate
  search input。
- DEV-012：Accepted，真实DeepSeek Candidate只消费answer-free authority与按需读取的task source，提交首个
  strict typed source proposal并通过terminal restart；尚未build、run或形成verdict。

CP0结论是`Go`：SIR继续留在当前建设路径。完整`IntentRecoveryInputV1`与
`IntentHypothesisSetProposalV1`已经闭合；第一个正式consumer也已从scoped user decision走到真实execution
observation、局部mechanism qualification、commit-before-publish local claim、answer-free Candidate input和
真实Candidate source proposal。下一步只沿这份artifact接入实际target/toolchain的最短build consumer。局部
claim和unbuilt proposal仍不等于完整CP1、`AdmittedOraclePortfolio`或release authority，也不恢复
DEV-002式第三人fixture review、通用qualification框架或固定多Agent拓扑。
