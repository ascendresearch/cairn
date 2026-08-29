# Cairn 开发路线图

- 状态：规范性开发路线图；runtime-model value first
- 日期：2026-08-29
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 具体切片：[`SLICE_CATALOG.md`](SLICE_CATALOG.md)

## 1. 路线图结论

Cairn是一个基于Agent的迁移应用。Repository coding agent构建应用；DeepSeek等runtime model面对每个此前
未知的迁移task进行推理。当前路线先证明这条真实model-backed path有价值，再建设Admission、Oracle、
Candidate和复杂governance。Fixture只评测应用，不定义应用。

当前critical path只有：

```text
DEV-004 generic DeepSeek SIR proposal path
→ DEV-005 cross-task and downstream-utility gate
→ CP0 Go
→ DEV-006 complete typed recovery input/proposal contract
→ DEV-007 model-free scoped user-decision request
→ DEV-008 typed user decision + smallest promotion boundary + contract-only Oracle policy
→ DEV-009 actual call-adapter observation + receipt-bound materialization
→ DEV-010 actual honest/fault qualification of one local Oracle claim
→ DEV-011 restricted commit/public local claim + first Candidate search input
→ DEV-012 bounded DeepSeek Candidate proposal episode
→ DEV-013..015 remote generic build/revision loop + host-fallback discovery
→ DEV-016..018 product-owned native ASC gate + native-feedback follow-up
→ DEV-019..020 repeatable explicit repair lineage + native rebuild
→ DEV-021 Controller-owned durable Candidate workflow spine
→ DEV-022 generic Proposal Host + persisted workflow request consumer
→ next: audit Controller supervision or the shortest native-success/NPU consumer
```

DEV-004..022现已accepted，CP0结论为Go：SIR保持proposal-only，并已有首个正式consumer。
完整`IntentRecoveryInputV1`/`IntentHypothesisSetProposalV1` → claim-scoped user decision → independent
Admission → `MigrationIntentContractV1` → collection comparator policy → actual observation/comparison已经
闭合；DEV-010/011又完成局部qualification、commit-before-publish local claim和answer-free Candidate input，
DEV-012则让configured DeepSeek通过durable agent runtime读取task-scoped source并提交首个typed Candidate
proposal。DEV-013–020 已经把该 artifact 推进到远端 generic/native build、receipt-bound diagnostic、隔离
DeepSeek revision/repair 和 rebuild；DEV-021/022已把这些transition固化到Controller state machine并由通用
Proposal Host消费persisted episode request。最新native gate仍为`SubjectFailed`。下一阶段继续寻找完整
Controller supervision或native success/NPU correctness的最短consumer；仍保持expected observation、
candidate output与Admission authority分离。尚未完成CP1、完整
Candidate verdict 链或通用 governance。

DEV-008 的最小 one-shot SIR ingress 只是首个真实 authority consumer 的历史capability proof，DEV-022已删除
该专用process；当前production SIR/Candidate loops由通用Proposal Host承载，Oracle仍等待真实consumer。DEV-010 的局部
qualification 也不授权完整 mechanism registry、七类 Planner、十一位置 Agent catalog 或面向未来 stage
的兼容接口。

## 2. Checkpoints

| Checkpoint | 可观察目标 | 退出条件 |
| --- | --- | --- |
| `CP0 Runtime SIR Value` | DeepSeek通过现有durable agent runtime读取task-generic context/tools并提交typed hypothesis proposal | 两个语义形态不同的task复用同一production path；有recorded replay和至少一次opt-in live run；无fixture answer leakage |
| `CP1 First Migration Outcome` | 只有CP0通过后，形成最小intent authority、Oracle/candidate和CUDA→Ascend C结果 | exact task可重建；用户/authority/observation分离；适用的真实build/device lane有明确事实 |
| `CP2 Generalization and Hardening` | 第二个真实operator验证核心边界，再补平台、安全、性能、知识/feedback | generic runtime/worker无需operator branch；真实收益与成本可测；首个公开baseline另行由用户决定 |

后两个checkpoint是方向，不是已授权的slice inventory。它们在上一个checkpoint产生事实后再切片。

## 3. CP0 — Runtime SIR Value

### 3.1 DEV-004：generic model-backed proposal

最小纵向路径：

```text
immutable migration task artifacts
→ task-generic context projection
→ configured DeepSeek profile + scoped source-inspection tools
→ durable agent episode
→ typed facts / competing hypotheses / unknowns / citations
```

要求：

- 复用已有`cairn-agent` provider、continuation、tool、budget和recovery基础；
- profile instruction只规定推理协议和输出shape，不包含reduction答案；
- public/restricted expected answers不进入model-visible projection；
- proposal没有admitted constructor、hidden access或execution authority；
- recorded provider闭合后才运行opt-in live DeepSeek；
- 不创建没有当前consumer的新crate、registry或process tree。

### 3.2 DEV-005：cross-task/value gate

使用同一个production code/profile shape运行：

1. D-039 reduction evaluation fixture；
2. 一个在algorithm、memory/side-effect或multi-kernel structure上实质不同的CUDA task。

比较三条路径：source-preserving baseline、user-declared intent、runtime SIR。至少回答：

- SIR能否引用实际task facts而非复述fixture答案；
- 是否保留真实竞争假设和calibrated unknown；
- 第二个task是否无需production branch/prompt结构修改；
- SIR是否改变一个下游迁移/Oracle决策，或显著减少用户工作；
- live model成本、失败和replay scope是什么。

任一核心条件失败即停止SIR critical path，不以增加prompt、fixture或review ceremony掩盖失败。

## 4. CP0之后的条件能力

只有value gate通过且有第一个真实consumer时，才按需要规划：

- claim-scoped Intent Admission与`MigrationIntentContract`；
- Oracle proposal/evaluation；
- Candidate generation/correction；
- CUDA reference、Ascend build/NPU execution；
- restricted evidence和diagnostic redaction；
- exact mechanism qualification；
- performance、knowledge/skill和feedback。

每项能力随纵向consumer实现。不得因为目标设计中出现一个概念就提前创建crate、slice、review role或fixture
taxonomy。

## 5. Fixture与泛化规则

- DEV-001 reduction保留为evaluator fixture；其claims/corpus/review identities不得成为runtime context。
- DEV-003 sanitation/provenance基础继续复用。
- D-040/DEV-002预建qualification bundle已被D-042 supersede并从current tree删除。
- Public fixture对developer可见不代表可进入product prompt。
- Restricted expectation只对evaluation/admission side可见；proposal agent不能读取。
- 第二个task用于证明无case-specific product branch，不要求先构建大benchmark。

## 6. 停止条件

出现以下任一事实时暂停并回到产品选择：

- 没有runtime-model执行却声称SIR工作；
- proposal由coding agent根据fixture答案代写；
- 更换task要求修改generic runtime或加入operator branch；
- SIR相对用户直接声明intent没有下游收益；
- 为了一个测试先建设Admission/qualification/platform framework；
- live result只看起来合理但没有typed citations、unknown和可重建episode。
