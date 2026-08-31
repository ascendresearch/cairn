# Cairn 开发计划

- 状态：规范性开发计划；runtime-model value first
- 日期：2026-08-29
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 上位规范：[`../SYSTEM_REQUIREMENTS.md`](../SYSTEM_REQUIREMENTS.md)、
  [`../DECISIONS.md`](../DECISIONS.md)、[`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md)
- 软件架构：[`../design/README.md`](../design/README.md)

## 1. 当前目的

Cairn 是一个基于 Agent 的迁移应用。Repository coding agent 负责构建通用运行时、边界和评测；DeepSeek
等 configured runtime model 才是面对每个未知迁移任务、阅读代码并提出高阶意图假设的 actor。

CP0已经回答第一个产品问题：SIR相对source-preserving路径确实改变了atomic compaction的下游
output-order决策；DEV-008又证明该结果可经用户authority和独立Admission进入contract-only comparator
policy。DEV-009–012 已把该 policy 推进到 local Oracle publication 与真实 DeepSeek Candidate proposal；
DEV-013–020 又用现有 Controller/remote Worker 打通 generic/native build、receipt-bound diagnostic、隔离
DeepSeek repair 与 rebuild。DEV-021–023已把Candidate native suffix固化成durable Controller workflow、通用
proposal step和single-task manager；DEV-024又删除role-specific runner并统一Host request lifecycle。最新native
rebuild仍为`SubjectFailed`；DEV-025冻结完整Controller typed skeleton，DEV-026已把SIR→decision requests接入
task-owned durable prefix并明确等待真实user decision。当前继续接入independent Intent Admission，或建设
Controller-owned experiment round-trip/native success/NPU correctness的真实consumer。

## 2. 文档地图

| 文档 | 用途 |
| --- | --- |
| [`../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md`](../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md) | SIR 的规范性目标、authority 边界与当前建设路线 |
| [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) | 当前代码事实、保留/删除边界和近期起点 |
| [`NEXT_SESSION.md`](NEXT_SESSION.md) | 下一会话的唯一启动入口、只读审计、DEV-026 边界和可复制启动消息 |
| [`CURRENT_IMPLEMENTATION_WALKTHROUGH.md`](CURRENT_IMPLEMENTATION_WALKTHROUGH.md) | 用atomic compaction样例逐步解释当前SIR→Admission→Oracle→Candidate proposal实现 |
| [`DEVELOPMENT_MODEL.md`](DEVELOPMENT_MODEL.md) | 如何按产品证据而不是架构清单切片 |
| [`ROADMAP.md`](ROADMAP.md) | 已打通纵向路径后的近期 critical path |
| [`SLICE_CATALOG.md`](SLICE_CATALOG.md) | DEV-001..026 的当前状态和边界 |
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
  strict typed source proposal并通过terminal restart。
- DEV-013–015：Accepted，exact proposal/revision 已经远端 build；generic success 同时暴露 host fallback，
  因而没有被误报为 native success。
- DEV-016–018：Accepted，product-owned ASC harness 强制 exact source 进入 `bisheng`/`dav-3510`，并把
  native diagnostic 交给新的隔离 DeepSeek episode 后重建。
- DEV-019–020：Accepted，建立显式可重复 repair lineage 并再次远端 native build；最新结果仍为
  `SubjectFailed`，不含 NPU/semantic/verdict evidence。
- DEV-021：Accepted，Candidate native suffix已固化为Controller-owned durable spine。
- DEV-022：Accepted，同一generic proposal step已承载SIR/Candidate role并接通persisted workflow request；
  没有live model/Worker或新的native evidence。
- DEV-023：Accepted，active Controller single-task manager接通Host supervision、scheduler/reconcile和receipt折回。
- DEV-024：Accepted，删除SIR/Candidate role-specific runner和旁路测试，统一freeze/episode/observation/strict
  submission/terminal lifecycle；没有live model/Worker或新的native evidence。
- DEV-025：Accepted，完整Controller十阶段顺序成为typed stage-port骨架；空stage无default并fail closed，真实
  Candidate suffix成为recover/select/execute子骨架；没有live effect或完整aggregate claim。
- DEV-026：Accepted，durable Controller prefix接通exact SIR request、Host start authority、proposal observation
  与decision requests，并停在真实user decision边界；SIR/Candidate共享Host supervision，没有live effect。

CP0结论是`Go`：SIR继续留在当前建设路径。完整`IntentRecoveryInputV1`与
`IntentHypothesisSetProposalV1`已经闭合；第一个正式consumer也已从scoped user decision走到真实execution
observation、局部mechanism qualification、commit-before-publish local claim、Candidate source、remote
native build 和 model repair。下一阶段采用
[`WORKFLOW_ARCHITECTURE.md`](../design/WORKFLOW_ARCHITECTURE.md) 冻结的 Controller state machine、通用
proposal step、统一 Worker 实验与 direct Worker→Controller 网络；局部 claim 和 compile feedback 仍不等于
完整 CP1、`AdmittedOraclePortfolio` 或 release authority，也不恢复 DEV-002 式第三人 fixture review、
预建通用 qualification 框架或固定多 Agent 拓扑。
