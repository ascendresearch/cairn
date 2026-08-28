# Cairn 开发计划设计

- 状态：规范性开发计划；实施授权按slice入口记录独立管理
- 日期：2026-08-28
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 上位规范：[`../SYSTEM_REQUIREMENTS.md`](../SYSTEM_REQUIREMENTS.md)、
  [`../DECISIONS.md`](../DECISIONS.md)、[`../SYSTEM_DESIGN.md`](../SYSTEM_DESIGN.md)
- 软件架构：[`../design/README.md`](../design/README.md)
- Oracle 设计不变量：[`../oracle/DESIGN_INVARIANTS.md`](../oracle/DESIGN_INVARIANTS.md)

## 1. 目的

本目录定义 Cairn 如何从当前实现基线演进到新的 CUDA → Ascend C 架构。它回答：

- 开发工作按什么原则和依赖排序；
- 当前代码哪些可以复用、哪些只是控制证据、哪些方向已经停止；
- 一个开发 slice 在何种条件下可以开始、合并和宣称完成；
- SIR、Oracle、Admission、Candidate、Hardware/Performance、Knowledge/Skill 和 Feedback 如何逐步接入；
- 哪些工作可并行，哪些必须等待 authority contract 或真实设备证据；
- 如何防止旧 Phase G、当前代码形态或模型演示反向定义新架构。

本目录不是实施授权。开始任何代码 slice 前，仍需满足其 entry gate、关闭对应 blocker，并形成
`DesignConformanceRecord`。

## 2. 文档地图

| 文档 | 主要问题 | 权威 |
| --- | --- | --- |
| [`DEVELOPMENT_MODEL.md`](DEVELOPMENT_MODEL.md) | 开发计划如何表示阶段、increment、slice、状态、依赖和证据？ | 规范性开发方法 |
| [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) | 当前实现事实、可复用基础、历史控制、缺口和停止方向是什么？ | 事实基线与迁移约束 |
| [`ROADMAP.md`](ROADMAP.md) | 从当前状态到 M2–M5 的阶段、关键路径和并行路径是什么？ | 规范性路线图 |
| [`SLICE_CATALOG.md`](SLICE_CATALOG.md) | 具体规划了哪些开发 slice，各自的输入、输出、依赖和验收是什么？ | 规范性 slice catalog |
| [`QUALITY_GATES.md`](QUALITY_GATES.md) | 开始、合并、阶段晋级和完成声明需要哪些控制？ | 规范性开发 gate |
| [`WORKSTREAMS.md`](WORKSTREAMS.md) | 工作流、代码归属、并行协作和集成顺序如何安排？ | 规范性协作设计 |
| [`DESIGN_CONFORMANCE_RECORD_TEMPLATE.md`](DESIGN_CONFORMANCE_RECORD_TEMPLATE.md) | 每个 slice 开始前如何记录其设计符合性？ | 必填计划模板 |
| [`records/README.md`](records/README.md) | 哪些 slice 已有持久化 conformance record，评审和 catalog 状态是什么？ | 开发入口评审记录 |

## 3. 阅读顺序

1. 先读本索引和 [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md)，区分“已经实现”和“目标设计”；
2. 读 [`DEVELOPMENT_MODEL.md`](DEVELOPMENT_MODEL.md)，理解计划语言和状态含义；
3. 读 [`ROADMAP.md`](ROADMAP.md)，理解关键路径和阶段目标；
4. 进入 [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 查看具体工作；
5. 开始 slice 前逐项执行 [`QUALITY_GATES.md`](QUALITY_GATES.md)，并填写 conformance record；
6. 多条工作流并行时遵循 [`WORKSTREAMS.md`](WORKSTREAMS.md)。

## 4. 计划与事实的关系

[`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) 保存当前实现事实、已验证基础、历史控制摘要和目标差距；
未来阶段、依赖、slice 和 gate 由本目录其他文档定义。Slice 开始、状态变化、accepted commit 和证据
链接同步写入当前基线或由其引用的后续状态记录。

旧 Phase G、Blue/Red prompt 和 dogfood 详细流水已从当前文档集删除，通过 Git 历史追溯。旧 Phase G
未完成条目不能未经 [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 映射直接恢复。

## 5. 固定开发边界

- 产品始终是 CUDA → Ascend C，不为未来异构迁移预建产品抽象；
- Cairn 尚处 pre-release V1，修改当前 V1 并同步更新代码、测试、fixture 和文档，不增加兼容层；
- 强类型是 authority boundary，不以 generic ID、字符串 role、整数单位或布尔 outcome 换取短期速度；
- 每个 slice 必须产生可验证的纵向结果，不能只铺空 crate、空 trait 或未来 plugin 框架；
- Proposal、Execution、Admission、Record 和 Policy/User authority 不在临时实现中合并；
- 模型、知识、skill、CUDA observation 和历史 fixture 都不能因开发便利获得正式 authority；
- hardware-free、recorded、live-model、CUDA、Ascend build、Ascend NPU 和 model-integration 是不同 lane；
- 设计完成、代码编译、模型看似合理和真实设备运行分别是不同完成事实。

## 6. 当前启动条件

三个 P0 选择已经关闭：D-039 冻结首个 Intent operator/claim/corpus，D-040 冻结首个 verifier
qualification profile，D-041 冻结历史 fixture curation policy。它们关闭的是设计选择，不是 ST0 evidence。

首个新架构product increment按依赖顺序推进：

- DEV-003已由commit `79a1174`接受，建立最小`cairn-testkit` provenance/sanitation contract并生成sanitized
  V1 fixtures、public/private disposition和扫描记录；
- DEV-001已由commit `9dc8243`接受，复用该contract物化D-039 clean-room CUDA/host source、public corpus和
  independently reviewed restricted sealed corpus；
- 为 D-040 冻结十项 qualification contracts、独立 golden/mutation/fault controls和review assignment；
  exact implementation receipts由DEV-100/102/103/104在对应实现写出后、首次进入Gate前生成；
- 完成 DEV-004 的 `DesignConformanceRecord` 与 exact change inventory。

用户于2026-08-28接受DEV-002 [`DesignConformanceRecord`](records/DEV-002.md) review package `955a09d`；
entry由`9b2502d`闭合后DEV-002已进入`InProgress`。它只定义future mechanism的考试与复核边界，不预填qualification结果。见
[`records/README.md`](records/README.md)。

DEV-002 review-pending contract/control bundle已由commit `a713d00`物化；现在按
[`independent private review`](records/DEV-002-PRIVATE-REVIEW.md)审查exact public subject和private controls，
尚未生成control-review receipt。在DEV-002和DEV-004接受前，ST1代码slice仍是`Blocked`。
决策、fixture或qualification contract已接受不能被误报为mechanism已经qualified。
