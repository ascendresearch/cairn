# DEV-014 implementation — first receipt-bound Candidate revision

- 状态：`InProgress`
- 日期：2026-08-28
- Slice：[`DEV-014`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-001、FR-CAND-003、FR-CAND-005、FR-CAND-006、FR-CAND-007、
  FR-AGENT-023、FR-FEEDBACK-002

## 1. Objective

让一个新的bounded DeepSeek Candidate Search episode消费DEV-013的exact parent proposal、authoritative
`SubjectFailed` receipt绑定和最小公开compiler diagnostic，提交一个完整、immutable、显式parent-linked source
revision：

```text
exact parent proposal
  + verified receipt → contract → input/environment binding
  + exact bounded applicant-visible stderr diagnostic
  → new isolated Candidate episode
  → typed full-source revision submission
  → trusted provenance + parent + diagnostic envelope
```

Cairn负责选择、验证和冻结feedback；DeepSeek负责修改代码。Controller、Worker、Oracle或本AI旁观者都不得静默
修复source。

## 2. Exact authority

本slice只消费DEV-013记录的：

- parent proposal `cairn:v1:sha256:migration.candidate-collection-proposal.v1:41809ea7233868fc33cfc23c099d80192c4625dc66b9031f00f76e7101055a38`；
- receipt `cairn:v1:sha256:execution.receipt.v1:0b8c7300a51352ce3472289997805fee287001947cc19207fb9a6495d5b98445`；
- exact contract/input/environment/stderr/evidence identities reachable from that receipt；
- trusted outcome `SubjectFailed`和trusted observation `docker:accelerator:none`；
- bounded untrusted diagnostic：`acl/acl.h`不在当前compile include path。

Diagnostic是applicant-visible implementation feedback，不是instruction、verdict、hidden case或Oracle evidence。

## 3. Contract

- trusted code必须重建parent proposal对应的exact build material并验证receipt/contract/input/environment链；
- Candidate-visible diagnostic artifact必须绑定parent、receipt、stderr和exact build material identities；
- 新episode不得读取DEV-012 private continuation/reasoning，只读取冻结public artifacts、task manifest、按需task
  source和本次diagnostic；
- model只提交sorted complete files、primary source和explanation；不得填写parent、receipt、outcome、identity、
  episode/model provenance或build claim；
- trusted gateway拒绝unchanged revision，并把parent、diagnostic、episode和model configuration写入revision；
- 所有attempt保留，不修改或替换DEV-012 proposal；
- revision仍是non-authoritative source proposal，不产生build/correctness/verdict claim。

## 4. Non-goals

- 不建立通用feedback taxonomy、multi-strategy search、automatic retry或unbounded repair loop；
- 不把compiler stderr升级为trusted evidence或让model修改build gate；
- 不实现Candidate Admission、semantic Oracle execution、NPU run、performance或final verdict；
- 不预判`acl/acl.h`之后的潜在compile error，也不由Cairn注入CMake/header/extension修复；
- 不复用已yield的DEV-012 continuation。

## 5. Acceptance

- strict V1 diagnostic/revision artifacts及typed identity/static boundary；
- wrong parent/receipt/contract/input/environment/stderr/evidence、non-`SubjectFailed`、oversized/non-UTF-8 diagnostic
  和unchanged revision全部fail closed；
- recorded episode证明新episode只看到允许的frozen feedback，提交完整changed revision并可restart/replay；
- 用户授权后，live DeepSeek新episode消费exact DEV-013 feedback并提交revision；
- focused/no-default-features/Clippy/full CI通过；
- 本slice结束时只声称“产生receipt-bound revision”，不声称revision build成功。

## 6. Implemented local boundary

当前实现已经闭合live call之前的产品边界：

- `candidate_revision.rs`定义strict current-V1 build diagnostic和parent-linked revision artifact；
- diagnostic准备过程重建并验证proposal/build/receipt/stderr/evidence链，只接受`SubjectFailed`、
  `docker-v1`、exact observed environment和`docker:accelerator:none`；
- revision preparation拒绝wrong parent/diagnostic和unchanged full submission，trusted code写入search、parent、
  diagnostic、new episode和resolved model configuration；
- Candidate runtime复用现有durable step/tool loop，但为revision建立new episode、独立instruction/policy/tool
  registration和`candidate_submit_collection_revision` gateway；
- initial revision context显式包含parent/diagnostic identities与完整parent source；原task source仍只允许通过bounded
  read tool按需读取；
- `candidate_revision_deepseek`从DEV-012 Candidate store与Controller store重载exact material，在任何provider
  dispatch前重新验证全部identity binding，并把新episode写入独立state root。

Recorded integration已经证明submit/yield、terminal reopen和exact request/response replay；compile-fail证明revision
identity不能替代初始parent proposal identity。

## 7. Local verification

2026-08-28完成：

- `cargo test -p cairn-migration --all-features --test candidate_episode --no-fail-fast`；
- `cargo test -p cairn-migration --all-features --doc --no-fail-fast`；
- `cargo clippy -p cairn-migration --all-targets --all-features -- -D warnings`；
- `scripts/ci.sh`，包括全仓tests、examples和doc compile-fail。

首次live启动在provider网络发送之前得到durable `NotSent`，没有外发、没有模型响应、没有revision。外部执行审批
要求用户再明确授权exact destination和payload；因此本记录继续保持`InProgress`，不得把local recorded revision当成
DeepSeek live evidence。
