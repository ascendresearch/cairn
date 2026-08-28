# Cairn 开发 Workstreams

- 状态：规范性协作边界；近期 runtime-value first
- 日期：2026-08-28
- 路线图：[`ROADMAP.md`](ROADMAP.md)

## 1. 角色定位

Workstream 是代码 ownership，不是运行时 Agent 或产品 authority。Repository coding agent 是应用构建者和
外部观察者；DeepSeek 等 configured runtime model 是逐任务推理 actor；evaluator 在 episode 后使用 fixture
expected answer；未来 Admission/Gate 才可能形成正式 authority。这四个角色不得合并。

## 2. 当前 workstreams

| Workstream | 当前责任 | 禁止承担 |
| --- | --- | --- |
| `WS-AGENT` | 复用 domain-neutral episode/provider/tool/budget/replay runtime | fixture answer、CUDA operator branch、admitted constructor |
| `WS-PRODUCT` | task projection、generic SIR profile、typed proposal adapter | hidden answer、execution/Gate authority |
| `WS-QUALITY` | post-episode evaluation、context-absence、cross-task/value tests | 反向定义 prompt 或 product policy |
| `WS-RECORD` | durable identities/events/replay 与 public projection | verdict semantics |

Admission、Oracle、Candidate、Execution、Performance、Knowledge 等 future workstream 只在 DEV-005 Go 且出现
第一个真实 consumer 后展开，不为它们预建 crate、role 或 review assignment。

## 3. DEV-004 集成顺序

```text
WS-PRODUCT: generic task projection + typed proposal shape
→ WS-AGENT: existing runtime/profile/tool invocation
→ WS-RECORD: durable recorded episode and replay facts
→ WS-QUALITY: post-episode evaluation and leakage checks
→ opt-in live DeepSeek run
```

约束：

- expected/private fixture data 永远不出现在 model-visible projection；
- recorded response 必须经过同一 runtime decode/control flow，不能直接构造 domain output；
- live run 有明确 model/deployment/token/tool/wall budget；
- malformed output、missing citation、unknown/conflict 有 typed failure；
- 不创建独立 SIR/Proposal Host/Admission 空 crate。

## 4. DEV-005 协作

WS-PRODUCT 提供一个实质不同的 CUDA task；WS-AGENT 不改 profile schema 或 control flow；WS-QUALITY 比较
source-preserving、user-declared intent 和 runtime SIR 三条路径；WS-RECORD保留 exact episode/cost/failure。

Go 必须有 cross-task reuse 和至少一个 downstream utility。No-go 时SIR离开critical path，只保留已有的最小
task-generic extension seam与domain-neutral agent runtime，待端到端架构稳定或真实consumer出现再评估扩展。
停止过度投入是成功的工程结论，不等于永久否定SIR。

## 5. Review 强度

| Change | 需要的检查 |
| --- | --- |
| proposal-only profile/context | product + agent owner；model-visible absence test |
| recorded/live provider use | agent + record owner；budget/replay/failure classification |
| restricted/secret visibility | record/security review；capability/exposure/redaction |
| admitted constructor/Gate/policy | future independent authority + negative/tamper review |
| CUDA/Ascend external claim | execution owner + exact build/device receipt |

普通 fixture、内部 refactor 或 proposal-only value spike 不要求人为复制 applicant、reviewer、control author 和
receipt authority。只有真实 authority 或受限数据边界出现时才增加相应独立性。

## 6. 代码和变更规则

- production code task-generic，testkit 只从测试侧消费；
- semantic identities/types 不因共享表示而合并；
- V1 schema 改变时同步所有 current consumers，删除 superseded path；
- enabling work 与第一个 consumer 同 change set，避免空抽象；
- live provider/device 在 scope、预算和停止条件明确后运行；
- changed claim、actual commands、remaining unknown 写入事实账本，不复制冗长评审叙事；
- coding agent 自报、多个 Agent 共识或 fixture green 都不替代实际 workflow evidence。
