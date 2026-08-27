# Cairn 开发计划模型

- 状态：规范性开发计划设计，尚未授权代码实施
- 日期：2026-08-27
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 索引：[`README.md`](README.md)

## 1. 核心结论

Cairn 使用“evidence-ordered vertical development”：计划按可证明的产品结果排序，不按文件、crate、
Agent 名称或团队职能堆积任务。最小计划单元是一个有明确 authority crossing、可执行控制和耐久证据的
development slice。

计划层次为：

```text
Development Program
  → Program Stage
    → Integration Increment
      → Development Slice
        → Change Set / Test Lane / Evidence Record
```

- Program Stage：对应一组产品能力和里程碑条件；
- Integration Increment：形成一个可演示、可重放但未必可发布的纵向结果；
- Development Slice：一次可独立审查和验收的最小开发承诺；
- Change Set：具体代码、schema、fixture、文档和配置修改，不单独代表完成；
- Evidence Record：证明 slice exit criteria 的 durable 输出。

## 2. 为什么不用原来的 Phase G 直接续写

旧 Phase G 同时承担历史日志、局部设计、下一步清单和完成声明，导致：

- transport/materialization 控制容易被误当最终 Oracle 架构；
- 已实现事实与被架构重置暂停的未来顺序混在一起；
- 固定 Blue/Red dogfood 容易自然外推为永久 Agent 拓扑；
- 新的 SIR、typed Admission、hardware/performance、feedback 和 knowledge/skill 边界无法插入；
- 一个大阶段内部的“in progress”不能说明具体 authority crossing 是否闭合。

旧条目因此按 [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) 分类复用，不整体迁移为新路线图。

## 3. Slice 设计规则

一个合法 slice 必须同时具备：

1. 单一、可观察的 objective；
2. 明确非目标，防止顺手扩大范围；
3. requirement、decision 和 focused design 引用；
4. 明确 applicant、proposal、authority、execution 和 record 边界；
5. 冻结输入与 typed 输出；
6. positive、negative 和至少一个适用的 conflict/unknown/bypass/tamper control；
7. hardware-free 或 recorded lane，除非能力本质上只能在真实硬件验证；
8. 对真实模型/硬件/外部服务的单独 lane 和 `NotExecuted` 语义；
9. crash/restart/replay 要求，或说明为什么该 slice 没有耐久 effect；
10. 完成后仍未知、未执行和未覆盖的范围；
11. 删除 superseded V1 路径的计划，不保留兼容 facade；
12. `DesignConformanceRecord`。

仅增加类型、trait、空进程、feature flag、future-proof adapter 或 mock pass 不构成合法 slice objective。
必要的 enabling work 应与第一个消费它的纵向结果放在同一 increment，或拥有独立的边界证明。

## 4. Slice 身份与状态

计划文档使用稳定的人类可读 `DEV-xxx` 标识；未来生产系统若持久化开发记录，应使用独立验证类型，
不能把该标签当作 task、run、episode、commit 或 artifact identity。

状态集合：

| 状态 | 含义 |
| --- | --- |
| `Proposed` | 已进入 catalog，但依赖、设计或验收仍可能变化 |
| `Blocked` | 一个具名决策、authority、环境或前置证据阻止开始 |
| `Ready` | entry gate 全部满足，可以被明确授权实施 |
| `InProgress` | 已有授权且存在活动 change set |
| `EvidencePending` | 代码已完成，但一个 required test/evidence lane 尚未闭合 |
| `Accepted` | exit gate 通过，事实账本记录 commit 与证据 |
| `RevalidationRequired` | 已接受事实仍保留，但新反馈、撤回或机制 refutation 要求后续重验证 |
| `Superseded` | 在开始前被新设计替代，或开发态旧路径被删除 |
| `Abandoned` | 有记录地停止且不宣称完成 |

禁止用百分比表达完成度。`90%` 无法说明缺失的是文档、真实 NPU，还是 Mechanical Gate。使用具体
未闭合 obligation 和上述状态。

### 4.1 状态转换

```text
Proposed → Blocked | Ready
Blocked → Proposed | Ready | Superseded | Abandoned
Ready → InProgress | Blocked | Superseded
InProgress → EvidencePending | Accepted | Blocked | Abandoned
EvidencePending → InProgress | Accepted | Blocked
Accepted → RevalidationRequired
RevalidationRequired → Proposed | Ready | Superseded
```

`RevalidationRequired` 不修改历史 `Accepted` 事实，而是创建后续 slice 或 impact record。计划状态不是
Cairn 内部持久化 schema 版本。

## 5. 依赖类型

依赖必须标明原因，不能只画先后箭头：

| 依赖 | 含义 |
| --- | --- |
| `ContractBeforeConsumer` | typed contract 和 invariants 先于消费方 |
| `AuthorityBeforeProposalUse` | proposal 可产生，但在 authority 建立前不能进入正式下游 |
| `QualificationBeforeGate` | verifier mechanism 先 qualification，后参与 Gate |
| `RecordedBeforeLive` | recorded/offline path 先闭合，再购买 live model/hardware |
| `BuildBeforeDevice` | target build identity 先于 device execution |
| `CorrectnessBeforePerformance` | required correctness 通过前不花稀缺性能预算 |
| `PublicBeforeRestricted` | public path 先验证协议，restricted path 仍需独立 capability 证明 |
| `FirstShapeBeforeSecondShape` | 第一个纵向路径证明闭合，第二个 operator 检验抽象边界 |
| `DecisionBeforeV1Freeze` | open question 在相应生产 V1 类型/政策前关闭 |

同一依赖可能允许代码并行，但禁止结果晋级。例如 Ascend Worker adapter 可与 Oracle proposal schema
并行开发，但没有 admitted Oracle 时不能宣称统一迁移完成。

## 6. Stage 晋级

Stage 不是“所有相关代码都写完”，而是满足一个系统能力断言。晋级需要：

- 所有 critical slices 为 `Accepted`；
- required evidence lanes 已运行，缺失硬件不能被 skip 伪装为 green；
- stage-level backwards audit 可从结论回到 artifact/receipt/event；
- 已知 blind spots、unknown、not-executed 和 open questions 被保留；
- architecture/requirements/docs 与当前 V1 一致；
- 没有为兼容旧开发数据保留 dual path；
- 下一 Stage 不需要猜测本 Stage 的 authority 或 contract。

## 7. 计划与事实的双账本

计划和事实分开：

```text
docs/dev/*
  owns desired order, slice contract, entry/exit gates

docs/dev/CURRENT_BASELINE.md
  owns current checkpoint, actual status, accepted commit/evidence summary

git + durable test artifacts
  own detailed historical ledger and concrete implementation evidence
```

一次状态更新必须同时检查：

- 计划 slice 是否仍符合规范；
- 实际 change set 是否越界；
- 证据是否来自声明的 lane；
- 是否出现新 blocker 或 design change；
- 是否需要同步 requirements/decisions/focused design。

代码存在不自动把 `Proposed` 变成 `Accepted`；文档写完也不自动把未实现能力变为事实。

## 8. 变更控制

以下变化需要回到规范层，而不是在 slice 内自行决定：

- 产品范围超出 CUDA → Ascend C；
- Proposal 获得 admitted constructor 或 hidden data；
- 新的 source-behavior authority 排序；
- required correctness/performance plane 被删除或合并；
- Agent interaction 改为共享上下文或投票；
- 新的 schema version、compatibility reader 或 migration path；
- 新的 public compatibility baseline；
- 未决 policy 被硬编码为默认值。

纯 adapter、测试 fixture、部署参数或实现算法可以在既定 contract 内演进，但仍需其 slice 记录 changed
variable、风险和 revalidation 范围。

## 9. 成本与优先级

默认调度顺序：

1. 会改变 authority/type contract 的便宜决策和 compile-time proof；
2. hardware-free deterministic/recorded control；
3. 可恢复的进程和存储边界；
4. live model 或普通 CPU/CUDA/Ascend build；
5. 稀缺 Ascend NPU correctness；
6. 稀缺 profiling/performance；
7. model-integration/production feedback。

成本优化不能改变 gate。预算不足产生 `Blocked`、`EvidencePending` 或明确 `NotExecuted`，不能缩减 required
evidence 后继续标记完成。
