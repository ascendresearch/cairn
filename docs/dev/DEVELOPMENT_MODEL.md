# Cairn 开发计划模型

- 状态：规范性；evidence-ordered vertical development
- 日期：2026-08-28
- 产品范围：仅限 CUDA → Ascend C 算子移植

## 1. 计划单位

开发按可证伪的产品结果排序，不按目标架构中的 crate、process、Agent role 或团队职能展开。

```text
Product question
→ smallest runtime workflow
→ observed value or failure
→ next consumer-driven slice or deletion
```

`Development Slice` 是最小承诺：一个可观察 objective、一个真实 consumer、明确输入/输出/非目标和足以
推翻该 objective 的测试。类型、fixture、空 trait、future adapter 或 review package 本身不是 vertical result。

## 2. Runtime-model-first 原则

对 SIR、Oracle synthesis、Candidate generation 等模型能力：

1. 必须由 configured runtime model 经过真实 production control flow 产生结果；
2. coding agent 只实现 orchestration、tools、typed boundaries、persistence 和 evaluation；
3. fixture expected answer 只在 episode 结束后由 evaluator 使用；
4. recorded episode 证明协议/replay，opt-in live episode证明所声称的模型行为；
5. 第一个 fixture 接线后，必须用实质不同任务验证同一路径；
6. 没有 downstream utility 就停止或删除该模型能力，不用更多 ceremony 延后结论。

## 3. Slice 最低要求

每个 slice 必须说明：

- objective、first consumer 和明确非目标；
- production inputs、typed outputs、authority/visibility 边界；
- positive control 与最主要失败模式的 negative control；
- 适用的 recorded/live/model/hardware lane，未运行项明确为 `NotExecuted`；
- restart/replay 要求，或为何没有 durable effect；
- superseded V1 code/tests/data 的删除项；
- acceptance 后仍 unknown 的事实和下一项 go/no-go。

只有 authority、restricted/secret visibility、external effect、public API 或 persisted/wire contract 变化才强制
使用精简 `DesignConformanceRecord`。普通 fixture、proposal-only 内部模块和 refactor 使用 catalog scope note
与测试即可。

## 4. 状态

| 状态 | 含义 |
| --- | --- |
| `Proposed` | objective 已提出，scope 尚待确认 |
| `Blocked` | 具名事实或依赖阻止开始 |
| `Ready` | scope 和 entry evidence 足够，可授权实施 |
| `InProgress` | 已授权且存在活动 change set |
| `EvidencePending` | 实现完成，required external lane 未闭合 |
| `Accepted` | objective 被实际 consumer 执行，evidence 与 remaining gaps 已记录 |
| `RevalidationRequired` | 新事实要求重验证已接受结论 |
| `Superseded` | 当前 V1 路径已被替代并删除 |
| `Abandoned` | 有记录地停止且不宣称完成 |

禁止用百分比表达完成度。历史 `Accepted` 只描述当时发生过的事实；如果其设计被替代，current tree 和
catalog 直接标 `Superseded`，不保留兼容实现。

## 5. 依赖

| 依赖 | 含义 |
| --- | --- |
| `RuntimeValueBeforeAuthority` | 先证明模型能力有产品价值，再建设正式 Admission/Gate |
| `ConsumerBeforeMechanism` | consumer 与风险存在后才实现其 verifier/qualification |
| `RecordedBeforeLive` | 先闭合可重放协议，再花 live model/device 预算 |
| `FirstShapeBeforeSecondShape` | 首个路径接通后用不同任务检验抽象边界 |
| `BuildBeforeDevice` | exact target build identity 先于设备执行 |
| `CorrectnessBeforePerformance` | required correctness 先于稀缺性能预算 |

`QualificationBeforeGate` 仍适用于未来真实 Gate：verdict-relevant mechanism 在首次产生 authority 前必须按
exact implementation/scope qualification。但它不是预建考试、fixture taxonomy 或当前 proposal proof 的理由。

## 6. 证据顺序

默认成本顺序取决于 objective：

1. 便宜的 type/invariant/unit proof；
2. 实际 recorded workflow；
3. 若 objective 声称模型能力，尽早做受预算约束的 live model run；
4. 只有结果有价值后补 durable authority/process 边界；
5. CUDA/Ascend build、device correctness 和 performance 按真实 claim 逐级执行。

“先把完整框架做严谨再看模型有没有用”不是风险降低，因为它把成本花在尚未成立的产品假设上。

## 7. 计划与事实

- `ROADMAP.md` / `SLICE_CATALOG.md` 管当前顺序和 scope；
- `CURRENT_BASELINE.md` 只记录 current tree 和已发生 evidence；
- Git 保存详细历史，不在 current docs 复制已 supersede 的 review 流水；
- requirements/decisions/focused design 改变时同步更新受影响文档；
- 代码存在、测试 green 或模型输出看似合理都不能自动升级产品结论。
