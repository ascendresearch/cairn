# DEV-024 implementation — unified proposal step request lifecycle

> Historical record: D-044/DEV-036 deleted this generic process request/terminal lifecycle. Current
> proposal work is a typed Controller workflow step.

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-024`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；本片未调用模型、远端 Worker、Docker 或 NPU

## 1. Objective

删除遗留的role-specific SIR/Candidate Agent Loop runner及其旁路测试，把所有现有proposal step profile固化到
同一条request lifecycle：冻结exact request与runtime facts，打开独立durable episode，只执行Host获准的
pure/read-only tool，把结果先归档为observation，再接收strict typed proposal并冻结terminal publication。

本片保留SIR与Candidate的领域schema、workspace、profile instruction和strict submission gateway；它们是同一
generic Agent Loop的真实领域consumer，不是专用process或第二套runtime。

## 2. Current-V1 lifecycle

`run_proposal_step_episode`现在只编排三个业务阶段：

1. `freeze_proposal_step_request`验证current-V1 exact request/runtime binding，并物化bounded task snapshot；
2. `drive_frozen_proposal_step_request`选择领域profile，但所有profile都进入同一个`run_proposal_loop`；
3. `freeze_proposal_step_terminal`构造并反向校验request-bound terminal artifact。

`run_proposal_loop`本身同样只保留可直接阅读的流程骨架：

```text
open_durable_proposal_episode
→ dispatch_agent_turn
→ settle_agent_turn
→ admit_agent_operations
→ execute_authorized_host_operations
→ project_operation_observations
→ advance_proposal_episode
```

每个步骤由独立函数实现，并通过`Opened`、`Dispatched`、`Settled`、`Admitted`、`Observed`、`Projected`
内部typestate传递；后一步不能接收错误阶段，terminal completion也不能伪装成active turn。顶层不再包含provider
decode、operation binding、gateway执行或continuation拼接细节。

共同loop在打开episode前冻结distinct `TaskId`/`EpisodeId`、validated role、model selection、budget、native request、
instruction/history/context/policy/tool-catalog content identity及validated capability grant。当前知识快照明确为empty，
不是从fixture或隐式环境读取。

## 3. Effect、observation与submission authority

- model dispatch和每个Host-local tool effect都先经过已有durable prepare/begin authority；
- capability grant按exact `ToolName`绑定trusted implementation和effect class，空grant或重复名字直接拒绝；
- Host只执行`Pure`/`ReadOnly` operation；mutating、idempotent或ambiguous external effect返回typed
  `ExternalEffectRequiresController`，不会直接调用Worker、Docker、device或网络；
- canonical tool result先归档为provenance-bearing `OperationResult`，再投影到native continuation；
- profile strict submission gateway负责schema、identity和lineage校验；无效提交原子拒绝，错误作为同一episode的
  operation result返回，Agent可在剩余budget内修复；
- completion只产生request-bound terminal；Controller process manager随后持久化terminal并把publication折回task
  workflow，proposal step本身不拥有下游Gate authority。

## 4. 替换与删除

已删除：

- public `run_sir_episode`及`SirEpisodeRunInput`/`SirEpisodeRunOutcome`；
- public Candidate initial/revision/native-followup/native-repair episode runners及对应run input/outcome；
- `tests/sir_episode.rs`与`tests/candidate_episode.rs`两套可绕过generic Host的role-specific runner测试；
- SIR与Candidate各自复制的model/tool/continuation/budget Agent Loop主体。

DEV-022已经删除历史`cairn-sir`独立binary/crate；本片删除的是最后的代码级独立runner接缝。SIR领域proposal、
workspace、citation/source tools及下游Intent consumer没有删除，因为它们仍是D-043定义的role profile与typed
artifact边界。

## 5. Tests与controls

- generic loop unit control证明external-effect tool会先durably bind，但Host绝不执行其gateway；
- duplicate semantic capability name被current-V1 grant constructor拒绝；
- proposal step integration先提交invalid SIR proposal，再在同一budget内修复为valid proposal；
- 同一Host test同时运行SIR与Candidate profile并验证cross-role publication隔离；
- persisted Candidate workflow request/terminal round-trip及strict request drift negatives继续覆盖Controller折回边界；
- source scan证明旧runner符号和role-specific runner test path已消失。

## 6. 明确非目标

- 没有调用live runtime model、remote Worker、Docker、NPU或互联网；
- 没有实现Controller↔Host的异步experiment request/yield/resume协议；当前安全语义是Host typed拒绝外部effect；
- 没有新增knowledge provider；current snapshot明确为空；
- 没有新增Oracle/Planner profile、Host pool、task intake/global catalog或Candidate Admission；
- 没有宣称native success、semantic correctness、Oracle adequacy、performance或`MigrationVerdict`；
- 没有V2、compatibility reader、legacy alias或数据转换路径。

## 7. Fixture/generic-ID audit

Production prompt、tool schema和loop没有`reduce-sum-f32`、D-039、fixture expected output或fixture identity。
Request、episode、step、attempt、operation、command和content identities继续使用各自strong type；capability grant使用
validated `ToolName`，没有引入generic string/UUID ID。SIR/Candidate只在closed typed role request/profile adapter处分派，
共同Agent Loop不包含fixture或领域结果分支。

## 8. Verification

在repository本地、无外部effect条件下通过：

```text
cargo fmt --all -- --check
cargo check --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
```
