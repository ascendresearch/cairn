# Cairn 开发质量 Gate

- 状态：规范性、风险分级
- 日期：2026-08-29
- Slice catalog：[`SLICE_CATALOG.md`](SLICE_CATALOG.md)

## 1. 原则

Gate用于发现真实产品风险，不用于为每个fixture或内部类型制造同等ceremony。验证强度由change实际跨越的
boundary决定。一个测试用例不能定义production architecture；一个看似严谨的review receipt也不能替代
runtime-model或真实设备执行。

## 2. G0 — Objective与泄漏边界

开始前必须明确：

- 当前可观察objective和第一个consumer；
- production input/output与明确非目标；
- coding agent、runtime model、evaluator和authority各自角色；
- fixture expected/private material是否被排除在model-visible context之外；
- applicable recorded/live model/hardware lane和预算；
- superseded code/tests的删除列表。

Authority、restricted/secret visibility、external effect、public API或persisted/wire contract变化需要精简DCR；
pure proposal、fixture、内部refactor不强制第三人review。

## 3. G1 — Production边界

- generic runtime/Worker无CUDA operator或fixture vocabulary；
- production crate不依赖`cairn-testkit`；
- task A/B经同一production control flow；
- proposal/admitted、observation/receipt、public/restricted等易混淆概念保持强类型；
- deserialization重跑constructor invariants并strictly拒绝non-V1；
- pre-release修改直接替换V1，无alias/converter/dual path；
- test-only expected values不可进入profile、prompt、tool output或runtime policy。

只有触及这些边界时才要求compile-fail/architecture test；不为普通值对象机械增加静态测试。

## 4. G2 — 实际workflow

Agent能力必须至少有：

- recorded或scripted runtime episode，而不是直接构造“模型输出”；
- exact profile/context/tool/budget identity；
- positive和一个针对主要失败模式的negative control；
- malformed output、missing citation或unknown处理；
- durable restart/replay（若使用现有durable runtime）；
- model-visible context absence test，证明fixture answer/restricted bytes未注入。

Pure function按其风险运行unit/property tests即可。未跨外部effect不要求虚构crash matrix。

## 5. G3 — External lanes

| Lane | 何时required | 最小事实 |
| --- | --- | --- |
| `RecordedWorkflow` | model/tool workflow进入产品路径 | exact request/response/tool replay和失败分类 |
| `LiveModel` | 声称runtime model能力或质量 | exact model/deployment/profile、预算、raw outcome classification；不形成authority |
| `CudaBuildRun` | 声称CUDA实际行为 | exact source/binary/device/launch/output receipt |
| `AscendBuild` | 声称target可构建 | exact source/toolchain/binary |
| `AscendNpuRun` | 声称target设备行为 | exact binary/device/launch/output |

未运行的required lane使slice保持pending；optional lane记录`NotExecuted`。不为当前objective无关的lane增加工作。

## 6. G4 — Acceptance

- objective由实际consumer执行；
- standard format/lint/unit/integration checks green；
- 适用的architecture、secret/body和`git diff --check`通过；
- superseded current-V1 code/tests/fixtures已删除；
- fact ledger只写实际scope、evidence和remaining gaps；
- 未把fixture通过误报为泛化、model prose误报为正确、recorded误报为live；
- 下一步由新事实触发，不从远期架构图自动展开。

## 7. 额外review触发器

| Change | 额外review |
| --- | --- |
| admitted constructor/Gate/policy | independent authority与negative/tamper review |
| restricted/secret path | capability、exposure、redaction与secret scan |
| provider/tool external effect | recorded counterpart、budget、ambiguous-effect/replay |
| CUDA/Ascend adapter | binary/device/ABI/launch/capture binding |
| public/persisted/wire contract | consumer audit、strict V1、negative decode |

普通fixture expected update、test-only manifest和proposal-only内部模块不自动触发上述全部review。

## 8. DEV-004/005 历史特例

- DEV-004不需要Admission、mechanism qualification、private review或NPU lane；
- required lanes是recorded workflow，声明DeepSeek能力时增加opt-in live model；
- DEV-005必须证明cross-task path和downstream utility；
- 若DEV-005 No-go，SIR离开critical path、保留最小generic extension seam并停止扩建即为合格closure。
