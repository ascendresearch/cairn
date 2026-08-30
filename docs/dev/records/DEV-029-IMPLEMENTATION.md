# DEV-029 implementation — Controller-authorized Proposal Host experiment round trip

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-029`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；未调用模型、远端 Worker、Docker、NPU 或互联网

## 1. Objective

补齐所有Proposal Loop共同需要的真实控制面接缝：Agent提出external-effect tool call后，Proposal Host不执行、
不报成不可恢复错误，也不重开episode；它返回request/episode/step/model-attempt/operation/implementation/effect/
exact arguments绑定的current-V1 durable yield。Controller重新验证Host journal，先提交operation start authority，
再允许选定的Worker adapter执行；receipt-bound observation归档后，同一个episode从原native continuation继续。

## 2. 可读业务骨架

`run_proposal_loop`顶层现在只表达：

```text
open or recover durable episode
→ dispatch/settle/admit Agent operations
→ execute Host-local operations or yield external operations
→ project observations
→ advance or complete
```

`execute_proposal_host_experiments`只表达：validate exact yield → recover durable bindings → prepare
Worker binding → commit tool-operation authorization/start → execute Worker adapter → archive canonical observation。
每一步继续由小函数和distinct strong types承载，不把恢复、权限或provenance塞回一个大函数。

## 3. Current-V1 contracts与authority

- `ProposalHostOutcomeV1`直接取代terminal-only stdout contract，只允许`Terminal`或
  `AwaitingController`；没有legacy reader、双写或版本升级；
- `ProposalHostExperimentRequestV1`绑定Host request、episode、step、model attempt和非空unique operations；
- 每个operation绑定`OperationId`、`ToolName`、`ToolImplementationVersion`、trusted
  `ToolEffectClass`、`ContentId<ToolArguments>`及exact canonical arguments；pure/read-only不能伪装成experiment；
- `ProposalHostExperimentDispatchV1`把Host operation与Controller选定的Worker job/attempt/contract绑定；
- `ProposalHostWorkerObservationV1`要求canonical `ExecutionReceipt`的job/attempt/contract及content identity与
  dispatch完全一致；模型可见`OperationResult`同时包含dispatch和完整receipt provenance；
- Proposal Host仍只有pure/read-only gateway authority。`authorize_tool_operation`和
  `begin_tool_operation`的durable facts在Worker adapter调用之前提交，Host和模型都不能制造start authority。

## 4. Same-episode resume与进程边界

Proposal Loop入口先调用`recover_agent_episode`。在external-effect safe point，它从step event history和CAS恢复
原`PreparedToolOperation`与native continuation；Controller observation完成后，loop先settle原step、把exact
`OperationResult`追加到continuation，再推进新step。不会重发提出experiment的model turn，也不会生成替代
episode/operation identity。

`cairn-proposal-host`对子进程yield和terminal都重开SQLite/CAS验证。只有terminal outcome写入terminal checkpoint；
yield保持episode可继续。Controller supervisor公开同一state directory上的验证/执行入口；Controller和Candidate
manager把yield报告成typed waiting/blocked状态，不再误分类为Host process failure。

## 5. 替代与删除的旧路径

- 删除`ProposalLoopError::ExternalEffectRequiresController(String)`硬错误路径；
- 删除terminal-only Proposal Host stdout解码假设，直接修改current V1为`ProposalHostOutcomeV1`；
- 删除Host restart必然要求completed episode的假设，改为分别验证terminal或bound-operation safe point；
- external effect不再经过Host-local `ToolGateway`，也没有fallback执行、generic-ID alias、兼容codec或隐式新episode。

## 6. Tests与静态controls

- external operation首次运行只产生durable yield，Host gateway调用次数为零；
- Controller先提交authorization/start，再记录observation；第二次loop调用恢复同一episode并只新增后续model turn；
- exact model dispatch count证明yielding turn没有重发；
- experiment operation strict decode拒绝Host-local effect和arguments/content-ID drift；
- Worker observation拒绝job/attempt/contract或receipt identity drift；
- generic Host recorded SIR child、terminal checkpoint和process restart仍通过current outcome contract；
- workspace format/check/Clippy/test、`git diff --check`、fixture/generic-ID/old-symbol扫描。

Production tool/API/prompt没有fixture identity、known answer、D-039/reduce-sum知识或fixture-derived分支。每个语义
身份继续使用专门类型；没有把episode、operation、job、attempt、contract、receipt或arguments压成generic ID/string。

## 7. 明确非目标

- 不新增某个SIR/Oracle/Candidate专用experiment tool或把任一实验设为必经步骤；
- 不在本片选择Oracle strategy、实现Oracle aggregate/Admission或连接完整Controller prefix/suffix；
- 不替具体tool implementation生成`JobContract`，Worker adapter仍由后续真实consumer按policy提供；
- 不调用live model、remote Worker、Docker或NPU，不产生新的runtime/receipt/correctness/adequacy/verdict claim；
- 不声称当前recorded control等于一次真实远端Worker实验。
