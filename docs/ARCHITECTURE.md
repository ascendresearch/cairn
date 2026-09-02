# Cairn 产品与系统架构

- 状态：当前唯一规范性设计
- 日期：2026-09-01
- 产品范围：此前未知的 CUDA 算子或 kernel family → Ascend 950PR（3510）亲和的 Ascend C migration package
- 内部格式：current V1；pre-release 阶段直接修改 V1，不保留兼容路径

本文同时定义产品边界、端到端工作流、authority、运行拓扑和关键领域不变量。其他文档不得补充隐含架构要求。

## 1. 产品承诺

Cairn 接受 CUDA source、host launcher、build files、caller declaration 和 exact target。PyTorch、CPU/Python
reference、framework tests、论文、模型图和生产 workload trace 都是可选证据，不是入口前提或天然 truth。

产品输出是开发者可以审查、重放和采用的 `MigrationPackageV1`：

- 一个 correctness-first Ascend C baseline；
- 按 shape、dtype、layout、alignment 或 workload 分区的 kernel variants；
- host tiling、`TilingKey`、dispatch、workspace、build 和 integration；
- admitted intent、validation bundle 和 exact target binding；
- correctness、numerical、integration、safety 和 performance outcomes；
- 已知限制、unknown、适用 domain 和 revalidation trigger；
- source diff、receipt lineage、benchmark/profile 与 replay commands。

近期产品范围是 operator/kernel-family migration，不承诺完整任意 CUDA application 的自动迁移。多 kernel graph、
通信和应用级 runtime 只有在 operator workflow 稳定且出现真实 consumer 后扩展。

## 2. 非目标

Cairn 不是：

- CUDA token、API 或 intrinsic 的机械替换器；
- 假定输入 CUDA 正确的等价抄写器；
- 只生成建议、测试计划或 Oracle 文本的咨询系统；
- 依赖一个固定模板库覆盖全部任务的 generator；
- 让模型通过自评、投票或角色共识准入自己的 Agent demo；
- 由 repository coding agent 读取 fixture 答案后代替 runtime model 的脚本；
- 在首个产品闭环前建设的通用多 Agent 平台或知识图谱产品。

高层库、`aclnn`、framework backend 或已有实现可以是 reference、baseline、seed 或显式 escape hatch，但不能
冒充生成的新 Ascend C 实现。任何 fallback 都必须在 artifact 和 outcome 中明确标识。

## 3. 产品不变量

### 3.1 Source 不是 specification

系统必须区分：

1. CUDA 程序在 exact 环境和输入下做了什么；
2. caller 希望保留什么；
3. independent reference 或 framework contract 声明什么；
4. Ascend C candidate 必须满足什么。

CUDA bug、race、越界、未初始化读、偶然 launch 行为或不必要数值误差不会因被观察到就自动成为 migration intent。

### 3.2 模型只有 proposal authority

模型可以提出 hypothesis、experiment、validation mechanism、candidate、diagnosis 和 stopping recommendation。
它不能写 authoritative workflow state、直接调度 Worker、制造 receipt、读取 hidden control、冻结 qualification
或发布 verdict。

模型能力、角色名称、fresh episode、多模型一致和高置信度都不增加 evidence strength。

### 3.3 真实 effect 必须形成 exact receipt

编译、执行、sanitizer、proof、profiling 和外部查询必须先获得 durable operation authority，再由匹配 capability
的 Worker 执行。receipt 绑定 task、artifact、revision、job、attempt、environment、input 和 output content。

host shell、CUDA、Ascend compile、simulator 与 950PR execution/performance 是不同 evidence class，不可互换。

### 3.4 发布标准与开发搜索分离

开发过程中可见的测试和诊断是搜索信号，不是独立发布证据。Qualification 位于 search generation 外部，使用冻结的
intent、Oracle、candidate、target、policy 和 proposal-invisible controls。

### 3.5 强类型承载 authority

语义不同的 identity、role、revision、capability、evidence、unit、lifecycle 和 outcome 必须使用不同 validated type。
wire/storage raw representation 只存在于 codec 或 persistence 边界，进入领域逻辑前立即验证。反序列化必须重跑构造器
invariant。容易混淆的类型应有 compile-fail 或等价静态边界测试。

## 4. 四个产品平面

| 平面 | 回答的问题 | 主要产物 |
| --- | --- | --- |
| Semantic Contract | 应当迁移什么？ | facts、hypotheses、unknown、source disposition、admitted intent |
| Platform Facts | exact target 允许和擅长什么？ | compile probes、microbench、profiler、capability 和 measured facts |
| Implementation Search | 如何得到硬件亲和实现？ | candidate family、tiling、pipeline、dispatch 和 revisions |
| Assurance and Delivery | 为什么可以采用？ | executable validation、qualification、performance、package 和 replay |

四个平面共享 evidence lineage，但不共享 generic identity、authority 或 outcome。

## 5. Authority 模型

| 参与者 | 拥有 | 不拥有 |
| --- | --- | --- |
| Controller | workflow、public record、budget、capability grant、Worker scheduling、mechanical transition | semantic truth、hidden answer、Admission verdict |
| Exploration/Review strategy | task-scoped reasoning和 typed proposal | execution、state mutation、Admission |
| Focused semantic investigation | exact material fork 的 hypothesis、evidence 和用户问题 | mandatory preflight classifier、用户决定 |
| Worker | exact opaque job 的执行 observation | intent 解释、scope expansion、verdict |
| Administrator/caller | desired semantics 和 policy 决定 | hidden evaluator、伪造 execution fact |
| Admission | frozen applicant、policy 和 trusted receipt 的机械重算 | applicant 生成、模型补答案、按 candidate 放宽 judge |
| Knowledge/skill | 有 provenance 的事实候选、方法和工具建议 | capability grant、evidence 或 verdict authority |

`Proposal != Observation != Decision`。相似的底层表示不能合并这些公共语义类型。

## 6. 端到端工作流

```text
Task Intake
    │ freeze source/caller/target/policy/model/tools/budget
    ▼
Candidate Search Generation
    │
    ├── Exploration Episode → typed action proposals
    ├── optional focused semantic investigation
    ├── Intent Admission
    ├── public Development Validation
    ├── CUDA/reference/Ascend experiments
    ├── candidate-family revision and target profiling
    └── evolving minimal Evidence/Assurance Graph
    │
    ├── continue with next immutable search state
    └── freeze organic work + complete policy challenge
                    │
                    ▼
            end search generation
                    │
                    ▼
Qualification Epoch
    ├── Oracle meta-qualification
    ├── honest/correct/mutant/hidden controls
    ├── exact 950PR correctness and performance
    ├── parent/current symmetric replay
    └── model-free Oracle and Candidate Admission
                    │
                    ▼
MigrationPackageAccepted | honest non-success terminal
```

### 6.1 Task Intake

Intake 冻结：

- source、host、build、tests、caller declarations 和允许读取的上下文；
- `TargetPlatformContextV1`：SoC、CANN、compiler、runtime、ABI、device 和 workload policy；
- model、adapter、tools、knowledge snapshot、Worker capabilities 和 budgets；
- data policy、promotion objective、minimum practical improvement 和 stopping policy；
- Oracle policy derivation、challenge-stage knowledge/skill exposure manifest、hidden control family、query/exposure policy 的 sealed
  identity。

初始 reasoning 看得到任务事实和 target，但看不到 hidden evaluator 或 Cairn 内部 concern taxonomy。

### 6.2 Candidate Search Loop

`CandidateSearchLoopV1` 是 `CudaMigrationWorkflow` 内 Controller-owned durable state machine，不是 Agent、工具或
模型内部 while-loop。每次 iteration：

1. Controller 从 durable state 构造 immutable、authorized projection；
2. 创建或恢复一个 model/tool episode；
3. Actor 返回 typed experiment、intent、assurance、candidate 或 stopping proposal；
4. Controller 验证 identity、authority、budget、parent 和 capability；
5. 获准 effect 进入 ordinary Worker，receipt 折回 public evidence；
6. Controller 创建下一 immutable state。

一个 `ExplorationEpisodeV1`、一次 search iteration、一个 search generation 和一次 qualification attempt 是不同
生命周期。模型说“完成”、confidence 很高或预算耗尽都没有 transition authority。

Experiment request 必须绑定 exact uncertainty/decision、competing predictions、required capability、input、cost/risk bound
和 requesting lineage。Previous feedback 是 typed、content-addressed 的 exact input，只能触发新 revision；continuation、
feedback、receipt 和 hidden exposure 都不能跨 lineage 投影。

Search generation 只有在没有 pending Worker job、未消费的 authorized receipt、未决 administrator request 或会改变 epoch
内容的 revision，并且 stopping/budget policy 给出机械结论后才能关闭。Qualification failure 若继续开发，必须创建新的
generation；旧 generation 不恢复，反馈只按 exposure policy 投影。

### 6.3 Source understanding 与 focused investigation

每次迁移都会发生 source understanding，但不运行 `ShouldStartSir`、readiness assessment 或其他改名后的 mini-SIR。
只有实际推理发现会改变 candidate、domain、comparator、ABI 或用户可见行为的多个 plausible interpretations，且当前
evidence 无法消除时，才物化 focused semantic investigation。

它必须记录 competing interpretations、exact evidence、对 candidate/judge 的差异、可区分预测和必要的 caller 问题。
direct path 与 focused path 最终提交同一种 `IntentContractProposalV1`。Intent Admission 独立发布 claim-scoped contract，
未经筛选的 transcript 和模型 summary 都不是 contract。

### 6.4 Evidence/Assurance Graph

Graph 只持久化具有 authority、consumer、effect、replay 或 recovery 价值的状态，不保存完整 chain-of-thought。最小节点包括
facts、intent claims、observations、unknown、assurance concerns、validation mechanisms、candidate revisions、experiments、
receipts、findings、qualification epoch 和 outcomes。

support、refute、dependency、shared/contaminated、applies-to-domain、tests-candidate、supersedes、invalidates 和
feedback route 使用不同 edge type。临时 planning detail 留在 episode 内。

Policy challenge 前物化的 `OrganicAssuranceConcernV1` 至少绑定 exact trigger、candidate acceptance risk、provisional domain、
与已有 concern 的 dependency/conflict，以及下一项 evidence/decision。Challenge 前先冻结 organic subgraph、visible evidence、
model/tool/skill exposure 和 receipts；challenge 后发现的 concern 不能回填成 organic discovery。

## 7. Candidate family 与搜索策略

### 7.1 Correctness-first

每个任务先争取最简单、最容易检查的 running baseline。性能 revision 不能删除 baseline 或覆盖其历史证据。Candidate 可按
shape、dtype、layout、alignment、resource 或 workload 形成 Pareto variants；部分 domain 更优的实现不能全局 supersede。

### 7.2 搜索必须是可替换策略，而不是自由漫游

Candidate strategy 可以是 Agent、template、enumerative search、evolutionary search、solver 或组合，但统一产生 immutable
candidate revisions。V1 的实际搜索至少支持：

- 多 parent/variant，而不是一个可变 `latest`；
- 有界 best-of-N baseline 和保留多样性的 population/beam 策略；
- host tiling/data movement 与 device kernel/schedule 两个耦合搜索面；
- compile、run、correctness、profile 和 resource feedback；
- plateau、dead-end、restart、budget 和 abstention；
- predeclared improvement claim 与 exact parent comparison。

Tree/MCTS/MAP-Elites 等算法只有在同预算消融中证明收益后成为默认，不写成产品不变量。

### 7.3 Hardware feedback 必须可行动

原始 compiler/profiler output 是 observation，不直接成为优化建议。确定性 analyzer 应把它变成带 provenance 的 bottleneck
classification 和 ranked action，例如：

- build/API/ABI violation；
- UB overflow、alignment、tail 或 queue/buffer lifecycle；
- memory-bound、compute-bound、occupancy/resource-bound；
- ineffective vector/cube path、pipeline stall 或 data-movement excess；
- timing instability 或 invalid measurement。

模型同时看到必要 raw slice 和结构化解释；是否真的改善仍由下一次 target measurement 决定。

Performance verdict 以同一 950PR workload 下的 meaningful baseline 为主。只有 computation/memory ceiling、device state 和
measurement method 经 calibrated probe 后，才报告 conditional roofline 或 bottleneck-supported claim；CUDA 与 Ascend 裸耗时
不能直接比较。Cairn 负责 operator-level evidence，模型/部署级业务接受权仍属于 upstream caller。

### 7.4 Target knowledge

低资源 Ascend C context 是第一阶段正确率的输入，不是 Admission evidence。最小 knowledge pack 包含 exact-version 官方文档、
compile-validated API/primitive examples、measured platform facts、diagnostic recipes 和 optimization motifs。每项 entry 绑定
provenance、target/version、scope、trust state 和 revalidation trigger。

Episode 只接收获准的 index/summary，并按需读取 exact entry。`OrganicReasoningSafe`、`PolicyChallengeOnly` 和
`AdmissionRestricted` 是不同 exposure authority；retrieval rank、模型 confidence 和 source reputation 不能提升 trust state 或
扩张 Worker/tool capability。

成功 fixture 的算法答案、expected output、task identity 或专用 prompt 不得进入 production knowledge。跨任务沉淀只允许
经过审查的通用方法、原语和 target fact。

## 8. Oracle 与验证

Oracle 是 executable `ValidationBundleV1`，不是测试建议文本：

- domain/input generators 和 public regressions；
- optional CUDA、CPU、framework 或高精度 reference providers；
- relational/metamorphic properties；
- numerical comparator 与 allowance provenance；
- ABI、integration、memory、state、concurrency 和 safety checks；
- performance workloads、baseline 和 measurement policy；
- hidden/mutant controls与 replay manifest。

每项 validation obligation 必须产生 candidate-facing mechanism、可反驳 claim、明确 evidence gap 或有依据的
`NotApplicable`。缺少 independent reference 时可以形成 partial assurance，不能把 CUDA 自动提升为 truth。

Property、case 和 mechanism 是不同层次：property 表达可反驳义务，case 是其 domain sample，mechanism 是绑定 exact candidate
observation 的执行方法。增加 input、shape 或 mutation 通常增加 case，不自动制造新 property。Accepted portfolio 必须做
跨 property 的 overlap、dependency、conflict、capability 和 binding coherence 检查。

Reference strength 必须记录 derivation 和 shared dependency。PyTorch、CUDA、CPU reference 或多个模型若共享实现路径，不能
按数量冒充独立 evidence。来源冲突时不得使用“多数票”；在 evidence dependency 足以定位前保持 unresolved。

### 8.1 Development 与 Qualification

`DevelopmentOracleRevisionV1` 对 search 可见，用于 repair、public regression 和 profiling。
`QualificationOracleRevisionV1` 在 formal attempt 前冻结，并包含 proposal-invisible controls、exact target、comparator、
query/exposure 和 promotion policy。Qualification Oracle 不是可无限查询的训练 API。

### 8.2 Policy challenge 与风险自适应结构

首个 episode 前 seal task-generic policy derivation；organic assurance work 在 policy 暴露前冻结。Intent admitted 后，Controller
机械实例化 complete concern ledger，fresh review 对每项 concern 给出 adopt、merge、split、case、informational、
not-applicable、unsupported 或 unknown disposition。

Catalog 是 coverage floor，不是允许列表。required unknown 阻塞 release。只有 high-severity gap、invalid mechanism、
overmerge/case inflation 或 repeated review failure 才升级为完整 property→case→mechanism review。模型 confidence 不能跳过
policy challenge，Controller 不能自行解释 semantic coverage。

Review 的独立性来自新增信息，不来自角色名称。只有新的 source slice、independent reference、Worker receipt、sanitizer、proof、
mutation、hidden challenge、不同 trust source 或 mechanical recomputation 才能提高 evidence strength；同一模型换名复述只提供
结构检查。Finding 必须绑定 exact artifact、graph node、revision 和 failure class，Reviewer 不能修改 admitted intent 或发布
Admission verdict。

Oracle control failure 必须强类型区分 `OracleArtifactRejected`、`NegativeChallengeAccepted`、
`MechanismProtocolViolation` 和 `ExecutionFailure`。只有可归因于 public Oracle artifact 的 defect 才进入 Oracle revision；
negative control 被错误接受进入 control reconciliation，不能要求 Developer 修改正确 Oracle 去适配错误 control。Candidate
failure 另行分类为 candidate defect、Oracle defect、platform fact gap、intent ambiguity、execution failure 或 protocol violation。
Hidden challenge 使用与 public/original item 不同且由 Controller 绑定的 strong identity；hidden material、expected result、control
identity 和 sibling receipt 永不投影给 proposal roles。

## 9. Qualification、Oracle change 与 Candidate promotion

`QualificationEpochV1` 不可变绑定：

```text
AdmittedIntentRevision
× QualificationOracleRevision
× TargetPlatformContext
× CandidateRevision / CandidateFamilyRevision
× PromotionPolicy
```

任一相关 revision 改变都会使 epoch 失效。旧 receipt 只有在 exact artifact、environment 和 policy 仍满足显式 reuse rule 时
才能重算，不能复制 verdict。

Oracle 变化必须使用互斥 typed cause：`OracleArtifactCorrection`、`CoverageExpansion`、`EvidenceStrengthening`、
`IntentContractChange`、`TargetPolicyChange` 或 `CandidateAccommodationAttempt`。前三类创建新 revision、经过独立
meta-qualification，并对 parent/current 对称重测；intent/target change 创建新 lineage/epoch；仅为当前 candidate 通过而放宽
judge 的 accommodation 直接拒绝并记录 authority violation。

Candidate promotion 依次通过：

1. revision integrity：artifact、parent、diff、generator episode、domain、toolchain、target、dispatch、hidden isolation、fallback 和
   execution authenticity；
2. required non-regression：全部 required semantics、numerical、integration、safety 和 public regressions；
3. claimed improvement：qualification 前冻结的 correctness、domain、precision、performance 或 resource claim；
4. comparative promotion：同 intent、Oracle、target、workload 和统计政策下比较 parent/current；
5. independent qualification：proposal-invisible correct、invalid、hidden 和 950PR controls。

correctness、安全、integration 和 required numerical allowance 是硬约束。性能只在这些 outcome 通过后比较。缩小 domain、
改变 workload weighting、挑最快 shape 或放宽 tolerance 不构成优化。

Candidate lifecycle 至少区分 `Exploratory`、`DevelopmentEligible`、`QualificationPending`、`Qualified`、`Rejected` 和
`Superseded`。`latest` 不是 authority；局部更优 revision 进入 domain-bound Pareto family，dispatch 和 fallback 分别验证。

Hidden control 有 `Hidden`、`FeedbackAbstracted`、`RetiredToPublic` 状态。公开具体 input、expected result 或可逆诊断后必须退休并
补充新 holdout。Formal qualification 冻结 submission/query budget、反馈粒度和 stopping rule。

## 10. 运行与数据架构

### 10.1 进程拓扑

```text
clients
   │
   ▼
cairn-server
  Controller + app composition + proposal episodes
   │                    │
   │                    ├── public event store / CAS
   │                    └── model providers / approved research
   │
   ├── cairn-admission-equivalent authority boundary
   │       └── restricted event store / CAS
   │
   └── managed cairn-workers
           ├── CUDA / sanitizer
           ├── CPU / reference
           ├── Ascend build
           └── 950PR run / profiler
```

当前代码可在现有 composition 中逐步落实该边界；文档中的逻辑组件不要求每个概念一个 crate 或微服务。只有 hidden authority、
applicant code/toolchain/device execution 或真实部署证据需要独立进程/credential。Proposal roles 是 Controller workflow step，
不是独立 service principal。

Managed Worker 主动通过 authenticated encrypted control/enrollment channel 连接 Controller；Controller 不反向拨号 Worker，
不以 SSH reverse tunnel 作为产品拓扑，也不自建 VPN。部署者提供可路由网络和 advertised endpoint；enrollment bundle、private
key 和 Worker credential 属于 Secret provider，不进入 repository、task context 或普通日志。

### 10.2 正常客户路径

```text
cairn-cli → cairn-server → migration app API → CudaMigrationWorkflow → managed Workers
```

fixture runner、recorded adapter、专用 binary、fake receipt 或直接调用内部 helper 都不能作为产品成功路径。

### 10.3 存储可见性

- Public store：task、proposal、candidate、public receipt、redacted diagnostic 和 public outcome；
- Restricted store：hidden cases、expected values、private mutants、full gate receipt 和 exposure ledger；
- Secret provider：token、private key 和 enrollment material，只在 exact effect adapter 解析。

三者使用不同 typed ID/domain 和 access port。知道任意 content ID 不能读取其他 namespace。普通日志不得包含 source、prompt、
model body、stdout/stderr、hidden content 或 credential；只记录稳定 identity、计数、状态和 failure class。

诊断正文只能通过绑定 exact node/revision/receipt 的显式授权读取；missing、sibling、cross-lineage 或 over-limit 请求 fail closed。
单次 artifact diagnostic projection 上限为 16 KiB，更大内容必须形成新的受限授权，不能借日志或模型上下文旁路读取。

### 10.4 Durable command/effect

外部 effect 使用 command→durable authority/event→execution→receipt。Controller restart、Worker disconnect 或 provider timeout
不能改变已授予 authority。已有 terminal receipt 的 job 不重复执行；retry 创建不同 attempt identity并保持同一 job contract。

## 11. Fail-closed 终态

合法终态包括：

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

模型弱、预算耗尽、Worker 不可用、reference 缺失或 query budget 用尽不能放宽 gate。Partial/exploratory package 必须明确不可
采用的范围和缺少的下一项 evidence，CLI/API 不显示模糊 success。

## 12. 架构演进规则

- 一个结构只有在承载真实 consumer、authority、effect、replay、recovery 或可测质量收益时才进入领域协议；
- 新 Agent role、Planner、service、crate、graph node 或 knowledge subsystem 必须由当前产品闭环的缺口证明；
- 策略、搜索算法和 reviewer topology 通过同预算实验选择，不因设计对称性固化；
- 当前设计变更直接修改本文、代码、测试和 current V1；不保留旧文档、alias、dual path 或 converter；
- 实现事实只更新 `IMPLEMENTATION.md`，评价方法只更新 `EVALUATION.md`；
- 历史设计、实验和实施流水通过 Git 追溯，不进入当前阅读路径。

## 13. 尚待证据决定的问题

这些问题属于当前方案的一部分，但答案尚未被真实运行证明。实现不得按便利静默选择；答案必须通过 frozen policy、对照实验、
exact receipt、authorized user decision 或明确的产品触发条件进入本文。

### 13.1 首个 package 或相应能力启用前

| 问题 | 当前保守边界 | 决策所需证据 |
| --- | --- | --- |
| semantic reference 的独立性 | 记录 derivation strength；source-derived reference 不冒充 independent | independent/source-derived/absent reference 与 correlated-failure controls |
| source、reference、caller 多源冲突 | 不用多数票，无法定位则 `IncompleteSpecification` | evidence-dependency graph 和受控 correlated failures |
| Intent Admission 最晚时机 | qualification 前必须 admission；高成本 950PR search 可由预算 policy 提前要求 | early/late admission 对返工、问题数和性能搜索成本的比较 |
| organic 阶段可见的 knowledge/skill | 只允许不会泄露 sealed taxonomy 的内容 | retrieval exposure 消融、leakage controls 和 downstream utility |
| knowledge/skill 的 trust profile | 未定义前只能指导 exploration，不产生 capability 或 Admission | 按 claim kind/role 区分 official-doc、reviewed、execution-validated evidence 的 controls |
| full structured fallback 阈值 | 仅响应 typed gap/severity，不让 Controller 解释语义 | gap severity × defect yield × cost 的预注册实验 |
| Evidence/Assurance Graph 最小持久化 | 只保存有 consumer/authority/replay/recovery 的节点 | restart、cross-feedback、graph churn 和遗漏率 |
| Reviewer topology | fresh episode 不自动算独立 | 同模型、异模型、人工和新 evidence channel 的增量 finding/cost |
| early Candidate 与 Oracle co-adaptation | Development feedback 可见，Qualification 仍隔离 | candidate-revealed obligation yield、late defects、hidden validity 和返工成本 |
| focused investigation 的 precision/recall | 没有 independent specification 时允许 `Indeterminate` | blinded evaluator、authorized user decision 和 replayable counterexample |
| early vs late policy challenge | release 前必做，当前不假定最佳时机 | anchoring、coverage、late reopen、full fallback 和总成本的同预算比较 |
| Qualification Epoch 粒度 | 默认 whole bound candidate/family，不能跨 workload 暗中复用 | family 与 variant/workload partition 的 invalidation/reuse 数据 |
| hidden query、反馈和补库政策 | 具体诊断 conservatively burn case；冻结 budget/stopping | task-risk 分层的 leakage、repair yield 和 replacement closure |
| Oracle meta-qualification corpus | 与 applicant lineage 隔离，correct/mutant family 不对 proposal 可见 | contamination controls、false accept/false reject 和 symmetric replay |
| performance/precision 最小实际改进 | qualification 前冻结；来源不明则不做 promotion claim | caller objective、workload evidence、noise 和 adoption threshold |
| exact 950PR performance profile | 不发明 ceiling、profiler field 或 threshold | exact CANN/compiler/firmware、device-state probe、baseline 和 calibrated microbench |
| weak-model 行为 | 允许 honest abstention，不降低 gate | model/budget/context capability strata 下的 coverage、fallback yield 和成本 |

### 13.2 由后续任务或产品表面触发

| 触发 | 未决方案边界 |
| --- | --- |
| 使用 metamorphic relation 做正式 evidence | relation-specific construction、浮点 allowance、adversarial inputs、broken-relation controls 和 admission strength |
| random/stateful/schedule-set operator | RNG/state model、reset isolation、legal outcome set、power、type-I/type-II error、multiple tests 和 inconclusive outcome |
| 更强 runner authenticity 声明 | binary/image/library/device/worker attestation；未独立证明前只报告 observable identity |
| model-level production feedback | context/privacy、workload weighting、first-divergence attribution；positive feedback 不证明局部正确，negative feedback 不自动改 Oracle |
| confidential source/evidence | local-only provider、private/encrypted CAS、redacted export 和 verifier access；导出必须说明不可重算的 claim |
| 公共远程 App API | stable resource model、authentication、streaming、reconnect 和 backpressure 先有测量，再选择 transport |
| 两个真实 out-of-tree integration | process-boundary extension 的 manifest、discovery、signing、permission 和 compatibility；此前不建设插件 ABI |
| 首次公开发布 | DCO/CLA、governance、trademark、dependency/license 与 source/fixture/corpus/model-output provenance policy |
| 多 target、多 kernel graph 或应用级迁移 | 先有 operator workflow 的重复成功和真实 consumer，再扩张 topology 与 authority |
