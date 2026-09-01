# 方案 D 消融实验测量协议

- 状态：待预注册的候选测量设计，尚未冻结为正式实验 manifest
- 日期：2026-09-01
- 适用设计：`BlindFirstPolicyChallengedHierarchicalReview`（D1/D2）
- 关联设计：[`BLIND_FIRST_ORACLE_SCOPE_DESIGN.md`](../../design/BLIND_FIRST_ORACLE_SCOPE_DESIGN.md)

## 1. 目的与边界

本文档定义如何判断方案 D 是否真的改善 Oracle scope、coverage、Review、evidence 和成本。它不把 A/B/C pilot
重新解释为正式结果，也不提前假设 D 优于已有 treatment。

正式运行前必须把本协议与以下内容共同冻结：

- exact commit、binary、configuration 和 prompt identities；
- runtime model、adapter、temperature/seed 和 budget；
- task corpus、target context 和 arm assignment；
- skill/knowledge/tool/capability exposure manifests；
- policy catalog、sealed task ledgers 和 semantic matching rubric；
- hidden evaluator、correct variants、mutants/challenges 和 severity weights；
- operator stopping、exclusion、failure classification 和 aggregation rules。

运行后不得因为某个 arm 表现不佳而更换分母、删除 task、改变 semantic equivalence 或选择新的 primary metric。

### 1.1 Upstream isolation

方案 D 的直接 treatment 从 admitted Intent 后开始。估计 Oracle scope topology/evidence 的主实验必须让同一 task 的各 arm
复用 exact：

- `AdmittedIntentContractV1`；
- `AdmittedIntentEvidenceSnapshotV1`；
- target context；
- upstream SIR/administrator/Intent Admission artifact 和 receipt identities。

否则 SIR 随机性、管理员 decision 差异或 evidence handoff 差异会与 Oracle treatment 混杂。完整
`CLI → SIR → Intent → Oracle` 仍必须作为 confirmatory product-path experiment 运行，但单独报告 upstream divergence，
不能用它替代 component-level 因果比较。

复用 upstream 不能通过手写 SIR/Intent、直接调用内部 helper 或构造测试专用 proposal 实现。共享 snapshot 必须来自一次
真实 `cairn-cli → cairn-server → migration app API → CudaMigrationWorkflow` 上游运行；各 downstream treatment instance
通过 typed experiment-composition lineage 绑定同一个 immutable snapshot，并继续使用正常 Controller/Agent/Worker ports。
必须记录 `UpstreamSnapshotId`、`TreatmentRunId` 和 parent lineage，防止跨 task 或跨 snapshot 替换。

`no-evidence` treatment 只禁止向 scope/property proposal roles 暴露新请求的 Worker observations。所有 arms 仍使用相同的
hidden evaluator、qualification controls、correct/invalid variants 和 Admission Worker capabilities。

## 2. 分析单位

所有记录使用 distinct strong identities。至少区分：

- `TaskRunId`：一个 task × treatment × repetition/seed 的完整分配；
- `PolicyConcernInstanceId`：sealed task ledger 中的一个 concern instance；
- `BlindDimensionId`：policy 可见前冻结的一个自然发现；
- `ConsolidatedOracleObligationId`：Scope Review 后的独立验证义务；
- `OraclePropertyId`、`OracleCaseId`、`OracleMechanismId`；
- `ReviewFindingId` 和 exact draft/revision lineage；
- `EvidenceExperimentRequestId`、job/attempt/receipt identity；
- `EvaluatorRequirementId`、`CorrectVariantId`、`InvalidChallengeId`；
- `AdmissionOutcomeId` 和 failure classification。

不能把 dimension、obligation、case、mechanism 或 evaluator requirement 都塞进 generic `item_id` 后按行计数。每个比率
必须声明分析单位和 denominator。Aggregate report 同时保留 per-task-run 原始计数，不能只给跨 task 平均值。

## 3. Ground truth 与语义匹配

### 3.1 Frozen evaluator requirement set

每个 task 在 proposal roles 不可见的 evaluator side 具有
`EvaluatorRequiredObligationSetV1`，包括：

- required semantic/numerical/integration/safety/performance obligations；
- applicable domain 和 severity/importance weight；
- correct variants 和 honest controls；
- targeted invalid variants、mutants 和 disjoint hidden challenges；
- 需要的 mechanism capability 和 target identity；
- semantic equivalence/matching rubric。

它不能由本轮 proposal 输出反向生成。若 runtime model 发现 catalog/evaluator 之外的 plausible novel concern，由事先指定、
对 treatment label 盲化的 evaluator process 按 frozen rubric adjudicate，并保留 `NovelEvaluatorAdjudicationId`；不能为了奖励
某个 arm 临时扩大 ground truth。

### 3.2 Semantic matching

Blind dimension、policy concern、consolidated obligation 和 evaluator requirement 之间的匹配不能使用字符串相等或模型
自报。匹配由 hidden evaluator 或 treatment-blinded human adjudication 产生，并至少区分：

- `ExactCoverage`；
- `PartialCoverage`，包含未覆盖 domain；
- `DuplicateCoverage`；
- `OverMerged`；
- `Unrelated`；
- `Indeterminate`。

多人 adjudication 时记录一致率和仲裁过程。`Indeterminate` 不自动算覆盖。

## 4. Primary outcomes

Primary outcomes 在实验前冻结，建议采用 correctness-first 的多指标 gate，而不是一个可相互补偿的总分。

### 4.1 Evaluator-qualified required coverage

对 task `t`：

```text
QualifiedCoverage(t) =
  Σ weight(r) for each required evaluator obligation r
    with at least one correctly matched, executable and qualified mechanism
  -----------------------------------------------------------------------
  Σ weight(r) for all required evaluator obligations r
```

只有结构完整但未运行、运行在错误 capability、只验证 CUDA source 或没有通过 qualification control 的 mechanism 不计入
qualified coverage。`PartialCoverage` 只按预注册 domain weight 计入，不能由 proposal confidence 决定。

### 4.2 False acceptance

```text
FalseAcceptanceRate(t) =
  invalid evaluator challenges accepted by the evaluated Oracle portfolio
  ------------------------------------------------------------------------
  invalid evaluator challenges successfully executed
```

同时报告 severity-weighted escape、每类 mutation/challenge 的结果和“至少一个 critical false acceptance”的 task-run 比例。
基础设施未执行的 challenge 不进入该比率分母，而进入 execution completeness；不能算作正确拒绝。
若分母为零，该比率标记 `NotEstimable`，不能填成 0%。

### 4.3 False rejection

```text
FalseRejectionRate(t) =
  correct variants or honest controls rejected by the evaluated Oracle portfolio
  ------------------------------------------------------------------------------
  correct variants or honest controls successfully executed
```

Provider、Worker、toolchain 或设备失败单独报告，不能算 candidate/Oracle semantic rejection。
若分母为零，该比率标记 `NotEstimable`，不能填成 0%。

### 4.4 Execution authenticity

```text
RequiredCapabilityClosure(t) =
  required mechanisms qualified on their exact required capability/target
  -----------------------------------------------------------------------
  all required mechanisms
```

Host arithmetic、CPU reference、CUDA execution、Ascend compile 和 Ascend 950PR execution 分别计算。低等级 capability 的
成功不能替代 policy 要求的高等级执行。

### 4.5 Correctness-gated efficiency

成本只有在预注册的 required coverage、false-acceptance 和 false-rejection gate 满足后才进入优胜判断。至少报告：

- total model tokens 和 wall time per qualified evaluator obligation；
- Worker/device milliseconds per qualified evaluator obligation；
- total cost per evaluator-confirmed defect detected before Admission；
- human decision/review time per task run。

成本低但 coverage 不足或错误接受 hidden challenge 的 arm 不能通过一个综合加权分数胜出。

## 5. Blind discovery 与 anchoring metrics

### 5.1 Blind discovery recall

```text
BlindRecall(t) =
  evaluator-required obligations with a matching frozen blind dimension
  ---------------------------------------------------------------------
  all evaluator-required obligations
```

只检查 policy challenge 前冻结的 artifact。后续补充不能回填 blind recall。

### 5.2 Blind discovery precision

```text
BlindPrecision(t) =
  blind dimensions adjudicated as necessary, distinct task concerns
  ---------------------------------------------------------------
  all validly submitted blind dimensions
```

Generic labels、重复项、仅为 concrete case 的项和 unsupported speculation 进入分母，并按 adjudication 分类。

### 5.3 Supplement dependency

最终 required obligations 中，blind artifact 没有匹配项、首次由 sealed policy challenge 引入的比例。按 severity 分层报告，
避免大量低风险补充掩盖一个关键遗漏。

### 5.4 Novel discovery yield

Catalog 外 blind discoveries 中，被 treatment-blinded evaluator 确认为真实、独立且 candidate-relevant concern 的数量和比例。
同时报告被拒绝和 `Indeterminate` 的数量。

### 5.5 Anchoring displacement

blind 阶段有效发现，在 challenge 后没有 evidence-backed disposition 就从 final obligation graph 消失或被缩窄的数量。
原 blind artifact 不可修改，因此该指标必须由 exact coverage/disposition lineage 重算。

### 5.6 Supplement over-adoption

policy challenge 后新增为独立 obligation、但 evaluator 判定为不适用、重复或应仅作为 case 的 concern 比例。它衡量模型
是否在看到清单后机械填矩阵。

### 5.7 Taxonomy leakage

记录 blind visibility manifest、materialized prompt/tool/skill/knowledge exposure 中出现 sealed catalog identity、内容或等价
内部 taxonomy 的次数。任何 confirmed leakage 都使该 task-run 的 blind-discovery metric 无效，但 run 仍保留在 operational
和authority failure 报告中，不能静默删除。

## 6. Decomposition 与 consolidation metrics

每个 task-run 报告完整漏斗：

```text
blind dimensions
+ policy concern instances
→ adopted/merged/split/rejected dispositions
→ consolidated obligations
→ properties
→ cases
→ executable mechanisms
→ evaluator-qualified mechanisms
```

至少计算：

- `obligation compression ratio`：进入 consolidation 的有效 scope nodes 与最终独立 obligations 的数量关系；
- `cross-claim reuse`：一个 obligation 正确覆盖多个 distinct claim identities 的数量；
- `semantic duplicate rate`：被 evaluator 判定与其他 obligation 重复的最终 obligations 比例；
- `case inflation rate`：应为 case 却被提升为独立 property/item 的比例；
- `over-merge rate`：需要不同 semantics/capability/failure interpretation 却被错误合并的 obligation 比例；
- `unnecessary split rate`：可共享一个 property/mechanism 却被拆分的比例；
- `post-consolidation coverage holes`：Scope Review accepted 后仍缺失的 evaluator-required obligations；
- `disposition accuracy`：adopt/merge/split/case/not-applicable/reject/unknown 与 evaluator adjudication 的 confusion matrix。

Compression 不是越高越好；必须与 qualified coverage、over-merge 和 false acceptance 共同解释。

## 7. Review metrics

### 7.1 Finding validity

- `confirmed finding precision`：Reviewer findings 中被 evaluator/后续 execution 确认为真实缺陷的比例；
- `defect discovery recall`：Review 可见且 evaluator 已知的缺陷中，在 Admission 前被 finding 捕获的比例；
- finding class 分布：scope gap、overlap、unsupported evidence、setup、objective、comparator、capability、safety、
  performance 等。

### 7.2 Revision effect

- `repair rate`：actionable findings 中在下一 accepted/pending revision 被正确修复的比例；
- `regression rate`：revision 新引入 evaluator-confirmed defect 的比例；
- `escape rate`：经过 Review accepted 仍被 evaluator 发现的缺陷比例；
- `revision depth`：每个 scope/property 到 accepted、terminal 或 interruption 的 revision 数分布；
- `no-new-information review rate`：没有新 finding、evidence、mapping 或 decision 的 Review episodes 比例。

### 7.3 Review efficiency

按 Blind Scope、Coverage、Consolidation、Property 和 Coherence Review 分别报告：

- confirmed unique findings per episode；
- confirmed unique findings per 10k output tokens；
- repair-confirmed findings per wall minute；
- Review token/time 占整个 task-run 的比例。

不能把同一 defect 在多个 revision 中重复描述计作多个 unique findings。

## 8. Evidence 与 Worker metrics

每个 request 在执行前冻结 intended decision、competing predictions、required capability 和成功/失败可能产生的 disposition。
Receipt 后由独立 evaluator 分类：

- `Discriminating`：区分至少两个预注册 predictions；
- `DecisionChanging`：通过 exact lineage 改变 scope、mapping、property、mechanism 或 Review decision；
- `Confirmatory`：独立重复支持已有结论；
- `Redundant`：没有新增独立 observation 或重复同源计算；
- `Ambiguous`：成功执行但不足以区分 predictions；
- `InvalidCapabilityClaim`：请求或解释超出 Worker 的真实 capability；
- `ExecutionFailure`：基础设施/toolchain/device failure；
- `SubjectFailure`：实验程序或被测 subject 的失败，需按 request semantics 再解释。

至少报告：

- request → scheduled → executed → receipt → consumed 的漏斗；
- decision-changing 和 discriminating receipt 比例；
- redundant/ambiguous/invalid-capability 比例；
- exact capability class 与 target identity 分布；
- independent versus common-dependency evidence；
- unique confirmed defects per Worker job/device second；
- experiment 引入的新 revision 和 regression；
- scheduling、execution 和 semantic outcomes 分离后的失败率。

Proposal Agent 的“这个实验很有用”不能作为分类依据。

## 9. Authority、安全与恢复指标

以下属于 zero-tolerance protocol outcomes，必须逐 run 报告而非混入平均质量分：

- blind taxonomy exposure violation；
- hidden evaluator/mutant/expected result 暴露给 proposal role；
- cross-task、cross-item、cross-revision 或 sibling receipt 读取成功；
- policy requirement 未经 authority 被降级；
- structural/model approval 被错误当作 semantic qualification；
- restart 后 continuation、visibility manifest 或 feedback lineage 交叉；
- execution failure 被误分类为 semantic rejection/acceptance；
- source、prompt、model body、stdout/stderr、credential 或 hidden content 进入安全日志。

每一项的目标都是零。发现一项时 run 仍保留，标记 protocol-invalid，并进入实现缺陷报告。

同时统计正确 fail-closed 分类率：required unknown、mechanism unavailable、execution failure、operator interruption 和
Admission rejection 必须保持不同 typed outcome。

## 10. Cost、延迟与人工负担

按 workflow stage、role 和 task-run 记录：

- model dispatch started/received/not-sent/ambiguous；
- input、output、cache-read/cache-write tokens 和模型计费（若可得）；
- episode、step、tool proposal/completion/rejection；
- Worker scheduling attempts、jobs、CPU/GPU/NPU/device milliseconds；
- content bytes、artifact 数和 durable storage 增量；
- wall-clock elapsed、queue/scheduling wait 和 active execution time；
- administrator decisions、人工 adjudication/review 数和耗时。

除总量外必须报告：

- per qualified obligation；
- per unique confirmed finding；
- per admitted required claim；
- per successful task-run；
- incomplete/failed run 已经消耗的成本。

## 11. 稳定性与泛化

同一 task/treatment 的多次 repetition 至少报告：

- blind dimension、obligation 和 property 集合在 semantic matching 后的 precision/recall 或 Jaccard；
- required coverage、false acceptance/rejection 和 cost 的分布、方差与区间；
- D1/D2 的 order/anchoring effect；
- Reviewer finding 和 evidence request 的重复性；
- Admission lifecycle outcome 的一致性。

Task corpus 必须包含语义和结构明显不同的类别，例如 elementwise/layout、reduction、数值敏感、state/concurrency、
integration 和 target-performance workload。任务不要求 PyTorch；有无独立 framework/reference 应作为分层变量而非入口条件。

一次 fixture 或一个 seed 的成功不能作为泛化结论。

### 11.1 Developer auditability 与 replay

作为 secondary product outcomes，由不知道 treatment label 的开发者或审计者完成冻结任务：

- 从 admitted claim 追溯到 obligation、property、case、mechanism 和 exact receipt 的正确率与耗时；
- 正确识别 `proved`、`partial`、`unknown`、`not executed` 和 failure classification 的比例；
- 在冻结环境中重放 mechanisms/controls 的成功率；
- 发现缺失 artifact、错误 capability、不可重放步骤和不诚实结论的数量；
- 对 downstream candidate developer 提供 actionable setup/comparator/domain 信息的比例。

主观满意度可以作为 exploratory feedback，但不能替代 replay、trace accuracy 或 hidden evaluator outcomes。

## 12. 不完整运行、失败与排除

### 12.1 Intention-to-treat

正式 manifest 冻结并完成 arm assignment 后，每个 run 都进入 assigned treatment 的结果，包括 provider failure、Worker
failure、typed terminal failure 和 operator interruption。不能只保留自然完成或表现较好的运行。

### 12.2 Preflight exclusion

只有在正式 measurement start 前、按预注册规则识别并修复的产品 wiring/configuration blocker 可以作为 preflight 排除。
必须保留 task identity、failure、fix 和 exclusion reason。正式 run 开始后的相同缺陷不能事后改名为 preflight。

### 12.3 Interrupted/censored runs

- 不为未执行的 Review、control 或 Admission 填入 approval；
- 不把未返回 dispatch 计作模型失败结论；
- 报告 exact stopping stage、已完成 coverage、已消耗成本和 stopping rule；
- primary endpoint 缺失时标记 incomplete，不用较早 artifact 推断 `OracleAccepted`；
- aggregate 时同时报告 completion rate，并使用预注册的 censored/missing-data 规则，不作有利于某 arm 的临时插补。

### 12.4 Failure taxonomy

Provider transport、budget、protocol、execution infrastructure、subject failure、Oracle artifact rejection、negative challenge
accepted、mechanism unavailable 和 operator interruption 分开报告。只有 evaluator-confirmed semantic outcome 进入对应
false acceptance/rejection 指标。

## 13. 随机化、聚合与统计报告

- 以 task 为 block，随机化 treatment 顺序；
- 每个 arm 使用相同 task set、repetition/seed policy 和 frozen capability manifest；
- component experiment 复用 exact admitted Intent/evidence；end-to-end confirmatory experiment 单独分层报告 upstream
  divergence；
- 报告 per-task paired differences，再报告跨 task aggregate；
- 同时给出中心趋势、离散程度/区间和完整原始 task-run 表；
- severity weighting、task weighting 和 partial coverage credit 必须预注册；
- 不以未经预注册的单一 composite score 替代 correctness gates；
- exploratory post-hoc metric 必须明确标为 exploratory，不能改写 primary conclusion。

样本量/power 由正式 evaluator corpus 和预期效应范围另行冻结；在此之前 pilot 只用于估计方差和发现 implementation
blocker，不用于显著性或优胜声明。

## 14. 最小结果报告

每次正式报告至少包含：

1. frozen manifest 和 treatment identity；
2. task-run inclusion/exclusion flow，以及 component experiment 的 upstream snapshot/treatment lineage；
3. primary correctness/authenticity outcomes；
4. blind discovery/anchoring diagnostics；
5. decomposition、Review 和 evidence metrics；
6. authority/protocol violations；
7. stage-level cost、latency 和 human burden；
8. incomplete/failure lifecycle；
9. per-task paired results、variance 和 limitation；
10. exact domain artifact/receipt identities，不包含 prompt、private reasoning、credential 或 hidden material。

正式结论必须区分：

- integration path 是否工作；
- proposal artifact 是否结构完整；
- hidden evaluator 是否证明 semantic coverage；
- Oracle 是否正确接受/拒绝；
- 哪个 treatment 在 correctness gate 满足后更高效。

这五者不能互相替代。
