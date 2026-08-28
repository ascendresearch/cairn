# CUDA → Ascend C Oracle 准入设计

- 状态：规范性目标设计
- 日期：2026-08-27
- 父设计：[Cairn 系统设计](../SYSTEM_DESIGN.md)
- 对应需求：`FR-ORACLE-*`、`FR-FEEDBACK-*`、`FR-PERF-*`、`QR-AUD-*`
- 上游设计：[高阶语义意图恢复](SEMANTIC_INTENT_RECOVERY_DESIGN.md)、
  [Oracle 探索](ORACLE_EXPLORATION_SYSTEM_DESIGN.md)
- 共用边界：[独立准入架构](INDEPENDENT_ADMISSION_DESIGN.md)
- 软件承载：[Admission 软件架构](../design/ADMISSION_ARCHITECTURE.md)
- 性能设计：[性能 Oracle 与硬件能力模型](PERFORMANCE_ORACLE_DESIGN.md)
- 跨会话检查：[设计不变量与实施前清单](DESIGN_INVARIANTS.md)

## 1. 目的

Oracle 是把执行 observation 转换成 CUDA → Ascend C 迁移 claim 的测量仪器。如果它不能区分
正确实现和相关错误实现，或者把 CUDA 偶然行为、模型猜测、宽松容差和 benchmark gaming 当成
正确性依据，那么一个精确-looking verdict 比明确的 `Unknown` 更危险。

Oracle Admission 的职责是回答：

> 对哪一项已准入用户意图、在哪个 domain 和环境中、依据哪些相互依赖或独立的 authority、通过
> 哪些正负控制和真实执行证据，这个 Oracle claim 有资格判断未来的 Ascend C candidate？

Oracle Explorer 的 synthesis/adversarial strategies、外部测试、知识和模型只产生 proposal 或
attack。只有隔离的 Oracle Admission 机械 gate 可以生成 `AdmittedOracleClaim`。可选的
`OracleControlPlannerProfile` 可以由 agent 承担，但它只编排和解释，不拥有最终裁决权。

## 2. 范围与相邻边界

### 2.1 Oracle Admission 不恢复用户意图

Oracle proposal 必须引用已准入的 `MigrationIntentContract`。算法意图、数值意图、模型/部署契约、
CUDA 实现伪影和源端疑似缺陷由独立 Semantic Intent Recovery 提案，再由 Intent Admission 授权。

如果 Oracle 探索发现会改变用户意图的矛盾，它必须产生 `IntentConflictFeedback` 并返回上游；不得
在 Oracle admission 中静默选择 CUDA、文档、reference 或教科书定义中的一方。

### 2.2 Oracle Admission 不预判 candidate

Admission 使用正确实现族、错误变体、trusted mutants、历史故障和绕过控制来评价 Oracle 的
鉴别能力，但不评价尚未冻结的目标 candidate。Candidate judgment 是后续独立阶段。

### 2.3 性能分成“仪器准入”和“候选结果准入”

候选生成前，Oracle Admission 可以冻结：

- 性能 workload 和权重；
- baseline 类别与比较方向；
- measurement、warmup、同步、统计和噪声政策；
- 已准入 hardware facts/conditional ceilings；
- profiling 和 anti-gaming obligations。

它不能预先宣称 candidate 达标、接近 roof 或已定位瓶颈。候选冻结并真实测量后，由 Performance
Admission 从权威 receipt 得出 outcome。性能不能补偿 semantic、numerical、execution 或 safety
失败。

### 2.4 产品范围

本文只设计 CUDA → Ascend C 的 Oracle 准入。任何 domain-neutral verification 机制只是内部依赖，
不构成通用异构迁移产品设计。

## 3. 准入对象：claim portfolio

### 3.1 Oracle claim

`OracleClaimProposal` 至少包含：

- `AdmittedIntentClaimId`：它 operationalize 的确切用户意图；
- `OracleClaimKind`：semantic、numerical、execution、safety、adequacy 或 performance-plan；
- `ClaimDomain`：dtype、shape、layout、值域、alias、环境和前置条件；
- `AuthorityDependencyGraph`：支持、反驳、共同依赖和未知；
- `ObservationPlan`：怎样获得 observation；
- `ExpectedRelation`：exact value、allowed set、property、状态、absence 或性能关系；
- `ComparatorProposal` 与依据；
- `CoverageObligationSet`；
- requested strength 和允许的降级；
- assumptions、conflicts、unknowns、blind spots 和 revalidation triggers。

Proposal 不能携带 trusted mutant、final allowance、admission decision、candidate verdict 或
`passed: bool`。

### 3.2 Portfolio

`OraclePortfolioProposal` 由多个局部 claim 构成，并保存：

- claim 间的依赖和 precedence；
- domain partition 与 coverage map；
- 公共 corpus、reference/property 和执行计划；
- numerical allowance derivation proposal；
- execution/safety/anti-bypass plan；
- performance instrument plan；
- 未覆盖区域和需要用户决策的问题。

Admission 按 claim 审查。一个 claim 可以 `Admitted`，另一个可以 `Conflict` 或 `Unverifiable`。
Portfolio 不存在一个能抹平局部结果的总置信分。

### 3.3 Admitted Oracle portfolio

`AdmittedOraclePortfolio` 只包含：

- 已准入的局部 claim 及 exact scope/strength；
- 每个 claim 的 policy 与 admission receipt；
- 冻结的 corpus、relation、comparator/allowance 和 observation plan；
- execution/safety/performance instrument obligations；
- claim dependency；
- blind spots、unknowns、conflicts、assumptions 和 revalidation triggers。

`PartiallyAdmittedPortfolio` 不能传给要求全量 required claims 的 candidate-release API。下游只能
消费其中类型上明确准入的 claim。

### 3.4 Required claim closure

每个 task 在 Oracle 探索前由 trusted policy 从 `MigrationIntentContract`、requested claims、target
环境和 release policy 派生 `RequiredOracleClaimSet`。它至少说明：

- 哪些 semantic/numerical/execution/safety/adequacy/performance-instrument claims 是 required；
- 每项最低 strength、domain 和 prerequisite；
- 哪些 claim 允许 `NotApplicable` 及其证明义务；
- 哪些 optional claims 只增加信息而不阻塞；
- claim dependency DAG 和聚合规则。

Explorer、synthesis/adversarial strategies 和 candidate 都不能删除 required claim。Gate 验证 dependency graph 无环，
且 prerequisite 不得由下游 claim 反向支持。`PortfolioClosure` 只能从每项底层 outcome 派生：
required claims 未全部达到 policy 要求时，portfolio 最多是 `PartiallyAdmitted`，不能进入整体发布
判断。

## 4. Oracle 的六个准入平面

| 平面 | 准入对象 | 不能由什么替代 |
| --- | --- | --- |
| Semantic/algorithmic | 算法、离散语义、状态和可观察契约 | CUDA 单次输出、sanitizer 或模型解释 |
| Numerical | 合法误差、allowed result set、非确定性包络 | 全局 `atol/rtol` 或同一测量自验证 |
| Execution/integration | binary、ABI、launch、真实 CUDA/NPU 路径和输出来源 | 编译成功、CPU twin 或 wrapper 返回成功 |
| Safety/concurrency | 越界、未初始化、竞争、同步、写覆盖 | 输出碰巧相等或工具“无报告” |
| Adequacy | domain、fault、history、mutation、bypass 检出能力 | case 数量或代码覆盖率 |
| Performance instrument | workload、baseline、measurement、ceilings、anti-gaming | 理论峰值、一次计时或 profiler 建议 |

前五个平面共同构成 correctness authority。性能是一级产品平面，但保持独立 outcome。

## 5. Authority 与非循环性

### 5.1 Authority 来源

可能的 evidence 包括：

- 已准入用户意图和明确的 `UserIntentDecision`；
- 独立规范、论文或框架 schema；
- 独立 CPU/高精度/区间/allowed-set reference；
- CUDA source behavior；
- metamorphic/property relation；
- upstream tests 和外部文档；
- 历史 CUDA→Ascend C 故障；
- sanitizer、trace、device 和 profiler observation；
- hardware T0/T1 claim；
- 上一轮反馈和真实模型接入 observation。

来源只决定 provenance，不自动决定 trust。每个 evidence edge 必须说明 exact claim、适用 domain、
strength 和 dependency。

### 5.2 Source CUDA 的地位

CUDA source 是重要行为证据，不是默认最高语义权威。Admission 先区分：

1. defined 且 deterministic；
2. defined 但允许多个结果；
3. race、越界、未初始化或其他 undefined behavior；
4. 与 admitted intent 冲突。

第 1 类可支持 differential claim；第 2 类需要 allowed-set/property/statistical relation；第 3 类
产生 source-defect/unknown，不生成正常功能 Oracle；第 4 类返回 Intent Admission 或用户政策。

### 5.3 Independence 不是布尔值

Authority graph 至少记录是否共享：

- 作者、model episode、prompt 或生成上下文；
- source/reference code、vendor library 或算法模板；
- compiler、runtime、device backend；
- expected-value derivation 或 corpus；
- 同一测量用于阈值推导和验证。

“三份 reference 一致”只有在对应 claim 和 failure mode 上足够独立时才增强证据。多个模型同意、
Blue/Red 同意或两个框架共享同一底层库都不构成自动 admission。

## 6. 角色和隔离

| Role | 目标 | 可提交 | 不可决定/读取 |
| --- | --- | --- | --- |
| Oracle synthesis strategy（当前可为 Blue） | 提出如何判断正确 | claims、references、properties、cases、valid variants、instrument plans | admission、hidden controls、candidate verdict |
| Oracle adversarial strategy（当前可为 Red） | 寻找 false accept/false reject | correct/wrong variants、attacks、coverage/conflict/bypass findings | admission outcome、synthesis 私有 continuation |
| `OracleControlPlannerProfile` | 为冻结 proposal 选择 obligation 实验、解释 receipt | typed execution plan、diagnostic proposal、stop recommendation | required-set 修改、final outcome、policy 写入、hidden answer 泄漏 |
| Executor/Worker | 运行 opaque authorized jobs | worker-controlled receipt/observation | 算子语义和 verdict |
| Mechanical Gate | 验证 identity、重算事实、应用 policy | admission receipt/outcome | 无证据创造语义 |
| Candidate Search | 生成 Ascend C | candidate revisions | hidden corpus、expected artifact、judge policy |

隔离必须由进程/数据/capability 实现，而不是 prompt 自律：

- proposal、admission 和 candidate 使用不同 event stream、write capability 和 continuation；
- hidden corpus、judge binary、trusted mutants 和 expected artifact 不挂载到 applicant workspace；
- worker evidence channel 不可由 proposal/candidate 写入；
- knowledge/skill 查询经过 role filter，不能旁路 hidden visibility；
- agent 不能通过 tool arguments 自授设备、网络、秘密或 adjudication 权限。

## 7. 输入集合

一次 `OracleAdmissionAttempt` 冻结：

- task、source/caller/context 和 `MigrationIntentContract`；
- exact Oracle proposal revision；
- typed `OracleAdmissionPolicy`；
- public/hidden/historical/production/revalidation corpus identities；
- reference/property/variant/mutant/adapter/executable identities；
- CUDA、CPU、Ascend build/NPU environment 和 device policies；
- comparator/allowance derivation policy；
- performance workload/baseline/hardware facts/measurement policy；
- previous-round feedback bundle，首轮使用显式 `NoPriorFeedback`；
- Planner model/prompt/tool/knowledge/skill snapshot 与预算。

任一 verdict-relevant 输入变化产生新的 attempt identity，不修改旧 attempt。Pre-release 期间仍直接
修改当前 V1 schema，不增加格式版本、兼容 reader 或 migration。

## 8. 上一轮反馈的正式输入契约

### 8.1 为什么 feedback 是一级输入

固定 corpus 和预设 mutants 永远不完整。上一轮真实 candidate failure、Oracle 误收/误拒、profiling
瓶颈、模型接入回归和用户决策可以揭示先验方案没有覆盖的语义、domain、fault 或 workload。

但 feedback 不能是一个 reward 分数或“模型效果变好/变差”的自然语言。它必须先形成可审计的
`OracleFeedbackBundle`。

### 8.2 单项 feedback 必须包含

- `FeedbackId`、origin task/run/attempt/candidate/oracle identities；
- feedback kind 和 target claim/domain；
- exact observation、environment、workload 和 receipt；
- producer/provenance 与 visibility/security classification；
- attribution status：localized、partially localized、correlated 或 unknown；
- reproducibility status 与最小复现；
- supports/refutes/conflicts/reveals-gap 关系；
- freshness 和 revalidation trigger；
- proposed downstream action，但不携带 admission authority。

### 8.3 Feedback 类型与期望作用

| 类型 | Admission 期望获得的内容 | 可以产生的义务 | 不能直接证明 |
| --- | --- | --- | --- |
| `SemanticCounterexample` | 输入、预期关系、实际差异、定位到 intent/claim 的证据 | 新 domain partition、reference/property 修订 | 其他区域错误或正确 |
| `OracleFalseAcceptFeedback` | 一个被独立确认错误却通过的实现和完整路径 | 新 fault class、mutant、observable、fatal regression | 新 Oracle 已充分 |
| `OracleFalseRejectFeedback` | 一个独立证明正确却被拒绝的实现 | valid-family、allowance、reference/comparator 修订 | 放宽容差一定安全 |
| `OracleConflictFeedback` | 两个 authority 的具体矛盾及依赖 | 区分实验、降级或返回 Intent Admission | 自动选择多数来源 |
| `CoverageGap` | 未覆盖 domain/path/fault/interaction | 新 mandatory/hidden obligation | gap 已关闭 |
| `ImplementationFeedback` | Ascend C 无法实现、代价高或受硬件约束的 claim | 检查是否是 artifact、优化自由度或真实 intent 冲突 | 应改变用户意图 |
| `PerformanceFeedback` | profile、baseline、roof、噪声和 workload 证据 | 新 measurement/ceiling/workload obligation | 功能正确性 |
| `ProductionObservation` | 模型版本、调用路径、输入权重、e2e 指标、first-divergence 证据 | production regression、workload reweight proposal | 单个 kernel 全域正确 |
| `UserIntentDecision` | 明确选择、适用 scope 和授权者 | 触发 Intent Admission 或显式 policy revision | 未声明范围的事实，也不能由 Oracle 直接提升意图 |
| `InfrastructureOrToolCorrection` | runner/profiler/sanitizer/comparator 缺陷及受影响 receipt | revalidation 与影响审计 | applicant 本身错误 |

涉及算法、数值、部署或可观察契约的 `UserIntentDecision` 必须先形成新的 admitted intent；Oracle
Admission 只能引用该 contract。只有纯 admission/release policy 决策才进入对应 policy authority。

### 8.4 正向与负向真实模型反馈

- 正向模型效果只支持“在该模型、部署、输入切片和观察窗口中未观察到问题”；不能替代局部
  correctness、safety 或 hidden coverage；
- 负向效果是高价值反例，但在 first-divergence、消融或等价归因完成前，不能自动判定迁移 kernel
  违反哪一 claim；
- kernel microbench 变快但模型变慢是有效 performance/integration conflict；
- 模型效果不变但局部数值 claim 被违反，仍然是 correctness failure；
- 模型 feedback 不能在原地修改 workload weight、容差、roof 或 release threshold，只能形成新
  proposal 并重新 admission。

### 8.5 Feedback admission 与去重

Feedback 在进入 mandatory corpus 或 reusable knowledge 前需检查：

1. receipt/identity 闭合；
2. 复现或明确标为一次性 observation；
3. attribution strength；
4. 是否与已有 obligation 重复；
5. scope 是否过拟合；
6. 是否泄漏 hidden/candidate-private 数据；
7. 是否应仅保留 task-local，还是可以进入 T1/T2 知识流程。

被 dismiss 的 feedback candidate 也记录理由，避免重复误用。

### 8.6 Feedback 使用分类与 held-out 污染

每项 feedback 必须获得一个 `FeedbackUseDisposition`：

- `ExplorationOnly`：只用于形成 hypothesis/实验；
- `ApplicantVisibleRegression`：成为公开 mandatory regression，不再具有 hidden 强度；
- `AdmissionOnlyRegression`：仅在当前 applicant 从未获得其区分信息且 policy 允许时作为 hidden
  control；
- `KnowledgeCandidate`：进入知识 crystallization，不直接进入 gate；
- `PolicyCandidate`：请求 policy review，不直接改变 policy。

同一 observation、同源最小复现或可推导等价 case 不能同时支持同一 claim 的 derivation 和 held-out
validation。来自当前 candidate lineage、已反馈给同一 model episode 或已写入其知识/skill snapshot
的材料默认是 applicant-visible。Corpus builder 必须保存 contamination graph；发现重叠时降低
evidence strength、换用新的 sealed case，或输出 `Unverifiable`，不能只更换 case ID。

## 9. Corpus 与 domain admission

### 9.1 Corpus 分区

- `PublicDerivationCorpus`：Explorer 可读，用于形成 proposal；
- `PublicValidationCorpus`：正负控制和公开调试；
- `HiddenAdmissionCorpus`：防止固定输入特化和 gaming；
- `HistoricalRegressionCorpus`：真实 CUDA→Ascend C 和 Oracle 缺陷；
- `ProductionFeedbackCorpus`：真实模型/部署 observation；
- `RevalidationCorpus`：工具链、设备、知识或政策变化后的控制。

Hidden 不等于正确。Hidden case 自身也需要 intent、expected relation、provenance 和审查。

### 9.2 Hidden corpus 暴露生命周期

每个 hidden case 具有独立状态：

- `Sealed`：applicant、其 model/skill/knowledge snapshot 和 candidate workspace 均未获得区分信息；
- `ConsumedWithoutDisclosure`：只产生不泄漏区分信息的聚合结果，可按 policy 继续使用；
- `BurnedToPublicRegression`：diagnostic、counterexample、日志或外部反馈已经泄漏足以特化的信息；
- `Retired`：case/authority/tool 已失效，不再用于新 admission。

Admission 记录按 applicant lineage 的 exposure ledger 和 diagnostic budget。一个 hidden failure 若要
向负责的 synthesis strategy/Candidate 返回可操作的最小反例，该 case 随即 burned，进入公开 regression，并在需要的
coverage partition 中补充新的 sealed case。仅隐藏 expected bytes 但公开完整输入与 pass/fail 查询
接口仍可能被自适应探测；重复查询、相似 case 和 diagnostic 粒度必须受 policy 约束。

Hidden corpus 的规模不是安全保证。泄漏、burn/replenish、访问审计和 contamination 状态都进入
admission receipt。

### 9.3 Mandatory derivation

可信代码根据 admitted domain 派生适用义务：

- min/max、inside/outside、zero/empty/one/singleton；
- tile/alignment/tail、stride/layout；
- dtype extrema、signed zero、NaN/Inf/subnormal、cancellation、scale；
- invalid pointer/size/shape/status；
- alias/in-place/workspace；
- reduction order、duplicate index、tie、atomic/nondeterminism；
- Ascend data movement、pipeline、多核、同步和写覆盖；
- historical faults、previous feedback 和 hidden interactions。

Synthesis/adversarial strategies、fuzzing 和 knowledge 可以增加 case，但不能删除 trusted mandatory
obligation。

### 9.4 Case intent

每个 case 必须说明它覆盖的 contract 条件、semantic partition、数值区域、tile/launch 路径、历史
故障、mutant、metamorphic relation 或真实 workload。随机 seed 和 case count 不能单独构成覆盖依据。

## 10. Semantic/algorithmic admission

### 10.1 Reference strategy

按 claim 选择并记录强度：

- specification-derived/independent reference；
- higher-precision、interval 或 allowed-result set；
- independent differential；
- property/metamorphic；
- translation-validation 子证明；
- implicit runtime-only；
- unavailable。

“f64 reference”本身不是 exact 证明。复杂超越函数、discontinuity、tie、量化和并行 reduction 都需
单独说明 reference strength。

### 10.2 三方/多方 observation

Case proposal、CUDA source、independent reference/property、correct variants 和 target observations
分开保存。两方一致只能在 dependency graph 支持时定位第三方 suspect；不是多数票真值。

Unresolved disagreement 的结果是 `Conflict`、`AdmittedWithLimits` 或 `Unverifiable`，不能为了自动化
率选择一个 expected output。

### 10.3 Correct-by-construction variants

False-reject control 必须引用独立 construction claim，例如合法 accumulation order、分块重组、
等价 partition、transpose 或受前置条件约束的代数变换。“它通过 Oracle”不是正确性理由。

Variant 数量、结构独立性、construction classes 和 saturation 是 policy 配置。不存在适合所有
operator 的硬编码全局数量。

### 10.4 随机、有状态和多合法结果语义

对随机、原子、多线程调度或跨调用状态 kernel，Oracle 必须先准入对应 contract：

- `DeterminismContract`：固定 input/environment 下是否要求唯一结果；
- `RandomnessContract`：seed、generator/state、sample independence、distribution、moment/tail 或
  sequence relation；
- `AllowedOutcomeSet`：原子/调度允许的结果集合及不变量；
- `StateTransitionContract`：调用前状态、输出、side effect、调用后状态和跨 stream/call ordering；
- `RepetitionPolicy`：重复次数、隔离/reset、统计功效、type-I/type-II error policy。

单次 CUDA 输出不能定义随机分布或合法调度集合。统计 Oracle 必须记录采样假设、multiple-testing
修正、最小检测效应和 inconclusive outcome；“没有显著差异”不等于等价。状态清理失败、seed 泄漏
或重复运行不独立会使 observation invalid，而不是 candidate pass/fail。

## 11. Numerical admission

### 11.1 必须分开的三种比较

| 比较 | 回答问题 | 产物 |
| --- | --- | --- |
| candidate/reference-property | 候选是否落在准入语义接受域 | candidate numerical observation |
| valid family/reference-property | 合法实现会占据多大差异空间 | false-reject evidence/allowance proposal |
| implementation/self repetitions | 声明条件下是否稳定或具有合法分布 | determinism/statistical claim |

一个执行可以复用 observation，但三项 claim、policy 和 receipt 身份不同。

### 11.2 Comparator family

按 domain 选择 bit/exact、normalized exact、absolute/relative、ULP、interval/range、set/multiset/
permutation、property 或 statistical envelope。全局 `atol/rtol` 只有在全域依据充分时才可准入。

### 11.3 Allowance provenance 与 assurance

Magnitude、provenance 和 assurance 是不同类型。Provenance 至少区分：

- exact/set-derived；
- measured correct family；
- measured adversarial；
- analytical/external prior；
- asserted/unsupported。

Assurance 至少区分：

- proven bound；
- exhaustive finite；
- identity-disjoint held-out validation；
- exploratory measurement；
- prior-only；
- unsupported。

经验 held-out evidence 最多支持显式 empirical claim。只有 proven bound 或 exhaustive finite 可以
支持无保留的全域 numerical claim。安全系数不会把 observed maximum 变成数学上界。

### 11.4 禁止自验证

若 measurement `M` 推导 threshold `T`，用同一个 `M` 对 `T` 做 validation 不增加 assurance。
Derivation/validation corpus 必须 identity-disjoint；mutant 刚好放在容差外只证明 comparator 执行了
`T`，不证明 `T` 合理。

## 12. Execution/integration admission

必须证明被比较的 observation 来自声明路径：

- exact source、candidate source、compiler/CANN 和 binary identity；
- kernel symbol、ABI 参数顺序/宽度、BlockDim、tiling key、workspace、stream；
- 指定 CUDA GPU 或 Ascend NPU/device identity；
- launch 确实发生、异步完成已同步、runtime/device status 已捕获；
- 输出完整写入，非初始化值、旧值或 fallback；
- adapter、runner、container 和 evidence channel 的适用 attestation；
- stdout/result artifact 不能自称来自 device。

CPU twin、host fixture 和 target build 分别只能证明 debug/transport/build claim。任何一项都不能替代
真实 NPU execution。无法独立观察时输出 unverified assumption，不升级强度。

## 13. Safety/concurrency admission

Safety claim 至少考虑：

- out-of-bounds 与 capacity shortfall；
- alignment、alias、partial overlap；
- uninitialized read/output unwritten；
- data race、多核写冲突和 pipeline contention；
- synchronization、event/flag 配对；
- timeout、hang、crash、async error；
- memory/resource leak（工具可观察时）。

CUDA Compute Sanitizer、Ascend msSanitizer 或其他工具的结果必须绑定工具版本、模式、运行路径和
已知限制。“无报告”只支持工具覆盖范围内的 absence claim。安全通过不证明算法正确，输出相等也
不证明安全。

## 14. Adequacy admission

### 14.1 正控制、负控制、冲突和绕过

每个适用 claim 至少选择：

- honest path；
- independently correct false-reject control；
- deliberately wrong false-accept control；
- trusted targeted mutation；
- authority conflict/unknown/domain-outside control；
- no-launch、constant-output、fixed-shape、fallback、stale-output、answer-leak bypass；
- historical and previous-feedback regressions；
- hidden corpus；
- replay/revalidation control。

### 14.2 Mutation grid

每个 applicable mutant × case cell 记录 injection、execution 和 comparison identity。Disposition：

- `PolicySized`：按公布边界构造，miss fatal；
- `ScaleFree`：确定性破坏，miss fatal；
- `CaseDependent`：在特定 case 可能被合法 allowance 吞掉，miss 成为 mandatory blind spot；
- `NotInjectable`：必须给受审查理由。

空 applicable grid、缺 cell 或只改 comparator receipt 都不能通过。Mutation score 只说明对已建模
fault 的敏感度，不是正确概率。

### 14.3 Coverage claim

Coverage receipt 保存 required/exercised obligations、domain partition、interaction、historical faults、
unexplored regions 和 metric limitation。高代码覆盖率、随机 case 数量或“全部公开样例通过”不能
替代 fault-detection evidence。

## 15. Performance instrument admission

Oracle Admission 在 candidate search 前准入的是测量仪器：

- workload 是否代表用户/模型调用，权重是否有 provenance；
- baseline 是否回答当前 claim；
- warmup、同步、重复、样本、异常值和 noise policy；
- device contention、温度、频率、功耗和后台占用控制；
- profiler 字段是否校准；
- hardware ceiling 是否适用于 exact dtype/shape/engine/memory/dataflow/toolchain；
- anti-gaming 和 hidden workload；
- correctness 前置 gate；
- measurement invalid/inconclusive outcome。

理论 peak、实测 sustainable ceiling、algorithmic roof、implementation roof、candidate observation
和业务 target 不可互换。候选的 `MeetsTarget`、`ImprovesBaseline`、`NearApplicableRoof`、
`BottleneckSupported` 等结果在后续 Performance Admission 产生。

## 16. Executed admission 流程

对每个 proposal revision，trusted orchestration：

1. 验证 V1 schema、strong types、identity、role、provenance 和 license/data policy；
2. 验证 `MigrationIntentContract` 及 claim/domain 对齐；
3. 加载 feedback bundle，检查 receipt、归因、去重和 visibility；
4. 可信代码派生 mandatory domain/corpus/fault/feedback obligations；
5. 验证 reference/property/comparator/observation/performance-instrument proposal；
6. 运行最便宜的 schema/静态/reference self-consistency；
7. 构建并执行 correct variants，建立 false-reject control；
8. 按允许 evidence 推导 numerical allowance；
9. 构建并执行 wrong variants 与完整 mutation grid；
10. 执行 CUDA source interrogation、sanitizer 和定义行为分类；
11. 在需要的强度上执行 Ascend/真实设备 observation-path controls；
12. 运行 conflict、unknown、domain、hidden 和 bypass controls；
13. 验证 performance workload/measurement/hardware fact 适用性；
14. 检查 historical/production/previous-feedback regressions；
15. 从底层 receipt 重算每个 claim outcome、strength 和 blind spots；
16. 生成 `Admitted`、`AdmittedWithLimits`、`Rejected`、`Unverifiable`、`Conflict`、
    `BudgetExhausted` 或 `InfrastructureFailure`；
17. 冻结 portfolio 或返回最小、非泄漏 diagnostic。

检查顺序可按成本和区分力优化，但一个便宜的 decisive failure 后不运行无必要的稀缺设备任务。
所有 skipped obligations 记录 typed reason。

## 17. Admission policy

`OracleAdmissionPolicy` 至少规定：

- required claim kinds 与最低 evidence strength；
- correct/wrong variant classes、independence 和 saturation；
- trusted mutant set 与 fatal-miss policy；
- corpus partitions、hidden/feedback/history obligations；
- comparator/allowance provenance 与 assurance；
- CUDA defined-behavior 和 authority-conflict policy；
- required execution/safety/device attestation scopes；
- performance instrument requirements；
- feedback attribution/reproducibility minimum；
- budget、停止和降级规则；
- diagnostic visibility；
- revalidation triggers。

Policy 是 trusted input，proposal 不能修改。不同 operator/claim 可以选择不同 policy profile；
不存在隐藏在 verifier 常量中的全局 variant 数量或 tolerance。

## 18. Receipt 与 evidence closure

`OracleAdmissionReceipt` 至少引用：

- proposal、intent、task、policy、environment 和 admission attempt；
- domain partitions 与各类 corpus；
- authority dependency graph；
- reference/property/comparator/allowance facts；
- correct/wrong variants、construction/fault evidence；
- mutation grid、historical/feedback/hidden/bypass trials；
- CUDA/Ascend execution 和 safety observations；
- performance instrument/hardware-fact checks；
- 每个 claim 的 outcome、strength、failed obligations 和 blind spots；
- conflicts、unknowns、assumptions、unverified facts；
- budget/stopping reason 和 revalidation triggers。

Gate 必须验证：

1. receipt 来自 controller 承认的 execution/tool completion；
2. job/attempt 绑定 exact artifact、environment、executable 和 policy；
3. declared outputs 完整且 identity 匹配；
4. worker evidence 不可由 applicant 写入；
5. comparison/statistics/fatal miss 从底层事实重算；
6. required obligations 没有 missing/duplicate/extra；
7. evidence graph 能回溯到原始 input 或明确 external reference。

存储的 summary 只是 projection。篡改 `passed`、outcome 或统计摘要不能覆盖 underlying trials。

## 19. Outcome 与诊断

| Outcome | 含义 |
| --- | --- |
| `Admitted` | exact claim/domain/strength 满足 policy |
| `AdmittedWithLimits` | 只有子域或较低强度满足，限制进入类型和 manifest |
| `Rejected` | 有可重现反例或违反 admission policy |
| `Unverifiable` | 现有 evidence 无法建立 requested strength |
| `Conflict` | authority 矛盾且政策无法裁决 |
| `BudgetExhausted` | obligations 未完成且预算结束 |
| `InfrastructureFailure` | 应有 observation 因系统/环境失败 |

Diagnostic 必须包含负责 role、frozen proposal、attempt、failed claim/obligation、公开 counterexample、
证据和可恢复缺陷。它不得泄漏 hidden expected value、mutant definition 或其他 role 的私有 continuation。

修订必须提交完整、改变后的 proposal，产生新 identity 和 attempt。重复投票或不变 proposal 不能
制造 admission。

Human/operator 可以另行签发 scoped `RiskAcceptanceDecision` 来决定是否继续搜索、部署实验或
接受 blind spot，但它不能更改 admission outcome，也不能把 `Violated`、`Unknown`、`Conflict` 或
`NotExecuted` 改写为 `Admitted`/`Satisfied`。

## 20. Candidate judgment 的关系

Candidate judge 只消费：

- exact `MigrationIntentContract`；
- exact `AdmittedOraclePortfolio`；
- frozen candidate/source/build/binary；
- admitted observation path 和 authoritative receipts；
- 后续 Performance Admission receipt；
- applicable production/model integration evidence。

它按 semantic、numerical、execution、safety、adequacy、performance 和 integration 输出 claim-scoped
`Satisfied`、`Violated`、`Unknown`、`Conflict`、`NotApplicable`、`NotExecuted` 或
`InfrastructureFailure`。Oracle admission 不能被 candidate 成功倒推为正确；candidate failure 也
不能自动证明 Oracle 正确。

## 21. Feedback、演化、撤回与 revalidation

以下事件触发新 admission 或影响审计：

- 新 semantic counterexample、false accept/false reject；
- 新真实模型 regression 或 workload 分布变化；
- 用户意图决策或 intent contract 改变；
- CUDA/Ascend source、compiler、CANN、firmware、SoC、runner 改变；
- comparator、policy、corpus、mutant 或 measurement 机制改变；
- knowledge、skill 或 hardware fact 被撤回；
- admission gate 自身发现 bug。

系统不会修改旧 Oracle 或 verdict。它创建新 proposal/admission identity，并反向列出依赖结果：

- unaffected；
- scope/strength reduced；
- revalidation required；
- unsupported/retracted。

Feedback 经 admission 和复用审查后可写为 task-local T3、measured T1 或 validated T2；不能从一次
运行直接写成通用规则。

## 22. 裁判与验证机制自身的 qualification

把 comparator、runner 或 gate 放进 trusted repository 只定义了 TCB 边界，不证明它们正确。
Oracle Admission 使用的 `VerificationMechanismSet` 至少包括：

- mandatory-case/domain derivation；
- reference/property evaluator；
- comparator 与 allowance/statistics engine；
- input materializer、call adapter 和 result parser；
- runner、device/launch attestation 和 evidence capture；
- mutant injector 与 coverage mapper；
- sanitizer/profiler adapter；
- admission policy evaluator、portfolio aggregation 和 diagnostic redactor。

每项机制保存 `Proposed → Reviewed → Qualified → Refuted` 生命周期、exact content identity、支持的
claim/domain/environment、positive/negative controls、已知限制和 requalification triggers。

Qualification 至少要求：

1. honest control 与 verified perturbation；
2. 证明 perturbation 作用于预期机制；
3. false-reject、empty/missing/extra/tampered input 控制；
4. applicable historical regression；
5. runner/tool 适用环境和 calibration；
6. gate/policy evaluator 的 independent recomputation 或 golden/property/mutation suite；
7. 机制自身无法通过 candidate-writable summary 自我授权。

这存在不可消除的 trust root：最底层 schema/identity/gate 不能完全用自己证明自己。Cairn 以小型
repository-owned TCB、代码审查、独立 test oracle、mutation/fault injection、真实工具校准和可重复
receipt 建立资格，并明确残余假设；不能用“第二个 agent 复核”掩盖 bootstrap 问题。

Admission policy 也是受控 artifact。改变 required claims、strength、mutant、hidden/feedback use、
allowance、aggregation、diagnostic visibility 或 stopping rule，需要 `PolicyQualificationReceipt`、
影响分析和新 policy identity。Applicant/Planner 无写权限。Mechanism 或 policy 被 refute 时，反向
审计所有依赖的 Oracle 和 candidate verdict。

## 23. 威胁模型

Admission 必须显式报告：

- SIR、Oracle synthesis/adversarial strategy、candidate 共享模型/provider prior；
- intent、reference、expected value 和 candidate 共享同一生成链；
- reference 从 CUDA source 派生却被标为独立；
- 多个 provider 共享底层库或数据；
- allowance derivation 与 validation 重叠；
- correct variant family 太小或 construction claim 不独立；
- sampled domain 未覆盖 adversarial/interaction region；
- comparator 正确但 build/launch/observe path 未测；
- CPU twin/build 证据冒充 NPU execution；
- sanitizer/profiler 字段在当前工具路径下未校准；
- hidden corpus 泄漏或 candidate 针对公开输入特化；
- performance benchmark 与真实模型 workload 不一致；
- positive model feedback 掩盖局部错误；
- negative model feedback 未完成归因；
- runner/device identity 无法独立 attestation。

这些是 typed assumptions/limitations，不是报告末尾的免责声明。

## 24. 强类型边界

以下概念必须是不同验证类型：

- `OracleClaimProposalId`、`OraclePortfolioProposalId`、`OracleAdmissionAttemptId`、
  `OracleAdmissionReceiptId`、`AdmittedOracleClaimId`、`AdmittedOraclePortfolioId`；
- proposed/admitted intent 与 proposed/admitted Oracle；
- semantic/numerical/execution/safety/adequacy/performance-plan claim；
- public/hidden/historical/production/revalidation corpus；
- correct construction、wrong fault injection 和 trusted mutant；
- allowance magnitude、provenance 和 assurance；
- planner recommendation、worker observation、authoritative receipt 和 gate outcome；
- feedback kinds、attribution 和 reproducibility；
- admission outcome、candidate claim outcome、task outcome 和 release policy outcome；
- theoretical peak、measured ceiling、candidate observation 和 business target。
- verification mechanism lifecycle、qualification receipt 与普通 Oracle admission receipt；
- hidden case state、exposure ledger、diagnostic budget 和 feedback-use disposition；
- `RequiredOracleClaimSet`、`PortfolioClosure` 与单项 claim outcome。

反序列化必须重新执行构造 invariant。Static/compile-fail tests 至少证明：

- `IntentHypothesisSet` 不能传给 Oracle Admission；
- proposal 不能传给 candidate judge；
- performance outcome 不能满足 semantic claim；
- public corpus ID 不能获得 hidden capability；
- exploratory feedback 不能冒充 `UserIntentDecision`；
- asserted allowance 不能传给要求 measured/proven allowance 的 policy；
- `AdmittedWithLimits` 不能传给要求全域 admitted claim 的 API。

## 25. 当前实现证据与缺口

现有 historical reduction control 已证明部分机制：

- correct/wrong implementation variants；
- measured-family allowance 与已知 false reject；
- mutation grid 和 case-dependent blind spot；
- host build/execute/observe/compare receipt closure；
- admitted Oracle 到 candidate correctness outcome 的硬件无关路径。

固定 `matmul-zero-k` f32 路径证明了模型结构化 proposal、ABI/shape/raw bits、expected artifact
隔离、adapter、capture 和 comparator 的 transport/materialization 链路。

它们尚未证明：

- SIR/Intent Admission；
- 完整 claim portfolio 与 authority graph；
- feedback bundle 和真实模型归因；
- 真实 CUDA/Ascend C candidate device execution；
- target safety/synchronization/anti-bypass；
- general numerical comparator/allowance；
- hardware facts、multi-roofline、performance instrument/Admission；
- knowledge/skill retraction 与 Oracle revalidation；
- 第二个真实 CUDA→Ascend C kernel。

因此这些实现继续作为回归和基础设施 evidence，不定义最终 Oracle 自动生成范式。

## 26. 首个新架构准入控制（进入实施前确定）

实现恢复前，首个控制至少应能证明：

1. 从已准入的局部 `MigrationIntentContract` 开始，而不是由 Blue 自行决定意图；
2. 一个 semantic claim、一个 numerical claim、execution/safety/adequacy obligations 和一个
   performance instrument plan 形成 portfolio；
3. 正确变体、错误变体、conflict、unknown、domain-outside 和 bypass controls 均有真实路径；
4. 上一轮 false accept/false reject 或 production observation 以 typed feedback 输入，并创建新的
   proposal/admission lineage；
5. positive model feedback 不会提升局部 correctness，negative feedback 未归因时不会误判 claim；
6. hidden corpus 和 expected artifact 对 Explorer/Candidate 不可见；
7. `OracleControlPlannerProfile` 的推荐无法绕过 mechanical gate；
8. 任一 required claim 未满足时不能导出可发布结果，性能也不能补偿；
9. receipt graph 可回溯，篡改 summary 无效；
10. 一个 hidden failure 被反馈后会 burn 为公开 regression，并补充新的 sealed control；
11. feedback derivation 与 held-out contamination 会被识别并降低强度或拒绝；
12. comparator、adapter、runner 和 policy evaluator 具有独立 qualification controls；
13. 所有未实现设备、工具、coverage 和 authority 均显式 `Unknown`/`NotExecuted`/blind spot。

首个 operator 与 non-adaptive hidden corpus 已由 D-039 决定；hardware profile、knowledge/skill、feedback
acquisition 和一般 adaptive hidden policy 仍分别由 OQ-020、OQ-021、OQ-022、OQ-024 决定。本文件不在
本轮授权实现。
