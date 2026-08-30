# DEV-026 implementation — durable Controller SIR-to-user-decision prefix

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-026`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；本片未调用模型、远端 Worker、Docker、NPU 或互联网

> Current correction：DEV-028已supersede本record中的固定Blue/Red composition形状；当前Controller骨架使用
> policy-selected `Oracle Exploration → Oracle Admission`。本record其余内容保留为当时实现事实。

## 1. Objective

把DEV-025的第一个真实空接缝接入task-owned durable Controller aggregate，同时保持业务流程本身就是可读的
架构骨架：冻结exact SIR Host request/input，先提交durable start authority，再运行统一Proposal Host，把terminal
作为带provenance的proposal observation持久化，机械生成用户意图决策请求，然后明确停在
`AwaitingUserIntentDecision`。Controller不得代替用户选择，也不得提前调用Intent Admission。

## 2. 可读业务骨架

active prefix driver `drive_controller_workflow_once`只表达：

```text
recover_controller_turn
→ select_controller_action
→ execute_controller_action
```

durable action/state骨架一眼可见：

```text
Frozen
→ AuthorizeSirEpisode
→ RunSirEpisode
→ DeriveIntentDecisionRequests
→ AwaitUserIntentDecision
```

每个步骤由小函数承担CAS、event、process和binding细节。完整composition skeleton同步纠正为：

```text
freeze → SIR → derive decision requests → await user decision → Intent Admission
→ Oracle Blue → Oracle Red → Oracle Admission → Candidate
→ Worker observations → Candidate Admission → terminal
```

## 3. Authority与持久化

- `ControllerWorkflowV1`以`TaskId`拥有独立`controller-workflow` event stream；
- `FrozenSirAuthorityV1`保存distinct typed task/request/recovery-input/episode identities，不使用generic ID；
- exact Host request和`IntentRecoveryInputV1`在任何模型effect前进入public CAS；
- `migration.controller-sir-episode-authorized`必须先提交，Host process才可启动；
- SIR terminal、proposal和decision-request batch各以自己的content domain归档；
- proposal必须同时绑定exact recovery input、episode和resolved model configuration；
- decision batch必须绑定exact proposal和recovery input；最终状态只给出等待用户的action。

## 4. 统一Host监督

通用Proposal Host的binary/model/runtime freeze、timeout、stdout/stderr bound、invocation marker和drift validation
已从`candidate_manager`搬到`proposal_host_supervisor`。SIR和Candidate消费同一真实process seam，不存在SIR专用
进程或复制的监督逻辑。invocation marker继续使用`create_new`阻止同一episode的并发/隐式重复启动；启动窗口不明
时必须reconciliation，不能用相同bytes作为再次执行外部effect的授权。

## 5. Tests与controls

- durable prefix exact order与restart recovery；
- cross-task SIR request拒绝；
- proposal model identity drift拒绝；
- exact command replay幂等、changed timestamp冲突；
- final next action严格为`AwaitUserIntentDecision`；
- compile-fail证明SIR proposal ID不能替代Host request ID；
- DEV-025完整skeleton ordering/fail-closed和Candidate manager controls继续通过。

测试使用公开fixture只构造测试输入；production prompt、event、API、状态和policy没有fixture name、expected answer、
fixture-specific branch或generic content identity。

## 6. 替代/删除的旧路径

- 通用Host supervision从Candidate专属模块删除，由SIR/Candidate共享模块替代；
- DEV-025中`SIR → Intent Admission`的过粗骨架被当前V1直接改为
  `SIR → decision requests → user decision → Intent Admission`；
- 没有保留alias、dual path、fallback reader或版本转换。

## 7. 明确非目标

- 不采集或伪造用户决定，不运行Intent Admission；
- 不接Oracle Blue/Red、Oracle Admission、Candidate Admission或global terminal；
- 不实现Controller↔Host external experiment yield/resume；
- 不增加task intake、multi-task catalog、Host pool或长期SIR service；
- 不产生live model/Worker/NPU evidence，不声称native success、semantic correctness或verdict。
