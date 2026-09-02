# Cairn 评价与准入方法

- 状态：当前评价规范
- 日期：2026-09-01
- 作用：定义如何证明产品结果、搜索收益和 assurance 增量价值

本文不定义产品架构，也不保存历史 pilot 结果。运行原始数据属于 durable event/CAS 或独立 artifact bundle；只有冻结协议和
影响当前决策的汇总结论进入当前文档。

## 1. 必须回答的问题

评价按以下顺序回答：

1. Cairn 能否通过正常产品路径产生 native、running Ascend C？
2. 生成实现是否满足 intended contract，而不只是模仿 source behavior？
3. target-aware search 是否在 correctness-gated 前提下产生实际性能收益？
4. independent qualification 是否比简单 compile+allclose harness 少接受错误实现，同时不过度拒绝正确实现？
5. 结构化/adaptive workflow、target context 和搜索算法的收益是否超过 token、Worker、device 和人力成本？
6. 结果能否被另一环境按 exact manifest 重放并被开发者采用？

不能先用 Agent 数量、schema 完整性、文档长度、角色共识或单次漂亮轨迹回答这些问题。

## 2. 任务语料

### 2.1 纵向集成控制

首个 product closure 使用至少三个语义和实现结构明显不同的 task：

- elementwise/integration；
- reduction 或 numerical-sensitive operator；
- layout/indexing、atomic、stateful 或 concurrency operator。

至少一个 task 不依赖 PyTorch/reference，至少一个 task 有 independent reference 或 framework contract。它们共享 normal
CLI/server/app/workflow/Worker path，不能有 fixture-specific code、prompt、identity 或 expected output。

三个 task 只证明纵向集成，不证明总体能力。

### 2.2 Benchmark 阶段

首个 package 后建立 20-task development corpus，再扩展到 50–100 个 held-out tasks。Corpus 应覆盖：

- 不同 source repositories、authors 和 code styles；
- framework-free 与 framework-integrated inputs；
- elementwise、reduction、normalization、attention/fusion、layout、atomic/state；
- dtype、shape、alignment、tail、alias 和 workload variation；
- 有/无 independent reference；
- strong target baseline、仅高层 fallback 和无现成实现三种场景。

Task identity、split、target、model deployment、knowledge snapshot、budget 和 evaluator 在正式运行前冻结。

## 3. 评价隔离

- runtime proposal actor 只看到任务允许的 source、caller、public evidence 和 target context；
- hidden semantic expectations、mutants、holdout inputs、expected results 和 admission receipts 位于 restricted authority；
- repository coding agent 可以维护 evaluator，但不能把答案写入 production context；
- public failure diagnostic 暴露具体 hidden case 后，该 case 退休为 regression并补充新 holdout；
- sibling task/revision receipt、evaluation identity 和 hidden metadata 不可跨 lineage 读取；
- evaluator 与 candidate build/run 使用 exact artifact、ABI、environment 和 capability binding。

## 4. Baselines 与 treatments

### 4.1 产品结果 baselines

按任务可用性比较：

- target framework/vendor implementation；
- open-source 或 expert Ascend C implementation；
- 最简单可运行的 Cairn correctness baseline；
- direct frontier coding Agent + compile/allclose loop；
- high-level fallback，单独标识且不计为 generated Ascend C；
- 人工迁移时间和修改量，在可获得时记录。

不同硬件的裸 CUDA/Ascend latency 不作为公平 performance verdict。Target performance 必须相对同 950PR workload 下的有意义
baseline。

### 4.2 搜索 treatments

同一 task/budget 至少比较：

- best-of-N 或 greedy repair baseline；
- bounded population/beam search；
- structured actionable diagnostics on/off；
- target knowledge/context on/off；
- host tiling-only、kernel-only 和 coupled search，在任务适用时。

更复杂的 MCTS、MAP-Elites、meta-prompt evolution 或 learned experience 只有在控制变量实验中证明收益后进入默认策略。

### 4.3 Reasoning decomposition treatments

在首个 end-to-end package 之前不以 reasoning decomposition 消融阻塞产品闭环。闭环后比较：

- up-front structured：先完整分解 assurance obligations，再搜索 candidate；
- adaptive co-design：intent、assurance 和 exploratory candidate 共演进，release 前再完成 sealed policy challenge；
- adaptive + mandatory full-review fallback；
- organic-only 只作诊断，不具备 release 资格。

所有 treatment 保持相同 authority、normal path、target、hidden evaluator 和 release gates；改变的是 reasoning/search structure，
不是安全标准。

Reasoning/search 评价至少覆盖两个 model capability、budget 或 context strata，不能只测最强模型。能力较弱时分别观察 honest
abstention、focused investigation、full structured fallback 和最终 coverage，判断结构是在恢复能力还是只增加 ceremony。

## 5. Controls

### 5.1 Oracle adequacy

每个 applicable claim 使用适合的组合：

- honest/correct variants，测 false rejection；
- targeted mutants 和 negative implementations，测 false acceptance；
- wrong ABI/binding、no-launch、constant-output、fallback 和 harness-detection controls；
- public 与 hidden/disjoint input distributions；
- source-defect traps，检查是否错误迁移 source bug；
- metamorphic/property controls；
- independent reference、high-precision partial reference 或 formal/bounded proof。

Oracle accepted 只表示它在 exact epoch 有资格判断 candidate，不表示 candidate 通过。

### 5.2 Candidate outcomes

至少独立记录：

- native compilation 和 execution authenticity；
- semantic/algorithmic correctness；
- numerical acceptance；
- ABI/framework/integration；
- memory/state/concurrency safety；
- performance、workspace、resource 和 stability；
- supported domain 与 dispatch/fallback validity。

性能不能补偿前述 required outcome 的失败。

## 6. Metrics

### 6.1 Product metrics

- native build success rate；
- 950PR run success 和 qualified package rate；
- time-to-first-running、time-to-qualified、time-to-reviewable/mergeable；
- hidden/mutant detection 与 correct-variant acceptance；
- source defect 误迁移率；
- target baseline speedup、latency、throughput、workspace、memory、energy（若可测）和 variance；
- supported workload coverage 与 Pareto variants；
- 人工问题、人工修改量、最终采用或上游合入率。

### 6.2 Search metrics

- compile-valid、run-valid、correct 和 promoted candidate 数；
- best-so-far curve、time/evaluations-to-improvement、plateau 和 restart；
- parent diversity、invalid submission、dead-end 和 fallback incidence；
- profiler diagnosis 的 fire rate、action adoption 和 measurable hit rate；
- host tiling、kernel rewrite 和 coupled change 的边际收益；
- raw model tokens、turns、Worker time、device time、wall time 和 monetary cost。

### 6.3 Assurance metrics

- obligation/claim coverage 与 required unknown；
- false accept、false reject 和 evidence independence；
- public-only harness 与 independent qualification 的增量 defects；
- hidden exposure、retirement、replacement 和 query budget closure；
- Oracle change causes、candidate accommodation rejection 和 symmetric replay；
- late intent reopen、graph churn、qualification invalidation 和返工成本。

### 6.4 System metrics

- restart/replay success；
- duplicate external effect count，应为零；
- wrong capability、cross-lineage read、hidden leak 和 authority violation，应为零；
- log redaction violations，应为零；
- exact environment/artifact mismatch，应 fail closed。

### 6.5 Adaptive co-design 的分析单位与防事后解释

正式 D/E 或 adaptive-search 实验必须为以下语义使用不同 identity，不能折叠成 generic event、`item_id` 或“Agent call”：

- task run、intent fork、focused semantic lineage；
- search generation、iteration、episode、actor action 和 authorized state projection；
- organic concern、decision-changing observation 和 exploratory candidate revision；
- development/qualification Oracle revision、Oracle change cause；
- promotion claim/decision、control exposure、escalation decision 和 qualification epoch；
- obligation、property、case、mechanism、receipt 和 evaluator finding。

关键派生指标在运行前按以下语义冻结：

| 指标 | 分子 | 分母 |
| --- | --- | --- |
| material ambiguity recall | 在受影响 qualification 前正确物化的 evaluator-confirmed material forks | 全部 evaluator-confirmed material forks |
| unnecessary investigation rate | 仅凭打开时已可见 evidence 即可解决的 focused lineages | 全部 focused lineages |
| late intent escape rate | 首次在受影响 exploratory candidate 之后发现的 material forks | 全部 evaluator-confirmed material forks |
| candidate-revealed obligation yield | 首次由 candidate observation 揭示的 required obligations | 全部 evaluator-required obligations |
| decision-changing candidate evidence | 有 exact lineage 并改变 intent/assurance/candidate decision 的 observations | 全部成功执行且 proposal-visible 的 candidate observations |
| epoch invalidation rate | 被 freeze 后相关 revision 变化作废的 epochs | 全部创建的 epochs |
| escalation finding yield | 该 escalation level 首次发现的 unique confirmed defects | 该 level 的 model、Worker、device 和 human cost |
| qualified promotion validity | 满足全部 frozen hard gates 和 improvement claim 的 promoted revisions | 全部标记 qualified/preferred 的 revisions |
| symmetric replay closure | parent/current/all compared variants 在同一新 epoch 重放的 Oracle revisions | 用于 comparative promotion 的 Oracle revisions |
| hidden replacement closure | 下次 qualification 前已补 independent coverage 的 retired controls | 因详细反馈 retired-to-public 的 controls |
| decision-changing iteration yield | 改变 intent、assurance、candidate 或 stopping 的 iterations | 全部 completed durable iterations |
| qualification reentry rate | 失败 qualification 后经授权进入新 generation 的 attempts | 全部失败 qualification attempts |

这些 rate 不是单独的优化目标。例如 reentry 必须和 promotion validity、hidden exposure、replacement closure 和总成本一起解释；
高 escalation yield 也必须报告其绝对成本。

`time-to-first-compiling`、`time-to-first-running` 和 `time-to-package` 从 normal CLI submission accepted 的 durable timestamp
起算，包含 provider queue、Worker wait、失败尝试和人工等待，并另报 active model/Worker/device time。

Material fork 必须由 treatment-blinded evaluator、independent specification、authorized user decision 或可重放 counterexample
确认，且不同解释会改变 candidate、domain、comparator、ABI 或用户可见行为。无 ground truth 标为 `Indeterminate`，不能按成功
或失败补值。Organic 指标只能读取 policy challenge 前冻结的 concern；challenge 后内容不得回填。重复文字 finding 按 exact
first-cause lineage 去重；基础设施失败进入 execution completeness，不进入 semantic 成功分母。

Promotion 只按预声明 claim 计分。运行后挑出的最快 shape、metric 或 workload 只能标为 exploratory；Oracle revision 未完成
symmetric replay 时 outcome 为 `NotComparable`。

## 7. 实验纪律

- 同一比较冻结 task、source、target、toolchain、model deployment、prompt/profile、knowledge、tools 和 evaluator；
- 若 provider 无有效 seed，每个 task/treatment 做多次独立重复并随机化顺序；
- 同时报告 fixed-budget 和 natural-closure 两种视角；
- task 与 repetition 作为 paired block，报告分布、方差和置信区间，而不是只报 best run；
- provider、Worker、operator interruption、abstention 和 incomplete 都按 intention-to-treat 保留成本与停止位置；
- 没有创建 candidate、没有执行 control 或没有到 Admission 不能补成零缺陷或 success；
- baseline/treatment 的 fallback、library call 和 generated-kernel authenticity 使用同一检查；
- threshold、aggregation、outlier、censoring 和 minimum practical improvement 在运行前冻结。
- semantic matching、severity weighting、partial credit、analysis denominator、randomization 和 aggregation 在 manifest 中预注册；
- diagnostic 读取量、model/tool exposure、search/qualification feedback granularity 和 human intervention 同时记录。

## 8. 首个 package 的 acceptance gates

首个 `MigrationPackageAccepted` 必须同时满足：

1. runtime model 通过 normal path 读取此前未知任务并产生 candidate；
2. product-owned build plan 获得 native Ascend C build success；
3. exact artifact 在 950PR 上执行；
4. admitted intent 不把未经授权的 source behavior 当 specification；
5. Validation Bundle 能执行并绑定 candidate observation；
6. Oracle 至少通过 honest、targeted mutant/negative 和 binding/authenticity controls；
7. Candidate required correctness/numerical/integration/safety gates 闭合；
8. performance 相对 meaningful 950PR baseline 测量，或明确为 informational/not-executed；
9. hidden query/exposure 未违规；
10. package 包含 code、scope、limits、receipts 和 replay commands；
11. 从冻结 manifest 重放得到同一 scoped outcome；
12. 没有 fixture-specific branch、fake receipt、host fallback 或 coding-agent answer substitution。

第二、第三个 materially different task 必须使用相同 production path。它们可以诚实失败，但失败必须证明系统保持了 authority、
evidence 和 attribution 边界。

## 9. Artifact policy

- 正式 run 的 manifest、events、model usage counts、artifacts 和 receipts 存在 durable store/CAS；
- restricted evaluator 和 hidden results 不进入 proposal-visible repository path；
- 可分享结果导出为 content-addressed artifact bundle，包含 schema、environment、license 和 redaction manifest；
- `docs/` 不保存逐 run JSON、完整 transcripts、DEV 流水、pilot completeness 或历史 comparison；
- 一个实验改变默认策略、architecture 或 roadmap 时，直接更新对应当前文档并在 commit message 引用 artifact identity；
- 原始结果不因文档删除而丢失，Git 追溯文档历史，runtime artifact store追溯执行事实。
