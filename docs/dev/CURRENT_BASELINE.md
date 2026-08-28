# Cairn 当前开发基线

- 状态：当前事实账本；不把目标设计误报为实现
- 日期：2026-08-28
- 产品范围：仅限 CUDA → Ascend C 算子移植

## 1. 当前结论

Cairn 已有可复用的 durable agent runtime、record/replay、tool loop、provider protocols、worker/scheduler 和
局部验证基础，但新的 CUDA → Ascend C 端到端 workflow 尚未完成。

CP0已经证明DeepSeek可作为runtime actor面对不同task产生task-generic SIR proposal，并在atomic compaction
task上改变一个具体Oracle选择。SIR当前以proposal-only seam进入架构，未来可随真实consumer扩展；这不授权
现在建设完整SIR → Admission → Oracle权威链。下一步应围绕第一个真实迁移结果规划最小consumer，而不是
恢复预建架构。

## 2. 可复用基础

| 基础 | 已有事实 | 当前限制 |
| --- | --- | --- |
| Record/protocol | 强类型 V1 codec、CAS/event、durable identity、record/replay、SQLite fault/restart | 不自动具有 product authority 或 restricted capability |
| Agent runtime | OpenAI-compatible/Anthropic paths、DeepSeek deployment、episode/tool/budget/repair、recorded provider | 保持 domain-neutral；旧 Blue/Red 拓扑不是目标产品拓扑 |
| Execution | scheduler/lease/attempt/output、Docker、CUDA/Ascend build 的历史证据 | Worker 不解释 operator intent，不把历史 run 变成当前 claim |
| Verification mechanics | comparison、mutation、receipt binding 和历史 reduction controls | 只有出现真实 Gate consumer 后才按 exact implementation qualification |
| Testkit | DEV-003 provenance/sanitation；DEV-001 clean-room reduction fixture | evaluator-only；production crate 不得依赖或读取 expected/private answer |

## 3. 历史证据的边界

- Blue/Red dogfood 证明 durable model/tool loop 和 artifact-mediated revision 的一部分，不证明 debate 是
  Admission 或固定 Agent topology。
- `matmul-zero-k` 证明一条狭窄 materialization/call-adapter 路径，不代表一般 Oracle coverage。
- historical reduction 证明若干 comparison/mutation blind spot，只作为 control；旧 domain shape 不定义新
  Intent/Oracle schema。
- DEV-001 commit `9dc8243` 和 DEV-003 commit `79a1174` 保留为 current evaluation foundation。
- DEV-002 的 review 在历史上确实发生，但 D-042 已 supersede 其预建 D-040 qualification 方向；对应 code、
  tests、public/private bundle 和 private review record 从 current V1 tree 删除，Git history 足以追溯。

## 4. 当前仍未完成

- 正式 Intent Admission、Oracle/Candidate authority chain；
- 统一 CUDA reference → Ascend build/NPU evidence graph；
- performance、knowledge/skill、feedback 和 platform/release hardening。

目标设计中的独立 process、十一位置 catalog、七类 Planner、mechanism registry 和 future crate 只是条件设计，
不是当前待办或已实现事实。

## 5. 当前必须停止的外推

- coding agent 根据 fixture 答案生成“模型 proposal”；
- 把 `reduce-sum-f32`、D-039 identity、expected hypotheses 写入 product prompt/type/policy；
- 用更多固定 case 数量代表 SIR 或 Oracle 已泛化；
- 在证明 SIR value 前创建 Admission/qualification/process/role 框架；
- 让 Controller/Proposal runtime 读取 restricted answer；
- 为 superseded development format 保留 alias、reader、converter 或 migration；
- 将 recorded 误报为 live、build 误报为 device run、合理 prose 误报为 correctness。

## 6. 当前输入与近期输出

当前可用输入：

```text
generic durable agent runtime + recorded provider
+ task artifacts and scoped source-inspection tools
+ DEV-001 evaluation fixture (answer visible only after episode)
+ DEV-003 sanitation/provenance controls
```

DEV-004 近期输出只允许是：

```text
typed cited facts
+ competing intent hypotheses
+ calibrated unknown/conflict
+ durable recorded/live episode facts
```

它不是 `MigrationIntentContract`，没有 hidden access、execution、Gate 或 verdict authority。DEV-005已用同一
production path处理atomic compaction task，并证明proposal可阻止把atomic output order误升格为intent。

## 7. 当前状态

| Slice | 状态 | 事实 |
| --- | --- | --- |
| DEV-001 | Accepted | reduction evaluator fixture；不供 runtime answer projection |
| DEV-002 | Superseded | 过早 qualification framework 已从 current tree 删除 |
| DEV-003 | Accepted | 最小 fixture provenance/sanitation foundation |
| DEV-004 | Accepted | generic proposal path经recorded replay、full CI和真实DeepSeek episode闭合；只证明runtime workflow接通 |
| DEV-005 | Accepted | reduction与atomic compaction共享production path；SIR改变order-sensitive Oracle选择，CP0 Go |

详细历史保留在 Git；当前状态以本表和 [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 为准。
