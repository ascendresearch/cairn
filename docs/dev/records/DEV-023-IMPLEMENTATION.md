# DEV-023 implementation — Controller-owned Candidate process manager

> Historical record: D-044/DEV-036 deleted the proposal child-process and supervision portions
> described below. Scheduler and Worker receipt evidence remains historical input only.

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-023`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；本片未调用模型、远端 Worker、Docker 或 NPU

## 1. Objective

把DEV-021的durable `CandidateWorkflowNextActionV1`、DEV-022 generic proposal step child和现有Controller
scheduler/assignment recovery连接为一个Controller-owned单任务process manager。Manager只监管一个配置中明确
给出的、已经打开的`TaskId`；这是真实product consumer，不是新的逐步手工launcher，也不预建task catalog、Host
pool或多租户协议。

## 2. Authority与进程边界

| 边界 | 拥有 | 明确没有 |
| --- | --- | --- |
| Candidate workflow | exact next action、publication/diagnostic lineage、revision budget、terminal classification | model/provider、Worker credential、compiler解释 |
| Controller manager | commit-before-effect、Host process lifecycle、scheduler调用、receipt机械折回 | source修改、Intent/Oracle移动、Admission/verdict |
| Generic proposal step | exact role request、episode-local durable Agent Loop、typed proposal terminal | Worker/Controller DB、Docker/device、restricted material |
| Managed Worker | exact immutable contract、execution journal、worker-controlled receipt | Candidate role、model、workflow transition |

Server current-V1配置新增一个可选`candidate_workflow_manager`，只包含一个exact `TaskId`及一个严格
`ProposalStepProcessConfigV1`。未配置时Controller继续只提供worker control/scheduler；配置时启动前必须证明该task
workflow已存在，随后在同一active Controller进程中恢复并监管。

## 3. Current-V1 types与Host start authority

- 新增distinct positive `ProposalStepProcessTimeoutMillis`、`ProposalStepStdoutByteLimit`、
  `ProposalStepStderrByteLimit`与`CandidateWorkflowPollIntervalMillis`；反序列化重新执行上下界校验。
- 新增`ProposalStepBinaryIdentity`，只接受canonical lowercase SHA-256；它与`WorkerBinaryIdentity`静态不可互换。
- `ProposalStepRuntimeV1`直接修改current V1，冻结exact Host binary identity；Controller与Host分别从实际executable
  bytes重算并复核，没有V2、fallback reader或legacy alias。
- Controller在提交`CandidateEpisodeRequested`前，为exact episode建立`invocation.v1.json` marker。Host没有该
  marker就拒绝运行；第一次运行在打开episode stores后、任何模型effect前建立exact request-bound
  `started.v1.json`。
- 已有start或terminal marker但SQLite/CAS缺失时Host fail closed，不能因换state root或删除store而悄悄重开同一
  Episode并重复provider effect。正常terminal checkpoint仍byte-exact replay。

## 4. Durable action consumer

Manager每次只消费workflow投影选择的一个action：

1. `PrepareNativeBuild`：物化并归档exact contract，分配全部typed scheduler IDs，再把dispatch提交到task stream；
2. `ScheduleNativeBuild`：只使用已提交dispatch调用现有scheduler；Offer/Accepted/Running进入poll；
3. `ReconcileNativeBuild`：查询同一attempt，不创建replacement；
4. `PrepareCandidateEpisode`：冻结binary/model/selection/budget/limits，归档runtime并先准备Host operation marker，再提交
   episode request；
5. `RequestCandidateEpisode`：以bounded canonical stdin/stdout启动generic Host，复核binary/model/state marker、exit
   status和terminal binding，再把publication提交回同一workflow；
6. `Terminal`：停止该task supervisor并报告typed terminal。

Optimistic transition竞争后，loser只恢复task stream；如果另一manager已经推进，它接受durable winner，绝不继续
使用自己未提交的Host/Worker IDs。

## 5. Worker receipt折回与blocked semantics

`ScheduledAssignmentPhase::Terminal`和reconcile action均从exact `AttemptId`联合execution stream与CAS恢复：

- complete `SubjectFailed` receipt重新物化exact revision/follow-up/repair native build，复核job/input/environment/
  contract/attempt，读取typed stderr/evidence，调用已有领域diagnostic constructor，先归档diagnostic再提交workflow；
- `Succeeded`、cancel、timeout、infrastructure或integrity terminal经已有`record_candidate_native_terminal`分类；
- 不解析compiler diagnostic，不生成source patch，不把build success提升成semantic success。

`NoCandidate`、expired-before-start、NotStarted、Ambiguous、reconciliation-required以及Host timeout/exit/limit/
invocation drift都成为closed typed blocked status。它们保留原dispatch/request，不自动分配replacement ID。

## 6. 替换与删除

已删除/关闭：

- `cairn-server`对`archive_proposal_step_runtime`、`prepare_candidate_proposal_step_request`、
  `prepare_candidate_native_build_dispatch`与`schedule_candidate_native_build`的public re-export；
- 只通过上述public helpers证明手工物化的`proposal_step_materialization` integration test。

四项low-level mechanics保留为private manager implementation。Generic scheduler、native materializers、diagnostic
constructors、`cairn-proposal-step`和role-specific runners都有当前consumer，不是legacy path。更早的generic
Candidate build/revision live smoke不属于本suffix manager的等价路径，本片不删除。

## 7. Tests与controls

- 两个source path、source bytes与TaskId materially different的Candidate revision通过同一production manager
  action path；没有product branch或prompt change；
- 两者都先durably冻结native dispatch，再在无eligible Worker时得到同一typed `NoCandidate`语义；重复drive恢复同一
  placement/attempt/assignment IDs，不生成replacement；
- 两个并发manager竞争同一Ready task时只接受一个durable dispatch；loser恢复winner，后续NoCandidate仍引用原
  placement且不产生第二组effect IDs；
- 本地受控subject-failure receipt、stderr和no-device evidence经manager机械生成typed diagnostic并推进同一task；
- generic Host real-child test要求Controller-prepared invocation marker，验证recorded terminal/restart exact replay，
  并证明缺少operation marker或已启动后丢失store都fail closed；
- oversized ingress、noncanonical/cross-bound terminal、binary/model drift、non-V1/zero/out-of-bound config均在typed
  boundary拒绝；
- no-default server/migration checks、all-target all-feature Clippy、full CI、fixture/generic-ID扫描和
  `git diff --check`闭合。

所有provider response仍来自local scripted/recorded transport；没有live model、Worker、Docker、NPU、新live receipt
或verdict。

## 8. 明确非目标

- 不实现task intake、全局workflow catalog、多个configured workflow、Host pool/prewarm或multi-tenant supervisor；
- 不新增Oracle/Planner role或扩大SIR capability；
- 不授予Host Worker、Docker、device、restricted store、Admission或verdict authority；
- 不自动re-place `NoCandidate`、expired、NotStarted或ambiguous attempt；
- 不调用live model/remote Worker，不取得native success或NPU evidence；
- 不实现Candidate semantic/safety/performance Admission或最终`MigrationVerdict`；
- 不处理独立历史`matmul-zero-k` seam；
- 不增加compatibility、schema migration或V2。

下一slice应从真实remaining gap选择：为已闭合manager取得native build success并接入最小Candidate Admission/NPU lane，
或在有真实task intake consumer时增加task discovery。不能仅因已有单任务manager就预建全局catalog或Host pool。
