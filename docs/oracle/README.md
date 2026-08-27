# Cairn Oracle 设计与调研文档

- 日期：2026-08-27
- 范围：仅限 CUDA → Ascend C 算子移植

本目录区分调研依据和规范性目标设计。调研文档解释“为什么”；设计文档规定“系统边界和目标
是什么”。设计完成不代表能力已经实现，实施状态以 [`../IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md)
为准。

后续会话在设计或实施前先检查
[`DESIGN_INVARIANTS.md`](DESIGN_INVARIANTS.md)。它规定跨文档权威、不可妥协边界、hidden/feedback
污染控制和每个实施 slice 的 conformance record。

Focused design 按对象分工：SIR 文档拥有 intent proposal 边界；Oracle Exploration 拥有 proposal
生成；Independent Admission 拥有所有 applicant 共用的 planner/gate 规则；Oracle Admission 在不
削弱共用规则的前提下拥有 Oracle-specific controls；Performance 文档拥有 hardware/measurement
语义；Knowledge/Skill 文档拥有 retrieval/trust lifecycle。交叉问题必须同时满足两边，不能以
“更具体”为理由削弱另一条 authority boundary。

## 建议阅读顺序

1. [`DESIGN_INVARIANTS.md`](DESIGN_INVARIANTS.md)：跨会话设计不变量和实施前检查；
2. [`ORACLE_RESEARCH_REPORT.md`](ORACLE_RESEARCH_REPORT.md)：学术界与工业界调研、Oracle 维度、
   自动生成方法和风险；
3. [`BORROWABLE_DIRECTIONS.md`](BORROWABLE_DIRECTIONS.md)：值得借鉴和不应照搬的方向；
4. [`SEMANTIC_INTENT_RECOVERY_DESIGN.md`](SEMANTIC_INTENT_RECOVERY_DESIGN.md)：高阶用户意图恢复、
   多假设输出和严格隔离；
5. [`ORACLE_EXPLORATION_SYSTEM_DESIGN.md`](ORACLE_EXPLORATION_SYSTEM_DESIGN.md)：Oracle claim portfolio、
   synthesis/adversarial strategies、反馈和独立准入；
6. [`INDEPENDENT_ADMISSION_DESIGN.md`](INDEPENDENT_ADMISSION_DESIGN.md)：可选 typed Planner、hidden
   controls、权威 receipt 和机械 gate 的共同准入架构；
7. [`ORACLE_ADMISSION.md`](ORACLE_ADMISSION.md)：Oracle claim portfolio 六个平面的具体准入、
   上一轮反馈输入、mutation、receipt 和 revalidation；
8. [`PERFORMANCE_ORACLE_DESIGN.md`](PERFORMANCE_ORACLE_DESIGN.md)：硬件事实、microbench、profiler、
   多 roofline 和性能准入；
9. [`KNOWLEDGE_AND_SKILL_TRUST_DESIGN.md`](KNOWLEDGE_AND_SKILL_TRUST_DESIGN.md)：知识/skill 信赖、
   生命周期、检索和撤回传播。

具体 Oracle calibration、mutation、上一轮反馈和多平面准入机制由
[`ORACLE_ADMISSION.md`](ORACLE_ADMISSION.md) 规定；Blue/Red agent loop 由
[`../ORACLE_AGENT.md`](../ORACLE_AGENT.md) 与 [`../ORACLE_PROMPTS.md`](../ORACLE_PROMPTS.md) 规定。
Blue/Red 是当前模型驱动的 synthesis/adversarial strategy profiles，不是永久固定 Agent 拓扑。
Admission 的 Planner profiles、进程和 plan validation 见
[`../design/ADMISSION_ARCHITECTURE.md`](../design/ADMISSION_ARCHITECTURE.md)。Agent-capable function、
strategy、profile、episode、Host、process、authority 的区分和 artifact-mediated interaction 见
[`../design/AGENT_ARCHITECTURE.md`](../design/AGENT_ARCHITECTURE.md)。

## 固定边界

- Cairn 产品只做 CUDA → Ascend C；
- Semantic Intent Recovery 只提案，Intent Admission 才能形成正式迁移意图；
- Oracle Explorer 只提案，Oracle Admission 才能授权 claim；
- Candidate Search 不可读取或修改 hidden judge material；
- 算法、数值、执行、安全、充分性和性能分别给出结论；
- 性能不能补偿 correctness 失败；
- 反馈、文档、知识和 skill 都是带 provenance 的输入，不因来源自动可信；
- schema 处于 pre-release V1，设计变化直接修改 V1，不建立兼容或迁移路径；
- production API 中容易混淆的 identity、unit、role、lifecycle、evidence 和 outcome 使用不同强类型。
