# Oracle 探索知识库与 Skill 信赖设计

- 状态：规范性目标设计
- 日期：2026-08-27
- 父设计：[系统设计](../SYSTEM_DESIGN.md)
- Agent 软件架构：[Agent 与 Strategy](../design/AGENT_ARCHITECTURE.md)
- 参考设计：`/data/projects/ascend-factory` 的知识/skill 生命周期；本文件给出 Cairn 的适配结论
- Oracle 调研依据：[Oracle 自动生成调研](ORACLE_RESEARCH_REPORT.md)、
  [可借鉴方向](BORROWABLE_DIRECTIONS.md)

## 1. 目的

Oracle Explorer 和语义意图恢复需要查询 CUDA、Ascend C、算子语义、数值分析、历史故障、
microbench、profiling 和模型接入经验，也需要按需加载分析/生成 skill。检索能力能扩大探索范围，
但检索结果和 skill 都不能因为“被系统加载”而变成正确性权威。

本子系统负责：

- 以 claim 为单位保存知识、证据和适用域；
- 管理知识与 skill 的生命周期、验证、撤回和影响传播；
- 为不同 agent role 提供有界、可审计的检索和加载能力；
- 把上一轮的有效反馈沉淀为可复用但不过度授权的事实或经验；
- 保证作者身份、来源层级、检索排名和模型信心都不等同于 trust。

## 2. 基本原则

1. **Author is provenance, never trust.** 官方、用户、模型、仓库作者都只说明来源。
2. **Trust is claim-scoped.** 一个文档整体“可信”不能授权其中每句话。
3. **Evidence must support the exact claim.** 有 receipt 不等于 receipt 支持当前结论。
4. **Retrieval is recall, admission is precision.** 搜索只找候选，独立准入决定能否用于权威判断。
5. **Cite, do not copy.** Oracle/意图/性能产物引用知识身份，避免复制后失去撤回传播能力。
6. **Content change invalidates validation.** 内容身份变化后重新审查，不继承旧 badge。
7. **Negative knowledge is first-class.** 失败条件、无效优化、工具盲区和反例与正向 recipe 同等重要。
8. **Unknown remains unknown.** 检索不到依据时不能用模型常识填成已验证事实。

## 3. 知识层级

借鉴 ascend-factory 的 T0–T3，但针对 Cairn 使用 claim-scoped 适用域：

| 层级 | 内容 | 典型例子 | 可支持的用途 |
| --- | --- | --- | --- |
| T0 | 机器提取或官方规格事实 | API/ABI、指令支持、硬件容量、工具字段定义 | 形成提案；经适用性校验后支持 contract/hardware fact |
| T1 | 带权威 receipt 的实测事实 | microbench ceiling、工具链行为、历史 bug 重现、设备限制 | 支持明确环境中的 observation claim |
| T2 | 经隔离归因验证的机制/recipe | 某类 tail 修复、搬运策略、数值 comparator 方案 | 指导搜索；不能直接证明当前候选 |
| T3 | 任务案例、轨迹和反馈 | 某 kernel 的候选演化、真实模型表现、失败反例 | 检索相似经验、产生回归义务 |

原则、政策和用户决策不应伪装成 T0–T3 事实。它们分别进入 `PolicyArtifact`、
`UserIntentDecision` 或设计规则，并拥有不同 authority。

### 3.1 Reference tier 与知识层级分离

知识层级说明材料形态，不自动说明语义权威。每条知识 claim 还需要 reference tier，例如：

- `AuthoritativeSpecification`；
- `IndependentReference`；
- `SourceBehavior`；
- `MeasuredTargetFact`；
- `SelfDerivedProposal`；
- `ProxyObservation`。

一项 T1 source behavior 仍然只是 CUDA 行为证据；一项 T0 官方理论峰值仍不能冒充实测 sustainable
ceiling。

## 4. 知识 claim 模型

一个 `KnowledgeClaim` 至少包括：

- claim kind 与结构化 statement；
- subject、适用 CUDA/Ascend 环境和 domain；
- provenance 与作者；
- evidence edges 和 dependency graph；
- reference tier、evidence strength 和独立性；
- 生命周期状态；
- freshness/revalidation triggers；
- 与其他 claim 的 supports/refutes/conflicts/supersedes 关系；
- 允许的 consumer role 与用途。

Claim 不存一个模糊 `trusted: bool`。同一材料可以支持某一窄 claim，而不支持更宽结论。

## 5. 生命周期

### 5.1 Knowledge claim

```text
Candidate
  → Reviewed
  → Admitted
  → Superseded
  → Retracted
```

- `Candidate`：可用于探索召回，显示未验证警告；
- `Reviewed`：结构、来源和安全已检查，但结论未被执行证据充分验证；
- `Admitted`：exact claim、domain 和 evidence 已通过适用的 admission；
- `Superseded`：新 claim 替代后保留历史引用；
- `Retracted`：被反例或证据失效推翻，禁止支持新权威结论。

撤回不是删除。所有引用该 claim 的 Oracle、意图、性能分析和历史 verdict 都需要反向影响审计。

### 5.2 Skill

Skill 的生命周期与知识 claim 分开：

```text
Unaudited → Reviewed → Validated → Refuted
```

- `Unaudited`：只能在最受限的探索 sandbox 中查看，默认不执行；
- `Reviewed`：已检查权限、数据流和指令边界，可用于探索但带未验证效果标记；
- `Validated`：具体能力 claim 已由 probe/receipt/recipe 支持；
- `Refuted`：效果或安全 claim 被反例推翻；保留审计材料但不进入默认菜单。

Skill author、仓库位置或“内置”标签都不授予 `Validated`。Skill 内容身份变化后回到 `Reviewed`，
因为 Cairn pre-release 不需要保留旧格式 reader；历史 run 仍引用当时精确内容身份。

## 6. Skill 能力与权限

Skill 是带指令、模板、工具使用方法和可选脚本的探索资产。它不拥有自己的权限，实际能力是
agent role、task policy、tool catalog 和 skill manifest 权限的交集。

能力至少分为：

- `ReadContext`：读取已授权 task artifact；
- `QueryKnowledge`：结构化/全文/语义检索；
- `ProposeArtifact`：生成 hypothesis、case、reference 或实验提案；
- `RequestExecution`：提出执行请求，但不直接获得执行 authority；
- `ExecuteSandboxedAnalysis`：运行低风险受限分析；
- `PrivilegedMeasurement`：使用 CUDA/NPU/外部服务，必须由独立 admission/approval 授权；
- `Adjudicate`：仅仓库受信 gate 可拥有，skill 永远不能自行声明。

未经验证的 skill 可以帮助探索以避免“必须先使用才能验证、必须验证才能使用”的死锁，但它：

- 不能支持 Intent/Oracle/Performance Admission 的关键 claim；
- 不能修改 comparator、hidden corpus 或 policy；
- 不能把输出写成 T0/T1；
- 不能获得超出调用 role 的网络、设备或秘密权限；
- 所有产物必须带 `UnverifiedSkillInfluence` provenance。

## 7. 检索架构

### 7.1 Progressive disclosure

推荐顺序：

1. 根据 task、operator family、claim kind、CUDA/Ascend 环境和 role 查询结构化索引；
2. 返回少量摘要、身份、信赖状态、适用域和冲突提示；
3. agent 明确选择后读取完整 claim/evidence 或 skill；
4. 需要时再读取原始 receipt、source 或大体积 artifact。

这样可以控制上下文成本，同时保证模型知道材料的信赖边界。不能只给正文而隐藏状态、适用域和
撤回信息。

### 7.2 检索方法

V1 可以从结构化字段、倒排/全文检索和内容文件开始。语义/vector retrieval 可以作为未来的
recall 优化，但必须满足：

- 排名不改变 trust；
- 返回结果仍需经过结构化 scope filter；
- query、index snapshot、embedding/model 和结果身份可重建；
- withdrawn/conflicting claim 不因相似度高而失去警告；
- admission 不依赖不可重建的“模型觉得相似”。

不应把当前不使用 vector RAG 固化成架构原则，也不应把引入 vector RAG 误认为知识质量提升。

### 7.3 查询结果

`KnowledgeQueryResult` 每项至少显示：

- claim/skill identity 和内容 identity；
- 匹配原因与 query scope；
- lifecycle/trust state；
- applicable domain/environment；
- evidence/reference tier；
- known conflicts、retractions 和 freshness；
- 是否允许当前 role 使用，以及仅可用于何种用途。

搜索 snippet 是展示，不是可执行或可准入 artifact。

## 8. 从反馈到知识

上一轮反馈不能直接写为通用 recipe。写回流程是：

```text
Raw feedback
  → typed observation
  → task-local claim
  → recurrence detection
  → crystallization proposal
  → evidence/admission review
  → T1 fact or T2 recipe
```

Crystallization 至少回答：

1. 该知识是否已经存在；
2. 它是否真正可跨 case/shape/operator 复用，还是过拟合；
3. 谁会在什么时候检索它；
4. 它的价值是否超过检索噪声和维护成本。

“跨两个项目”不应作为统一门槛。Cairn 只有 CUDA→Ascend C 范围，不同 claim 需要不同复现条件：
硬件勘误可能一次权威重现就足够，优化 recipe 通常需要多个独立 shape 或 kernel，数值规律可能需要
证明或穷举而不是次数。

被拒绝的 crystallization candidate 也应记录原因，避免反复提出同一个过拟合规则。

## 9. Admission 使用政策

不同用途允许的最低信赖强度不同：

| 用途 | Candidate/Reviewed | Admitted/Validated |
| --- | --- | --- |
| 生成搜索 query、假设或测试想法 | 允许，必须标注 | 允许 |
| 排序候选优化 | 允许，作为 prior | 允许，仍需本任务测量 |
| 形成 caller/intent refinement proposal | 允许，保留来源 | 允许，仍需适用域检查 |
| 支持 expected value/comparator/tolerance | 不允许单独支持 | 仅在 exact claim/domain 兼容时允许 |
| 支持 hardware ceiling | 不允许 | 需要 T0/T1 对应 claim 和环境 |
| 改变 admission policy 或 hidden corpus | 不允许 | 仍需独立政策变更流程 |
| 直接给 candidate verdict | 永不允许 | 永不允许；judge 读取本任务 receipt |

知识可以减少重复探索，不能替代本任务真实执行和准入。

## 10. 撤回与反向审计

当 claim/skill 被 refute、过期或适用环境变化时：

1. 记录新的反证和 adjudication；
2. 更新生命周期投影，不修改历史内容；
3. 反向遍历所有引用它的 intent、Oracle、performance claim、candidate verdict 和 T2 recipe；
4. 按 policy 标记 `RevalidationRequired`、`ScopeReduced` 或历史结论仍有效；
5. 默认检索排除已撤回材料，除非 role 明确请求历史/反例；
6. 保留为何曾被接受、为何被撤回的完整证据。

撤回传播依赖引用身份，所以产物应 cite claim ID，而不是复制一段自然语言结论。

## 11. 安全与 prompt-injection 边界

外部文档、知识正文和 skill 附带材料都是不可信数据。系统必须：

- 将仓库拥有的 role instruction 与 retrieved content 分层；
- 禁止检索内容提升自身权限或修改 tool schema；
- 对 URL、repository scope、文件路径、输出大小和许可证策略做有界控制；
- 不把秘密写入 query、event、CAS 或模型上下文；
- skill 脚本使用固定 sandbox、无默认网络、只读输入和独立输出；
- 将代码执行结果当 observation，不当语义 authority；
- 记录实际加载的 skill/knowledge 内容身份，而不只记录名称。

Hidden admission material 不进入普通知识索引、embedding/vector store、全文检索、skill asset 或
query-count facet。否则即使正文不可读，存在性、相似度、metadata 或检索排序也可能泄漏
coverage。Burned 的 case 可以按 policy 作为公开 regression 写入知识；sealed case 只能由 hidden
admission capability 按 exposure policy 使用。

## 12. 强类型边界

必须使用不同验证类型表示：

- `KnowledgeClaimId`、`KnowledgeEvidenceId`、`SkillId`、`SkillContentId`、
  `KnowledgeSnapshotId`、`QueryRunId`；
- T0/T1/T2/T3、reference tier、evidence strength 和 lifecycle state；
- proposed/reviewed/admitted/retracted claim；
- unaudited/reviewed/validated/refuted skill；
- role、capability、policy outcome 和 allowed use；
- supports/refutes/conflicts/supersedes edge；
- task-local observation 与 reusable knowledge。

反序列化必须重跑构造约束。静态测试应证明 reviewed skill 不能传给要求 validated skill 的权威
路径，T2 recipe 不能传给只接受 T1 measured ceiling 的接口，retracted claim 不能被普通 loader
返回为 active claim。

## 13. 首期范围

首期不需要复杂知识平台。足够的 V1 是：

- 内容寻址的结构化 claim 文件和 receipt 引用；
- 按 ID、operator family、claim kind、environment 和全文检索；
- role-scoped progressive disclosure；
- claim/skill 生命周期、content-change invalidation 和撤回传播；
- 从 Oracle/性能/真实模型反馈中形成 task-local observation；
- 一个明确的 curator/admission 流程。

首期简单的是检索实现，不是信赖模型。任何实现都必须保留 claim 级 provenance、适用域、冲突和
用途约束。
