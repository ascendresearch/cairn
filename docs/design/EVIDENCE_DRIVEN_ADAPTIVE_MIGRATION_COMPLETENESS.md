# 方案 E 设计完整性审计

- 审计对象：[`EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md`](EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md)
- 审计日期：2026-09-01
- 结论：已保留前序产品定位、A/B/C 实验发现、方案 D 防护、SIR/Oracle 第一性原理复盘和本轮逻辑修正；未把候选设计
  误报为当前实现或已验证结论

## 1. 第一性原理结论核对

| 必须保留的结论 | 文档落点 | 核对结果 |
| --- | --- | --- |
| 必要的是 specification、reality、trust 三个问题的解决功能，不是固定阶段名称 | 2、4 | 已覆盖 |
| 模型进化很快，不能把旧论文或旧 workflow 当永恒前提 | 2、3 | 已转化为不依赖固定模型能力的设计 |
| “足够强的 runtime model”不是可靠架构假设 | 3、27.1 | 已明确否定 |
| 模型能力随 task、预算和上下文变化，不是二元属性 | 3、25.3、26.5 | 已覆盖并进入实验分层 |
| 结构应约束 authority/effect/evidence，而非固化全部认知步骤 | 2、6、9、16 | 已覆盖 |
| 无法证明时应 fail closed/abstain，而不是追求 workflow completion | 3、22 | 已覆盖 |

## 2. SIR 逻辑漏洞核对

| 本轮结论 | 文档落点 | 核对结果 |
| --- | --- | --- |
| 让模型先评估“是否需要 SIR”本身就在扮演 SIR | 4.1、10.1 | 已明确 |
| 不新增 `ShouldStartSir`、readiness assessment 或 skip Reviewer | 10.1、27.3、30 | 已明确禁止 |
| 源语义理解是每次迁移不可避免的认知功能 | 4.1 | 已覆盖 |
| 可选的是独立、持久、需实验/用户确认的 focused SIR protocol | 4.2、10.2 | 已覆盖 |
| 是否物化 SIR 来自实际迁移推理中的 material semantic fork | 10.2 | 已覆盖 |
| Controller 不通过代码特征机械解释语义 | 6.1、10.2、16 | 已覆盖 |
| 无 focused SIR 时仍必须在 qualification 前提交统一 intent proposal | 10.3、11 | 已覆盖 |
| direct path 与 focused SIR 不产生两个下游 contract 或兼容路径 | 10.3 | 已覆盖 |
| 后续 evidence 可以暴露新歧义并进入新 Intent lineage | 10.2、20.3 | 已覆盖 |
| 没有独立 specification 时不能保证发现所有真实意图 | 10.4、27.1 | 已诚实保留不可识别性 |

## 3. Oracle 与 Candidate 关系核对

| 结论 | 文档落点 | 核对结果 |
| --- | --- | --- |
| 验证功能必要，但 complete Oracle 不必严格早于第一个 Candidate | 2、14 | 已覆盖 |
| Candidate-specific ABI/layout/compiler/performance 事实可能只能由候选暴露 | 13 | 已覆盖 |
| early Candidate 必须是无发布 authority 的 exploratory artifact | 13、30 | 已覆盖 |
| Oracle 应作为持续演化的 Assurance 子图 | 12、14 | 已覆盖 |
| Candidate 和 Oracle 可以共同演化，但不能互相迎合 | 14、19、20.3 | 已覆盖 |
| comparator/semantics revision 必须有独立依据并使旧 epoch 失效 | 14、19、27.4 | 已覆盖 |
| final validation bundle、Oracle Admission 和 Candidate Admission 仍不可省略 | 4.3、19、20 | 已覆盖 |
| Oracle 必须面向 future Ascend C candidate，不能只审查 CUDA 源码 | 14 | 已覆盖 |

## 4. 方案 D 与 blind-first 结论核对

| D 中必须继承的防护 | 文档落点 | 核对结果 |
| --- | --- | --- |
| 首轮不以内部维度 taxonomy 锚定模型 | 6.3、8、15.1 | 已覆盖 |
| catalog/derivation/exposure 在首 episode 前 sealed | 8、15 | 已覆盖 |
| admitted claims 尚未产生时不能伪造 task ledger | 8 | 已改为预先 seal derivation policy，后续机械实例化 |
| policy challenge 关闭模型不知道的遗漏 | 15.2 | 已覆盖 |
| catalog 是 coverage floor，不压制 novel discovery | 15.2 | 已覆盖 |
| concern 使用 typed disposition，不强制每项生成独立 item | 15.2 | 已保留全部八类 disposition |
| global consolidation 与 property/case/mechanism 分层 | 15.3、16、27.7 | 已覆盖 |
| required unknown、execution failure 与 not-applicable 不混淆 | 6.4、15.2、20、22 | 已覆盖 |
| D 既是独立对照，也是 E 的 full fallback | 15.3、24、25.2 | 已明确 |
| release 时模型不能自行跳过 policy challenge | 15.3、28 | 已明确 |

## 5. 产品定位与用户输入核对

| 已确认产品要求 | 文档落点 | 核对结果 |
| --- | --- | --- |
| 优先生成特定硬件亲和 Ascend C | 5.2 | 已覆盖 |
| 首个目标是 Ascend 950PR（3510） | 5.4 | 已覆盖 |
| 不把模板库或高层算子覆盖率当产品能力上限 | 5.2、28 | 已覆盖 |
| 输入不要求 PyTorch | 5.3 | 已覆盖 |
| 有 PyTorch/reference 时可作为 Oracle evidence 之一 | 5.3、18 | 已覆盖 |
| 不为输入 CUDA 代码质量担保 | 5.1 | 已覆盖 |
| source behavior、desired intent 和 candidate acceptance 必须分开 | 5.1、11、14 | 已覆盖 |
| 最终输出是开发者可采用的 migration package | 23 | 已覆盖 |

## 6. 前序 SIR/Oracle 要求核对

| 要求 | 文档落点 | 核对结果 |
| --- | --- | --- |
| SIR/assurance 可通过 Worker 验证猜想，尤其 CUDA Worker | 10.2、18.1 | 已覆盖 |
| SIR/intent evidence 可传递给 Oracle/assurance | 11、12 | 已通过 shared graph 与 admitted snapshot覆盖 |
| Oracle 获得 exact target platform context | 8、19 | 已覆盖 |
| previous feedback exact、typed、content-addressed | 8、12、17、21 | 已覆盖 |
| skill/knowledge 供模型按需调用，但不产生 authority | 18.2 | 已覆盖 |
| correctness、numerical、performance 及 integration/safety/adequacy 独立 | 6.4、20、23 | 已覆盖 |
| performance 始终有 disposition，不能补偿 correctness | 6.4、20.2 | 已覆盖 |
| 真实外部 effect 只通过 capability-matched ordinary Worker | 18.1 | 已覆盖 |
| 不恢复 Proposal Host 或专属 proposal binary | 18.1、28 | 已覆盖 |

## 7. Authority、强类型与运行边界核对

| 边界 | 文档落点 | 核对结果 |
| --- | --- | --- |
| coding agent 不解释 fixture 替代 runtime model | 1、25.4、28 | 已覆盖 |
| Controller 是唯一 workflow writer | 6.1、7、9 | 已覆盖 |
| cairn-server 业务无关、migration app composition、ports 非独立进程 | 7、8 | 已覆盖 |
| focused roles 使用真实 Agent Loop，外层 revision/experiment/control 机械编排 | 7、9 | 已覆盖 |
| model/Reviewer 只有 proposal authority | 3、6.1、17 | 已覆盖 |
| Worker 只执行不判意图 | 6.1、18.1 | 已覆盖 |
| Admission model-free | 6.1、20、30 | 已覆盖 |
| hidden evaluator/material 与 proposal roles 隔离 | 6.1、14、19、20 | 已覆盖 |
| semantically distinct identity/state 使用强类型 | 12 | 已覆盖 |
| deserialization 重跑 invariant、compile-fail 和不合并强类型 | 12 | 已显式覆盖 |
| durable continuation、effect、receipt、revision 和 epoch | 21 | 已覆盖 |
| 安全日志不含 source/model body/stdout/stderr/credential | 21 | 已覆盖 |
| 实验只保存 token usage 计数，不保存 token/prompt/response 内容 | 21、26 | 已覆盖 |
| 正常 CLI/server/app/workflow/Worker 路径 | 8、25.4、30 | 已覆盖 |
| pre-release V1 不建兼容层或新内部格式版本 | 1、28 | 已覆盖 |

## 8. Evidence 与失败语义核对

| 要求 | 文档落点 | 核对结果 |
| --- | --- | --- |
| Evidence/Assurance Graph 不持久化全部思维链 | 12.1 | 已覆盖 |
| observation、claim、candidate、mechanism、receipt 不共用 generic ID | 12.2 | 已覆盖 |
| support/refute/dependency/contamination/revision edge 分开 | 12.3 | 已覆盖 |
| host/CUDA/Ascend compile/950PR capability 不互换 | 18.1、20、30 | 已覆盖 |
| Worker request 必须说明 competing predictions 和 decision | 18.1 | 已覆盖 |
| Candidate/Oracle/intent/platform/execution failure 分路反馈 | 16、17、20.3 | 已覆盖 |
| Oracle control 四类失败保持强类型 | 20.1 | 已覆盖 |
| hidden challenge 不复用 public item，exit 31 进入 control reconciliation | 20.1 | 已覆盖 |
| diagnostic 只读 exact authorized receipt、16 KiB、拒绝 sibling/missing/over-limit | 20.1、21 | 已覆盖 |
| Candidate revision lifecycle 不以 latest/best 布尔值覆盖历史 | 20.4 | 已覆盖 |
| Oracle change cause、meta-qualification 与 parent/current 对称重测 | 20.5 | 已覆盖 |
| integrity、non-regression、improvement、comparative、independent qualification gates | 20.6 | 已覆盖 |
| 性能/精度 minimum practical improvement、同 epoch 与 Pareto family | 20.7 | 已覆盖 |
| hidden query budget、exposure state、retire-and-replace | 20.8 | 已覆盖 |
| 搜索状态、promotion 条件和 plateau/abstain 终止 | 20.9 | 已覆盖 |
| 弱模型、预算或 Worker failure 不放宽 Gate | 22 | 已覆盖 |
| partial/unknown/not-executed/abstain 是正常诚实终态 | 22 | 已覆盖 |

## 9. 指标体系核对

| 指标族 | 文档落点 | 核对结果 |
| --- | --- | --- |
| qualified coverage、false accept、false reject、capability closure | 26 | 已继承 D primary gates |
| correctness-gated cost | 26 | 已覆盖 |
| blind recall/precision、supplement dependency、novel/anchoring | 26.4 + D protocol | 已覆盖 |
| duplicate、case inflation、overmerge/split | 26.4 + D protocol | 已覆盖 |
| Review finding、repair、regression、escape、no-new-info | 17、26.4 + D protocol | 已覆盖 |
| evidence discriminating/decision-changing/redundant/capability | 18、26 + D protocol | 已覆盖 |
| authority/security/recovery zero tolerance | 26.6 | 已覆盖 |
| cost、latency、Worker/device、人力 | 25、26 | 已覆盖 |
| developer auditability、trace 和 replay | 23、26.3 | 已覆盖 |
| time-to-first-running 与 time-to-package | 26.1 | 已新增 |
| SIR materialization、missed ambiguity、late reopen | 26.2 | 已新增 |
| graph churn、cross-feedback、epoch invalidation、co-adaptation | 26.3 | 已新增 |
| promotion validity、symmetric replay、hidden replacement closure | 26.3、26.7 | 已新增 |
| escalation value 与 full D fallback 收益 | 26.4 | 已新增 |
| model capability/task difficulty interaction | 25.3、26.5 | 已新增 |
| E-specific analysis units、分母、时间起点和 first-cause 去重 | 26.7 | 已新增并可预注册 |
| incomplete/failure intention-to-treat | 26 + D protocol | 已继承 |

## 10. 消融实验完整性核对

| 实验要求 | 文档落点 | 核对结果 |
| --- | --- | --- |
| D 与 E 正面对照 | 24、25.2 | 已覆盖 |
| E late challenge 与 full-D fallback 可分离 | 25.2 | 已覆盖 |
| organic-only 只作诊断，不取得产品 release 资格 | 25.2 | 已明确 |
| 不只测试最强模型 | 25.3 | 已明确能力梯度 |
| task 包含无 framework 和有 reference 的不同类别 | 25.3 | 已覆盖 |
| 同一正常 product path | 25.4 | 已覆盖 |
| generous、bounded budget，不一点点试 | 25.4 | 已覆盖 |
| 多 repetition、随机顺序、common hidden evaluator | 25.4 | 已覆盖 |
| runtime model 真正读取未知任务，coding agent 不代答 | 25.4 | 已覆盖 |
| 至少两个语义明显不同任务 | 25.4、30 | 已覆盖 |
| E 必须有真实 Candidate consumer，不能只输出 Oracle JSON | 29 | 已覆盖 |

## 11. 写后反向检查发现并修正的问题

1. **“SIR 可选”可能只是一场命名游戏**：正文现已明确 source understanding 每次都会发生；可选的是 focused SIR
   protocol，mandatory intent contract 只有一个下游语义。
2. **task ledger 无法在 Intent 之前具体化**：现区分 pre-task sealed catalog/derivation policy 与 post-Intent mechanically
   instantiated ledger，避免既伪造 future claims 又允许事后挑选 policy。
3. **弱模型可能自行跳过全部防护**：late policy challenge 是 release 固定边界，不由模型 confidence 关闭；严重 gap 可
   full D fallback。
4. **Candidate/Oracle 共设计可能循环迎合**：新增 immutable Qualification Epoch、hidden controls 和 revision invalidation。
5. **early Candidate 可能被误报为交付成功**：新增独立 `Exploratory` lifecycle/authority 与明确 terminal outcomes。
6. **Controller 的“自适应”可能偷偷解释语义**：升级表只允许其响应 typed gap、receipt、Gate、budget 和 capability；
   semantic attribution 仍由 runtime roles 提案。
7. **E 可能退化成一个超长单 Agent**：Reasoning Kernel 被定义为可替换 episodes；只持久化有 consumer 的图状态，关键
   Review 仍可使用 fresh episode。
8. **E 实验可能只比较漂亮文档**：实施切片要求真实 exploratory Ascend C artifact 和 ordinary Worker build/run；没有
   950PR 能力必须诚实阻塞。
9. **只测最强模型会掩盖架构脆弱性**：新增 model/budget/context capability strata 与交互指标。
10. **已有 D 指标可能漏掉共设计价值与返工**：新增 time-to-first-run、SIR materialization、graph churn、cross-feedback、
    epoch invalidation 和 escalation-value 指标，并在 26.7 固定 E-specific identities、候选公式、时间边界、ground-truth
    要求、去重和 intention-to-treat 规则。
11. **E 的 blind result 可能无法重算**：新增 `OrganicAssuranceConcernV1`；它只在实际迁移出现跨 episode consumer 时
    物化，必须带 trigger/risk/domain/evidence，且 challenge 后不能回填，从而不重新引入预先列维度的 stage。
12. **“下一版更好”没有正式语义**：新增 Candidate lifecycle 和五层 Promotion Gates；latest revision 不自动替换 parent，
    improvement 必须预声明并在同一 epoch 比较。
13. **Oracle 改版可能帮助当前 Candidate 作弊**：新增六类 Oracle revision cause、独立 meta-qualification 和 symmetric replay；
    无独立依据的 Candidate accommodation 直接拒绝。
14. **反复 hidden Gate 查询会把 holdout 变成训练集**：新增 control exposure state、formal query budget、粗粒度反馈和
    retire-and-replace；已给出详细诊断的 control 不再计作 hidden。
15. **性能/精度可能通过挑 workload 或调 tolerance 伪造提升**：新增 minimum practical improvement、冻结统计 policy、
    required non-regression、domain-bound specialization 和 Pareto candidate family。

## 12. 有意保留的开放问题

正文第 31 节保留了 knowledge taxonomy exposure、Intent Admission 时机、full-D escalation threshold、co-adaptation、epoch
粒度、Reviewer配置、真实任务 SIR ground truth、late/early challenge、Graph 最小持久化和弱模型行为等问题。它们缺少实验
依据，不应在实现时凭便利静默决定。

## 13. 最终防丢结论

方案 E 没有删除语义理解、Oracle 或结构化 Review，也没有把系统托付给一个“足够强的模型”。它删除的是所有任务固定
相同的认知时序：不再先做一个 mini-SIR 判断是否做 SIR，不再要求完整 Oracle 永远早于第一个 Candidate。

最终保留的完整结构是：runtime model 在真实迁移中自然提出语义、证据、validation 和 exploratory Ascend C candidate；
material semantic fork 才物化 focused SIR；Evidence/Assurance Graph 让 source、target、candidate 和 assurance 共同演化；
发布前固定执行 sealed-policy challenge，必要时进入完整 D；Development/Qualification Oracle 分离，Oracle changes 使用 typed
cause、meta-qualification 和 parent/current symmetric replay；Candidate 不因 latest 自动晋升，而是在同一 immutable epoch
通过五层 Promotion Gates、predeclared performance/precision improvement 和 bounded hidden-query controls。ordinary Workers、
hidden retire-and-replace、model-free Admissions 共同决定能否交付 exact 950PR migration package。
