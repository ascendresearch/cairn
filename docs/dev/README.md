# Cairn 开发计划设计

- 状态：规范性开发计划设计，尚未授权代码实施
- 日期：2026-08-27
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

首个新架构 product increment 仍被以下 P0 决策阻塞：

- `OQ-019`：首个 Intent Admission operator、claim set 与 corpus；
- `OQ-023`：首个 verifier mechanism qualification profile；
- `OQ-016`：被首批新架构测试复用的历史 fixture 如何净化和归档。

不依赖这些答案的只读审计和计划工作可以继续；会冻结生产 V1 intent/admission 类型或 corpus 的代码
不能自行选择默认值。
