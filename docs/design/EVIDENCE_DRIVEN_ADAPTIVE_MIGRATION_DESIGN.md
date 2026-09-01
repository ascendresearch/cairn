# 证据驱动、自适应迁移共设计

- 状态：第一性原理复盘后的候选设计，尚非当前规范或实现事实
- 日期：2026-09-01
- 产品范围：此前未知 CUDA 软件到硬件亲和 Ascend C migration package
- 首个目标硬件：Ascend 950PR（3510）
- 简称：方案 E，`EvidenceDrivenAdaptiveMigrationCoDesign`

## 1. 文档地位

本文档记录方案 D 之后对 SIR、Oracle、Candidate 关系的完整重新推导。它不假设既有工作流必须保留，也不因模型能力
快速进化就删除已经证明有价值的 authority、evidence、Worker、Review 和 Admission 边界。

方案 E 当前是下一轮设计和消融实验的候选输入，不会自动替换
[`CAIRN_CURRENT_PRODUCT_DESIGN.md`](CAIRN_CURRENT_PRODUCT_DESIGN.md)、
[`SIR_ORACLE_CURRENT_DESIGN.md`](SIR_ORACLE_CURRENT_DESIGN.md) 或当前实现。方案 D 仍由
[`BLIND_FIRST_ORACLE_SCOPE_DESIGN.md`](BLIND_FIRST_ORACLE_SCOPE_DESIGN.md) 完整保存，作为 E 的对照和可升级安全路径。

若实验支持 E，应直接修改当前 V1 领域模型、代码、测试和规范；不得创建 V2、legacy alias、双读写、转换器或
compatibility fallback。本文与 `AGENTS.md` 冲突时，以 `AGENTS.md` 为准。示例只解释通用机制，不能进入 production
prompt、fixture branch 或已知答案。

写后完整性核对见
[`EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_COMPLETENESS.md`](EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_COMPLETENESS.md)。

## 2. 第一性原理结论

代码迁移不可消除的不是名为 SIR、Oracle 或 Candidate 的三个固定阶段，而是三个问题：

1. **Specification gap**：输入源码、测试、caller 声明和真实用户意图可能不完整、互相矛盾或包含缺陷；
2. **Reality gap**：编译器、CUDA 行为、Ascend C、CANN、ABI、950PR 设备和性能事实不能由语言模型思考出来；
3. **Trust gap**：模型的自信、schema 完整、角色共识和自然语言解释都不能成为可重放的产品证据。

因此必须保留的逻辑功能是：

- 理解并约束迁移目标；
- 观察 exact source/reference/target reality；
- 形成能反驳候选的验证要求；
- 搜索和修订硬件亲和实现；
- 在受控环境中执行；
- 独立重算什么可以被相信；
- 对无法证明的结论保持 `Unknown`、`Partial`、`Rejected` 或 `NotExecuted`。

但这些逻辑功能不必全部被固定成“先完成一份完整 SIR，再完成一份完整 Oracle，最后才第一次产生 Candidate”的时间顺序。
Candidate 往往会暴露 ABI、layout、compiler、数据搬运、数值和性能问题；在看见候选之前假装能够完整预知所有验证
mechanism，可能产生昂贵而虚假的完备性。

方案 E 的核心判断是：

> 固定 authority、effect、evidence、release 和 replay 边界；让可逆的认知分解按任务与新证据自适应；让语义契约、
> 验证义务和 Ascend C 候选在同一个有 lineage 的证据循环中共同演化。

## 3. 不以“足够强的 runtime model”为前提

方案 E 不把 runtime model 分成“足够强”与“不够强”两个稳定类别。同一个 deployment 会随任务、上下文压力、预算、
tool feedback 和 provider 版本出现局部能力变化。典型失效包括：

- 能概括算法，却漏掉异步、alias、tail 或特殊值语义；
- 能生成可编译候选，却不主动构造能反驳自己的控制；
- 能报告已知 unknown，却不知道自己遗漏了什么；
- 在简单 task 上一次闭合，在新 dtype、layout、并发或性能任务上突然失效；
- 产生完整、流畅、自洽但没有 execution evidence 的错误结果。

因此：

- 模型始终是 proposal actor，不是 authority；
- Controller 不把模型 confidence 用作 workflow transition；
- 同一模型换角色或多个模型达成共识不构成 evidence；
- 结构化升级必须能由 evidence gap、Review finding、policy ledger、control failure 和 Admission failure推动；
- 低能力模型允许更早 `Abstained` 或 fail closed，不能为了 workflow completion 伪造成功；
- 模型、adapter、预算和工具配置是需要记录和实验的能力条件，不是被架构隐藏的常量。

方案 E 的产品价值不是保证任意模型都能迁移任意 CUDA，而是让能力不稳定的模型在可验证边界内工作：能完成时交付
证据，不能完成时准确暴露缺口。

## 4. 必须区分的三个层次

### 4.1 不可避免的认知功能

任何能够迁移代码的 runtime model 都必须在推理中理解源程序、形成迁移假设并考虑怎样判断结果。无论这些思考是否
写入独立 artifact，它们都已经发生。

因此不能先增加一个 `IntentReadinessAssessment`、skip-SIR Reviewer 或“重新评估是否需要 SIR”的通用前置 episode，
再声称 SIR 是可选的。该前置判断本身就在执行 SIR 的认知功能，只是换了名称。

### 4.2 可选的显式工作流协议

方案 E 中的 **SIR protocol** 专指：当真实迁移推理遇到会改变下游结果、且不能从当前证据消除的语义分叉时，将源语义
假设物化为独立、持久、可实验、可 Review、可请求用户确认的澄清工作流。

方案 E 中的 **up-front complete Oracle workflow** 专指：在任何 Candidate 出现前，先完整枚举和实现全部验证义务。
该时间顺序也不是默认必经路径。

### 4.3 发布前不可省略的权威制品

无论认知过程如何组织，发布 migration package 前都必须存在并冻结：

- exact admitted intent contract；
- exact target platform context；
- candidate-facing validation bundle；
- 完整 policy concern disposition；
- qualified Worker receipts 和 capability closure；
- Oracle Admission 与 Candidate Admission outcomes；
- known limitations、unknown、revalidation trigger 和 replay lineage。

所以 E 不是“不要语义契约和 Oracle”，而是“不要把它们的完整形式化提前固定为所有任务相同的认知流水线”。

## 5. 产品边界

### 5.1 输入 CUDA 不是 specification

CUDA source、tests 和 CUDA execution 都是 evidence。CUDA bug、race、undefined behavior、错误边界结果或偶然数值行为
不会自动成为 Ascend C 迁移要求。系统必须区分 source behavior、desired behavior、independent reference 和 candidate
acceptance。

### 5.2 Ascend C first

长期输出优先是针对 exact Ascend SoC 和 toolchain 的硬件亲和 Ascend C kernel family、host tiling、dispatch 和 integration，
而不是默认退化为 API 替换、现有高层算子或模板库包装。现有库、PyTorch、Triton-Ascend 或 framework implementation
可以作为 reference、baseline、seed 或显式 escape hatch，但不能定义产品能力上限。

### 5.3 PyTorch 可选

最低任务可以只有 CUDA source、host launcher、build files 和 caller declaration。若存在 PyTorch/CPU/framework reference、
tests 或 OpInfo，应作为有 provenance 的一个 evidence provider 使用；它不天然正确，也不是启动任务的前置条件。

### 5.4 第一个目标固定为 950PR（3510）

所有 Ascend build、execution、profile、performance 和 platform claim 必须绑定 exact 950PR、CANN/toolchain、ABI 和环境。
其他设备、模拟器、host shell 或 CUDA receipt 不能替代要求的 950PR evidence。

## 6. 核心不变量

### 6.1 Authority

- runtime model 可以提出 intent、experiment、validation 和 candidate，不能准入自己的结论；
- Controller 是唯一 workflow writer，只做 typed routing、freeze、capability grant、durability 和机械闭合；
- Controller 不通过 atomics、dtype、API 名称或 coding-agent fixture interpretation 推断用户语义；
- Worker 只执行 exact job contract，不解释 intent、不产生 verdict；
- administrator 只对 exact semantic/policy decision request 授权；
- Intent、Oracle 和 Candidate Admission 独立重算，不调用模型补齐缺失事实；
- hidden evaluator、mutant、negative challenge 和 expected result 永不暴露给 proposal roles。

### 6.2 Evidence

- 模型陈述、同源 Review 和结构完整只形成 proposal/review provenance；
- compiler、CUDA、reference、Ascend、950PR 和 profiler observation 必须来自匹配 capability 的真实 Worker receipt；
- source observation 不拥有 intent authority；
- candidate success 不能反向证明 comparator 正确；
- execution failure、subject failure、semantic rejection 和 protocol violation必须保持不同类型；
- required evidence 缺失时 fail closed，不用 confidence、budget exhaustion 或 fallback 补足。

### 6.3 Cognitive freedom

- 不规定模型必须先列固定维度再思考；
- 初始迁移 actor 看不到 Cairn 内部 Oracle taxonomy，但看到真实用户要求、代码、target 和获准 evidence；
- 不持久化仅为了让流程看起来完整、没有 authority/effect/replay consumer 的 planning artifacts；
- 可以使用一个或多个 Agent episodes，但角色数量不是 assurance 指标；
- 只有引入新信息、聚焦真实缺陷或承载 authority/recovery 的分解才值得保留。

### 6.4 Release completeness

- 最终 concern ledger 没有静默缺口；
- correctness、numerical、integration、safety、Oracle adequacy 和 performance 均有 typed disposition；
- `Unknown`、`NotApplicable`、`ApplicableInformational`、`NotExecuted` 和 `Rejected` 不互相冒充；
- performance 不能补偿 required correctness 或 safety 失败；
- package 只声明 exact qualification epoch 已证明的范围。

## 7. 总体架构

```mermaid
flowchart TD
    intake["Frozen task + caller + 950PR target"]
    reason["Migration reasoning episodes\nsource + contract + assurance + candidate hypotheses"]
    graph[["Evidence / Assurance Graph"]]
    worker["Managed Workers\nCUDA / CPU / Ascend build / 950PR / profile"]
    sir{"Semantic ambiguity\nmaterially changes migration?"}
    clarify["Focused SIR protocol\nexperiments + administrator decision"]
    explore["Exploratory Ascend C candidate family"]
    challenge["Late sealed-policy coverage challenge"]
    qualify[["Qualification Epoch"]]
    oracle{"Mechanical Oracle Admission"}
    candidate{"Mechanical Candidate Admission"}
    package[["Reviewable Migration Package"]]

    intake --> reason
    reason <--> graph
    reason --> sir
    sir -->|yes| clarify --> graph
    sir -->|no separate protocol| explore
    graph <--> worker
    explore <--> worker
    worker --> graph
    graph --> challenge --> qualify
    explore --> qualify
    qualify --> oracle --> candidate --> package
    oracle -->|typed defect / evidence gap| graph
    candidate -->|typed diagnosis| graph
```

这不是一个自由运行的单 Agent。它是一个 Controller 管理的共设计状态机：模型可以灵活组织可逆推理，所有外部 effect、
authority transition、artifact revision、information boundary 和 release decision 仍然被严格控制。

`cairn-server` 继续保持业务无关，`cairn-migration-app` 负责产品 composition。Intent、assurance/Oracle、Candidate 和
Admission 都是主 workflow ports 背后的函数边界，不是独立 proposal 进程。各 focused role 使用真实 Agent Loop；graph
node、candidate revision、experiment、control 和 feedback 的外层遍历由 Controller 机械编排，不是嵌套 Agent Loop。

## 8. Task Intake 与预先承诺

正常入口保持：

```text
cairn-cli → cairn-server → migration app API → CudaMigrationWorkflow → managed Workers
```

Task Intake 至少冻结：

- task source、host launch、build、tests 和 caller scope；
- target `TargetPlatformContextV1`，包括 950PR、CANN/toolchain、ABI 和 capability；
- source/reference/framework materials 的访问 authority；
- runtime model、adapter、budget、tool 和 data policy；
- skill/knowledge snapshot；
- exact previous feedback 或 `NoPriorFeedback`；
- completion goal 和 operator stopping policy。

在首个 reasoning episode 打开前，Controller 还必须 seal 但不向初始 actor 暴露：

- task-generic Oracle policy catalog identity；
- 从 future admitted claims、target 和 operator policy 派生 task concern ledger 的 frozen derivation policy；
- challenge-stage exposure manifest；
- hidden evaluator、control family 和 Admission policy identities。

由于 admitted claims 此时可能尚未产生，task-specific ledger 可以稍后实例化；但实例化必须由已 seal 的 derivation policy
机械完成，绑定 exact admitted contract，并在 challenge 开始前冻结。operator 或 coding agent 不能根据 actor 的自然发现
临时修改 catalog 或 derivation rule。

## 9. Migration Reasoning Kernel

“kernel”指一组可恢复、可替换的 runtime-model reasoning episodes，不表示必须由一个模型、一个长 continuation 或一个
进程完成。它直接处理真实迁移任务，可以同时维护：

- source/caller semantic hypotheses；
- competing interpretations 和 unknown；
- target platform questions；
- candidate algorithm/layout/schedule hypotheses；
- exploratory Ascend C revisions；
- validation properties、cases 和 mechanism hypotheses；
- evidence requests；
- failure diagnosis 和下一步选择。

它不先运行一个通用“是否需要 SIR”分类器，也不先填 Cairn 规定的维度矩阵。它从实际迁移工作开始；只有出现需要跨越
authority、effect、episode 或 release boundary 的内容时，才把相应节点物化进 Evidence/Assurance Graph。

Controller 可以基于预算、job completion、typed outcome 和 Gate failure决定暂停、恢复或升级结构，但不能替模型完成
semantic mapping。模型可以被替换或使用新的 reviewer episode；所有 actor 消费同一冻结 lineage，而不是依赖隐藏聊天记忆。

## 10. SIR 的准确定位

### 10.1 不存在单独的启动判断阶段

方案 E 明确禁止为每个 task 增加以下通用前置物：

- `ShouldStartSir` classifier；
- `IntentReadinessAssessment`；
- 专门审查“是否可以跳过 SIR”的 Reviewer；
- Controller 基于代码特征自动决定语义是否含糊。

这些步骤都已经在做源语义分析，等价于一个改名后的强制 SIR。

### 10.2 何时物化 focused SIR protocol

Migration Reasoning Kernel 在真实理解、生成或验证过程中，如果发现两个或多个 plausible interpretations 会导致不同的
Ascend C candidate、comparator、domain、ABI 或用户可见行为，且当前 evidence 不能消除分叉，则提交 typed
`IntentClarificationRequiredV1`。它至少绑定：

- exact competing interpretations；
- 每个 interpretation 的 source/caller/reference evidence；
- 它们对 candidate 或 validation 的具体差异；
- 可以区分它们的 Worker experiment；
- 若 evidence 仍不足，需要 administrator 决定的 exact 问题。

Controller 只验证 binding 和 authority，然后进入 focused SIR protocol。它不判断哪种解释正确。SIR 可以请求 CUDA、
reference、sanitizer 或其他 Worker，并最终形成新的 intent proposal 与 administrator decision lineage。

用户显式提出语义冲突也可以直接形成该请求。后续 Worker、Reviewer、Oracle control 或 Candidate failure 只能先形成
observation/finding；仍由获准的 runtime reasoning actor解释它是否暴露 intent 分叉，再提交同一种 typed request。

### 10.3 没有 focused SIR 时

若实际迁移推理没有发现需要外部澄清的分叉，不运行独立 SIR episode。Reasoning Kernel 仍必须在进入 qualification 前
提交统一的 `IntentContractProposalV1`，说明：

- candidate 必须保持的 semantics；
- implementation freedoms；
- source behavior dispositions；
- supported domain；
- exact evidence、unknown 和 limitation。

这承认模型必然进行了源语义理解，但不为该认知功能再创建一套重复的“是否启动 SIR”协议。`IntentContractProposalV1`
只有一个语义定义；focused SIR 和直接 reasoning path 都产生它，不建立 legacy/fast-path 两种下游 contract。

### 10.4 不可保证发现所有语义歧义

如果输入没有独立 specification、用户也未声明某个行为，任何模型或工作流都无法从源码外推唯一“真实意图”。方案 E
不能承诺消除不可识别性。它通过 source-defect controls、late policy challenge、independent evidence、candidate feedback、
hidden evaluation 和诚实 unknown 降低风险，但不把缺失 specification伪装成已证明事实。

## 11. Intent Admission

发布或 qualification 前必须完成 Intent Admission。它消费 exact proposal、evidence、source revision、target、用户 decision
和 provenance，机械检查：

- source observation 是否被误提升为 desired semantics；
- competing hypotheses、conflict 和 unknown 是否被保留；
- administrator decision 是否对应 exact request；
- claim/domain/freedom 是否有允许 evidence；
- evidence independence 是否被夸大；
- prior feedback 是否属于当前 lineage。

输出仍是 claim-scoped `AdmittedIntentContractV1` 和 `AdmittedIntentEvidenceSnapshotV1`。下游不能读取未筛选的全部
reasoning transcript，也不能只依赖自然语言 summary。

Intent 可以在 co-design 中较早 admission，也可以在 exploratory evidence 后 admission；但没有 admitted contract 的
Candidate 只能保持 `Exploratory`，不能进入最终 qualification 或 package。

## 12. Evidence / Assurance Graph

### 12.1 作用

Graph 是跨 episode、跨 Worker、跨 candidate revision 的共享事实与保证状态，不是模型全部思维链的永久化。只有具有
后续 consumer、authority、effect、replay 或审计价值的内容进入图。

### 12.2 强类型节点

至少区分：

- source/caller/framework facts；
- proposed、admitted 和 rejected intent claims；
- competing hypotheses、unknown 和 user decisions；
- source/reference/platform observations；
- target platform facts；
- candidate family、variant 和 revision；
- policy 暴露前自然形成的 `OrganicAssuranceConcernId`；
- validation obligation、property、case 和 mechanism；
- experiment request、job、attempt 和 receipt；
- Review finding、feedback 和 revision；
- policy concern、disposition 和 coverage mapping；
- qualification epoch、Admission outcome 和 limitation。

这些节点不能共享 generic ID。不同 evidence class、target、revision、scope 和 authority state必须由类型表达。

wire/storage boundary 的 raw string、integer 或 digest 必须立即转换成 validated domain type。反序列化必须重新执行 public
constructor invariants；容易混淆的 identity、revision、capability、evidence class 和 outcome 应有 compile-fail 或等价静态
boundary test。两个类型当前共享表示不是合并为 generic abstraction 的理由。

### 12.3 强类型边

至少区分：

- `Supports`、`Refutes`、`LeavesUndetermined`；
- `DerivedFrom`、`ObservedUnder`、`AppliesToDomain`；
- `DependsOn`、`SharesDependencyWith`、`ContaminatedBy`；
- `MotivatesExperiment`、`ConsumedByDecision`；
- `CoversClaim`、`TestsCandidate`、`QualifiesMechanism`；
- `SupersedesRevision`、`InvalidatesEpoch`、`RoutesFeedbackTo`。

图的结构可以帮助 Controller 重算 lineage、缺口和 capability closure，但语义等价、coverage mapping 和缺陷解释仍由
runtime model/Reviewer 提案并受 evaluator 检查，不能用字符串或 embedding 自动获得 authority。

## 13. 早期 Exploratory Candidate

方案 E 允许在 complete Oracle accepted 前生成和执行
`ExploratoryAscendCandidateV1`，目的是获取只有真实 candidate consumer 才能暴露的信息：

- Ascend C build 和 host integration 是否成立；
- ABI、shape、layout、tiling、alignment 和 workspace 假设是否真实；
- CANN/compiler 对表达方式和 resource 的限制；
- Validation mechanism 能否真实绑定 target candidate；
- correctness baseline、数值差异和 failure signature；
- 950PR 上可行的 pipeline、memory、core mapping 和性能方向。

Exploratory Candidate：

- 绑定当前 working/admitted intent、target、evidence 和 revision；
- 可以请求 Ascend build、NPU execution 和 profile Worker；
- 不能获得 final Candidate verdict；
- 不能修改 admitted intent、放宽 comparator 或删除 failed cases；
- 不能被交付为 migration package；
- 若基于未 admitted hypothesis 构建，必须显式标记 provisional domain。

Candidate family 从容易检查的 correctness baseline 开始，再探索 target-specific variants 和 dispatch。性能变体不能覆盖
baseline 的历史 evidence。

## 14. Oracle 作为持续演化的 Assurance 子图

方案 E 不要求在第一个 Candidate 前完成最终 Oracle portfolio。Reasoning Kernel 可以随着 source、reference、target 和
candidate evidence逐步提出：

- candidate-facing properties；
- input/domain case families；
- expected/reference/metamorphic relations；
- numerical comparator 与 allowance derivation；
- ABI/integration、安全和并发 obligations；
- performance workload 与 measurement policy；
- executable mechanisms 和 capability requirements。

每个 validation node 必须回答它将如何判断 future Ascend C candidate。只评价 CUDA source quality、无法产生 candidate
verdict 的分析不计入 Oracle coverage。

Oracle 与 Candidate 可以共同演化，但不能互相迎合：

- candidate failure 可以发现 Oracle gap，也可以发现 candidate defect，必须 typed diagnosis；
- 因看见 candidate 失败而修改 comparator，必须由 admitted intent 或独立 evidence 支持；
- Oracle revision 会使依赖旧 Oracle 的 qualification epoch 失效并要求重跑；
- final hidden cases、mutants 和 expected results 始终对 reasoning actor 不可见；
- qualification 前 public validation bundle 必须冻结，并先证明它能接受正确变体、拒绝定向错误行为。

## 15. 从方案 D 继承的 blind-first policy challenge

方案 E 不删除 D 解决的“模型不知道自己遗漏了什么”问题，而是改变它出现的时机。

### 15.1 Organic blind work

初始 Reasoning Kernel 不读取 Cairn 内部 concern taxonomy。它在实际迁移、候选探索和证据请求中自然形成 semantic、
numerical、integration、safety、adequacy、performance 或任务特有 concern。用户明确说出的精度、性能等要求仍然可见。

在首次 policy challenge 前，Controller 冻结当前 assurance subgraph、visible evidence、model/tool/skill exposure 和 receipt
lineage。该 artifact 是 E 的 blind result；不需要额外运行一个只为了列维度的 Blind Scope episode。

为了能够审计自然发现而不重新引入 scope-first 阶段，actor 在实际迁移中一旦把一个风险提升为跨 episode consumer，就以
`OrganicAssuranceConcernV1` 物化：它必须绑定 exact trigger evidence、candidate acceptance risk、applicable domain、与已有
concern 的关系和下一项 evidence/decision。它不是预先要求模型列出的维度清单；没有后续 consumer 的临时思考仍留在
episode 内。policy challenge 后不能回填或改写这些 pre-challenge identities。

### 15.2 Late policy coverage challenge

Intent admitted 后，Controller 使用预先 sealed derivation policy 实例化完整 task concern ledger。新的 Coverage Auditor
读取 frozen organic graph 和 policy ledger，为每个 concern instance 提出 mapping、gap、overlap、case inflation 和
capability challenge。

最终仍使用 D 的 typed disposition 语义：

- `AdoptAsIndependent`；
- `MergeIntoObligation`；
- `SplitAcrossObligations`；
- `RepresentAsCase`；
- `ApplicableInformational`；
- `NotApplicable`；
- `RejectBlindProposalAsUnsupported`；
- `UnknownRequiresEvidence`。

Catalog 是 coverage floor，不是允许列表。organic novel concern 必须保留，不能因 catalog 没有同名项删除。required
unknown 继续阻塞；Worker 不可用不是 `NotApplicable`。

### 15.3 Adaptive consolidation

若 organic graph 已经形成清晰、低重复、candidate-facing 且 mechanism-ready 的 obligations，challenge 后只需要一次全局
Consolidation/Scope Review。若发现严重 gap、overmerge、case inflation 或不可靠 mechanism，则升级到方案 D 的完整
obligation → property → case → mechanism 与 per-property Review。

因此 D 在 E 中有两个地位：

1. 独立的 up-front 对照 treatment；
2. E 的结构化升级与 fail-safe 路径。

E 不允许模型自行宣布“无需 policy challenge”。release ledger 的完整 disposition 是固定 assurance boundary。

## 16. 自适应结构升级

结构升级不是 Controller 对源码做语义分析，而是对已经出现的 typed state 作出机械响应：

| 当前事实 | 下一动作 | 谁解释语义 |
| --- | --- | --- |
| reasoning actor 提交 exact semantic fork | focused SIR、Worker 或 administrator decision | runtime model / administrator |
| evidence request 已授权 | capability-matched Worker execution | Worker 只观察，不解释 |
| receipt 与预测不一致 | 新 diagnosis episode | runtime model / Reviewer |
| policy ledger 存在 unmapped required concern | Coverage/Consolidation Review | Coverage roles |
| mechanism 缺 binding/capability/receipt | typed compile、execution 或 qualification | Controller / Worker |
| Review 发现 actionable defect | exact revision loop | Developer/Reviewer role |
| no-new-information Review 重复 | 停止重复角色调用，保持 gap 或升级 evidence | Controller 按 policy |
| hidden/negative control failure | Oracle reconciliation，不能让 Candidate 调宽 Oracle | Admission / exact feedback route |
| Candidate failure | candidate、platform、Oracle 或 intent typed diagnosis | independent reasoning episode |

一个可实现的升级梯度是：

1. **Organic co-design**：一个或少量 reasoning episodes，直接产生 evidence、candidate 和 validation graph；
2. **Focused intervention**：只对 exact ambiguity、failed mechanism 或 gap 增加实验/Review；
3. **Global policy challenge**：发布前固定执行，关闭完整 ledger；
4. **Full D decomposition**：对严重 gap、高风险或 Review failure 展开独立 obligations 和 per-property loops；
5. **Human decision / Abstain**：缺 authority、specification 或可执行 evidence 时明确停止。

是否进入第 4 层由 frozen policy 对 gap severity、Gate state 和 evidence closure 决定，不由模型 confidence 决定；但
Controller 仍不自行解释业务语义。

## 17. Review 原则

Review 只在以下条件下值得增加独立 episode：

- 聚焦 exact artifact 和已知 failure mode；
- 可以读取新的 source slice、evidence、receipt、policy challenge 或 candidate result；
- 产生会被 Controller/Admission 消费的 typed finding；
- 承担 release-critical adversarial inspection。

同一模型换名复述同一材料不增加 evidence strength。Reviewer 必须区分：

- scope/coverage gap；
- unsupported evidence；
- objective/comparator defect；
- case inflation、duplicate、overmerge 或 split error；
- mechanism binding/setup/capability defect；
- candidate implementation defect；
- source/intent ambiguity；
- execution infrastructure failure。

finding 必须路由到 exact graph node 和 revision。Reviewer 不能批量重写 portfolio、修改 admitted intent 或直接发布
Admission outcome。

## 18. Worker、tool、skill 与 knowledge

### 18.1 Worker

SIR、assurance 和 candidate reasoning 都可以请求普通 managed Worker。至少区分：

- host/reference arithmetic；
- CPU/framework reference；
- CUDA compile/execution/sanitizer；
- Ascend C compile；
- simulator/emulator；
- Ascend 950PR execution；
- profiler/performance measurement。

每个 request 必须冻结 exact uncertainty/decision、competing predictions、required capability、input、cost/risk bound 和
requesting lineage。receipt 只投影给 exact authorized consumer。POSIX shell 不能冒充 CUDA 或 950PR evidence。

系统不恢复 Proposal Host、专属 proposal binary 或 effect-yield 旁路。

### 18.2 Skill 与 knowledge

Skill、official documentation、platform facts、debug methods 和 reusable Ascend C primitives 可以扩大模型有效能力，但：

- 具有 exact provenance、scope、target/version、trust state 和 revalidation trigger；
- 只能建议推理或工具请求，不能扩张 capability 或产生 authority；
- 按需 progressive disclosure，不把整个库复制进 prompt；
- 初始 organic 阶段只能读取不会泄露 sealed policy taxonomy 的内容；
- challenge-only 与 Admission-restricted material 使用不同强类型 exposure authority；
- fixture 的算法答案、expected output、ID 和 prompt 不能沉淀为产品知识。

方案 E 需要保留这条 seam，但当前消融阶段不以前置实现通用知识库或 skill 库为条件。

## 19. Qualification Epoch

方案 E 必须把 proposal-visible、可反复使用的 `DevelopmentOracleRevisionV1` 与拥有 release authority 的
`QualificationOracleRevisionV1` 分开。Development Oracle 是搜索反馈，可以被 Candidate 学习；Qualification Oracle 绑定
冻结 policy、独立 controls 和 exposure state，不能作为无限次可查询的训练接口。

当 Controller 判断 release prerequisites 在结构上可尝试闭合时，创建 immutable `QualificationEpochV1`，冻结：

- admitted intent contract 与 evidence snapshot；
- exact 950PR target context；
- candidate family/variant revisions 和 dispatch policy；
- public validation bundle；
- complete policy ledger、coverage graph 和 dispositions；
- admissible Worker receipts 和 capability manifest；
- model/tool/skill/knowledge snapshot identities；
- qualification、hidden control 和 Admission policy identities。

任何会影响 semantics、domain、target、candidate、comparator、mechanism 或 policy disposition 的 revision 都必须
`InvalidateQualificationEpoch` 并创建新 epoch。不能把旧 receipt 静默附到新 revision，也不能用 Candidate 的当前表现修改
同一 epoch 的 judge。

一个 qualification verdict 只对以下不可拆分的组合成立：

```text
AdmittedIntentRevision
× QualificationOracleRevision
× TargetPlatformContext
× CandidateRevision / CandidateFamilyRevision
× PromotionPolicy
= QualificationEpoch
```

不存在脱离 exact epoch 的“该 Candidate 已通过”。Oracle、Intent、target、Candidate 或 promotion policy 任一相关 revision
变化，旧 verdict 只能作为历史 evidence，不能自动继承。

## 20. Oracle 与 Candidate Admission

### 20.1 Oracle Admission

model-free Oracle Admission 先重算：

- policy concern closure 和 candidate-facing coverage；
- mechanism binding、execution authenticity 和 exact capability；
- honest/correct-variant acceptance；
- targeted mutant、negative 和 hidden disjoint challenge rejection；
- numerical/domain/comparator provenance；
- evidence independence、shared dependency 和 contamination；
- required unknown、execution failure 和 protocol outcome。

`OracleArtifactRejected`、`NegativeChallengeAccepted`、`MechanismProtocolViolation` 和 `ExecutionFailure` 保持不同反馈路径。
只有可归因于 public Oracle artifact 的 defect 才进入 Oracle revision；不能要求 Developer 修改正确 Oracle 去适配错误 control。

Hidden challenge 必须使用不同于 public/original item、但由 Controller 确定且强类型绑定的 challenge identity，不能错误复用
同组 plan 的相同 item。control exit 31 等“负向挑战被错误接受”的执行语义必须归入 `NegativeChallengeAccepted` 和
Controller control reconciliation，而不是伪装成 `OracleArtifactRejected` 发送给 Developer。

Developer/Reviewer 若需要读取诊断，只能读取当前 graph node、artifact revision 和 exact authorized receipt 的 stdout/stderr
projection；sibling receipt、missing content 和 over-limit content 一律拒绝。当前单 artifact 上限保持 16 KiB。正文内容不进入
普通日志，也不能因 E 的 shared graph 扩大可见范围。

### 20.2 Candidate Admission

只有 Oracle accepted 后，Candidate Admission 才对 exact epoch 中的 candidate family 重算：

- semantic/algorithmic correctness；
- numerical acceptance 与 assurance；
- ABI/framework/execution authenticity；
- memory/state/concurrency safety；
- target resource/performance；
- variant applicability 和 dispatch correctness；
- package completeness、known limitation 和 replayability。

性能必须在 exact 950PR 上相对有意义的 target baseline 和真实 workload测量。CUDA 与 Ascend 裸时间不能直接成为公平
verdict。性能不能补偿 required correctness、numerical、integration 或 safety failure。

### 20.3 反馈不能循环放宽标准

Candidate failure 必须先分类为 candidate defect、Oracle defect、platform fact gap、intent ambiguity、execution failure 或
protocol violation。若回到 intent 或 Oracle，旧 epoch 失效并完整 requalification。hidden material和 sibling diagnostic 不会
因此暴露给 proposal actor。

### 20.4 Candidate lifecycle 与“晋升”

每次代码、tiling、dispatch、integration 或适用 domain 修改都产生新的 immutable `CandidateRevisionId`，不能覆盖 parent。
至少区分：

- `Exploratory`：可以请求 build/run/profile，只拥有证据发现 authority；
- `DevelopmentEligible`：通过当前公开开发门禁，可以进入同版本比较；
- `QualificationPending`：已绑定一个 exact `QualificationEpochId`，该 artifact 不再修改；
- `Qualified`：通过该 epoch 的全部 Oracle、Candidate 和 promotion gates；
- `Rejected`：在 exact epoch 中失败；
- `Superseded`：同一 domain 内已有更合适的 qualified revision，历史与 evidence 仍保留。

`latest`、`best`、`passed` 不能作为未绑定 domain/epoch 的布尔状态。新 revision 不因时间更晚、模型声称修复、公开测试分数
更高或一次 benchmark 更快而替代上一轮。

### 20.5 Oracle revision change control

Oracle 根据 Candidate feedback 变化时，必须先形成以下互斥的 typed cause：

| 变化类型 | 语义 | 必须发生的动作 |
| --- | --- | --- |
| `OracleArtifactCorrection` | harness、binding、observation 或 comparator 实现有缺陷，但 admitted intent 未变 | 新 Oracle revision；旧 epoch 失效；新旧 Candidate 与 controls 对称重测 |
| `CoverageExpansion` | 同一 admitted intent/domain 内发现遗漏 property、case 或 failure mode | 新 Oracle revision；所有相关 Candidate 进入同一扩大后的 portfolio |
| `EvidenceStrengthening` | 增加独立 reference、mutant、950PR receipt 或更强 capability | 重算依赖旧 evidence 的 qualification，不改写历史 observation |
| `IntentContractChange` | desired semantics、domain 或 implementation freedom 发生变化 | 新 Intent lineage；不能把结果表述为旧任务上的 Candidate 改进 |
| `TargetPolicyChange` | 950PR/CANN、workload、resource 或 performance policy 改变 | 新 target/policy lineage 和 qualification epoch |
| `CandidateAccommodationAttempt` | 只因为当前 Candidate 失败而希望放宽 judge，且无前述独立依据 | 拒绝并记录 authority violation |

Oracle change proposal 不能由当前 Candidate 的“需要通过”作为证据。Artifact correction、coverage expansion 和 evidence
strengthening 必须先通过独立 Oracle meta-qualification：接受 correct variants/honest controls、拒绝 targeted mutants/negative
controls，并证明变化与 admitted intent 一致。

最关键的对称性不变量是：

> Oracle revision 改变后，不能只重测最新 Candidate；parent、当前 revision 和所有被比较 variants 必须在同一新 Oracle
> revision、target 和 policy 下重新评价。

### 20.6 Candidate Promotion Gates

一个新 revision 只有依次通过以下门禁，才能成为同一 domain 的新 qualified/preferred Candidate。

#### 20.6.1 Revision Integrity Gate

- exact task、parent、source diff、generator episode 和 artifact identity；
- exact compiler/toolchain、build、target、host integration 和 dispatch binding；
- 无 hidden material、sibling receipt、test identity 或未授权 fallback；
- receipt 对应真实提交的 Ascend C artifact，而不是 host/reference 替身；
- 新增 specialization、fallback 和 domain partition 均显式声明。

#### 20.6.2 Required Non-Regression Gate

新 revision 必须重新通过当前 epoch 的全部 required semantic、numerical、integration、safety、domain 和既有 public regression
obligations。修复一个 finding 不能替代全量 required replay；性能提升不能抵消任何 required failure。

#### 20.6.3 Claimed Improvement Gate

每个 promotion proposal 必须在看到正式 qualification outcome 前冻结 exact improvement claim，例如：

- 修复一个 evaluator-confirmed correctness/safety defect；
- 扩大 supported domain；
- 改善预先声明的 numerical metric；
- 改善 exact 950PR workload 的 latency/throughput；
- 降低 workspace、resource 或 instability；
- 增加一个有独立适用 domain 的 specialized variant。

proposal 同时声明可能的 regression surface、最低实际有意义变化和所需 evidence。没有 improvement claim 的 revision 可以继续
探索，但不能仅凭“更新”晋升为 preferred Candidate。

#### 20.6.4 Comparative Promotion Gate

parent/baseline 与新 revision 必须在相同 Intent、Oracle、target、workload、measurement 和统计 policy 下比较。正确性、
安全、integration 和 required numerical allowance 是不可交易约束；通过后才比较 performance、precision、workspace、domain
和稳定性。

晋升只允许以下情形：

- 相同 domain 内至少一个预声明目标达到 minimum practical improvement，其他 required outcome 无不允许的回退；
- 修复 required defect，且没有引入新的 required regression；
- 扩大 admitted supported domain，同时原 domain 不回退；
- 成为 Candidate Family 中新的 Pareto variant，绑定不同 workload/domain，而不是声称全局替代。

若新 revision 仅在一部分 shape/dtype/workload 上更优，Controller 保留多个 variants 并验证 dispatch；不得用局部 benchmark
宣布全局 supersession。

#### 20.6.5 Independent Qualification Gate

最后使用 proposal actor 不可见的 correct variants、targeted mutants、hidden/disjoint inputs、execution-authenticity controls、
source-defect traps 和 950PR workload samples。只有全部 required Gate 闭合，才产生
`QualifiedCandidateRevisionV1`；否则保持 rejected、partial 或 exploratory。

### 20.7 性能、精度和多目标规则

“性能更快”和“精度更好”必须是预先冻结的可判定 claim，不能在看到数据后挑指标：

- 性能绑定 exact 950PR/CANN、workload distribution、baseline、warmup、重复、稳定性、统计方法和 minimum practical
  improvement；
- 小于测量噪声或预注册实际意义阈值的差异不构成 promotion；
- numerical improvement 绑定 exact metric、reference provenance、domain、special-value/tail policy 和最低改善；
- 平均误差改善不能掩盖 critical tail、特殊值或 required domain regression；
- 通过缩小 domain、调宽 tolerance、删除 workload 或改变 weighting 制造的“提升”属于 protocol violation；
- 性能、精度、workspace 或稳定性互有优劣时保留 Pareto variants，由 admitted workload/dispatch policy 决定，不强造单一总分。

如果当前 revision 的目标只是 correctness repair，可以不要求性能严格提升，但必须满足冻结的 non-regression/resource policy。

### 20.8 Hidden control 的自适应查询与曝光

固定 hidden tests 并不足以阻止作弊：多次 submission 的 pass/fail 也是信息。每个 restricted control 至少有不同的
`ControlExposureStateV1`：

- `Hidden`：proposal roles 从未获得 identity、input、expected result 或可逆推出它们的诊断；
- `FeedbackAbstracted`：只公开预注册的粗粒度 typed failure class；
- `RetiredToPublic`：为修复提供了具体 counterexample/诊断，此 control 不再计作 hidden，进入 public regression，并补充新的
  independent hidden coverage。

同一个详细泄露后的 case 不能在下一 epoch 继续冒充 unseen challenge。正式 qualification 必须有查询预算、submission count、
holdout refresh/rotation policy 和 stopping rule；不能让 runtime model 通过无限次 Gate 调用逐位学习 hidden evaluator。

其他防护包括：

- clean-room build/run，Candidate 看不到 control identity、harness secret 或 sibling execution；
- randomized/disjoint domain sampling、metamorphic properties、mutation 和多机制 evidence，而非少量固定答案；
- 检查 harness detection、hard-coded expected outputs、fixture/test-ID branching 和 benchmark-only specialization；
- legitimate specialization 必须在 qualification 前声明 domain、dispatch 和 fallback，并在独立 workload 上评价；
- qualification feedback 只给完成下一次公共开发所需的最小 typed category；若公开具体输入，则执行 retire-and-replace。

系统不能理论上证明任意程序不会对有限 evaluator 过拟合；它必须限制反馈泄漏、扩大分布与机制多样性，并把最终 claim
限制在真实 evidence 覆盖范围内。

### 20.9 搜索与终止

第 `k` 轮搜索状态可以表示为：

```text
S_k = admitted/provisional intent
    + target/platform facts
    + public assurance graph
    + candidate family
    + proposal-visible evidence
    + exact public feedback

CandidateRevision_(k+1) = RuntimeModelProposal(S_k)
```

但 promotion 必须在同一 epoch 中计算：

```text
Promote(c_new, c_parent |
        admitted_intent_e,
        qualification_oracle_e,
        target_e,
        promotion_policy_e)
```

只要条件之一改变，就建立新 epoch并对称重测。停止条件不是“模型认为优化完成”，而是以下之一：

- 至少一个 candidate/variant 通过全部 required Gate，并满足其 improvement claim；
- 新 revision 只产生测量噪声或未达到 minimum practical improvement，保留原 qualified Candidate；
- 连续探索没有新 evidence/qualified improvement，达到预注册 plateau/budget rule；
- required specification、capability 或 evidence 缺失，进入 typed abstain/partial terminal；
- policy 允许交付 Pareto candidate family，而不是继续追求不存在的全局唯一最优。

开发循环可以具有对抗搜索性质；release Gate 不能加入追逐。Candidate 可以学习 Development Oracle，不能无限学习仍被称为
独立证明的 Qualification Oracle。

## 21. 持久化、恢复与安全日志

必须 durable 的最小集合包括：

- frozen intake、sealed policy identities 和 exposure manifests；
- materialized intent/SIR artifacts、decisions 和 Admission；
- graph authority nodes、edges 和 revisions；
- candidate family、validation bundle 和 feedback lineage；
- Worker request/job/attempt/receipt/projection；
- blind freeze、policy challenge、disposition 和 Scope Review；
- qualification epoch、controls 和 Admission outcomes。

Controller restart 必须恢复 exact visibility 和 revision。已经有 terminal receipt 的 job 不重复执行；旧 continuation、feedback、
receipt 或 policy exposure 不能跨 lineage。

日志只记录 task、stage、role、episode、graph node、revision、job/receipt、epoch、计数、状态和失败分类。不得记录 source、
prompt、model body、stdout/stderr、hidden content、provider/auth token、credential 或用户敏感数据。诊断正文只通过 exact
authorization 和 content limit读取。实验可在独立 typed usage record 中保存 input/output/cache token **计数**和成本，不保存
token 内容、prompt 或 response body。

## 22. Fail closed、Abstain 与诚实产品边界

方案 E 必须允许以下正常终态，而不是只允许 success：

- `NeedsAdministratorDecision`；
- `IncompleteSpecification`；
- `IncompleteEvidence`；
- `RequiredCapabilityUnavailable`；
- `ExecutionFailure`；
- `OracleRejected`；
- `CandidateRejected`；
- `PartialQualification`；
- `Abstained`；
- `MigrationPackageAccepted`。

模型弱、预算耗尽、Worker 不可用或无法构造独立 reference 都不是放宽 Gate 的理由。系统可以交付 exploratory artifacts
或 partial report，但必须明确不能采用的范围和下一步所需 evidence。

## 23. 开发者交付

最终 `MigrationPackageV1` 至少包含：

- Ascend C kernel family；
- host tiling、TilingKey、dispatch、build 和 integration；
- validation properties、cases、references、mechanisms 和 replay commands；
- benchmark/profile scripts 与 exact 950PR context；
- supported dtype、shape、layout、alias、framework 和 workload domain；
- admitted semantics、implementation freedoms 和 source behavior dispositions；
- correctness、numerical、integration、安全、adequacy 和 performance outcomes；
- known limitations、unknown、not-executed 和 revalidation trigger；
- exact qualification epoch、receipt lineage、source diff 和采用建议。

成功标准是开发者能够理解、重放、审查并采用 package，而不是 workflow 到达 `accepted` 字符串。

## 24. 方案 D 与 E 的精确区别

| 问题 | 方案 D | 方案 E |
| --- | --- | --- |
| 首个主要动作 | admitted Intent 后先做 blind Oracle scope | 直接开始迁移、evidence、validation 与 exploratory candidate 共设计 |
| SIR | 沿用上游已经完成的 Intent | focused SIR 仅在真实推理出现需权威澄清的分叉时物化 |
| Oracle 时机 | candidate 前完成 scope 和分层展开 | assurance graph 随 candidate/evidence 演化，qualification 前冻结 |
| blind artifact | 独立 `BlindOracleScopeProposalV1` | policy 暴露前冻结的 organic assurance subgraph |
| policy challenge | scope discovery 后立即 | qualification 前固定执行；gap 可升级到完整 D |
| 结构深度 | 默认完整 obligation/property Review | evidence/risk/Gate 驱动；release completeness 不变 |
| Candidate | D 文档范围外、位于 admitted Oracle 后 | early exploratory candidate 是发现 platform/validation facts 的工具 |
| 主要风险 | 前置 scope 和 Review 成本、维度膨胀 | actor 漏报 ambiguity/gap、candidate 与 Oracle 共适应 |
| 主要防护 | sealed catalog、global consolidation、independent Review | 同样的 late challenge，加 qualification epoch、hidden controls 和 D fallback |

E 不是对 D 的否定。D 测量“先完整分析再实现”的价值；E 测量“让实现与 assurance 互相暴露问题、只在必要时展开
结构”是否能在相同可信门槛下降低成本并提高真实迁移成功率。

## 25. 消融实验

### 25.1 实验问题

需要回答的不是“SIR/Oracle 是否存在”，而是：

> 在模型能力和任务难度变化时，何时物化语义澄清、何时展开完整验证结构、何时生成探索候选，能够以最低成本达到
> 相同或更高的最终可信度？

### 25.2 建议 treatments

1. **D-Upfront**：admitted Intent 后执行完整 blind scope → policy challenge → obligation/property/mechanism，再生成 Candidate；
2. **E-Adaptive**：organic co-design、early exploratory Candidate、late policy challenge、按 gap 升级；
3. **E-Full-D-Fallback**：与 E 相同，但任何预注册 high-severity gap 或 repeated qualification failure 都进入完整 D；
4. **E-Organic-Only diagnostic**：不向 actor展示 policy challenge，用于测量自然发现能力；该组仍接受同一 hidden evaluator，
   但不具备产品 release 资格，不能因为便宜被选为默认架构。

若需要隔离 SIR，可在 exact task/evidence 条件下另设：direct contract proposal 与真实 ambiguity-triggered focused SIR。不能
为了实验手工构造“应该启动 SIR”的 fixture answer；ground truth 必须来自独立 specification、用户 decision、source-defect
trap 或 treatment-blinded adjudication。

### 25.3 能力梯度

实验不能只使用最强、最大预算模型。至少按以下条件分层：

- 不同 frozen model deployments；
- 同一模型不同 reasoning/token/tool budget；
- 不同 context pressure；
- 有/无独立 reference；
- 简单、数值敏感、layout、reduction、state/concurrency、integration 和 performance-sensitive tasks。

需要观察强模型是否能减少无价值结构、中等模型是否被 policy/Review/Worker及时补救、较弱模型是否安全 fail closed。

### 25.4 公平性与正常路径

- 同一 task 的 treatment 使用相同 source、caller、target、model class、capability 和 hidden evaluator；
- treatment 顺序随机化，多次重复，不能以单 seed 或单 fixture 结论；
- 固定 generous 但 bounded 的预算，不逐次小幅放宽；
- 全部运行通过正常 CLI/server/workflow/Worker 路径；
- 不调用 internal helper、手写 proposal、伪造 receipt 或构造测试专用 Candidate；
- coding agent 不解释 fixture 后宣称 runtime model 成功；
- 至少包含两个语义、结构和 evidence shape 明显不同的此前未知任务。

## 26. 测量体系

方案 E 继承 [`D_MEASUREMENT_PROTOCOL.md`](../experiments/reasoning-decomposition-ablation/D_MEASUREMENT_PROTOCOL.md)
的 correctness-first 原则、semantic matching、intention-to-treat、failure taxonomy、authority zero-tolerance 和成本分母。
Primary gates 不变：

- evaluator-qualified required coverage；
- hidden invalid challenge false acceptance；
- correct variant/honest control false rejection；
- exact required capability closure；
- 只有通过 correctness gate 后才比较成本。

E 还必须增加以下指标。

### 26.1 迁移结果

- time-to-first-compiling Ascend C；
- time-to-first-running 950PR implementation；
- time-to-qualified、reviewable、mergeable package；
- end-to-end accepted package rate；
- qualified promotion rate、rejected revision rate 和 retained-parent rate；
- source defect 被错误 promotion 或复制的比例；
- target baseline 上真实性能、workspace、stability 和 workload coverage；
- 人工问题数、修改量和最终采用率。

### 26.2 SIR/Intent 自适应性

- focused SIR materialization rate 和原因；
- evaluator-confirmed material ambiguity recall；
- unnecessary clarification rate；
- missed ambiguity 在 Candidate/Oracle/hidden evaluation 中晚发现的比例；
- late intent reopen 次数、成本和已作废 epoch；
- direct contract proposal 的 source-evidence coverage 与错误 promotion；
- administrator decision 的必要性、可操作性和耗时。

这里不能把“模型说无需 SIR”作为正确标签。是否遗漏必须由独立 specification、用户 decision、counterexample 或 evaluator
事后判定。

### 26.3 共设计与图演化

- 首个 Candidate 前/后的 required obligation discovery 比例；
- Candidate evidence 导致 decision-changing validation revision 的比例；
- validation evidence 导致 candidate revision 的比例；
- graph node/edge churn、duplicate、orphan 和 unsupported-claim rate；
- invalidated qualification epoch 数量及原因；
- Oracle comparator 因 Candidate 表现被无独立 evidence 放宽的次数，目标为零；
- Oracle revision 后 parent/current/variant 的 symmetric replay closure；
- Qualification Oracle query count、control exposure/retirement/replacement 和 holdout refresh；
- hard-coded case、harness detection、benchmark-only specialization 和 candidate accommodation finding；
- 从 claim → property → mechanism → receipt → verdict 的 trace accuracy 和 replay time。

### 26.4 自适应结构价值

- 每层 escalation rate、触发事实和最终 outcome；
- focused intervention、late challenge、full D fallback 各自新增的 unique confirmed findings；
- no-new-information Review 和 revision rate；
- policy supplement dependency、over-adoption、anchoring displacement 和 novel discovery；
- E 相对 D 的 obligation/property/case/mechanism 数量与 qualified coverage；
- 每个 evaluator-confirmed defect 的 model/Worker/human cost。

### 26.5 模型能力稳健性

- 各 capability/budget strata 的 coverage、false acceptance/rejection、completion 和 abstention curve；
- treatment × model capability 与 treatment × task class interaction；
- 同 task 多 repetition 的 semantic graph、candidate strategy 和 outcome stability；
- 弱模型是否更常诚实 fail closed，而不是增加未经证明的 accepted；
- 强模型节省的结构成本是否在 hidden evaluator 下保持相同可信度。

### 26.6 Authority、执行与安全

继续逐 run 报告：taxonomy/hidden leakage、cross-lineage receipt access、错误 capability claim、source observation promotion、
unapproved policy downgrade、candidate accommodation、retired control 被错误复用为 hidden、qualification query 超额、
execution failure 误分类、restart lineage 污染和敏感日志泄露。每项目标为零，不能被平均质量分抵消。

### 26.7 E-specific 分母与时间边界

正式实验必须在运行前冻结以下分析单位：`TaskRunId`、`IntentForkId`、`FocusedSirLineageId`、
`OrganicAssuranceConcernId`、`ExploratoryCandidateRevisionId`、`DecisionChangingObservationId`、
`CandidateRevisionId`、`DevelopmentOracleRevisionId`、`QualificationOracleRevisionId`、`OracleRevisionCauseId`、
`PromotionClaimId`、`PromotionDecisionId`、`ControlExposureStateId`、`EscalationDecisionId`、`QualificationEpochId` 和 D 协议
已有的 obligation/property/mechanism/receipt/evaluator identities。
它们不能退化成 generic event 或 `item_id`。

建议预注册以下定义：

```text
MaterialAmbiguityRecall(t) =
  evaluator-confirmed material intent forks correctly materialized before affected qualification
  ----------------------------------------------------------------------------------------------
  all evaluator-confirmed material intent forks

UnnecessarySirMaterializationRate(t) =
  focused SIR lineages adjudicated as resolvable from already-visible evidence
  ---------------------------------------------------------------------------
  all focused SIR lineages opened

LateIntentEscapeRate(t) =
  material intent forks first discovered after an affected exploratory candidate revision
  --------------------------------------------------------------------------------------
  all evaluator-confirmed material intent forks

CandidateRevealedObligationYield(t) =
  evaluator-required obligations first evidenced by an exploratory candidate observation
  ---------------------------------------------------------------------------------------
  all evaluator-required obligations

DecisionChangingCandidateEvidenceRate(t) =
  candidate observations with exact lineage to a changed intent/obligation/mechanism decision
  -------------------------------------------------------------------------------------------
  all successfully executed proposal-visible exploratory candidate observations

EpochInvalidationRate(t) =
  qualification epochs invalidated by a relevant post-freeze revision
  -------------------------------------------------------------------
  all qualification epochs created

EscalationFindingYield(level, t) =
  unique evaluator-confirmed defects first found at that escalation level
  -----------------------------------------------------------------------
  model + Worker + human cost consumed at that level

QualifiedPromotionValidity(t) =
  promoted candidate revisions that satisfy all frozen hard gates and claimed improvement
  ---------------------------------------------------------------------------------------
  all candidate revisions marked Qualified or preferred in a domain

SymmetricOracleReplayClosure(t) =
  Oracle revisions for which parent/current/all compared variants were replayed under one epoch
  ---------------------------------------------------------------------------------------------
  all Oracle revisions used for a comparative promotion decision

HiddenControlReplacementClosure(t) =
  retired-to-public controls replaced by independent hidden coverage before next qualification
  --------------------------------------------------------------------------------------------
  all controls retired to public after detailed feedback
```

`time-to-first-compiling`、`time-to-first-running` 和 `time-to-package` 从 normal CLI submission accepted 的 durable timestamp
开始，到 exact artifact/receipt/package terminal event结束。provider queue、Worker wait、失败尝试和人工等待同时报告，不得只
计 active model time。

“material intent fork”必须由 treatment-blinded evaluator、独立 specification、authorized user decision 或可重放
counterexample确认，并且不同解释会改变 candidate、domain、comparator、ABI 或用户可见 behavior。模型自报 uncertainty
不能自己进入 numerator；没有 ground truth 的 fork标记 `Indeterminate`，不自动算成功或失败。

E 的 blind metrics 只使用 policy challenge 前已冻结的 `OrganicAssuranceConcernId`。challenge 后新增 concern 不得回填
organic recall；只存在于 episode 私有思考、没有 durable pre-challenge identity 的内容也不能事后声称为自然发现。

Candidate/validation 相互作用必须按 first-cause lineage 计数。同一 observation 触发多个文字改写不能重复计为多个
decision-changing findings；同一 defect 被多个 Review 重复描述只计一个 unique defect。基础设施失败不进入 semantic
rate 的成功分母，而进入 execution completeness。

Promotion 的“改善”只按 frozen claim 计分。运行后挑选的最快 shape、最好 metric 或最有利 workload 只能标记 exploratory；
不能回填 numerator。performance/precision difference 同时报告 effect size、uncertainty、minimum practical improvement 和
non-regression outcome。Oracle revision 未完成 symmetric replay 时，promotion outcome 标记 `NotComparable`，不能把新
Candidate 计作胜出。

所有 assigned runs 遵循 intention-to-treat。provider failure、Worker failure、operator interruption、`Abstained` 和
incomplete run 保留其成本与停止位置；未创建 Candidate、未执行 control 或未到 Admission 不能补成零缺陷或 accepted。
正式 D/E manifest 还必须冻结 semantic matching、severity weighting、partial credit、censoring、randomization 和 aggregation，
不能只引用本节的候选公式后临时选择有利口径。

## 27. 主要风险与防护

### 27.1 模型没有意识到自己遗漏语义

防护：不依赖 self-report；late sealed policy challenge、independent evidence、source-defect traps、candidate feedback、hidden
controls 和 fail-closed Admission共同工作。仍不可识别的 specification gap 必须诚实保留。

### 27.2 E 退化成自由漫游的单 Agent

防护：Controller 管理预算、effect、artifact、revision、information exposure 和 Gate；Graph 只保存有 consumer 的 typed
状态；关键 Review 使用新 episode；模型没有 workflow writer authority。

### 27.3 SIR 被改名后实际仍然每次运行

防护：禁止通用 `ShouldStartSir`、readiness assessment 和 skip review。只有实际 reasoning 产生 material semantic fork 才
进入 focused SIR；普通 path 直接提交同一 intent proposal。

### 27.4 Candidate 与 Oracle 相互过拟合

防护：pre-task sealed policy、pre-challenge blind freeze、independent correct/invalid variants、hidden disjoint controls、
immutable qualification epoch、Oracle change cause 和 symmetric replay。任何 comparator/semantics revision 都使 epoch 失效
并要求 parent/current variants 在同一新 epoch 重新 qualification。

### 27.4.1 重复 qualification 泄露 hidden evaluator

防护：formal query budget、coarse typed feedback、`ControlExposureStateV1`、retire-and-replace、fresh/disjoint holdout 和
submission stopping rule。详细公开的 case 不再计 hidden coverage。

### 27.4.2 用局部 benchmark 或调宽精度标准制造晋升

防护：predeclared promotion claim、same-epoch comparison、minimum practical improvement、required non-regression 和 Pareto
candidate family。缩小 domain、改变 weighting 或放宽 comparator 必须形成新 policy/Intent/Oracle lineage，不能算 Candidate
优化。

### 27.5 太晚发现核心 intent 错误

防护：actor 一旦发现 material fork立即 yield；source/reference experiments可在 Candidate 前运行；高代价 device search 前可由
预算 policy 要求先 admission 当前 contract。最终指标记录 late reopen，而不是隐藏返工。

### 27.6 自适应规则偷偷变成机械语义判断

防护：Controller 只对 typed gap、receipt、Gate、budget、policy requirement 和 capability响应；semantic fork、coverage mapping、
defect attribution 由 runtime roles 提案并接受独立 evaluator检查。

### 27.7 结构不足或结构爆炸

防护：release ledger 和 full D fallback防止不足；global consolidation、property/case separation、mechanical mechanism compiler 和
no-new-information stopping防止爆炸。最终按 qualified coverage/cost而非 artifact 数评价。

### 27.8 Worker 存在被误当成目标证据

防护：capability 与 evidence class 强类型；host、CUDA、Ascend compile、simulator 和 950PR execution 不可替换；receipt 只按
exact scope消费。

### 27.9 早期 Candidate 被用户误认为可采用

防护：`Exploratory` 与 `Qualified` 使用不同 lifecycle/authority types；CLI/API/日志不展示模糊的 success；package 只引用
accepted qualification epoch。

## 28. 非目标

方案 E 不意味着：

- 假定一个足够强的模型会一次想全所有问题；
- 删除 SIR 的扩展 seam、Intent Admission、Oracle 或 Candidate Admission；
- 让 Controller 或 coding agent 机械解释未知 CUDA；
- 把 CUDA 输出当作默认 truth；
- 允许模型自行跳过 release policy challenge；
- 用 Candidate 当前输出训练或放宽同一轮 Oracle；
- 用多个模型投票替代编译、执行、reference、hidden controls 或 mechanical Gate；
- 要求任务包含 PyTorch；
- 依赖 CUTLASS 式模板覆盖率或把 Ascend C 降级为高层库调用；
- 当前立即建设通用 knowledge/skill 库；
- 恢复 Proposal Host、专属 binary 或测试旁路；
- 为实验 fixture 加已知答案、generic ID、compatibility path、authority fallback 或 V2；
- 在没有正式对照前宣布 E 优于 D。

## 29. 建议的实施与实验切片

E 需要真实 Candidate consumer，不能只在现有 Oracle-only path 上伪造“共设计”。若用户确认实施，建议：

1. 先冻结 E/D experiment manifest、任务语料、model capability strata、hidden evaluator 和 generous budgets；
2. 定义 Evidence/Assurance Graph 的最小强类型节点/边，只纳入真实 consumer；
3. 让同一正常 workflow 支持 `ExploratoryAscendCandidateV1` 的 build/run/diagnostic lineage；
4. 使用一个统一 `IntentContractProposalV1`，接通 direct path 与 focused SIR path，删除任何 mini-SIR gate；
5. 在首个 reasoning episode 前 seal catalog derivation/exposure，接通 late blind freeze 与 policy challenge；
6. 定义 Development/Qualification Oracle、Oracle revision cause、meta-qualification 和 symmetric replay；
7. 定义 `QualificationEpochV1`、revision invalidation、control exposure/query/replacement；
8. 接通 Candidate lifecycle、五层 Promotion Gates、Pareto family 和 exact typed feedback routing；
9. 先跑一个无 framework 的未知 CUDA task，再跑一个有独立 reference 或不同语义结构的 task；
10. 人工审查 graph、candidate、validation bundle、Oracle changes、promotion claims、receipts、controls、epoch、package 和 replay；
11. 只有 correctness、promotion validity、hidden exposure 和 capability gates 闭合后比较 D/E 成本与产品价值。

第一个 slice 不能仅输出 graph JSON 或到达 workflow state；必须至少让 runtime model 生成此前未知 task 的 exploratory
Ascend C candidate，并由 ordinary Worker 对 exact artifact 做真实 build 或执行。没有对应 950PR capability 时应停在明确的
`RequiredCapabilityUnavailable`，不能用 shell receipt 宣称成功。

## 30. 设计验收标准

方案 E 的实现必须同时满足：

- 没有通用 mini-SIR/readiness/skip-classifier 前置阶段；
- runtime model 在真实迁移推理中提交 material semantic fork，Controller 不解释源码；
- 无 focused SIR 的 path 仍在 qualification 前产生同一 typed intent proposal 并完成 Intent Admission；
- 输入 CUDA behavior 不被自动 promotion；
- early Candidate 始终是无发布 authority 的 `Exploratory`；
- initial actor 看不到 sealed policy taxonomy，pre-challenge organic graph 可审计；
- catalog derivation policy 在首 episode 前 seal，late concern ledger 完整闭合；
- required gap 能升级到完整 D，且 model confidence 不能关闭 gap；
- validation 与 Candidate revision 相互反馈但不能循环放宽标准；
- Development Oracle 与 Qualification Oracle 不混合，后者不是无限次训练接口；
- Oracle change 必须有 typed cause、独立 meta-qualification 和 parent/current symmetric replay；
- Candidate revision lifecycle 不以 latest/best 覆盖历史；
- integrity、required non-regression、predeclared improvement、same-epoch comparison 和 independent qualification 全部闭合；
- performance/precision promotion 绑定 minimum practical improvement、冻结统计 policy、domain 和 Pareto semantics；
- hidden control 具有 query budget、exposure state 和 retire-and-replace closure；
- qualification epoch 在任何相关 revision 后失效；
- exact 950PR capability 和 performance receipts不可由其他 Worker 替代；
- hidden evaluator/material 对所有 proposal roles不可见；
- Admission model-free，failure types 和 feedback routes不混合；
- strong identities、restart、receipt authorization 和安全日志边界保持；
- 正常 CLI/server/app/workflow/Worker path 完成；
- 至少两个语义明显不同的未知任务无需产品代码或 prompt特殊分支；
- 最终交付是可运行、可重放、可审查的 migration package，而不只是 SIR、Oracle 或 Candidate 文本。

## 31. 尚待实验回答的问题

- organic reasoning 在 policy challenge 前应允许看到哪些通用 debugging/validation skill，才能既有用又不泄露 taxonomy？
- intent contract 最迟应在第一次高成本 950PR search 前还是 qualification 前 admission？
- 哪些 typed gap/severity 足以机械要求 full D fallback，而不让 Controller做语义判断？
- early Candidate 对发现 Oracle gap 的边际收益是否大于 candidate/Oracle co-adaptation 风险？
- Qualification Epoch 应按 whole candidate family 还是按 variant/workload partition冻结？
- formal Qualification Oracle query budget、feedback granularity 和 holdout refresh rate应如何按 task risk设定？
- performance/precision 的 minimum practical improvement由用户、产品 policy 还是 workload evidence共同确定？
- Oracle meta-qualification 的 correct/mutant family 如何避免被当前 Candidate lineage污染？
- 同一 model configuration 的 fresh Review 是否足够，何时值得使用不同模型或人工 Reviewer？
- focused SIR 的 materialization precision/recall如何在没有独立 specification的真实任务上评价？
- late policy challenge 相比 D1 的 early challenge是否降低 anchoring，同时避免昂贵返工？
- Graph 的最小持久化边界是什么，哪些 reasoning state 可以安全留在 episode 内？
- E 在弱模型下是否主要增加诚实 abstention，还是能够通过结构升级恢复足够 coverage？

这些问题必须通过 frozen treatment、真实 runtime model、ordinary Workers、exact 950PR capability 和 common hidden evaluator
回答，不能由设计直觉静默关闭。

## 32. 最终方案

方案 E 的完整语义是：

> runtime model 直接面对此前未知的任务，在没有内部 Oracle taxonomy 锚定的情况下共同推导源语义、验证要求和
> Ascend C candidate；其推理能力不被假定可靠，所有输出都只是 proposal。只有当实际推理暴露会改变迁移结果的语义
> 分叉时，才把 SIR 物化为 focused clarification protocol，而不是先运行一个等价的 mini-SIR。Candidate 可以在最终
> Oracle 前以无发布 authority 的 exploratory 身份尽早 build/run，用真实 target feedback推动 assurance graph和实现共同
> 演化。发布前，系统冻结 organic graph，使用预先 sealed 的完整 policy challenge关闭模型可能不知道的遗漏，必要时
> 升级到方案 D 的完整结构化 Review；随后把 admitted Intent、Qualification Oracle、950PR target、Candidate revision 和
> promotion policy 冻结为 qualification epoch。新 revision 只有通过 integrity、required non-regression、predeclared
> improvement、same-epoch comparison 和 independent qualification 才能晋升；Oracle 改版时 parent/current 对称重测，已泄露
> hidden control 退休并替换。最终通过 ordinary Workers、model-free Oracle Admission 与 Candidate Admission，只交付 exact
> evidence支持的 950PR migration package。

固定的是可信边界，不是模型每一步怎么想；自适应的是认知结构，不是发布标准。
