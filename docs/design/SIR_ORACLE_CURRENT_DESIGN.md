# Cairn SIR 与 Oracle 联合权威设计

- 状态：当前规范性设计
- 日期：2026-09-01
- 产品范围：此前未知 CUDA 软件 → 硬件亲和 Ascend C
- 首个硬件目标：Ascend 950PR（3510）
- 覆盖范围：Task Intake、intent reasoning、focused SIR、Intent Admission、Assurance/Oracle 演进、Oracle
  qualification，以及与 exploratory Candidate、Candidate promotion 的权威接口
- 不覆盖：Ascend C 代码生成算法、搜索器内部实现和 Candidate Admission 的全部产品交付细节

## 1. 文档地位

本文档是 Cairn 当前 SIR 与 Oracle 子系统及其 Candidate 交互边界的单一权威设计。它直接更新 current V1 语义，取代本文件
旧版中固定 `Task Intake → SIR → complete Oracle → first Candidate` 的时间顺序。

[`EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md`](EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md) 保存更广的方案 E 候选和
消融设计；本文只把已经明确的 SIR/Oracle authority、evidence、qualification 和 promotion interaction 收敛为当前规范。
[`CAIRN_CURRENT_PRODUCT_DESIGN.md`](CAIRN_CURRENT_PRODUCT_DESIGN.md) 继续定义产品使命和四个产品平面；其中旧流程图应读作
release authority dependency，不再表示 complete Oracle 必须早于任何 exploratory Candidate。

旧的 SIR、Oracle、performance、knowledge/skill、workflow 文档保留为历史材料，不能给本文增加隐含行为。若本文与
`AGENTS.md` 冲突，以 `AGENTS.md` 为准；若代码不同，差异是待实现缺口，不得用 compatibility reader、legacy alias、V2、
fixture branch、generic ID 或 authority fallback 掩盖。

## 2. 核心结论

代码迁移不可消除的是三个问题，而不是三个固定阶段：

1. **Specification gap**：源码、测试、caller 和 desired semantics 可能不完整或矛盾；
2. **Reality gap**：CUDA、compiler、CANN、Ascend C、ABI 和 950PR 事实必须真实执行；
3. **Trust gap**：模型自信、schema 完整和角色共识不能替代独立 evidence 与 mechanical Admission。

任何 runtime model 为了迁移都必然理解源程序并考虑如何验证结果；这些是不可避免的认知功能。可选的是是否把某个
语义分叉物化成独立 focused SIR protocol，以及是否在第一个 Candidate 前完整展开全部 Oracle structure。

无论认知时序如何，release 前必须具备：

- exact admitted intent contract 和 admitted evidence snapshot；
- exact 950PR target context；
- complete、candidate-facing、qualified Validation Bundle；
- policy concern ledger 的完整 typed disposition；
- trusted Worker receipts 和 required capability closure；
- immutable qualification epoch；
- model-free Oracle Admission 和 Candidate Admission。

系统不以“足够强的 runtime model”为前提。模型始终是 proposal actor；它可能漏掉自己不知道的风险。发布完整性由
sealed policy challenge、independent Review、Worker evidence、hidden controls、qualification epoch 和 fail-closed Gate 保证，
而不是模型自我判断。

## 3. Source、Intent、Oracle 与 Candidate 必须分开

至少区分：

1. `SourceBehaviorObservation`：CUDA 程序在 exact environment/input 下实际做了什么；
2. `IntentContractProposal`：runtime model 根据 source/caller/reference/evidence 提议迁移什么；
3. `AdmittedIntentContract`：系统获准让下游依赖什么；
4. `DevelopmentOracleRevision`：proposal-visible、用于搜索和诊断的 judge；
5. `QualificationOracleRevision`：冻结 policy/controls 后拥有 release qualification authority 的 judge；
6. `ExploratoryCandidateRevision`：用于发现 target 和 validation facts、无发布 authority 的实现；
7. `QualifiedCandidateRevision`：在 exact qualification epoch 下通过全部门禁的实现。

CUDA bug、race、越界、未初始化读、错误边界结果、偶然 launch 行为或不必要数值误差不会因为被观察到就自动进入迁移
目标。Source execution receipt 只有 observation authority；只有 Intent Admission 能发布 downstream exact claim。

## 4. 当前端到端结构

```mermaid
flowchart TD
    intake["Frozen task + caller + 950PR target"]
    loop["Controller-owned Candidate Search Loop"]
    actor["Exploration Actor Episode"]
    graph[["Evidence / Assurance Graph"]]
    fork{"Material semantic fork found\nduring actual migration reasoning?"}
    sir["Focused SIR protocol\nsource/reference experiments + user decision"]
    intent{"Intent Admission"}
    exploratory["Exploratory Ascend C candidate revisions"]
    workers["Managed Workers\nCUDA / CPU / Ascend build / 950PR / profile"]
    dev["Development Evaluation"]
    freeze["Freeze organic assurance graph"]
    challenge["Sealed-policy coverage challenge\n+ adaptive D fallback"]
    exit{"Search exit prerequisites closed?"}
    epoch[["Qualification Epoch"]]
    oracle{"Mechanical Oracle Admission"}
    promotion{"Candidate Promotion / Admission"}
    package[["Migration Package"]]

    intake --> loop
    loop --> actor
    actor -->|typed proposals / requests| loop
    loop <--> graph
    actor --> fork
    fork -->|yes| sir --> graph
    fork -->|no separate SIR| loop
    loop --> exploratory --> loop
    loop -->|authorized effect| workers
    workers -->|receipt| graph
    loop --> dev --> graph
    graph --> intent
    intent --> freeze --> challenge --> exit
    exit -->|no: next episode / typed terminal| loop
    exit -->|yes: freeze and leave search| epoch
    epoch --> oracle --> promotion --> package
    oracle -->|authorized feedback; new generation| loop
    promotion -->|authorized feedback; new generation| loop
```

Intent Admission 可以在 co-design 中较早完成；但最迟必须在 policy ledger 实例化和 qualification 前完成。没有 admitted
intent 的 Candidate 只能保持 `Exploratory`。

`cairn-server` 保持业务无关，`cairn-migration-app` 负责 composition，`CudaMigrationWorkflow` 是唯一公共 workflow writer。
Intent、Oracle、Candidate 和 Admission 是 workflow ports 背后的函数，不是独立 proposal 进程。各 reasoning/review role
使用真实、可恢复的 Agent Loop；graph node、revision、experiment、control 和 feedback 的外层遍历是机械编排，不是嵌套
Agent Loop。不存在 Proposal Host、专属 proposal binary 或测试旁路。

### 4.1 Exploration Actor 是参与者，不是循环拥有者

`ExplorationActorV1` 是 runtime model 在一次 `ExplorationEpisodeV1` 中承担的 proposal role。它可以理解 source、形成 competing
hypotheses、提出 intent/assurance revision、请求实验、生成 Candidate、诊断公开开发反馈，并建议继续搜索或尝试
qualification。

Exploration Actor 是默认通用 co-design role，但不吞并 focused SIR、Oracle Developer/Reviewer 或 Coverage Review。它可以提出
进入 focused intervention；只有 Controller 能暂停当前 episode并启动相应 peer role episode。focused role消费受限 state
projection，返回 typed artifact/finding，不是 Actor内部调用的子 Agent，也不拥有嵌套 Search Loop。

Actor 不能直接写 workflow state、覆盖 Candidate revision、调度 Worker、扩大 capability、读取 hidden/sibling receipt、冻结
Qualification Epoch 或产生 promotion verdict。Agent Loop 内的 tool call 只是一个受约束的 effect request；Controller 校验后才
能提交普通 Worker。

### 4.2 Candidate Search Loop 是 Controller-owned orchestration

`CandidateSearchLoopV1` 位于 `CudaMigrationWorkflow` 内，是 durable mechanical state machine，不是另一名 Agent、独立服务或
模型内部 while-loop。调用方向固定为：

```text
CandidateSearchLoop
    → 创建 ExplorationEpisode，并授予 exact state projection/capabilities
    → Exploration Actor 返回 typed action proposals
    → Controller 校验并持久化 revision，或调度获准 effect
    → Worker/Development Evaluation 产生 evidence
    → Controller 构造下一 immutable search state
    → 创建 fresh/resumed ExplorationEpisode
```

loop 至少负责 episode lifecycle、budget、effect scheduling、Candidate lineage、assurance revisions、Development feedback、
stopping counters 和机械 transition。它不解释源码、判断哪个语义 hypothesis 正确，或替模型设计 Ascend C。

Actor action 至少强类型区分：evidence experiment request、intent clarification、intent contract proposal、assurance revision、
Candidate revision、development diagnosis、qualification-attempt recommendation 和 gap-bound abstention。模型 confidence、预算
耗尽或一句“完成”都不具有 transition authority。

### 4.3 每轮状态、generation 与 qualification 边界

三个时间单位必须分开：`ExplorationEpisodeV1` 是一次可暂停/恢复或替换的 runtime-model continuation；search iteration 是
Controller 从一个 immutable state 推进到下一个 state的 durable transition，可以消费多个 episode/effect；search generation
是两次 qualification边界之间的完整开发搜索。Qualification attempt 本身不是 iteration，也不能重新打开已经冻结的
generation。

每轮 actor 输入是授权的 immutable projection，而不是依赖完整聊天记忆：

```text
SearchState_k
    = exact intake/intent/target revisions
    + public assurance graph revision
    + candidate family and selected parent
    + proposal-visible Worker evidence
    + Development feedback
    + budget/query/stopping counters
    + model/tool/skill snapshot identities
```

只有有 consumer、authority、effect、replay 或 recovery 价值的 typed output被持久化。多个 actor 可以提出竞争 variants，但只能
由 Controller 以不同 revision/parent 合入 Candidate Family，不能并发覆盖 generic `latest`。

Qualification 明确位于 Candidate Search Loop 外。进入 qualification 前，Controller 结束当前 search generation，冻结 exact
Candidate、Qualification Oracle、public evidence、promotion claim、target 和 policy。Qualification failure 不能作为普通 tool
result 原样回灌旧 episode：若 policy允许继续开发，必须建立新 generation，只投影授权的粗粒度反馈；若公开具体
counterexample，该 control 立即退休为 public regression并由新 holdout替换。无可授权反馈或 query budget用尽时，正常终止为
rejected、partial 或 abstained。

Actor建议尝试 qualification后，Controller只机械检查：Intent已 admitted、Candidate已 `DevelopmentEligible`、public required
checks和 known findings有闭合状态、organic freeze与 policy challenge完成、Oracle/target/capability/promotion revisions齐全、
不存在 pending effect/decision，以及 hidden exposure/query/feedback policy已 seal。它不据此判断语义正确；允许创建 epoch只
表示制品结构足以接受独立资格化，最终仍可被 Oracle或 Candidate Admission拒绝。

## 5. Authority 与数据边界

| 参与者 | 可以做什么 | 不能做什么 |
| --- | --- | --- |
| Exploration Actor | 阅读授权 projection；提出 intent、Candidate、assurance、experiment、diagnosis 和 stopping recommendation | 拥有 Search Loop；写 workflow state；直接调度 Worker；准入自己；读取 hidden material |
| Candidate Search Loop | 创建 actor episode；校验 action；调度获准 effect；持久化 lineage；构造下一 state；执行机械 stopping/transition | 解释源码或 intent；替模型生成 Candidate；把 qualification 当公开开发工具 |
| Focused SIR role | 针对 exact semantic fork 收集 evidence、提出 hypotheses 和用户问题 | 作为每个 task 的 mandatory classifier；替用户决定语义 |
| Controller | 冻结输入与 revision；授予 capability；调度 Worker；保存 lineage；执行机械闭合与 transition | 通过代码特征解释语义；制造 receipt；改写 model artifact |
| Worker | 在 exact job contract 下编译、执行、profile 并返回 receipt | 解释 intent；扩大 scope；产生 Admission verdict |
| Administrator | 回答 exact semantic/policy decision request | 获得 hidden evaluator 内容；替 Worker 伪造事实 |
| Intent Admission | 检查 proposal、evidence 和 authorized decision，发布 exact claims | 调用模型补答案；把 CUDA behavior 自动提升为 intent |
| Oracle roles | 提出 concern、property、case、mechanism、revision 和 experiment | 修改 admitted intent；准入自己的 Oracle；读取 hidden controls |
| Coverage/Review roles | 对 frozen graph、policy ledger 和 exact artifact 提出 typed finding | 获得 workflow writer 或 Admission authority |
| Oracle Admission | 从 frozen artifacts、policy 和 trusted receipts 重算结果 | 生成 Oracle 内容；按当前 Candidate 表现放宽 comparator |
| Candidate Admission | 在 exact epoch 下重算 Candidate outcome/promotion | 用最新 revision 覆盖历史；跨 epoch 继承 verdict |
| Skill/knowledge | 提供有 provenance 的方法、事实候选和工具建议 | 产生 authority；授予额外 capability；替代 receipt |

模型配置不同、使用 fresh episode 或多个角色同意都不能提升 evidence strength。Reviewer 的价值来自新的 evidence、policy、
counterexample、execution 或独立信息，而不是角色名称。

## 6. Task Intake 与 sealed commitment

正常客户入口固定为：

```text
cairn-cli → cairn-server → migration app API → CudaMigrationWorkflow → managed Workers
```

Task Intake 至少冻结：

- CUDA/source、host launch、build files、tests 和获准调用链；
- caller declaration、migration scope 和显式 unknown；
- `TargetPlatformContextV1`；
- source/reference/framework/research 的读取 authority；
- runtime model、adapter、tool、Worker capability、budget 和 data policy；
- skill/knowledge snapshot；
- exact prior feedback 或 `NoPriorFeedback`；
- completion goal、promotion 和 operator stopping policy。

### 6.1 Target 不是一段 prompt

`TargetPlatformContextV1` 至少表达：

- Ascend 950PR（3510）或显式 unknown；
- Ascend C/CANN toolchain 和版本约束；
- build、runtime、ABI 和 framework environment；
- dtype、layout、alignment、memory hierarchy 和 execution capability；
- allowed Worker capabilities；
- performance 的设备、时钟、warmup、重复、稳定性和 workload policy。

其他 SoC、host shell、CUDA、simulator 或 Ascend compile receipt 不能替代 required 950PR execution/performance evidence。

### 6.2 Blind-first sealed commitment

首个 reasoning episode 打开前，Controller 必须 seal 但不暴露：

- task-generic Oracle policy catalog identity；
- 从 future admitted claims、target 和 operator policy 派生 task ledger 的 frozen derivation policy；
- challenge-stage skill/tool/knowledge exposure manifest；
- hidden evaluator、control family、query budget 和 Admission policy identities。

由于 admitted claims 尚未产生，task-specific concern ledger 可以在 Intent Admission 后实例化，但只能由 pre-sealed derivation
policy 机械生成，并在 challenge 前冻结。operator、coding agent 或 runtime model 不能看到 organic result 后临时挑选政策。

初始 actor 看不到 Cairn 内部 concern taxonomy 或通过 skill/tool/example 间接复制的等价清单，但看到真实代码、用户明确
要求、target 和获准 evidence。“blind”不隐藏任务事实。

## 7. Source understanding 与 focused SIR

### 7.1 源语义理解每次都会发生

runtime model 直接开始实际迁移 reasoning，必然形成 source semantics、implementation freedom 和验证假设。这一认知功能
不是可选项，也不需要为它创建独立 workflow stage。

本文中的 **focused SIR protocol** 只指：真实推理发现会改变 Candidate、domain、comparator、ABI 或用户可见行为的多个
plausible interpretations，且当前 evidence 无法消除时，建立独立、持久、可实验和可请求用户 authority 的澄清 lineage。

### 7.2 禁止 mini-SIR

每个 task 前不得统一运行：

- `ShouldStartSir` classifier；
- `IntentReadinessAssessment`；
- 专门判断是否可跳过 SIR 的 Reviewer；
- Controller 基于 atomics、dtype、API、代码长度或固定风险特征得出的 SIR 结论。

这些步骤本身已经在做 SIR 分析，只是改名后的 mandatory detour。

### 7.3 Material semantic fork

Exploration Actor 在实际理解、生成或验证过程中提交 `IntentClarificationRequiredV1`，至少包含：

- exact competing interpretations；
- source/caller/reference evidence 与反证；
- 每种解释对 Candidate/validation 的具体差异；
- 可区分它们的 experiment 与 competing predictions；
- evidence 仍不足时需要 administrator 回答的 exact question。

Controller 只验证 identity、binding、capability 和 authority，然后路由 focused SIR；它不判断哪种解释正确。用户显式冲突可
直接进入同一 protocol。Worker receipt 或 Reviewer finding 先形成 observation/finding，再由授权 reasoning role 判断是否构成
material semantic fork。

### 7.4 无 focused SIR 的 direct path

如果实际推理没有形成需要外部澄清的分叉，不运行独立 SIR episode。Reasoning actor 最迟在 qualification 前仍提交唯一
`IntentContractProposalV1`：

- candidate 必须保持的 semantics；
- implementation freedoms；
- source behavior keep/reject/unknown disposition；
- supported domain；
- exact evidence、conflict、unknown 和 limitation。

direct path 与 focused SIR 都产生同一个下游 proposal type；不得创建 fast/legacy 两种 contract、fallback reader 或 V2。

### 7.5 SIR Worker experiments

focused SIR 和普通 migration reasoning 都可请求会改变 intent decision 的 Worker evidence，尤其是 CUDA Worker：

- boundary、empty、odd/extreme shapes；
- NaN、Inf、signed zero、subnormal、overflow；
- launch geometry、stream、sync 和 repeatability；
- sanitizer、race、OOB 和 uninitialized access；
- alias、overlap、in-place、partial write 和 error behavior；
- CPU、PyTorch 或其他 independent reference differential；
- 能区分 competing hypotheses 的最小输入。

每个 request 冻结 exact hypotheses、predictions、capability、cost/risk 和 insufficient outcome。receipt 只能成为
`SourceBehaviorObservationV1`、`ReferenceObservationV1` 或其他精确 evidence class；它不拥有 intent authority。

Worker unavailable、toolchain failure、budget exhausted 和 missing receipt 是 `ExecutionFailure`/`IncompleteEvidence`，不能被
误记为 semantic hypothesis 被反驳。实验期使用 generous、显式、bounded 的预算，不逐次小幅试探正常路径。

## 8. Intent Admission 与 evidence snapshot

Intent Admission 独立检查：

- proposal 是否绑定 exact task、source revision、target 和 prior feedback；
- 每条 claim 是否有允许 provenance；
- source behavior 是否被误当 desired semantics；
- administrator decision 是否来自 exact request 和 authorized actor；
- conflict、unknown、source defect 和 implementation freedom 是否诚实保留；
- evidence independence/common dependency 是否被夸大。

输出是 claim-scoped `AdmittedIntentContractV1` 与 `AdmittedIntentEvidenceSnapshotV1`。Oracle、Candidate 和 Reviewer 不读取
未经筛选的全部 transcript，也不能只接收模型 summary。

每项 admitted evidence 至少保存 content/source/environment identity、claim edge、domain、provenance class、allowed usage、
shared dependency/contamination 和 visibility。至少区分 caller declaration、observed program fact、source behavior、independent
reference、framework contract、target platform fact、external research、knowledge entry 和 prior feedback。

Candidate feedback 若暴露 intent ambiguity，创建新的 focused SIR/Intent lineage；不得由 Oracle 或 Candidate revision 原地
修改 admitted claim。

## 9. Evidence / Assurance Graph

Graph 是跨 reasoning episode、Worker、Oracle 和 Candidate revision 的共享状态，不是模型全部 chain-of-thought 的永久化。
只有具有下游 consumer、authority、effect、replay 或审计价值的内容进入图。

至少使用独立强类型表达：

- source/caller/framework facts；
- proposed/admitted/rejected intent claims；
- hypothesis、unknown、administrator decision；
- source/reference/platform observation；
- organic assurance concern；
- policy concern、mapping 和 disposition；
- validation obligation、property、case、mechanism；
- candidate family、variant 和 revision；
- experiment request、job、attempt、receipt；
- review finding、feedback 和 revision；
- qualification epoch、control exposure 和 Admission outcome。

边至少区分 support/refute/undetermined、derived-from、observed-under、applies-to-domain、depends/shared/contaminated、
motivates-experiment、consumed-by-decision、covers-claim、tests-candidate、qualifies-mechanism、supersedes、invalidates 和
routes-feedback。

raw wire/storage representation 必须立即转换为 validated type；deserialization 重跑 constructor invariant。容易混淆的
identity/capability/outcome 应有 compile-fail 或等价静态测试。共享表示不是合并成 generic abstraction 的理由。

## 10. Exploratory Candidate 作为 evidence consumer

complete Oracle accepted 前可以生成 `ExploratoryAscendCandidateV1`，用于暴露只有真实 consumer 才能发现的问题：

- Ascend C build、host integration 和 ABI；
- shape/layout/tiling/alignment/workspace；
- compiler/resource/capability 限制；
- Validation mechanism 是否真实绑定 Candidate；
- correctness、numerical 和 failure signature；
- 950PR pipeline、memory、core mapping 和 performance direction。

Exploratory Candidate 可以请求 ordinary Ascend build/NPU/profile Worker，但：

- 没有 final verdict 或 package authority；
- 绑定 current provisional/admitted intent、target、evidence 和 revision；
- 不能修改 intent、放宽 comparator 或删除 failed case；
- 基于未 admitted hypothesis 时显式标记 provisional domain；
- 不得在 CLI/API 中显示为模糊 success。

Oracle/assurance 可以因 Candidate evidence 修订，Candidate 也可以因 validation evidence 修订；所有反馈必须 typed，并且任何
影响 qualification 的 revision 都使旧 epoch 失效。

Exploratory Candidate 由 Exploration Actor 通过 `SubmitCandidateRevision` 提出，但只有 Candidate Search Loop 能校验 parent、
domain、artifact、improvement claim 和 authority binding，创建 immutable revision并调度 Development Evaluation。Actor 返回源码
正文不等于 workflow 已接受一个 Candidate revision。

## 11. Oracle 是持续演化的 Assurance 子图

Oracle 的最终产品形态仍是 executable Validation Bundle，但不要求在第一个 exploratory Candidate 前完整产生。Assurance
Graph 可以逐步形成：

- typed input/domain generators；
- public regressions；
- hidden/adaptive cases；
- optional CUDA/CPU/framework references；
- relational/metamorphic properties；
- numerical comparator 和 allowance derivation；
- ABI/framework/integration checks；
- memory/concurrency/safety checks；
- performance workloads 和 measurement policy；
- provenance、dependency 和 replay manifest。

每个 concern/property/mechanism 必须回答如何判断 future Ascend C Candidate。只审查 CUDA source quality、不能产生 expected
value、property、comparator、execution/safety/performance obligation 的内容不计 Oracle coverage。

没有 PyTorch 或 independent reference 时，可以组合 source observations、caller decision、metamorphic relations、multiple
implementations、high-precision partial references、formal tools 和用户 authority；缺口保持 unknown/partial，不让 CUDA
自动成为 truth。

## 12. Blind organic discovery、policy challenge 与自适应 D fallback

### 12.1 Organic assurance concern

初始 Exploration Actor 不读取内部 taxonomy。在实际迁移中，一个风险只有成为跨 episode consumer 时才物化为
`OrganicAssuranceConcernV1`，并绑定：

- exact trigger evidence；
- future Candidate acceptance risk；
- provisional domain；
- 与已有 concern 的 overlap/dependency/conflict；
- 下一项 evidence 或 decision。

这不是要求模型预先列一张维度表。没有 authority/effect/replay consumer 的临时思考留在 episode 内。

### 12.2 Pre-challenge freeze

首次 policy 暴露前，Controller 冻结 organic assurance subgraph、visible evidence、model/tool/skill exposure、receipts 和
continuation identity。challenge 后不能回填或改写自然发现。

### 12.3 Complete policy challenge

Intent admitted 后，由 pre-sealed derivation policy 实例化 `TaskPolicyConcernLedgerV1`。fresh Coverage Auditor 读取 frozen
organic graph 和完整 ledger，为每个 concern instance 提出 mapping、gap、overlap、case inflation 和 capability challenge。

最终每个 organic/policy concern 使用 typed disposition：

- `AdoptAsIndependent`；
- `MergeIntoObligation`；
- `SplitAcrossObligations`；
- `RepresentAsCase`；
- `ApplicableInformational`；
- `NotApplicable`；
- `RejectBlindProposalAsUnsupported`；
- `UnknownRequiresEvidence`。

Catalog 是 coverage floor，不是允许列表。novel concern 不因没有 catalog 名称删除；required unknown 阻塞；Worker 不可用不是
`NotApplicable`；模型不能降低 sealed requirement。

### 12.4 Adaptive structure

若 organic graph 已 candidate-facing、低重复且 mechanism-ready，challenge 后只需 global consolidation/scope review。若存在
high-severity gap、overmerge、case inflation、invalid mechanism 或 repeated Review failure，则进入方案 D 的完整：

```text
Consolidated Obligation → Property → Case → Mechanism
                     → per-property Review/revision
                     → Portfolio Coherence Review
```

具体 input、shape、dtype 或 mutation 通常是 case，不自动提升成独立 property。Full D 是结构化 fallback，不由模型 confidence
跳过；Controller 只响应 typed gap/severity/Gate，不自行判断 semantic coverage。

## 13. Oracle 验证平面

用户可以概括为正确性、精度和性能；内部至少保留：

1. semantic/algorithmic correctness；
2. numerical behavior 与 assurance；
3. execution/integration authenticity；
4. memory/state/concurrency safety；
5. Oracle adequacy；
6. resource/performance。

每个 concern 为 `Required`、`ApplicableInformational`、`NotApplicable` 或 `UnknownApplicability`。性能平面始终有 disposition；
没有业务目标时可以 informational/unknown/not-executed，不能静默删除。性能不能补偿 required correctness、numerical、
integration 或 safety failure。

## 14. Property、Case、Mechanism 与 Review

- `OraclePropertyV1`：独立、candidate-facing、可反驳的 acceptance property；
- `OracleCaseV1`：boundary input、example、mutation、metamorphic transformation 或 workload slice；
- `OracleMechanismV1`：Candidate/ABI/input/execution/observation/comparator/receipt binding。

多个 cases 不自动产生多个 Agent Loop。只有 acceptance semantics、capability、failure interpretation 或 mechanism 无法共享时，
才提升为不同 property。

mechanism 在语义 Review 前尽可能通过 typed compilation/mechanical checks：artifact/ABI binding、allocation、shape/layout/
pitch/alignment、launch、observation、comparator、capability、target、output bound 和 receipt lineage。机械检查不替代“是否真的
证明 property”的 Review。

每个需要独立审查的 property 使用 Developer → Reviewer → exact feedback → revision Agent Loops。所有 required properties
闭合后执行 Portfolio Coherence Review，检查跨 property gap、冲突、重复、shared dependency、capability mismatch 和
performance workload。Review 同意不能跳过 Worker qualification 或 Admission。

## 15. Worker experiments 与 evidence authority

SIR、assurance 和 Candidate reasoning 可请求：

- host/reference arithmetic；
- CPU/Python/PyTorch reference；
- CUDA compile/execution/sanitizer；
- Ascend C compile；
- simulator/emulator；
- Ascend 950PR execution；
- profiler/performance measurement；
- honest/correct-variant/mutant/hidden controls。

每个 request 冻结 exact uncertainty/property/finding、competing predictions、decision impact、required capability、cost/risk 和
requesting lineage。每个 receipt 绑定 task、intent、graph node、revision、mechanism、job、Worker capability、environment、
input、attempt 和 output content identity。POSIX shell、CPU、CUDA、Ascend compile 和 950PR evidence 不可互换。

proposal-visible observation 与 Admission-only control 分开。receipt 只投影到 exact authorized lineage；sibling/task/revision
receipt 不可读取。experiment failure 不改变 semantic hypothesis 或 applicability。

## 16. Development Oracle 与 Qualification Oracle

### 16.1 Development Oracle

`DevelopmentOracleRevisionV1` 对 proposal actor 可见，支持 public regression、mechanism debugging、profiling 和 Candidate
revision。它是搜索信号，可以被 Candidate 学习，因此不能独立证明 release trust。

### 16.2 Qualification Oracle

`QualificationOracleRevisionV1` 在一次正式 qualification 前冻结：

- admitted intent/evidence；
- complete policy ledger/disposition；
- public Validation Bundle；
- correct variants/honest controls；
- targeted mutants/negative/hidden disjoint controls；
- exact target/capability；
- comparator、performance、query 和 exposure policy。

它不是 runtime model 可无限查询的训练 API。

### 16.3 Qualification Epoch

`QualificationEpochV1` 是以下组合的不可变 binding：

```text
AdmittedIntentRevision
× QualificationOracleRevision
× TargetPlatformContext
× CandidateRevision / CandidateFamilyRevision
× PromotionPolicy
```

任一相关 revision 变化都会 `InvalidateQualificationEpoch`。不存在跨 epoch 自动继承的“Candidate passed”或“Oracle
accepted”。旧 receipt 只有在 exact artifact/environment/policy 仍满足新 epoch 的明确 reuse rule 时才能被重算接受，不能静默
复制。

## 17. Oracle revision change control

Oracle 因 Candidate 或新 evidence 变化时，必须分类：

- `OracleArtifactCorrection`：judge 实现缺陷；
- `CoverageExpansion`：同一 Intent/domain 内遗漏 property/case；
- `EvidenceStrengthening`：加入更独立或更高 capability evidence；
- `IntentContractChange`：desired semantics/domain/freedom 改变，进入新 Intent lineage；
- `TargetPolicyChange`：target/workload/resource/performance policy 改变；
- `CandidateAccommodationAttempt`：只为当前 Candidate 通过而放宽 judge，无独立依据，必须拒绝。

前三类必须先通过独立 Oracle meta-qualification，并创建新 revision/epoch。Oracle 改变后，parent、current 和所有被比较
Candidate variants 必须在同一新 Oracle/target/policy 下对称重测；不能只重测最新 Candidate 后宣称改进。

Candidate failure 必须先分为 candidate defect、Oracle defect、platform gap、intent ambiguity、execution failure 或 protocol
violation。反馈只能进入 exact lineage；不能把失败当作调宽 tolerance 的理由。

## 18. Candidate promotion 的联合边界

本文不定义 Candidate generator，但 Oracle 必须支持正式 promotion。Candidate lifecycle 至少区分：

- `Exploratory`；
- `DevelopmentEligible`；
- `QualificationPending`；
- `Qualified`；
- `Rejected`；
- `Superseded`。

latest revision 不自动替代 parent。新 Candidate 只能在同一 epoch 依次通过：

1. **Revision Integrity Gate**：artifact、parent、diff、toolchain、target、dispatch、hidden isolation 和 execution authenticity；
2. **Required Non-Regression Gate**：全部 required semantic/numerical/integration/safety/domain/public regression replay；
3. **Claimed Improvement Gate**：在 qualification 前冻结修复、domain、precision、performance、resource 或 specialization claim；
4. **Comparative Promotion Gate**：parent/current 在同 Intent/Oracle/target/workload/statistical policy 下比较；
5. **Independent Qualification Gate**：proposal-invisible correct/invalid/hidden/950PR controls。

correctness、安全、integration 和 required numerical allowance 是硬约束。通过后才能比较 performance、precision、workspace、
domain 和稳定性。只在部分 workload 更优的 revision 成为 domain-bound Pareto variant，不全局 supersede；dispatch/fallback
必须分别验证。

performance/precision improvement 必须预先冻结 metric、domain、baseline、minimum practical improvement、noise/statistical
policy 和 non-regression。缩小 domain、挑最快 shape、修改 workload weighting 或放宽 tolerance 不构成 Candidate 优化。

## 19. Hidden controls、作弊与自适应查询

Candidate 可以反复学习 Development Oracle，不能无限学习仍被称为独立证明的 Qualification Oracle。每个 restricted control
具有不同 `ControlExposureStateV1`：

- `Hidden`；
- `FeedbackAbstracted`；
- `RetiredToPublic`。

一旦为修复公开具体 hidden input、expected result 或可逆诊断，该 control 退休为 public regression，并在下一次 qualification
前补充 independent hidden coverage。详细泄露后的 case 不能继续计 hidden。

formal qualification 必须冻结 query/submission budget、coarse feedback、holdout refresh/rotation 和 stopping rule。执行环境防止
读取 control identity、harness secret、sibling receipt 或网络旁路。使用 randomized/disjoint sampling、metamorphic properties、
mutation、source-defect traps 和多机制 evidence，检查 hard-coded outputs、fixture/test-ID branch、harness detection 和
benchmark-only specialization。

合法 specialization 必须在 qualification 前声明 domain、dispatch 和 fallback；在独立 workload 上评价。系统不能理论上
证明任意程序不会对有限 evaluator 过拟合，因此必须限制反馈泄漏并限制最终 claim 的 evidence scope。

## 20. Honest、negative 与 hidden control failure

- Honest/correct-variant control 验证 Oracle 是否接受已知合格行为；
- Negative/targeted-mutant control 验证 Oracle 是否错误接受不合格行为；
- Hidden challenge 使用不同于 public/original item、但由 Controller 确定且强类型的 identity，不能错误绑定同组相同 item；
- hidden material、expected result 和 sibling receipt 永不暴露给 proposal roles。

Oracle control failure 必须区分：

- `OracleArtifactRejected`；
- `NegativeChallengeAccepted`；
- `MechanismProtocolViolation`；
- `ExecutionFailure`。

只有 honest control 证明当前 public Oracle artifact 有可归因 defect 时，feedback 才进入 Developer revision。exit 31 等负向
挑战被错误接受属于 `NegativeChallengeAccepted`，进入 Controller control reconciliation，不能要求 Developer 修改正确
Oracle 去适配错误 control。

Developer/Reviewer 只能读取当前 graph node、artifact revision 和 exact authorized receipt 的 stdout/stderr projection。sibling、
missing、over-limit content 拒绝；单 artifact 上限 16 KiB。诊断正文不进普通日志。

## 21. PyTorch 与其他 reference

任务不要求 PyTorch。若存在 CUDA extension、PyTorch/CPU expression、OpInfo、tests 或 framework contract，可以作为一个
reference provider，但必须冻结 operator/version、dtype/shape/layout/device/domain、forward/backward/mutation/alias/dispatch
scope、backend/shared dependency、comparator 和 unsupported behavior。

PyTorch 与 CUDA provider 若共享底层实现，不能计作两份 independent evidence。纯 PyTorch workload 不替代产品的 CUDA →
Ascend C 输入。没有 framework/reference 时保持 partial/unknown，并使用 properties、metamorphic、source observations、
multiple implementations 和用户 decisions。

## 22. Previous feedback

feedback 是 exact、typed、content-addressed input，不是无限聊天历史。至少区分：

- `SirEvidenceFeedback`；
- `IntentAdmissionFeedback`；
- `OrganicConcernReviewFeedback`；
- `CoverageChallengeFeedback`；
- `OraclePropertyReviewFeedback`；
- `PortfolioCoherenceFeedback`；
- `OracleAdmissionFeedback`；
- `CandidatePromotionFeedback`；
- `WorkerExecutionDiagnostic`；
- `PerformanceMeasurementFeedback`。

feedback 只能触发新 revision，不能原地修改 frozen artifact。Candidate feedback 若暴露 intent ambiguity 返回 focused SIR/
Intent lineage；若暴露 Oracle defect，按第 17 节 change control；若只是 Candidate defect，不改 Oracle。

## 23. Skill 与 knowledge

每次 episode 只看到 Controller 授权的 index/summary，并按需读取 exact content。entry 具有 content identity、provenance、
target/version/time、scope、trust state、dependency 和 revalidation trigger。

- skill 建议步骤或工具，不扩张 capability；
- knowledge 支持 exploration hypothesis，不直接支持 Admission；
- retrieval rank、模型信心和来源声誉不替代 provenance；
- blind organic 阶段只读取 `OrganicReasoningSafe` content；
- `PolicyChallengeOnly` 和 `AdmissionRestricted` 使用不同强类型 exposure authority；
- fixture answer、expected output、ID 和特定 prompt 不进入 production knowledge。

当前消融阶段保留最小 content-addressed seam，不以前置建设通用向量知识库/skill 库为条件。

## 24. Oracle Admission

Oracle Admission 是 model-free Gate，只消费 frozen qualification epoch、portfolio、policy、authority、trusted receipts、hidden
control closure、admitted intent/evidence 和 target identity。它重算：

- policy ledger closure 和 cross-node binding；
- mechanism executable authenticity；
- honest/correct-variant acceptance；
- targeted-mutant/negative/hidden rejection；
- comparator/domain/numerical provenance；
- exact capability/950PR execution；
- evidence independence/shared dependency；
- query/exposure/replacement closure；
- required unknown 和 failure classification。

输出是 claim/domain-scoped `Admitted`、`Partial`、`Rejected`、`Unknown` 或 `NotExecuted`。只有 required Oracle claims 闭合，
才允许同 epoch Candidate Promotion/Admission。Oracle accepted 只说明 judge 在 exact epoch 中有资格评价 Candidate，不说明
Candidate 已通过。

## 25. 持久化、恢复、强类型与日志

必须 durable：intake、sealed policy/exposure、intent/SIR、graph authority nodes/edges、Oracle/Candidate revisions、Worker
request/job/receipt/projection、blind freeze、challenge/disposition、Review、control exposure、qualification epoch、promotion 和
Admission outcomes。

restart 后恢复 exact visibility、continuation、revision 和 query budget。已有 terminal receipt 的 job 不重复；旧 feedback、
receipt、control 或 hidden exposure 不跨 lineage。

日志只记录 task/stage/role/episode/node/revision/job/receipt/epoch identity、计数、状态和失败分类。不记录 source、prompt、
model body、stdout/stderr、hidden content、auth token、credential 或用户敏感信息。实验可在独立 typed usage record 保存 token
**计数**和成本，不保存 token 内容或模型正文。

## 26. Fail closed 与正常终态

允许：

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

模型弱、预算耗尽、Worker 不可用、reference 缺失或 qualification query 用尽都不能放宽 Gate。可以交付 exploratory/partial
报告，但必须说明不能采用的范围和下一步 evidence。

## 27. Dogfood 与实验验收

真实验证必须由 runtime model 通过正常入口：

1. 读取此前未知 CUDA task、caller 和 exact 950PR context；
2. Controller 创建 Candidate Search Loop generation与首个 Exploration Episode，actor直接开始 reasoning，不运行 mini-SIR；
3. actor 以 typed action请求实验、提出 assurance/Candidate revision；Controller校验后才调度 Worker或修改 durable state；
4. 若发现 material semantic fork，通过 focused SIR/Worker/administrator 决定；
5. Intent Admission 发布 contract/evidence snapshot；
6. 生成无发布 authority 的真实 exploratory Ascend C Candidate；
7. ordinary Worker 对 exact Candidate 做真实 build/run/profile；
8. Development Evaluation反馈进入下一 immutable search state，assurance graph随 source/target/Candidate evidence演进；
9. policy 暴露前 freeze organic graph，再完成 complete challenge/disposition；
10. 必要时进入 full D property/case/mechanism Review；
11. Search Loop机械检查 exit prerequisites，结束 generation并冻结 Development/Qualification Oracle和 exact Qualification Epoch；
12. loop外的 ordinary Workers执行 honest、correct、negative、hidden和 950PR controls；
13. Oracle Admission 后，对 parent/current Candidates执行同 epoch promotion；
14. qualification failure若继续开发，创建新 generation并只投影 exposure policy允许的反馈；
15. 只有 qualified revision/family进入 migration package。

人工必须能审查 intent、fork、evidence、organic concerns、policy mappings、Candidate revisions、Oracle changes、promotion claims、
receipts、control exposure、epoch 和 verdict。至少两个语义/结构/evidence shape 明显不同的任务使用相同 product path；一次
fixture 只是 integration control。

不算成功：coding agent 代 runtime model 解释 fixture；内部 helper/伪 proposal/receipt；shell 冒充 CUDA/950PR；latest revision
自动覆盖 parent；Oracle 为当前 Candidate 调宽；已泄露 hidden case 继续计 holdout；挑最快 shape 伪造 performance；删除
performance/unknown；compatibility/V2/generic ID/fixture branch。

## 28. 当前实现优先顺序

本文描述的是 current V1 target，当前代码尚未完整实现。下一步应按最小真实 consumer推进：

1. 删除/绕开任何 mandatory SIR classifier，统一 direct/focused path 的 `IntentContractProposalV1`；
2. 定义 `CandidateSearchLoopV1`、`ExplorationEpisodeV1`、typed action envelope、immutable search state 和 generation lifecycle；
3. 建立最小 Evidence/Assurance Graph 强类型节点、边和 exact feedback routing；
4. 在首 reasoning 前 seal policy derivation/exposure，接通 pre-challenge organic freeze；
5. 接通真实 `ExploratoryAscendCandidateV1` 及 ordinary Ascend build/run Worker；
6. 实现 Development/Qualification Oracle distinction 和 Oracle revision causes；
7. 实现 `QualificationEpochV1`、revision invalidation、loop-exit boundary 和 control exposure state；
8. 实现 Candidate lifecycle、五层 promotion gates、same-epoch symmetric replay；
9. 接通 exact 950PR performance/precision policy 与 Pareto candidate family；
10. 用正常 CLI/server/app/workflow/Worker 路径运行无 framework 和有 reference 的不同任务；
11. 在 correctness、hidden、capability 和 replay Gate 通过后再比较 D/E 成本。

不先实现通用知识库、额外角色网络、Proposal Host、fixture-specific logic 或 compatibility path。

## 29. 设计验收不变量

- 没有 mini-SIR/readiness/skip classifier；
- runtime model 在实际 migration reasoning 中提出 material semantic fork，Controller 不解释源码；
- Candidate Search Loop 是 Controller-owned durable process，不是 Actor 调用的工具或模型内部循环；
- Exploration Actor 只返回 typed actions；Worker effect、revision、stopping 和 transition由 Controller控制；
- 每轮使用 immutable authorized state projection，不依赖未持久化聊天记忆；
- qualification 位于 Search Loop 外；失败后继续开发必须创建新 generation并执行 feedback exposure policy；
- direct/focused path 产生同一 intent proposal，Intent Admission 在 qualification 前完成；
- CUDA behavior 不自动 promotion；
- exploratory Candidate 无发布 authority，但能通过 ordinary Worker 产生真实 target evidence；
- initial actor 看不到 sealed taxonomy，organic graph 在 challenge 前冻结；
- complete policy ledger 无静默 gap，required unknown 阻塞，full D 可升级；
- Development Oracle 可学习，Qualification Oracle 不可无限查询；
- Oracle revision 有 exact cause、meta-qualification 和 same-version symmetric replay；
- latest Candidate 不自动晋升；五层 Gate、predeclared improvement 和 same-epoch comparison闭合；
- performance/precision 不通过挑 workload、缩 domain 或调 tolerance 伪造；
- hidden control 有 exposure/query/replacement policy；
- qualification epoch 在任何相关 revision 后失效；
- exact 950PR receipt 不被低 capability 替代；
- hidden challenge identity、exit 31、diagnostic receipt/16 KiB边界保持；
- Admission model-free、failure types/feedback routes不混合；
- strong identities、validated deserialization、restart 和安全日志保持；
- 正常 CLI/server/app/workflow/Worker path和第二个不同任务通过；
- 最终交付可运行、可重放、可审查的 950PR migration package，而不是 SIR/Oracle/Candidate 文本。
