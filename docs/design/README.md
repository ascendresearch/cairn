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
| [`ARCHITECTURE_OVERVIEW.md`](ARCHITECTURE_OVERVIEW.md) | 软件架构风格、关键边界、共同约束和设计取舍是什么？ | 规范性软件架构总览 |
| [`WORKFLOW_ARCHITECTURE.md`](WORKFLOW_ARCHITECTURE.md) | 端到端状态机、各 Agent Loop、反馈路由、统一 Worker 实验和直连网络如何组合？ | 规范性工作流设计 |
| [`CODE_ORGANIZATION.md`](CODE_ORGANIZATION.md) | Rust workspace、crate、模块、port/adapter 和测试代码应如何组织？ | 规范性代码组织设计 |
| [`LOGICAL_ARCHITECTURE.md`](LOGICAL_ARCHITECTURE.md) | bounded context、aggregate、command/event、数据流和 capability 如何组合？ | 规范性逻辑架构设计 |
| [`RUNTIME_ARCHITECTURE.md`](RUNTIME_ARCHITECTURE.md) | Controller、提案进程、Admission、存储和各类 Worker 如何运行、隔离与恢复？ | 规范性运行架构设计 |
| [`ADMISSION_ARCHITECTURE.md`](ADMISSION_ARCHITECTURE.md) | Typed Planner profiles、required evidence、机械 Gate 和 Admission 软件如何组合？ | 规范性 Admission 软件架构 |
| [`AGENT_ARCHITECTURE.md`](AGENT_ARCHITECTURE.md) | Agent-capable function、strategy、profile、episode、Host、process 与 authority 如何区分和交互？ | 规范性 Agent 软件架构 |

## 阅读顺序

1. 先读 [`../SYSTEM_REQUIREMENTS.md`](../SYSTEM_REQUIREMENTS.md) 和
   [`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md)，确认产品与 authority 边界；
2. 读本目录总览，理解软件形态；
3. 读 [`WORKFLOW_ARCHITECTURE.md`](WORKFLOW_ARCHITECTURE.md)，理解已经冻结的端到端阶段、反馈与
   实验路径；
4. 按当前任务进入代码、逻辑、运行、Agent 或 Admission 架构；
5. 涉及 SIR 或 Oracle 实施前，再逐项检查
   [`../oracle/DESIGN_INVARIANTS.md`](../oracle/DESIGN_INVARIANTS.md) 和相应 focused design；SIR 以
   [`../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md`](../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md)
   为规范性业务设计；
6. 以 [`../dev/CURRENT_BASELINE.md`](../dev/CURRENT_BASELINE.md) 判断哪些目标已经实现。

规划实现顺序、slice、entry/exit gate 和并行 workstream 时，另读
[`../dev/README.md`](../dev/README.md)；该目录不能降低本目录的架构边界。

## 与其他规范的关系

- Requirements 决定系统必须做什么；
- Decisions 记录已经接受的架构选择；
- System Design 决定总体 authority 和端到端结构；
- 本目录细化软件组成，不得削弱上面三者；
- `docs/oracle/` 决定各 Oracle 相关子系统的业务语义，本目录只决定其软件承载方式；
- `docs/dev/` 同时区分未来实施顺序/gate 与当前事实基线，不得把目标目录树误报为现状；详细历史流水
  通过 Git 追溯。

发生冲突时遵循
[`../oracle/DESIGN_INVARIANTS.md`](../oracle/DESIGN_INVARIANTS.md) 的规范冲突规则，暂停受影响的
实施 slice 并同步修正文档；pre-release V1 不建立双路径或兼容层。
