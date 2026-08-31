# DEV-025 implementation — readable Controller workflow skeleton

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-025`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；本片未调用模型、远端 Worker、Docker 或 NPU

> Current correction：DEV-028已supersede本record中的固定`Oracle Blue → Oracle Red`stage形状；当前
> Controller骨架是`Oracle Exploration → Oracle Admission`。本record其余内容保留为当时实现事实。

## 1. Objective

让Controller业务流程本身成为可读的架构骨架。先冻结完整CUDA→Ascend C产品顺序，再逐个把已有实现接到明确
stage port；尚未实现的Oracle Blue/Red、Oracle Admission和Candidate Admission保留为无默认实现的typed port，
不能用no-op或伪artifact冒充成功。

同时把现有真实Candidate native suffix从一个大`drive_candidate_workflow_once` match重构为
`recover → select → execute`三步Controller subflow，每个exact action只进入一个小函数。

## 2. 完整Controller骨架

`run_controller_workflow`现在只表达：

```text
freeze Controller request
→ SIR Proposal Loop
→ Intent Admission Gate
→ Oracle Blue Proposal Loop
→ Oracle Red Proposal Loop
→ Oracle Admission Gate
→ Candidate Proposal Loop
→ Worker observations
→ Candidate Admission Gate
→ save terminal outcome
```

`ControllerWorkflowStages`为每一环定义独立associated artifact type和async stage port。Proposal、admitted
authority、observation与terminal不能通过骨架API互换。Trait没有default implementation；一个环节没有real
implementation时，concrete driver必须显式返回error，骨架会立即停止，不能继续产生下游authority。

这是一项有recorded ordering/fail-closed test consumer的composition skeleton，不是完整product workflow已经接通
的声明。

## 3. 当前真实Candidate subflow

`drive_candidate_workflow_once`只保留：

```text
recover_candidate_controller_turn
→ select_candidate_controller_action
→ execute_candidate_controller_action
```

action router一眼展示当前已实现的真实分支：freeze native build、schedule Worker、reconcile observation、freeze
Candidate proposal step episode、run Candidate Proposal Loop、return terminal。原有dispatch commit-before-effect、
Host invocation marker、receipt folding、blocked semantics和optimistic recovery不变，具体代码限制在各自函数内。

## 4. 占位规则

- stage port只有签名，没有返回成功值的空body；
- associated types由未来real implementation绑定到已有或新增的strict domain artifacts；
- 不为未实现环节创建persisted event、content domain、schema version、generic ID或compatibility path；
- unavailable/error stage终止骨架，测试证明Oracle Blue失败后Oracle Red及所有下游stage都不会运行；
- 接入一个stage时只修改其implementation与相邻artifact binding，不把业务细节放回顶层骨架。

## 5. Tests与controls

- recorded stage driver证明完整十阶段顺序与顶层代码一致；
- injected unavailable Oracle Blue证明fail closed且没有下游authority；
- 现有Candidate manager two-material、concurrency、NoCandidate exact-ID和config controls复用真实
  `recover/select/execute`路径；
- full workspace format/check/clippy/test通过。

## 6. 明确非目标

- 没有把Oracle Blue/Red、Oracle Admission、Candidate Admission接到production runtime；
- 没有宣称SIR→Intent→Oracle→Candidate端到端Controller aggregate已经存在；
- 没有调用live model、remote Worker、Docker、NPU或互联网；
- 没有新增task intake/global catalog、Host pool、knowledge provider或experiment resume协议；
- 没有改变现有Candidate workflow persisted V1、protocol或artifact版本；
- 没有native success、semantic correctness、Oracle adequacy、performance或`MigrationVerdict`新证据。

## 7. Verification

在repository本地、无外部effect条件下通过：

```text
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
```
