# Cairn 软件架构设计

- 状态：规范性目标设计
- 日期：2026-08-29
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 实施状态：本目录主要描述目标架构；DEV-008–020 已实际贯通一条窄的 SIR → 用户决定 → Intent
  Admission → Oracle → Candidate → remote native build → diagnostic → repair → rebuild 路径。最后一次 build
  仍为 `SubjectFailed`，尚未形成 native success、NPU runtime correctness 或最终 verdict；其他组件是否
  存在仍以开发基线为准

本目录把 [`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md) 中的总体系统设计落实为软件结构。它回答
“代码放在哪里、业务边界如何协作、进程如何部署和恢复”，不重新定义 Oracle、意图、性能或
知识/skill 的业务语义。

## 文档地图

| 文档 | 主要问题 | 权威 |
| --- | --- | --- |
| [`CAIRN_CURRENT_PRODUCT_DESIGN.md`](CAIRN_CURRENT_PRODUCT_DESIGN.md) | 当前产品使命、四平面、交付与 D/E 实验方向是什么？ | 当前全局产品设计 |
| [`SIR_ORACLE_CURRENT_DESIGN.md`](SIR_ORACLE_CURRENT_DESIGN.md) | focused SIR、Evidence/Assurance Graph、Oracle qualification 与 Candidate promotion 如何共同工作？ | 当前 SIR/Oracle 联合权威设计 |
| [`SIR_ORACLE_CURRENT_COMPLETENESS.md`](SIR_ORACLE_CURRENT_COMPLETENESS.md) | 联合权威更新是否保留旧安全边界并完整解决 Candidate/Oracle 猫鼠与文档冲突？ | 当前联合设计写后完整性审计 |
| [`ARCHITECTURE_OVERVIEW.md`](ARCHITECTURE_OVERVIEW.md) | 软件架构风格、关键边界、共同约束和设计取舍是什么？ | 规范性软件架构总览 |
| [`WORKFLOW_ARCHITECTURE.md`](WORKFLOW_ARCHITECTURE.md) | 端到端状态机、各 Agent Loop、反馈路由、统一 Worker 实验和直连网络如何组合？ | 规范性工作流设计 |
| [`CODE_ORGANIZATION.md`](CODE_ORGANIZATION.md) | Rust workspace、crate、模块、port/adapter 和测试代码应如何组织？ | 规范性代码组织设计 |
| [`LOGICAL_ARCHITECTURE.md`](LOGICAL_ARCHITECTURE.md) | bounded context、aggregate、command/event、数据流和 capability 如何组合？ | 规范性逻辑架构设计 |
| [`RUNTIME_ARCHITECTURE.md`](RUNTIME_ARCHITECTURE.md) | Controller、提案进程、Admission、存储和各类 Worker 如何运行、隔离与恢复？ | 规范性运行架构设计 |
| [`ADMISSION_ARCHITECTURE.md`](ADMISSION_ARCHITECTURE.md) | Typed Planner profiles、required evidence、机械 Gate 和 Admission 软件如何组合？ | 规范性 Admission 软件架构 |
| [`AGENT_ARCHITECTURE.md`](AGENT_ARCHITECTURE.md) | Agent-capable function、strategy、profile、workflow step、Worker execution 与 authority 如何区分和交互？ | 规范性 Agent 软件架构 |
| [`BLIND_FIRST_ORACLE_SCOPE_DESIGN.md`](BLIND_FIRST_ORACLE_SCOPE_DESIGN.md) | 如何先冻结 runtime model 的无 taxonomy 锚定 scope，再由完整 policy challenge 补漏并形成最小 Oracle obligation graph？ | 消融实验后的候选设计，尚非当前规范 |
| [`BLIND_FIRST_ORACLE_SCOPE_COMPLETENESS.md`](BLIND_FIRST_ORACLE_SCOPE_COMPLETENESS.md) | 方案 D 是否完整保留用户要求、authority、信息隔离、强类型、恢复和实验边界？ | 候选设计的写后完整性审计 |
| [`EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md`](EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md) | 如何让语义、assurance 和 exploratory Candidate 共演化，并以 same-epoch Promotion Gates 防止 Candidate/Oracle 猫鼠迎合？ | 第一性原理复盘后的方案 E，整体仍待消融 |
| [`EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_COMPLETENESS.md`](EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_COMPLETENESS.md) | 方案 E 是否遗漏 SIR 修正、弱模型防护、D fallback、Oracle change、Candidate promotion、实验和指标？ | 候选设计的写后完整性审计 |

## 阅读顺序

1. 先读仓库 [`../../AGENTS.md`](../../AGENTS.md)；
2. 读 [`CAIRN_CURRENT_PRODUCT_DESIGN.md`](CAIRN_CURRENT_PRODUCT_DESIGN.md)；
3. 涉及 SIR、Oracle、exploratory Candidate 或 promotion 时读
   [`SIR_ORACLE_CURRENT_DESIGN.md`](SIR_ORACLE_CURRENT_DESIGN.md)；
4. 读本目录总览，再按任务进入 workflow、code、logical、runtime、Agent 或 Admission 架构；旧 workflow 的固定阶段图按
   current 联合权威文档解释，不得恢复 mandatory SIR 或 complete-Oracle-before-exploration；
5. [`../SYSTEM_REQUIREMENTS.md`](../SYSTEM_REQUIREMENTS.md)、[`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md) 和
   `docs/oracle/` 保留仍未被 current documents 修改的通用边界与历史依据，不得反向覆盖 current V1 时序；
6. 以 [`../dev/CURRENT_BASELINE.md`](../dev/CURRENT_BASELINE.md) 判断实现事实，不能把设计完成误报为代码完成。

规划实现顺序、slice、entry/exit gate 和并行 workstream 时，另读
[`../dev/README.md`](../dev/README.md)；该目录不能降低本目录的架构边界。

## 与其他规范的关系

- Requirements 决定系统必须做什么；
- Decisions 记录已经接受的架构选择；
- System Design 决定总体 authority 和端到端结构；
- 本目录细化软件组成，不得削弱上面三者；
- 当前 SIR/Oracle 业务语义由 [`SIR_ORACLE_CURRENT_DESIGN.md`](SIR_ORACLE_CURRENT_DESIGN.md) 决定；`docs/oracle/` 中未被
  current document 明确保留的固定阶段叙述只作历史依据；
- `docs/dev/` 同时区分未来实施顺序/gate 与当前事实基线，不得把目标目录树误报为现状；详细历史流水
  通过 Git 追溯。

发生冲突时遵循
[`../oracle/DESIGN_INVARIANTS.md`](../oracle/DESIGN_INVARIANTS.md) 的规范冲突规则，暂停受影响的
实施 slice 并同步修正文档；pre-release V1 不建立双路径或兼容层。
