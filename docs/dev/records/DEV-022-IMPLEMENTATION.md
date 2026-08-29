# DEV-022 implementation — generic role-scoped Proposal Host

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-022`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；本片未调用模型、远端 Worker、Docker 或 NPU

## 1. Objective

用一个真正的 `cairn-proposal-host` process 和一套 current-V1 typed contract 承载 SIR、Candidate initial、
Candidate revision、Candidate native follow-up 与 Candidate native repair role profile。领域角色继续拥有各自
prompt、tool gateway、continuation 与 publication 类型；Host 只负责共同的隔离、冻结输入、runtime binding、
durable episode 和 terminal boundary。

本片同时让 DEV-021 持久化的 `CandidateEpisodeRequestV1` 成为真实 Host consumer seam：Controller 从 public
CAS 按 exact typed identity 重建 Host request，Host publication 再提交回同一个 `MigrationWorkflowV1` aggregate。

## 2. Authority 与 capability

| 边界 | 拥有 | 明确没有 |
| --- | --- | --- |
| Controller composition | workflow next action、public CAS materialization、exact invocation snapshot | 模型推理、Candidate source修改、restricted store转交 |
| Proposal Host | exact role request、public task snapshot、model selection/budget、durable Agent episode | Worker credential、Docker/device access、Admission constructor、restricted/hidden material |
| role profile | role-specific prompt、tools、submission validation、typed publication | 调度、receipt、workflow terminal或verdict authority |
| Admission/Worker | 保持原有独立process/authority | 不因Host引入而合并或转交 |

同一 Host implementation 支持不同领域角色不等于共享 episode：每个请求冻结 distinct `EpisodeId`、model
configuration、context、budget、task snapshot、role variant 和 publication domain。不同 capability/OS boundary
仍必须启动不同 Host instance。

## 3. Current-V1 contract

- `ProposalHostRuntimeV1`冻结 episode、exact `AgentResolvedRuntimeModelArtifact`、selection、budget、output limit
  与 task limits，并以 `ProposalHostInvocationArtifact` identity 绑定 workflow request；
- `ProposalHostTaskSnapshotV1`只携带 Controller 已物化、重新验证的 bundle/source bytes，不传任意 filesystem root；
- closed `ProposalHostRoleRequestV1`区分五种当前消费者，反序列化后重新验证 task、recovery、parent、diagnostic、
  workflow request、episode 与 invocation binding；
- later native repair同时携带exact parent-repair artifact，Host重新推导immediate parent identity与root follow-up
  lineage；首轮repair必须没有parent-repair，不能只凭两个看似合理的ID拼接lineage；
- `ProposalHostTerminalV1`绑定 exact request/episode、role-specific publication identity、completion reason 与 step count；
- `CandidateEpisodeRequestV1`直接加入 distinct typed invocation identity；旧 current-V1 读写路径同步修改，没有
  V2、legacy alias、dual reader/writer 或 converter；
- 原 `SirResolvedRuntimeModelArtifact`的角色泄漏命名直接替换为
  `AgentResolvedRuntimeModelArtifact`，content domain 保持当前通用 `agent.resolved-runtime-model.v1`，没有 generic ID。

## 4. Durable process 与 workflow consumer

`cairn-proposal-host`从 canonical stdin 接收一个请求，验证 exact resolved-model bytes，在 episode 专属 SQLite
event/CAS 中运行现有 durable agent loop。正常完成后，它重新打开 store 校验 durable Agent terminal，再原子保存
canonical `terminal.v1.json` checkpoint；同一请求的后续进程启动先验证 checkpoint、request binding 与 Agent
projection，然后原样重放 terminal，不再次 dispatch 模型。

如果进程在 Agent episode 已完成、Host checkpoint 尚未提交的窄窗口崩溃，底层 episode identity 会阻止隐式
重开/重复模型 effect；该状态需要显式 reconciliation。本片没有用猜测或自动重试掩盖跨存储提交窗口。

`cairn-server`新增 composition：归档 exact Host runtime invocation，并从 workflow request 指向的 public CAS
加载 canonical search/recovery/task/parent/diagnostic material。Host只返回 proposal；native follow-up/repair
publication 仍通过既有 workflow command 回到同一个 task aggregate，由 Controller 决定下一步。

## 5. 替换与删除

已直接删除、未保留兼容路径：

- `cairn-sir` crate、binary 及其 process/live authority tests；
- `sir_process` module/export；
- SIR、Candidate initial、Candidate revision 三个 one-shot DeepSeek examples；
- 上述三条路径对应的 example JSON configs。

保留 domain-specific SIR/Candidate runners，因为它们是通用 Host 的 role profile consumer，不是平行 process
launcher。`cairn-admission`仍为独立 model-free authority。

## 6. Tests 与 controls

- 同一个 production Host function 在不同 episode 中运行 SIR 与 materially different Candidate task/profile；
- full durable workflow 从 persisted native follow-up request，经 Controller/Host contract返回 publication 到同一 aggregate；
- non-V1、task snapshot drift、workflow invocation drift、publication/diagnostic domain drift fail closed；
- real child process 只消费 canonical stdin，拒绝 oversized ingress，并在 SQLite reopen 后验证 terminal；
- 第二次 child launch不提供 recorded exchange，byte-exact重放 checkpoint，证明没有第二次模型 dispatch；
- compile-fail/static boundary证明 model configuration ID 不能替代 Host invocation ID；
- no-default-features server/migration checks、all-feature Clippy、full CI、fixture/generic-ID扫描与 `git diff --check`。

所有模型响应都来自本地 scripted/recorded transport。没有 live model、Worker、Docker、NPU、新 receipt 或 verdict。

## 7. 明确非目标

- 不实现完整 top-level Controller process manager、Host pool/supervisor、IPC authentication 或多租户 sandbox；
- 不增加 Oracle synthesis/adversarial/Planner profile；只有出现真实 consumer 才扩展 closed role enum；
- 不授予 Host Worker、Docker、network research、restricted store、Admission 或 verdict capability；
- 不自动解释 compiler diagnostic、修复 source 或推进下一轮 build；
- 不重新运行历史 live SIR/Candidate，也不声称 recorded response 证明模型质量；
- 不证明 native build success、NPU semantic/safety/performance 或最终 `MigrationVerdict`；
- 不处理独立历史 `matmul-zero-k` product seam，也不把其 fixture identity带入 Host；
- 不建立 compatibility、schema migration或V2。
