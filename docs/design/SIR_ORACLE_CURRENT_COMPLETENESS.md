# SIR 与 Oracle 联合权威设计完整性审计

- 审计对象：[`SIR_ORACLE_CURRENT_DESIGN.md`](SIR_ORACLE_CURRENT_DESIGN.md)
- 审计日期：2026-09-01
- 结论：已直接更新 current V1 时序，保留旧设计中仍有效的 authority、Worker、control、diagnostic、target 和 evidence
  边界，并纳入方案 E 的 focused SIR、evolving Oracle、Qualification Epoch 与 Candidate Promotion Protocol

## 1. 本轮逻辑修正

| 要求 | 文档落点 | 结果 |
| --- | --- | --- |
| 不用 mini-SIR 决定是否启动 SIR | 2、7.1、7.2 | 已覆盖 |
| 源语义理解不可避免，focused SIR protocol 才是可选物化 | 7.1 | 已覆盖 |
| material semantic fork 由实际 runtime reasoning 提出 | 7.3 | 已覆盖 |
| Controller 不靠代码特征解释语义 | 5、7.2、7.3 | 已覆盖 |
| direct/focused path 产生同一 intent proposal | 7.4 | 已覆盖 |
| 不以“足够强模型”为架构前提 | 2 | 已覆盖 |
| complete Oracle 不再是第一个 exploratory Candidate 的前置条件 | 4、10、11 | 已覆盖 |
| Oracle accepted 仍是正式 Candidate Admission 的前置条件 | 16、24 | 已覆盖 |

## 2. Candidate/Oracle 猫鼠问题

| 风险/规则 | 文档落点 | 结果 |
| --- | --- | --- |
| Development Oracle 与 Qualification Oracle 分离 | 3、16 | 已覆盖 |
| qualification 绑定 Intent × Oracle × target × Candidate × policy | 16.3 | 已覆盖 |
| Oracle change 使用六类 typed cause | 17 | 已覆盖 |
| Artifact correction/coverage/evidence strengthening 先 meta-qualification | 17 | 已覆盖 |
| Oracle 改版后 parent/current/all compared variants 对称重测 | 17 | 已覆盖 |
| Candidate accommodation 无独立依据时拒绝 | 17 | 已覆盖 |
| latest revision 不自动替代 parent | 18 | 已覆盖 |
| integrity、non-regression、improvement、comparison、qualification 五层 Gate | 18 | 已覆盖 |
| performance/precision 预声明、同 epoch、minimum practical improvement | 18 | 已覆盖 |
| 局部最优进入 domain-bound Pareto family，不冒充全局最优 | 18 | 已覆盖 |
| hidden query budget、exposure state、retire-and-replace | 19 | 已覆盖 |
| 已泄露 control 不继续计 hidden | 19 | 已覆盖 |
| hard-coded output、harness/test ID、benchmark-only specialization 防护 | 19 | 已覆盖 |

## 3. 旧联合设计边界保留

| 边界 | 文档落点 | 结果 |
| --- | --- | --- |
| CUDA source/behavior 不拥有 specification authority | 2、3 | 已覆盖 |
| exact 950PR（3510）与 CANN/toolchain | 1、6.1 | 已覆盖 |
| PyTorch 可选且不天然独立/正确 | 21 | 已覆盖 |
| SIR 可请求 CUDA/reference Worker experiment | 7.5 | 已覆盖 |
| admitted evidence snapshot 传给 Oracle/Candidate | 8 | 已覆盖 |
| previous feedback exact、typed、content-addressed | 22 | 已覆盖 |
| skill/knowledge 有 provenance、不产生 authority | 23 | 已覆盖 |
| correctness/numerical/integration/safety/adequacy/performance 独立 | 13 | 已覆盖 |
| property/case/mechanism 分层，case 不自动膨胀为 item | 14 | 已覆盖 |
| independent Review 与 Portfolio Coherence 保留 | 14 | 已覆盖 |
| ordinary capability-matched Worker，无 Proposal Host | 4、15 | 已覆盖 |
| shell/CPU/CUDA/Ascend compile/950PR evidence 不互换 | 6.1、15 | 已覆盖 |
| Oracle Admission model-free | 5、24 | 已覆盖 |

## 4. 方案 D 防护保留

| 防护 | 文档落点 | 结果 |
| --- | --- | --- |
| catalog/derivation/exposure 在首 episode 前 seal | 6.2 | 已覆盖 |
| initial actor 看不到内部 taxonomy | 6.2、12.1 | 已覆盖 |
| organic concern 必须有 trigger/risk/domain/next evidence | 12.1 | 已覆盖 |
| policy challenge 前 freeze，之后不可回填 | 12.2 | 已覆盖 |
| complete ledger 每项 typed disposition | 12.3 | 已覆盖 |
| catalog 是 floor，不压制 novel discovery | 12.3 | 已覆盖 |
| required unknown 阻塞，Worker 不可用不是 NotApplicable | 12.3 | 已覆盖 |
| high-severity gap 可进入 full D fallback | 12.4 | 已覆盖 |
| Controller 响应 typed gap，不做 semantic coverage | 12.4 | 已覆盖 |

## 5. Control 与诊断安全边界

| 边界 | 文档落点 | 结果 |
| --- | --- | --- |
| hidden challenge 与 public/original item 不同且强类型 | 20 | 已覆盖 |
| `OracleArtifactRejected`、`NegativeChallengeAccepted`、protocol、execution 分开 | 20 | 已覆盖 |
| exit 31 进入 control reconciliation，不误发 Developer | 20 | 已覆盖 |
| diagnostic 只读 exact graph node/revision/receipt | 20 | 已覆盖 |
| sibling/missing/over-limit 拒绝，单 artifact 16 KiB | 20 | 已覆盖 |
| hidden material、expected result、sibling receipt 不暴露 | 19、20 | 已覆盖 |
| 日志不记录 source/prompt/model body/stdout/stderr/credential | 25 | 已覆盖 |
| token usage 只保存计数，不保存内容 | 25 | 已覆盖 |

## 6. 软件与数据边界

| 边界 | 文档落点 | 结果 |
| --- | --- | --- |
| cairn-server 业务无关、migration-app composition | 4 | 已覆盖 |
| ports 不是独立 proposal 进程 | 4 | 已覆盖 |
| focused roles 是真实 Agent Loop，外层遍历机械编排 | 4 | 已覆盖 |
| normal CLI/server/app/workflow/Worker path | 6、27 | 已覆盖 |
| semantically distinct node/edge/state 使用强类型 | 9 | 已覆盖 |
| deserialization 重跑 invariant、compile-fail boundary | 9 | 已覆盖 |
| restart 恢复 visibility/revision/query budget | 25 | 已覆盖 |
| pre-release current V1，无 compatibility/V2 | 1、27 | 已覆盖 |
| 至少第二个语义明显不同任务 | 27、29 | 已覆盖 |

## 7. 写后发现并修正的冲突

1. **全局产品文档仍把旧箭头读成固定时序**：已更新
   [`CAIRN_CURRENT_PRODUCT_DESIGN.md`](CAIRN_CURRENT_PRODUCT_DESIGN.md)，明确图表达 release dependency，focused SIR
   可选，exploratory Candidate 可早于 final Oracle。
2. **README 仍把 A/B/C 写作“下一次冻结实验”**：已改为 preserved pilots，并写入 D/E、same-epoch promotion、Oracle
   symmetric replay 和 hidden exposure。
3. **设计索引仍让旧 workflow/oracle 文档覆盖 current design**：已把 current product 和本联合文档放到阅读顺序前部；旧
   fixed-stage 叙述只作历史依据。
4. **E 只有 epoch invalidation、没有正式 promotion 语义**：已在 E 和本联合文档中补齐 Candidate lifecycle、Oracle change
   control、五层 Gate、多目标规则和 adaptive hidden-query 防护。
5. **新 shared graph 可能扩大旧 diagnostic 权限**：已明确 exact node/revision/receipt 与 16 KiB 上限不因 graph 共享而放宽。

## 8. 尚未由设计冒充已解决的问题

- query budget、holdout refresh 和 feedback granularity 的 task-risk policy；
- performance/precision minimum practical improvement 的 authority 来源；
- Oracle meta-qualification correct/mutant family 的构建与污染防护；
- Qualification Epoch 按 family 还是 variant/workload partition；
- full D fallback 的 mechanical severity threshold；
- focused SIR 在缺少 independent specification 时的真实 recall；
- 当前代码尚未实现 Graph、exploratory Ascend C consumer、Qualification Oracle 或 Promotion Gates。

这些必须通过正式设计 slice、真实 runtime model、ordinary Workers、exact 950PR 和 common hidden evaluator解决，不能因为
联合权威文档已经更新就误报为实现完成。

## 9. 最终防丢结论

当前联合规范不再把 SIR、Oracle 和 Candidate 当成固定认知流水线。runtime model 直接开始迁移；material semantic fork 才
物化 focused SIR；assurance 与无发布 authority 的 exploratory Candidate 共同演进；release 前执行 sealed-policy challenge，
冻结 Qualification Oracle 和 epoch。后续 Candidate 只有在同一 epoch 通过五层 Gate 才晋升；Oracle 改版时新旧 Candidate
对称重测；hidden feedback 受 query/exposure/retire-and-replace 管理。最终 authority 仍来自 exact intent、950PR receipts、
independent controls 和 model-free Admissions。
