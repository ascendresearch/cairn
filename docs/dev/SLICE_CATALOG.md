# Cairn Development Slice Catalog

- 状态：规范性计划 catalog，尚未授权代码实施
- 日期：2026-08-27
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 路线图：[`ROADMAP.md`](ROADMAP.md)
- 通用验收：[`QUALITY_GATES.md`](QUALITY_GATES.md)

## 1. Catalog 规则

本文件中的 `DEV-xxx` 是计划标识，不是 persisted schema version。所有 slice 默认还需通过通用 gate；
表中只列专属目标和增量验收。状态变化必须记录到 [`../IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md)
的事实账本并引用 commit/evidence，不能只改本表。

当前没有任何新架构代码 slice 被授权为 `InProgress`。决策/计划 slice 为 `Proposed`；依赖它们的代码
slice 为 `Blocked`。

## 2. ST0 — Planning Readiness

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-001` | Proposed | 关闭 OQ-019，冻结首个 deterministic kernel、Intent claim set、公开/hidden corpus、冲突与用户决策 policy | 用户/产品选择，历史和新 fixture 研究 | requirements/decision/focused design 同步；正、竞争假设、source artifact、unknown fixtures 列表 |
| `DEV-002` | Proposed | 关闭 OQ-023，定义首个 `VerificationMechanismSet` qualification | 选定 DEV-001 的所需机制范围 | mechanism identities、golden/property oracle、mutation/fault controls、review/requalification policy |
| `DEV-003` | Proposed | 关闭 OQ-016 的首批范围并生成 fixture sanitation plan | 历史 evidence inventory | public/private disposition、new fixture provenance、secret/path/provider scan、保留/删除列表 |
| `DEV-004` | Blocked | 为 ST1 完成首份 `DesignConformanceRecord` 和 exact change inventory | DEV-001/002/003 | 已审查 record，列出 V1 类型、crate/file、authority、tests、删除路径和 unknown scope |

### DEV-001 选择约束

首个 kernel 应优先满足：

- deterministic，避免被 OQ-025 统计政策阻塞；
- 足以产生两个以上合理 intent hypotheses 或至少一个实现伪影/部署特化；
- 有可运行 CUDA source 和明确 host launch；
- 可在无 NPU时完成 Intent 区分控制；
- 后续有现实 Ascend C candidate 路径；
- 不是只靠固定 expected bytes 就能掩盖意图问题的 trivial case。

不能因为当前 `matmul-zero-k` 已物化就自动选择它。选择需服务 intent authority proof，而非最大化旧代码
复用率。

## 3. ST1 — Intent Authority Proof

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-100` | Blocked | 直接把 `cairn-migration` 当前 V1 替换为 `cairn-cuda-ascend` 并建立 dependency/vocabulary architecture gates | DEV-004 | workspace/fixtures/docs 全部切换；旧 crate/alias/compat path 不存在；行为控制仍通过 |
| `DEV-101` | Blocked | 建立最小 public/restricted/secret typed ports 与独立 `cairn-admission` process boundary | DEV-100、DEV-002 | Controller/proposal 无 restricted access；Admission 无 model dependency；wrong-store compile/runtime denial；restart control |
| `DEV-102` | Blocked | 建立 `cairn-sir` frozen input/process protocol 和 `IntentHypothesisSet` proposal-only output | DEV-100、DEV-001 | competing/unknown/conflict hypotheses；SIR crash/restart；admitted constructor 静态不可达 |
| `DEV-103` | Blocked | 机械派生 `RequiredIntentEvidenceSet`，实现 exact deterministic recipe 或 `IntentEvidencePlannerProfile` 和 typed validator | DEV-101/102 | applicant 不能删 obligation；wrong-kind compile/decode fail；planner-absent path 按 DEV-001 policy 工作 |
| `DEV-104` | Blocked | 执行 Intent controls并由 separate Mechanical Gate 形成 `MigrationIntentContract` 或非成功 outcome | DEV-101/102/103、DEV-002 | positive/conflict/unknown/wrong-hypothesis/tamper/receipt-closure/restart controls |
| `DEV-105` | Blocked | 让 admitted intent 生成一个 typed `OracleClaimProposal` 并完成 ST1 backwards audit | DEV-104 | 无 admitted intent 时调用失败；artifact/event graph 完整；Candidate Search 未启动 |

### ST1 Integration Gate

ST1 只能整体宣称完成，不能以任一子 slice 代替。必须演示：

```text
frozen task/context
→ out-of-process SIR proposal
→ required evidence
→ independent receipts
→ out-of-process mechanical admission
→ admitted intent
→ one Oracle claim proposal
```

还需分别演示 `Conflict` 或 `Unknown` 不能进入要求 admitted intent 的下游 API。

## 4. ST2 — Oracle Generation Core

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-200` | Blocked | 从 admitted intent 机械派生 `RequiredOracleClaimSet` 和 acyclic dependency graph | ST1 | required/optional/not-applicable controls、cycle rejection、partial/full compile boundary |
| `DEV-201` | Blocked | 建立多平面 `OraclePortfolioProposal`、domain partition、relation/comparator/instrument proposal types | DEV-200 | semantic/numerical/execution/safety/adequacy/performance-instrument 不可互换；strict V1 controls |
| `DEV-202` | Blocked | 注册 synthesis strategy contract，并把当前 Blue profile 接到正式 typed gateway | DEV-201 | model/deterministic substitution；model 不填 trusted IDs/policy；research provenance 与 proposal 分离 |
| `DEV-203` | Blocked | 建立 deterministic adversarial baseline：mutation/property/boundary/bypass search | DEV-201 | honest path、targeted false-accept/false-reject、coverage gap 和 non-injectable controls |
| `DEV-204` | Blocked | 按 policy 接入 Red model-backed adversarial profile | DEV-202/203 | frozen revision→finding→changed revision；独立 continuation；无投票/admission edge |
| `DEV-205` | Blocked | Explorer coordinator 组合策略、反馈与 common-dependency graph，形成 ready-for-admission portfolio | DEV-200..204 | 每个 claim 有 provenance、coverage、blind spots 和 unresolved state；无 admitted type |

### ST2 成本顺序

每个 claim 先运行 deterministic schema/static/property/boundary controls，再购买模型对抗；模型仅在 evidence
gap、conflict、novelty 或 expected information gain 支持时启动。Red 不因 catalog 存在而强制每次运行。

## 5. ST3 — Independent Oracle Admission

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-300` | Blocked | 把 DEV-002 qualification profile 落为 exact mechanism registry/lifecycle 与 Gate precondition | DEV-002、ST1 | unqualified/refuted mechanism fail closed；requalification/impact controls |
| `DEV-301` | Blocked | 实现 `OracleControlPlannerProfile`、required set projection 和 deterministic plan validation | DEV-200/205 | Planner不能删/满足 obligation；wrong-kind experiment/hidden request 拒绝；deterministic fallback |
| `DEV-302` | Blocked | 建立 public correct-family、negative mutation、conflict、bypass execution/receipt closure | DEV-300/301 | applicant-authored passed/receipt tamper/candidate-writable evidence 全部变红 |
| `DEV-303` | Blocked | 建立 restricted hidden corpus store、one-time Worker capability、redacted diagnostics 与 exposure ledger | DEV-101、DEV-302；adaptive 时需 OQ-024 | Controller/Proposal 无 hidden bytes；digest knowledge 不授权；burn/replenish 或明确 non-adaptive policy |
| `DEV-304` | Blocked | 分别实现 semantic、numerical、execution、safety、adequacy、performance-instrument Mechanical Gates | DEV-300..303 | 各 plane 独立 positive/negative/unknown/not-executed；性能不能补偿 correctness |
| `DEV-305` | Blocked | 实现 Oracle portfolio closure、partial/full typed boundary 和 revalidation triggers | DEV-304 | incomplete required claim 不能进入 full consumer；blind spots/assumptions/strength 保留 |
| `DEV-306` | Blocked | 把当前 Blue/Red、matmul、historical reduction controls映射为新架构 regression，完成 M2 audit | DEV-305、DEV-003 | 没有旧 admitted shape/compat reader；完整 backwards graph；明确未覆盖 target/performance scope |

## 6. ST4 — Candidate and Real Execution

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-400` | Blocked | 建立 Candidate Search product profile、frozen revision lineage、public diagnostic contract | ST3 | Candidate 不能读取 hidden/policy；每个修订新 identity；budget/stop/replay |
| `DEV-401` | Blocked | hardware-free recorded candidate build/run/comparison/correction 纵向闭环 | DEV-400 | 一个 gate rejection 在同 episode 修复；旧 attempt 保留；无 test-only admission shortcut |
| `DEV-402` | Blocked | 为首个 operator 建立真实 CUDA build/run/reference adapter | DEV-401 | binary/image/device/launch/output receipts；两次独立执行；fallback/tamper controls |
| `DEV-403` | Blocked | 为同一 candidate 建立真实 Ascend C build adapter | DEV-401 | exact CANN/compiler/source/binary identity；build failure 与 infra failure 分离 |
| `DEV-404` | Blocked | 建立真实 Ascend NPU execution adapter 与 device evidence | DEV-403、可用设备 | exact SoC/device/binary/launch/sync/output-write；not-executed 不变 green |
| `DEV-405` | Blocked | 建立 target safety/concurrency/sanitizer 或适用替代控制 | DEV-404 | exact tool/mode/coverage；fault controls；不适用项有 typed disposition |
| `DEV-406` | Blocked | Candidate Admission 从 admitted Oracle 和 receipts 重算多平面 verdict | DEV-401..405 | semantic/numerical/execution/safety/adequacy/performance outcome 分离；stored pass ignored |
| `DEV-407` | Blocked | controller/worker/proposal/admission crash、loss、ambiguous effect 和 replay 的 M3 audit | DEV-406 | task可恢复；不双执行；完整 exportable evidence graph |

DEV-402、DEV-403 可在 DEV-400/401 contract 稳定后并行；DEV-404 必须消费 DEV-403 exact binary identity。

## 7. ST5 — Hardware and Performance

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-500` | Proposed | 关闭 OQ-020，冻结第一 Ascend hardware/performance profile | 真实环境审计 | exact SoC/CANN/compiler/firmware、metrics、baseline、workload、device-state policy |
| `DEV-501` | Blocked | 建立 theoretical/official hardware fact proposal 与 admission | DEV-500、DEV-300 | unit/scope/provenance/freshness controls；official 不自动 trusted |
| `DEV-502` | Blocked | 建立 microbench registry、runner 和 measured ceiling admission | DEV-500/501 | warmup/repetition/state/outlier/clock controls；同 measurement 不自证 threshold |
| `DEV-503` | Blocked | qualification 并接入 profiler adapter | DEV-500/300 | known-workload calibration、field/unit mutation、missing/unsupported metrics |
| `DEV-504` | Blocked | 计算 algorithmic/implementation intensity、applicable roofline 和 bottleneck hypotheses | DEV-501..503 | scope/applicability 强类型；theoretical/measured/algorithm/current roofs 不混用 |
| `DEV-505` | Blocked | Candidate performance experiment/admission 与 workload aggregation | DEV-406、DEV-504 | correctness prerequisite；target/baseline/statistics/tail outcome；performance不能改 correctness |

## 8. ST6 — Knowledge, Skill and Feedback

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-600` | Proposed | 关闭 OQ-021，冻结首批 per-role knowledge/skill profile | SIR/Oracle needs | claim kind、allowed use、sandbox、promotion evidence 和 role matrix |
| `DEV-601` | Blocked | exact knowledge claim/content identity、T0–T3、lifecycle、retrieval snapshot | DEV-600 | retrieval rank/official origin 不提升 trust；conflict/freshness/retraction controls |
| `DEV-602` | Blocked | reviewed/validated skill lifecycle、sandbox、capability probe | DEV-600/601 | content mutation失去 validation；skill不能扩权/支持未授权 admission claim |
| `DEV-603` | Proposed | 关闭 OQ-022，冻结 model-integration feedback acquisition/attribution policy | ST4 的 operator/deployment | data boundary、workload weighting、first divergence/ablation 和 external refs |
| `DEV-604` | Blocked | feedback classification、attribution、contamination 和 allowed-use routing | DEV-603 | positive/negative/unknown 分离；held-out污染控制；原 artifact 不变 |
| `DEV-605` | Blocked | revalidation branch、impact graph 和知识写回 review | DEV-601/604 | retraction/feedback 触发受影响 claim；历史 verdict 可重建且不被重写 |

## 9. ST7 — Boundary Validation and Platform

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-700` | Blocked | 选择第二个语义形态不同的 CUDA kernel 和边界假设 | ST4 | 选择理由、不同维度、expected core changes 与 forbidden generic changes |
| `DEV-701` | Blocked | 第二个 operator 跑完整 intent→Oracle→candidate→device/performance/revalidation 路径 | DEV-700、required ST5/6 | domain-neutral core 无 product branch；第一个 operator artifacts 不变 |
| `DEV-702` | Blocked | 稳定 resource-oriented App Server 与 reference CLI | 至少 ST4 | reconnect/backpressure/resource projection；internal event 不成为 public API |
| `DEV-703` | Blocked | 完成 hardware-free CI、显式 hardware lanes、安全/贡献/许可/release gates | DEV-701/702、OQ-017 | public contributor 可运行核心；hardware absence 明确 unavailable；provenance/license green |
| `DEV-704` | Blocked | 首个端到端 workflow 与 public compatibility baseline readiness review | DEV-701..703 | 仅形成评审材料；是否宣布 baseline 仍需用户明确决定 |

`DEV-704` 不能自行递增内部格式版本或建立 migration framework。只有用户明确宣布完成首个端到端
workflow 并建立 public compatibility baseline 后，开发阶段规则才另行评审。

## 10. Cross-cutting slices

Cross-cutting work不单独形成无边界“大重构”，随首个消费者落地：

| 能力 | 首个 owning slice | 后续扩展方式 |
| --- | --- | --- |
| Product observability | DEV-102/104 | 每个新增 lifecycle 同步事件与 semantic-parity control |
| Proposal Host isolation | DEV-202 | 新 profile 复用 contract suite，不复制 Host |
| Restricted data plane | DEV-101/303 | 新 admission kind 复用 typed capability，不通用化 store handle |
| Mechanism qualification | DEV-002/300 | 每个新 comparator/adapter/profiler 加 exact qualification |
| Architecture tests | DEV-100/101 | 每个 slice 增量增加 forbidden dependency/type controls |
| Fixture sanitation | DEV-003 | 新历史材料先 provenance/disposition 后进入 test tree |
| Metrics/dashboard | owning product slice | 指标不成为 durable truth 或 gate authority |

## 11. Catalog 维护

新增或拆分 slice 必须说明：

- 为什么当前 slice 无法保持单一 objective；
- 是否改变 critical path 或里程碑；
- 哪个 requirement/decision/design 产生新义务；
- 新依赖是 contract、authority、qualification、hardware 还是 policy；
- 旧 slice 的状态和证据如何保留；
- 是否需要更新 `ROADMAP.md`、`WORKSTREAMS.md` 和事实账本。

删除未开始 slice 可直接更新当前计划；已产生代码/证据的开发态旧路径按 V1 规则删除 superseded 实现，
但历史 commit/evidence 仍保留，不建立 runtime compatibility。
