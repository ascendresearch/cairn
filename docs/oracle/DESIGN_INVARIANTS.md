# Oracle 设计不变量与实施前检查清单

- 状态：规范性跨文档约束
- 日期：2026-08-27
- 范围：CUDA → Ascend C Oracle、意图、准入、反馈、性能和知识/skill 子系统

## 1. 用途

本文件不是新的架构层，也不替代详细设计。它把后续会话最容易遗漏或漂移的约束压缩成可执行的
审查清单。任何实现计划、schema、Rust API、agent prompt、skill、tool 或实验设计开始前，都应逐项
回答本文件；无法回答的 required 项目保持 blocked/unknown，不得由当前实现惯性替代目标设计。

## 2. 文档权威与冲突处理

仓库内文档按职责而不是简单“最后修改时间”解释：

1. [`../SYSTEM_REQUIREMENTS.md`](../SYSTEM_REQUIREMENTS.md) 定义系统必须表现、拒绝和证明什么；
2. [`../DECISIONS.md`](../DECISIONS.md) 记录已接受的产品/架构选择；
3. [`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md) 定义满足需求的总体 architecture/authority；
4. [`../design/README.md`](../design/README.md) 及其 code/logical/runtime/Agent/Admission designs 定义软件承载、
   依赖和进程/存储边界，但不能改变上层产品 authority；
5. 本目录的 focused designs 定义各子系统的更具体业务边界，但不能降低上层 requirement；
6. [`../dev/README.md`](../dev/README.md) 及其开发计划文档定义未来 stage、slice、依赖和 gate，但不能
   覆盖目标设计或自行关闭 open question；
7. [`../IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md) 记录实施事实、commit 和 evidence ledger，
   不得把历史顺序升级为新架构授权；
8. research reports 提供依据但不直接产生规范；
9. [`../OPEN_QUESTIONS.md`](../OPEN_QUESTIONS.md) 是未决事项，不是自行选择某个答案的授权；
10. 当前代码和历史 fixture 是 implementation evidence，不会因“已经存在”自动成为目标 requirement。

若 requirement、decision、总体设计和 focused design 出现真实冲突，后续工作必须暂停在冲突边界，
明确指出冲突并同步修改所有受影响的规范文档；不能选择最方便的一篇，也不能在代码中添加 fallback
兼容两种含义。

## 3. 不可妥协的不变量

### 3.1 产品与意图

- [ ] 产品范围仍严格是 CUDA → Ascend C；内部 domain-neutral runtime 不扩大产品定位。
- [ ] 执行单元仍是一个 kernel + 显式 host launch；更宽 caller/model context 只服务意图恢复。
- [ ] SIR 只产生 hypothesis/conflict/unknown，不产生正式 `MigrationIntentContract`。
- [ ] Intent Admission 与 SIR 在 type、process/data/capability 和 hidden corpus 上隔离。
- [ ] 用户只对 desired semantics/policy 有决策权，不能用决定覆盖 execution/math evidence。
- [ ] CUDA observation 只证明 source 做了什么，不证明用户想要什么。
- [ ] 异常 source region 有 preserve/follow-intent/exclude/split/block 的 claim-scoped disposition。
- [ ] `OptimizationFreedom` 引用不允许破坏的 admitted invariants，不是自由文本授权。

### 3.2 Oracle 与准入

- [ ] Oracle 是局部 claim portfolio，不是 expected bytes、测试集合或全局置信分。
- [ ] Semantic、numerical、execution、safety、adequacy、performance-instrument 平面不互相冒充。
- [ ] `RequiredOracleClaimSet` 由 trusted policy 派生，applicant 不能删除 required claim。
- [ ] Partially admitted portfolio 在类型上不能进入要求 closure 的 release path。
- [ ] Human risk acceptance 与 evidence outcome 分离，不能把 unknown/violation 改写为 pass。
- [ ] Oracle synthesis/adversarial strategies 和 typed Planner 只能提案；机械 gate 从 authoritative
  receipt 重算 outcome。
- [ ] Trusted policy 在可选 Planner 之前机械派生 kind-specific `RequiredEvidenceSet`；Planner 不能
  删除、降级、满足或替换 required obligation。
- [ ] Intent、Oracle、Hardware、Performance、Candidate、Knowledge、Skill planner profile 的输入、
  obligation、plan、diagnostic、receipt 和 outcome 类型不可互换；确定性 recipe 足够时不强制用 Agent。
- [ ] Agent-capable function、strategy、profile、episode、Host、process 和 authority 没有被混成一个概念；
  当前 11 个逻辑位置是 catalog 派生值，不是固定进程、并发或 protocol 常量。
- [ ] 跨 episode 交互只经过冻结 artifact、typed request/diagnostic 和 durable event；private continuation、
  mutable scratch、pending tool result、未提交推理和 draft 不跨 episode。
- [ ] Agent 共识、投票或重复反思不提升 evidence strength，也不替代 authoritative receipt 或 gate。
- [ ] 正确变体的 construction claim 独立于 Oracle-under-test。
- [ ] Mutation/coverage 只评价已建模 fault，不表示正确概率。
- [ ] CUDA undefined behavior 不进入正常 differential Oracle。
- [ ] Comparator、runner、adapter、parser、sanitizer/profiler adapter、gate 和 policy 自身已有 qualification。

### 3.3 Feedback、hidden corpus 与学习

- [ ] 首轮显式记录 `NoPriorFeedback`；后续 feedback 有来源、scope、receipt、归因和复现状态。
- [ ] Feedback 是 evidence，不是 reward；不会原地修改 intent、Oracle、threshold、weight 或 verdict。
- [ ] Positive model feedback 不提升局部 correctness；negative feedback 未归因时不自动归咎 kernel。
- [ ] Applicant-visible/derivation-equivalent feedback 不作为同一 claim 的 held-out evidence。
- [ ] Hidden case 有 exposure ledger；泄漏区分信息后 burn 为公开 regression 并按需补充 sealed case。
- [ ] Diagnostic 可修复但不泄漏 hidden answer；自适应查询受预算和粒度控制。
- [ ] Feedback 写入知识前经过 scope、recurrence、attribution、evidence 和 usefulness review。

### 3.4 数值、随机与状态

- [ ] Comparator/allowance 按 claim/domain 选择，没有无依据的全局 `atol/rtol`。
- [ ] Allowance magnitude、provenance 和 assurance 是不同类型。
- [ ] Derivation 与 held-out validation corpus 在内容和等价推导上无污染。
- [ ] 同一 measurement 不自证由其推导的 threshold。
- [ ] 随机/有状态/原子 kernel 明确 determinism、RNG/state、allowed outcomes、reset 和 repetition policy。
- [ ] 统计检查声明采样假设、功效、type-I/type-II error 和 inconclusive outcome。
- [ ] 输出数值相等不替代 side effect、状态转换、ordering 或 safety 检查。

### 3.5 Execution、安全与性能

- [ ] Build、CPU twin、真实 CUDA、Ascend build、真实 NPU 和模型集成是不同 evidence scope。
- [ ] Binary/device/ABI/launch/synchronization/output-write/fallback 均有 execution evidence。
- [ ] Worker-controlled evidence 不可由 candidate workspace 写入。
- [ ] Sanitizer “无报告”只在 exact tool/version/mode/coverage 内成立。
- [ ] 性能不能补偿 correctness-plane failure。
- [ ] 性能在 candidate 前只准入 instrument；candidate 后才产生 performance outcome。
- [ ] Theoretical peak、measured ceiling、algorithmic roof、implementation roof、candidate observation
  和 business target 不可互换。
- [ ] Roof 绑定 SoC/dtype/shape/engine/memory/dataflow/toolchain/device state。
- [ ] Workload aggregate 不隐藏 required region/quantile/tail/SLO regression。
- [ ] Workload drift 触发新的 corpus/weight admission，不改写历史 verdict。

### 3.6 Knowledge、skill、记录和类型

- [ ] Author/vendor/official/built-in/retrieval rank 都不自动产生 trust。
- [ ] Knowledge claim 和 skill 分别有 lifecycle、exact content identity 和 allowed use。
- [ ] Reviewed/unvalidated skill 只能影响受限探索，不能支持 admission-critical claim 或扩大权限。
- [ ] Retraction 反向传播到 intent、Oracle、hardware/performance claim 和 verdict。
- [ ] 每个 model-visible/tool-visible verdict-relevant input 均可从 event/CAS 或明确 external reference
  重建。
- [ ] Proposed/admitted、不同 IDs/roles/units/states/evidence/outcomes 使用不同 Rust 强类型。
- [ ] 反序列化重跑 constructor invariants，混淆边界有 compile-fail/static tests。
- [ ] Cairn pre-release 内部格式保持当前 V1；修改 V1 并重建开发状态，不增加 migration/兼容 reader。

## 4. 每个实施 slice 的 design-conformance record

实施计划中的每个 slice 在开始前应形成简短 `DesignConformanceRecord`：

- slice objective 和明确非目标；
- 对应 requirement IDs、decision IDs 和 focused-design sections；
- applicant、authority、trusted mechanism 和 capability matrix；
- proposed/admitted 类型边界；
- required claims、corpus partitions、feedback/hidden use；
- positive、negative、conflict、unknown、bypass 和 tamper controls；
- hardware/tool/environment requirements；
- receipt closure 与 replay/impact edges；
- V1 schema/strong-type changes及 compile-fail obligations；
- 当前仍 unknown/not-executed 的内容；
- 若失败，哪些内容允许修订，哪些需要用户/政策授权。

此 record 是计划/审查 artifact，不是 admission receipt，也不因写完而证明实现正确。

## 5. 进入实现前仍需关闭的选择

以下 open questions 对相应 slice 是真正 blocker：

- OQ-019：首个 Intent Admission 的 operator、claim set 和 hidden corpus；
- OQ-020：首个 Ascend SoC、CANN/compiler、microbench、profiler 和 baseline；
- OQ-021：首批 knowledge claim 与 skill 的 per-role admission profile；
- OQ-022：真实模型反馈的数据边界、workload 获取和 first-divergence/归因政策；
- OQ-023：首个 comparator/adapter/runner/gate/policy mechanism qualification profile；
- OQ-024：adaptive admission 的 hidden exposure、diagnostic budget 和 replenishment policy；
- OQ-025：仅在随机/有状态/多调度结果 operator 上需要的统计准入 policy。

未涉及某一 blocker 的基础设施 slice 可以继续；需要其答案的业务 slice 不得自行填默认值。

## 6. 设计变更规则

新的研究、真实设备测量或模型反馈可以推翻当前设计，但必须：

1. 指明被推翻的 claim 和证据；
2. 更新 requirements、decisions、总体设计、focused design、open questions 和 implementation plan 中
   所有受影响位置；
3. 说明历史 evidence/verdict 是 unaffected、scope-reduced、revalidation-required 还是 unsupported；
4. 在 pre-release 直接修改当前 V1，删除被取代设计，不建立旧/新双路径；
5. 更新本清单，避免后续会话继续执行已经撤回的不变量。
