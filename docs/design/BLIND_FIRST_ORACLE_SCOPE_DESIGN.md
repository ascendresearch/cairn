# Blind-First、Policy-Challenged Oracle Scope 设计

- 状态：消融实验后的候选设计，尚未成为当前规范或实现事实
- 日期：2026-09-01
- 产品范围：CUDA → Ascend C 迁移中的 Oracle scope discovery 与后续分层展开
- 首个目标硬件：Ascend 950PR（3510）
- 简称：方案 D，`BlindFirstPolicyChallengedHierarchicalReview`

## 1. 文档地位

本文档完整记录 A/B/C pilot 之后提出的第四种推理分解方案。它是下一轮设计与消融实验的候选输入，不会仅凭
一次 `simplePitchLinearTexture` pilot 自动替换
[`CAIRN_CURRENT_PRODUCT_DESIGN.md`](CAIRN_CURRENT_PRODUCT_DESIGN.md) 或
[`SIR_ORACLE_CURRENT_DESIGN.md`](SIR_ORACLE_CURRENT_DESIGN.md) 的当前规范地位。

方案 D 若经后续实验接受，应直接修改当前 V1 模型、代码、测试和规范，不创建 V2、兼容 reader、双写或旧路径。
本文档与 `AGENTS.md` 冲突时以 `AGENTS.md` 为准。任何示例都只是设计解释，不能成为 runtime prompt 中的 fixture
答案、固定 hypothesis 或产品特殊分支。

写后反向核对记录见
[`BLIND_FIRST_ORACLE_SCOPE_COMPLETENESS.md`](BLIND_FIRST_ORACLE_SCOPE_COMPLETENESS.md)。

## 2. 问题与实验依据

A/B/C pilot 不是正式因果实验，但共同暴露了三个可用于设计的事实：

1. A 的 whole-portfolio 视野能够较快形成广泛、candidate-facing 的计划，但缺少独立批判，重复和弱机制会直接
   留在 portfolio 中；
2. B 的逐 item 独立 Review 能发现真实的 evidence、binding、launch、reference 和 pass-condition 缺陷，但过早
   按 dimension/item 局部分解，使 Reviewer 高成本地审查重复义务；
3. C 的 Worker evidence 能实质性改变 Review 和 revision，但 evidence capability 不会自动纠正错误粒度，反而可能
   放大一个重叠 item set 的成本。

现有固定 concern 提示还存在 anchoring 风险：若 runtime model 在首次观察任务时就看到“正确性、精度、性能、
内存安全”等内部分类，它可能围绕系统词表填空，而不是从 actual code、Intent、target 和 evidence 中独立发现任务特有
风险。反过来，完全相信模型自行想全所有维度也不符合 Cairn 的可信性目标。

方案 D 同时保留两种能力：

- **blind discovery**：先观察模型在不受内部维度清单锚定时自然发现什么；
- **policy challenge**：随后保证产品要求的完整 concern inventory 没有被静默遗漏。

## 3. 核心决策

方案 D 的原则是：

> 模型先独立报告准备分析的维度、触发证据和风险，Controller 冻结该 proposal；随后独立 Coverage Auditor 使用
> 产品 policy concern inventory 提出补充和映射 challenge；最后形成全局最小验证义务图，只对必要且独立的义务
> 展开 property、case 和 mechanism。

“blind”只隐藏 Cairn 内部的 Oracle taxonomy，不隐藏真实任务事实。“policy-challenged”不是把清单中的每一项
强制生成一个 item，而是要求每项 policy concern 都有 exact、可审计的处置。

完整关系为：

```text
完整 policy concern universe
              │ 暂不暴露给 Blind Discoverer
              │
actual task + SIR evidence + admitted Intent + target context
              │
              ▼
Blind Scope Proposal ── freeze ──► Policy Coverage Challenge
                                      │
                                      ▼
                           Consolidated Obligation Graph
                                      │
                                      ▼
                           Property → Case → Mechanism
```

## 4. 不变量

### 4.1 全面性不变量

- Blind Discoverer 可以遗漏 policy concern，但遗漏必须在 challenge 阶段变得显式；
- 最终 ledger 中每个 current-policy concern 必须恰好有一个可重算的 disposition；
- `Unknown` 不能被当作 `NotApplicable`，也不能因为预算或 Worker 不可用而被当作已解决；
- required concern 未闭合时 Oracle Admission 必须 fail closed；
- 性能平面不能静默删除。它可以按 task policy 成为 `Required`、`ApplicableInformational`、
  `NotApplicable` 或 `UnknownApplicability`。

### 4.2 非锚定不变量

- Blind Discoverer 看不到 policy concern inventory、policy concern identity 或系统准备的补充提示；
- 盲阶段不得授权会直接泄露同一 taxonomy 的 skill、knowledge entry、example 或 tool description；
- 盲阶段仍必须看到用户显式要求、admitted Intent、Ascend 950PR target context 和真实 capability manifest；
- 用户说出的“性能”“精度”或其他要求是 task evidence，不是应隐藏的系统 anchoring；
- blind artifact 一旦提交不能被后续 revision 覆盖或伪装成事后才产生的内容。

### 4.3 Authority 不变量

- Blind Discoverer、Coverage Auditor、Scope Consolidator 和 Scope Reviewer 都只有 proposal/review authority；
- Controller 是唯一 workflow writer，负责冻结 artifact、授予不同阶段的可见性和机械遍历；
- Coverage Auditor 不能读取 hidden evaluator、mutation、expected output 或 restricted Admission material；
- policy concern catalog 是任务通用产品 policy，不是 hidden answer；
- Worker observation 只能成为有 provenance 的 evidence，不能自己准入 dimension、Intent 或 Oracle；
- Oracle Admission 仍然是 model-free 的机械 Gate。

### 4.4 强类型不变量

以下身份和状态不得退化成 generic ID、字符串或布尔值：

- blind proposal、blind dimension、policy concern、coverage challenge；
- coverage mapping、dimension origin、dimension disposition；
- consolidated obligation、property、case、mechanism；
- evidence reference、experiment capability、review finding 和 revision lineage。

语义相同但 authority identity 不同的 admitted claims 仍保留独立 claim identity。共享验证只能通过显式 coverage
edge 表达，不能合并、覆盖或丢弃 claim identity。

## 5. 输入与信息隔离

方案 D 从已经完成 Intent Admission 的 task 开始。Blind Discoverer 的输入至少包括：

- exact admitted claims；
- `AdmittedIntentEvidenceSnapshotV1`；
- task source、build、host launch、tests 和 caller scope；
- `TargetPlatformContextV1`，包括 Ascend 950PR、CANN/Ascend C、ABI 和真实 Worker capabilities；
- previous feedback 的 exact references；
- 本阶段授权的 research、knowledge、skill 和 experiment manifests；
- model、budget、数据和日志 policy。

在 Blind Discoverer episode 打开前，Controller 必须先冻结但不暴露：

- task-generic `OraclePolicyConcernCatalogV1` 的 exact identity；
- 绑定本任务 admitted claim set、target context 和 operator policy 的
  `TaskPolicyConcernLedgerV1`；
- 每个 concern instance 的 policy requirement level 和适用条件；
- challenge 阶段将使用的 model/tool/skill/knowledge exposure manifest。

该 sealed commitment 防止系统、coding agent 或 operator 在看到 blind proposal 后临时增删补充维度。ledger 不是
claim × concern 的机械笛卡尔积：它在 task scope 枚举 policy concerns，并由后续 coverage edges 显式绑定一个或多个
admitted claims。Catalog 和 ledger 可以包含通用 applicability condition，不能包含 fixture answer、hidden expectation 或
根据本次模型输出事后编写的提示。

Blind Discoverer 不读取：

- `OraclePolicyConcernCatalogV1`；
- policy concern 的名称、枚举顺序、identity 或示例；
- Coverage Auditor prompt/output；
- hidden evaluator、negative challenge、mutant 或 Admission receipt；
- 通过 skill、knowledge 或 tool description 间接复制的 policy taxonomy。

信息隔离由 Controller 的 capability/exposure policy 实现，不能只靠 prompt 要求模型“忽略清单”。正式实验必须冻结
并记录 blind 与 challenge 阶段各自的 exact tool/skill/knowledge exposure manifest。

## 6. 阶段一：Blind Scope Discovery

### 6.1 目标

Blind Discoverer 不生成最终 Oracle item，也不填写预设维度矩阵。它回答：

> 仅根据当前代码、admitted Intent、目标平台和已授权证据，本任务有哪些彼此不同、可能改变 future Ascend C
> candidate 接受结论的分析方向？为什么？

### 6.2 输出

`BlindOracleScopeProposalV1` 至少包含：

- exact task、Intent、evidence snapshot 和 target context bindings；
- 一个或多个 `BlindProposedDimensionV1`；
- proposed dimension 之间的重叠、蕴含、依赖、冲突或独立关系；
- 未能形成 dimension 但值得保留的 calibrated unknown；
- 已使用和计划请求的 evidence；
- exact model、episode、tool exposure 和 budget identity。

每个 `BlindProposedDimensionV1` 至少包含：

- 强类型 `BlindDimensionId`；
- task-specific dimension statement，而不是只有“correctness”之类的名称；
- code、Intent、target、prior feedback 或 observation 中的 exact trigger evidence；
- 若遗漏该维度可能造成的 candidate acceptance risk；
- provisional applicability domain；
- 与其他 blind dimensions 的关系；
- competing interpretations 或 remaining uncertainty；
- 计划使用的 source/reference/CUDA/Ascend evidence 或 experiment；
- 当前证据不足时的明确 unknown，而不是自信猜测。

第一阶段允许按授权请求 Worker，但实验必须用于判断一个 scope uncertainty 是否存在或如何表述，且必须绑定 competing
predictions、所需 capability 和 exact requesting episode。为了制造列表而打印源码、重复计算或请求无决策影响的实验
应被 Reviewer 标记。Worker 不可用时保留 `IncompleteEvidence`，不能删除对应风险。

### 6.3 防止 generic laundry list

系统不通过一个很小的数量上限强迫模型遗漏维度，但每个 proposed dimension 必须有 exact trigger、candidate risk 和
与相邻维度的区别。机械 schema validation 可以在 freeze 前拒绝缺字段的 submission；语义上空泛或重复的 label 必须在
Coverage Challenge、Scope Consolidation 和独立 Scope Review 中显式合并或拒绝，不能悄悄从 blind artifact 删除。

具体 shape、shift、dtype、边界值和 mutation 通常属于后续 `OracleCase`，不能仅因输入不同就提升为独立 dimension。

## 7. 冻结 Blind Proposal

Blind proposal 在任何 policy 提示出现前被 content-addressed、持久化并关闭其 episode。Controller 记录：

- blind proposal content identity；
- episode、model configuration 和 continuation identity；
- exact visible evidence 和 capability manifest；
- 所有 Worker request/receipt lineage；
- 首次提交和 typed rejection/revision 历史；
- sealed policy catalog/ledger identity 已在 episode 前存在、但尚未对该角色可见的证明性配置事实。

后续阶段只能引用 blind proposal，不能原地修改它。即使 Scope Consolidator 在 challenge 后改变判断，原始 blind
发现仍然存在，便于审计 anchoring、novel discovery 和 supplement dependency。

## 8. 阶段二：Policy Coverage Challenge

### 8.1 Coverage Auditor

Coverage Auditor 使用新的独立 Agent Loop，不继承 Blind Discoverer 的 continuation。它可以使用同一 frozen model
configuration，但 role、episode、context、tool exposure 和 output identity 必须不同。它读取：

- frozen blind proposal；
- admitted Intent 和 target context；
- task-generic `OraclePolicyConcernCatalogV1`；
- 允许验证 blind citation 的 source/evidence；
- 首轮的 `NoPriorCoverageChallengeFeedback`，或后续 exact challenge-revision feedback。

它不生成 Oracle item 或 check plan，只提交绑定 sealed catalog 和 task ledger identity 的
`OracleScopeCoverageChallengeV1`。Challenge 必须枚举 ledger 中的每一个 policy concern instance，而不是只列 Auditor
认为“缺失”的项，并包含：

- blind dimension 到 policy concern 的 proposed mapping；
- 看似未覆盖的 policy concerns；
- 一个 blind dimension 声称覆盖过多 concern 的可疑 mapping；
- blind dimensions 内部的 overlap、case inflation 或错误粒度；
- task-specific novel dimension，不能因为 catalog 中没有同名项而删除；
- 每个 challenge 的 explanation、evidence 和 required response。

Auditor 不能仅用字符串或 embedding 相似度决定覆盖。语义映射始终是 proposal，必须经过后续处置和独立 Review。

### 8.2 为何不由 Controller 直接计算“缺失维度”

Controller 可以机械检查 catalog identity 是否都出现在最终 ledger 中，但不能可靠判断一个 task-specific blind
dimension 是否语义覆盖某个 policy concern。让 Controller 根据名称或 coding-agent fixture 解释自动补齐，会把领域推理
藏进不受审查的编排代码。

因此 Controller 只负责完整枚举和账本闭合；Coverage Auditor 负责提出语义 mapping/challenge，不拥有最终 authority。

## 9. 阶段三：Scope Consolidation

Scope Consolidator 使用新的 Agent Loop，不恢复 blind continuation。它读取 frozen blind proposal 和 exact coverage
challenge。对每个 blind dimension 和每个 policy concern instance，它必须提交显式处置。

建议的 current-V1 disposition 语义为：

- `AdoptAsIndependent`：形成一个独立验证义务；
- `MergeIntoObligation`：重要但与其他维度共享一个验证义务，必须给出 exact coverage edges；
- `SplitAcrossObligations`：一个 concern 的不同 domain/risk 需要多个独立义务，必须给出 non-empty、互不重复的
  obligation edges 和不能合并的理由；
- `RepresentAsCase`：不是独立 property，而是某个义务下的输入、边界或反例 case；
- `ApplicableInformational`：适用但不阻塞当前 acceptance，仍保留证据与适用范围；
- `NotApplicable`：有 evidence-backed task/Intent/target 理由证明不适用；
- `RejectBlindProposalAsUnsupported`：只适用于 blind dimension；保留原 proposal，但以 exact evidence/review finding
  说明其不应进入最终 obligation graph；
- `UnknownRequiresEvidence`：当前无法处置，必须形成 exact evidence gap 和下一动作。

这些是领域上不同的强类型 outcome，不能使用 `bool applicable` 或自由字符串。

`NotApplicable` 不接受“代码中没看到”“模型认为不重要”或“当前 Worker 做不了”作为充分理由。Worker/toolchain
失败属于 execution state，不是 applicability。`UnknownRequiresEvidence` 对 required concern 保持阻塞，直到新 evidence
产生新的 scope revision 或管理员修改 policy/scope。

Scope Consolidator 不能降低 sealed ledger 中的 policy requirement level。若它认为 operator policy 本身需要改变，只能
形成 exact policy decision request；未经授权的 proposal 保持原 requirement。一个 disposition 可以包含多条 coverage
edge，但 ledger 中仍只有一个顶层 disposition，保证 Controller 能机械检查“恰好一次处置”。

## 10. Consolidated Obligation Graph

Scope Consolidator 的主要输出不是两个维度列表的拼接，而是
`ConsolidatedOracleObligationGraphV1`：

```text
AdmittedClaim ─┐
BlindDimension ├── CoverageEdge ──► ConsolidatedObligation
PolicyConcern ─┘                         │
                                       ├── Property
                                       ├── Case
                                       └── Mechanism requirements
```

每个 `ConsolidatedOracleObligationV1` 必须：

- 绑定 non-empty admitted claim set；
- 绑定 non-empty blind/policy origin set；
- 说明为什么它与相邻 obligation 不重复；
- 给出 candidate-facing acceptance question；
- 声明 applicable domain、target 和 evidence needs；
- 区分 semantic property、case families 和 mechanism capability；
- 保留 unknown、shared dependency 和 contamination edges。

一个 obligation 可以覆盖多个 policy concerns 和多个 distinct admitted claim identities。每个 source identity 仍可追溯，
不会因共享 mechanism 而被合并成 generic claim。

Controller 在 Scope Review 前重算：sealed task ledger 的每个 concern instance 恰好被一个 disposition 覆盖；每个未被
typed rejection 的 blind dimension 也恰好有一个 disposition；每条 adopted/merged/split edge 指向存在的 obligation；
required `UnknownRequiresEvidence` 仍保持 blocking。

## 11. 独立 Scope Review

在任何 per-item Developer loop 开始前，独立 Scope Reviewer 审查完整 obligation graph。它只聚焦：

- policy concern ledger 是否完整；
- mapping 和 disposition 是否有证据；
- 是否存在 cross-dimension gap；
- 是否把具体 case 提升成重复 property；
- 是否把多个独立风险错误合并；
- candidate-facing 问题是否真实指向 future Ascend C candidate；
- target-hardware、numerical、execution、安全和 performance concern 是否被静默省略；
- novel blind discovery 是否因 catalog anchoring 被无证据删除；
- evidence independence、shared dependency 和 unknown 是否诚实保留。

被拒绝时，Controller 把 exact review feedback 路由给新的 Scope Consolidation revision。blind proposal 本身仍不修改。
外层 revision loop 是机械编排，不是嵌套 Agent Loop。

## 12. Property、Case 与 Mechanism 分层展开

Scope accepted 后才进入详细 Oracle Exploration：

1. `OraclePropertyV1` 表达一个独立、candidate-facing 的可判定性质；
2. `OracleCaseV1` 表达边界输入、concrete example、mutation、metamorphic transformation 或 workload slice；
3. `OracleMechanismV1` 表达如何绑定 candidate、输入、执行、observation、comparator 和 receipt。

多个 cases 不自动产生多个完整 Developer/Reviewer Agent Loops。只有当两个 cases 需要不同 acceptance semantics、不同
capability、不同 failure interpretation 或不能共享 mechanism 时，才可以被提升为独立 property/item。

Mechanism 在进入模型 Review 前应尽可能通过 typed compilation/mechanical validation 检查：

- candidate artifact 和 ABI binding；
- input/source/output allocation；
- shape、layout、pitch、alignment 和 launch domain；
- observation 是否真实生成；
- comparator 和 pass/fail mapping；
- Worker capability、environment 和 target identity；
- output bounds、diagnostic limits 和 receipt lineage。

机械完整性不能替代语义 Review；它的作用是让 Reviewer 聚焦“这个 mechanism 是否真的证明 property”，而不是反复
发现可以由类型或编译器拒绝的缺字段和无效 binding。

每个最终 property 仍经历独立 Developer → Reviewer → feedback → revision loop。当前证据不足以安全实行“低风险 item
免审”；风险自适应 Review 必须等待新的对照实验。

所有 required properties 闭合后，Controller 仍必须执行一次完整 Portfolio Coherence Review，检查 cross-property gap、
冲突、重复、shared dependency 和 capability mismatch。随后由普通 capability-matched Workers 执行 qualified honest、
correct-variant、targeted-mutant 和 hidden disjoint controls，最后交给 model-free Oracle Admission。Scope Review 或 item
Review 的同意不能跳过 coherence、controls 或 Admission。

## 13. Evidence 与 Worker 设计

Blind、consolidation、property development 和 Review 阶段可以按 treatment policy 请求 Worker evidence，但每个请求必须
绑定：

- exact uncertainty、mapping challenge、property 或 review finding；
- competing predictions；
- observation 将改变的 decision；
- required capability；
- execution/cost bound；
- requesting role、episode、scope revision 和 evidence class。

Worker capability 与 evidence class 至少区分：

- host/reference arithmetic；
- CPU 或 framework reference；
- CUDA compile；
- CUDA execution；
- Ascend C compile；
- Ascend simulator/emulator；
- Ascend 950PR execution；
- profiler/performance measurement。

普通 POSIX shell Worker 的 receipt 不能被描述为 CUDA 或 950PR evidence。重复实验可以保留为 `Confirmatory`，但不能
被计作新的独立 evidence。实验失败、Worker 不可用和 scheduling failure 不改变 semantic hypothesis 或 applicability。

每个 receipt 只投影给 exact authorized lineage。proposal roles 不能读取 hidden challenge、sibling item receipt 或
Admission-only material。

Intent Admission 已在本流程之前完成，因此 scope 阶段新产生的 observation 不会倒灌并改写 admitted Intent。Controller
将其归档为 claim/scope-revision-bound `OracleScopeEvidenceObservationV1`，标明 exploration-only、qualification-eligible
或需要重新进入 SIR/Intent Admission 的用途。若 observation 暴露 admitted Intent 本身存在歧义，workflow 必须创建新的
SIR/Intent lineage，不能由 Scope Consolidator 擅自改变 claim。

## 14. Skill 与 Knowledge 的阶段性可见性

盲阶段可以使用 task facts、hardware facts、API documentation、debugging method 和通用分析 skill，但这些资源不得包含
或复述 policy concern catalog。Controller 必须为资源声明用途，例如：

- `BlindDiscoverySafe`：不含 Oracle taxonomy，可在盲阶段读取；
- `PolicyChallengeOnly`：包含 coverage checklist，只在 blind proposal 冻结后授权；
- `AdmissionRestricted`：proposal roles 永不可见。

这三种 authority 必须是不同强类型，不能由 metadata 字符串或命名约定决定。知识条目仍只有 exploration authority，
不会因为在 challenge 阶段出现就获得 Admission authority。

任务不要求存在 PyTorch 或其他 framework。若存在独立 reference，它可以成为 evidence provider；若不存在，scope
discovery 仍必须基于 CUDA、caller、target、tests、外部规范和可执行 probe 工作。

## 15. 持久化、恢复与可观察性

以下 artifact 和 transition 必须 durable：

- blind scope proposal 及其 visibility manifest；
- coverage challenge；
- 每项 disposition 和 coverage mapping；
- consolidated obligation graph revision；
- scope review 和 exact feedback；
- evidence request、job、receipt 和 projection；
- property/case/mechanism revision；
- portfolio coherence 和 Admission outcome。

Controller restart 后必须恢复当前阶段的 exact information boundary。它不能把已经可见的 policy catalog 注入一个恢复中的
blind episode，也不能让旧 blind continuation 在 challenge 后继续冒充未受锚定的响应。

安全日志只记录 task、stage、role、episode、proposal、challenge、obligation、revision、job/receipt identity、计数、
状态和失败分类。不记录源码、prompt、模型正文、Worker stdout/stderr、hidden content、credential 或用户敏感数据。

## 16. 推荐的提示时机

默认产品方案选择：

> Blind Discoverer 提交维度、触发证据、风险、关系和 evidence plan 后立即冻结并进入 policy challenge；不要等待它
> 为所有自选维度完成详细 Oracle 分析。

理由：

- 已经得到不受 catalog anchoring 的可审计信号；
- 避免在不完整或错误 scope 上投入大量 Developer/Reviewer 成本；
- blind-discovered 与 policy-supplemented concern 可以接受相同深度的后续分析；
- 对照实验能清楚区分自然发现和系统补充；
- 不会因为先分析的维度获得更多 token/experiment 而制造不对称质量。

“完整自选分析后再提示”保留为研究 treatment，不是默认产品路径。D2 必须在 policy catalog 可见前额外冻结
`BlindDetailedScopeAssessmentV1`，其中包含每个自选 dimension 的 provisional decision、完整 evidence、实验 receipt 和
remaining unknown；challenge 后的新结论只能形成新的 consolidation artifact，不能覆盖该 blind assessment。

## 17. 消融实验设计

方案 D 同时改变 scope 顺序、全局合并和 evidence 使用方式，不能只跑一次后与 A/B/C 数值比较并宣称优胜。建议分两步：

### 17.1 拓扑消融

- `B`：当前逐 dimension/item discovery + Review；
- `D1-no-evidence`：blind scope → policy challenge → global obligation review → per-property Review。

两组使用相同代码、模型、prompt revision、任务、target、skill/knowledge、预算、Worker capability 和 hidden evaluator。
用于隔离 Oracle topology 的主实验还必须复用 exact admitted Intent contract、evidence snapshot 和 upstream SIR receipts；
目的只估计 global blind-first topology 对遗漏、重复和成本的影响。完整端到端 CLI 路径另做 confirmatory experiment，
不能把不同 SIR 输出造成的差异归因给 D。

### 17.2 Evidence 消融

- `D1-no-evidence`；
- `D1-with-evidence`：拓扑相同，仅增加 proposal-visible typed Worker evidence。

目的估计 evidence capability 是否发现新缺陷、减少错误 revision，或增加冗余/低信息量实验。
`no-evidence` 只表示 scope/property proposal roles 看不到新请求的 Worker observation；所有 arms 的 hidden evaluator、
qualification controls 和 Admission Worker authority 必须相同，不能用关闭验证来制造低成本。

### 17.3 提示时机研究

- `D1`：提交 blind scope prospectus 后提示 policy concern；
- `D2`：完成自选维度的详细判断后才提示。

D2 用于研究完全自主分析能力和 anchoring，不作为默认产品路径。所有 treatment 必须随机顺序、多任务、多次重复，并使用
同一 common hidden evaluator。

### 17.4 指标

下面是设计层必须覆盖的指标族；精确定义、分母、失败样本和聚合规则见
[`D_MEASUREMENT_PROTOCOL.md`](../experiments/reasoning-decomposition-ablation/D_MEASUREMENT_PROTOCOL.md)。该协议必须在
正式运行前与 hidden evaluator manifest 一起冻结，不能在看到 arm 输出后选择指标或改变 semantic equivalence rubric。

Primary outcomes 必须优先回答：

- required obligation 的 evaluator-qualified coverage；
- 对 hidden invalid variants/challenges 的 false acceptance；
- 对 correct variants/honest controls 的 false rejection；
- mechanism 是否在要求的 CUDA/Ascend/950PR capability 上真实执行；
- 只有前三项满足预注册 correctness gate 后才比较 cost/efficiency。

Blind discovery 与 anchoring diagnostics 至少记录：

- `blind discovery recall`：最终必要 concern 中由 blind 阶段自主发现的比例；
- `blind discovery precision`：blind dimensions 中最终被证明独立且必要的比例；
- `supplement dependency`：关键 concern 依赖 policy challenge 才出现的比例；
- `novel discovery`：catalog 外但 evaluator 证明有价值的 concern；
- `supplement over-adoption`：看到 catalog 后无 task evidence 地新增独立 obligation 的比例；
- `merge rate`：多个 dimensions 被共享 obligation 覆盖的比例；
- `case inflation`：具体 case 被错误提升为独立 property/item 的数量；
- `anchoring displacement`：challenge 后无证据删除或扭曲 blind discovery 的数量；
- taxonomy/exposure violation：blind episode 意外看到 policy/hidden material 的次数，目标必须为零。

Decomposition、Review 和 evidence diagnostics 至少记录：

- blind dimensions、policy instances、consolidated obligations、properties、cases 和 mechanisms 的逐级数量与压缩率；
- semantic overlap、错误合并、错误拆分和 consolidation 后 coverage hole；
- Reviewer finding precision/recall、每次 revision 的 repair、regression、escape 和 no-new-information rate；
- experiment 的 decision-changing、discriminating、confirmatory、redundant、ambiguous、invalid-capability 和 execution-failure
  分类；
- evidence independence、capability class，以及每个 evaluator-confirmed defect 的 model/Worker cost。

Operational 与稳定性 diagnostics 至少记录：

- 按 stage 分解的 model input/output/cache tokens、dispatch、episode、tool call、Worker/device time、wall time 和人工决策；
- 每个 evaluator-qualified obligation 和每个 confirmed defect 的归一化成本；
- 多 seed/repetition 的 semantic obligation-set stability、outcome variance 和 order effect；
- fail-closed、unknown、provider failure、execution failure、operator interruption 和 Admission lifecycle outcome。

这些指标必须由 frozen artifact、receipt 和 hidden evaluator 重算，不能由 proposal Agent 自报。

## 18. 主要风险与防护

### 18.1 模型提交 generic 大清单

防护：每个 blind dimension 必须有 exact trigger、candidate risk、domain 和与相邻维度的区别；Scope Review 合并无信息量
label。不要用很小的数量上限掩盖问题。

### 18.2 challenge 后盲目接受所有系统维度

防护：每个 concern 必须采用强类型 disposition 并绑定 evidence；独立 Scope Reviewer 检查无依据的
`AdoptAsIndependent` 和矩阵填充。

### 18.3 challenge 后声称原 dimension 已覆盖一切

防护：blind artifact 不可修改；每条 mapping 都是独立 content-addressed edge，并由 Coverage Auditor/Scope Reviewer
检查。

### 18.4 固定 catalog 压制 novel discovery

防护：catalog 是 coverage floor，不是允许列表；novel blind dimension 只能经有证据的 merge/not-applicable/rejection
处理，不能因没有 policy ID 自动删除。

### 18.5 通过 skill/knowledge 泄露 taxonomy

防护：阶段性强类型 exposure authority、冻结资源 snapshot，并测试 blind role 不能读取 challenge-only 条目。

### 18.6 把 Worker 存在误当成强证据

防护：精确 capability/evidence class；shell、CUDA、Ascend compile、950PR execution 和 hidden controls 不能互换。

### 18.7 新增 scope roles 反而增加成本

防护：scope roles 只运行一次或少量 revision，且不生成详细 plans；用最终独立 obligation 数、evaluator-qualified coverage
和总成本评估，而不是用 episode 数证明成功。

## 19. 非目标

方案 D 不意味着：

- 让模型自由删除系统认为必要的 concern；
- 让 coding agent 根据 fixture 手工补维度；
- 用名称相似度自动决定语义覆盖；
- 把每个 concern、case 或输入都变成独立 Agent Loop；
- 用多个模型同意替代 executable Oracle qualification；
- 把 CUDA source behavior 当作默认 truth；
- 要求任务必须包含 PyTorch；
- 在当前 pre-release V1 中引入兼容层或新格式版本；
- 因一次 pilot 直接替换现有规范和生产 workflow。

## 20. 实施前最小切片

若用户确认进入实现，应先完成一个只到 accepted obligation graph 的窄切片，不立即重做全部 Oracle：

1. 定义 blind proposal、policy concern、challenge、mapping、disposition 和 obligation 的 distinct V1 types；
2. 在 blind episode 前冻结 sealed task concern ledger 和 challenge exposure manifest；
3. 建立 Blind Discoverer 与 Coverage Auditor 的不同 visibility/capability policy；
4. 持久化 blind freeze 和 challenge lineage，验证 restart 不跨越信息边界；
5. 实现 complete concern ledger 的机械闭合检查；
6. 实现独立 Scope Review/revision；
7. 用现有正常 CLI/server/workflow 路径输出 accepted obligation graph；
8. 对一个已用 pilot 和一个语义明显不同的未知任务做人工审查；
9. 通过该 gate 后再把 obligation graph 接到 property/case/mechanism 和现有 item loops。

该切片不需要知识库、fixture-specific rule、Candidate 生成或测试旁路。

## 21. 设计验收标准

方案 D 只有同时满足以下条件才算正确实现：

- blind episode 的可见内容中不存在 policy catalog 或等价 taxonomy 泄露；
- sealed policy catalog/ledger 和 challenge manifest 在 blind episode 前已经冻结，且不能根据 blind output 事后改变；
- blind proposal 在 policy challenge 前 durable freeze；
- policy inventory 的每个 concern 在最终 ledger 中恰好有一个 typed disposition；
- novel blind discovery 不会因 catalog 无对应项而丢失；
- distinct admitted claim identities 可以共享 obligation，但 identity 和 coverage lineage 不丢失；
- cases 不会仅因输入不同机械扩张为独立 item；
- required unknown、execution failure 和 not-applicable 不会互相冒充；
- Scope Reviewer、Worker 和 Admission authority 相互独立；
- all proposal-visible receipts 与 exact requesting lineage 绑定；
- final properties 和 mechanisms 面向 future Ascend C candidate 与 exact 950PR context；
- hidden evaluator/material 对所有 proposal roles 保持不可见；
- 全流程经正常 `cairn-cli → cairn-server → migration app API → CudaMigrationWorkflow` 运行；
- 至少一个语义明显不同的任务不需要产品代码或 prompt 特殊分支。

## 22. 尚待实验回答的问题

- Blind Discoverer 的一次 scope episode 是否足够，还是需要独立 blind Reviewer 后再 freeze？
- Coverage Auditor 默认读取完整 sealed task ledger；catalog/ledger 的摘要和 progressive disclosure 是否能在不破坏
  完整枚举与 exact identity 的前提下降低上下文成本？
- Scope Consolidator 应复用 Blind Discoverer 的模型配置，还是使用独立模型/episode 以降低自我辩护？
- 什么证据足以支持 `NotApplicable`，哪些理由可以机械验证？
- property/case 的提升规则可以有多少机械约束，多少仍需模型 Review？
- typed mechanism compiler 能消除多少 B/C 中的 setup findings？
- D1 相比 B 是否在不降低 hidden evaluator coverage 的前提下降低重复和总成本？
- D2 的额外无锚定分析是否产生足够 novel discovery，值得其成本和不对称性？

这些问题必须通过 frozen treatment、真实 runtime model、普通 Worker 和 common hidden evaluator 回答，不能由设计文档
中的直觉直接关闭。

## 23. 完整性核对

本方案已经显式覆盖：

- blind 阶段隐藏什么、不能隐藏什么；
- policy catalog/ledger 在 blind 前 sealed commitment、blind 后才可见的时序；
- 默认补充提示时机与完整盲分析的实验变体；
- blind proposal 的内容、冻结和恢复；
- policy catalog、Coverage Auditor 和语义 mapping 的边界；
- 每个 policy concern 的 typed disposition；
- concern split、blind proposal rejection 和 policy requirement 不可由模型降级；
- novel discovery 的保留；
- claim identity 与共享 coverage 的关系；
- global obligation graph 与独立 Scope Review；
- property、case、mechanism 的分层；
- mechanical completeness 与 semantic Review 的分工；
- Worker capability、evidence、failure 和 lineage；
- scope evidence 不倒灌修改 Intent 的反馈路由；
- skill/knowledge taxonomy 泄露防护；
- performance、unknown、not-applicable 和 fail-closed 语义；
- persistence、日志、安全和 Admission authority；
- D1/D2 及 topology/evidence 消融；
- 指标、风险、非目标、最小实施切片和验收 gate。

因此本文档表达的不是“先让模型选维度，再只做它选中的维度”，而是：

> 先保存模型不受内部 taxonomy 锚定的独立发现，再用完整产品 policy 挑战遗漏，最后把两者合并为有 provenance、
> 无静默缺口、尽可能不重复的 candidate-facing Oracle obligations。
