# DEV-013 implementation — first exact Candidate remote Ascend build

- 状态：`InProgress`
- 日期：2026-08-28
- Slice：[`DEV-013`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Runtime Architecture`](../../design/RUNTIME_ARCHITECTURE.md)、
  [`Worker Execution`](../../WORKER_EXECUTION.md)、[`Scheduler`](../../SCHEDULER.md)
- Requirements：FR-CAND-002、FR-CAND-004、FR-CAND-005、FR-EXEC-003、FR-EXEC-004、
  FR-EXEC-018、FR-EXEC-030、FR-EXEC-031、FR-EXEC-032
- 决策：D-030、D-034、D-035、D-042

## 1. Objective

让DEV-012真实DeepSeek提交的exact、immutable、unbuilt `CollectionCandidateProposalV1`进入已有remote
no-device Ascend build lane：

```text
exact Candidate proposal bytes + exact typed proposal ID
  + explicit environment-under-test
  → Controller-side content-addressed InputBundleV1 / Docker environment / JobContract
  → existing scheduler + reservation + lease + material manifest
  → outbound-mTLS remote npu-build worker
  → worker-local verified CAS + remote Docker/CANN build
  → authoritative terminal ExecutionReceipt
```

成功或`SubjectFailed`都可成为本slice的产品证据。只有`Succeeded`证明该exact proposal在该exact build
environment下完成build；`SubjectFailed`证明首个可修复build divergence；timeout、infrastructure、integrity或
ambiguous状态不能冒充Candidate defect。

## 2. Correct runtime topology

Container不在Controller主机执行。Controller独占public SQLite/CAS并冻结job、placement、reservation、lease和
outbox；另一台机器上的managed `npu-build` worker保持outbound mTLS WebSocket，按material manifest分块拉取
input/environment到worker-local CAS，校验typed ContentId后接受assignment，收到独立start authority后才使用
worker本机Docker执行。Worker-controlled receipt返回Controller；stdout/stderr保持untrusted。

不得增加local-Docker shortcut、SSH execution adapter或绕过scheduler的fixture runner。

## 3. Environment-under-test is not selected product target

DEV-012 recovery input中的SoC、toolchain和environment仍是`not-selected`。DEV-013使用已经实际probe并通过两次
no-device build gate的现有lane作为一次明确的environment-under-test：

| Dimension | Exact value |
| --- | --- |
| worker pool | `npu-build` |
| execution backend | `docker-v1` |
| toolchain vendor | `ascend` |
| CANN | `9.1.0-beta.1` |
| architecture | `dav-3510` |
| accelerator policy | `none` |
| historical exact image | `sha256:17b6708374ddbde5e36931927aefb2cbcd5596409f3be34244cf43e6de14fb60` |

这些值是本次build placement/environment事实，不修改`IntentRecoveryInputV1`，不声称用户选择了产品target，
也不解除OQ-020对未来hardware/performance profile的约束。

## 4. Materialization and authority

- 调用者必须同时提供canonical proposal bytes和expected typed proposal ID；bytes-only输入不足以选择authority；
- Controller重新strict decode、canonical encode并校验proposal identity；
- input bundle包含完整exact proposal artifact、一个fixed build runner以及proposal的每个source file；
- source文件逐字映射到`source/<candidate-relative-path>`，全部为data mode；不替换Candidate的`CMakeLists.txt`、
  不重命名source、不注入tiling header、不修正CANN API；
- runner只复制exact source tree并执行Candidate声明的CMake configure/build；不调用NPU或network；
- immutable Docker image identity进入execution-environment artifact；exact pool/capabilities进入placement；
- generic scheduler/worker不理解Candidate、operator semantics或build pass policy；
- receipt outcome和trusted worker evidence有authority，compiler stdout/stderr只有diagnostic value。

## 5. Scope and non-goals

本slice只做首个exact proposal build consumer。明确不实现：

- Candidate revision/parent lineage或model diagnostic continuation；
- 自动修复CMake、source extension、headers、kernel/host split或API；
- CUDA reference run、Ascend NPU execution、semantic comparison或Candidate/Migration verdict；
- selected product target、OQ-020 hardware/performance profile、profiler或performance；
- Candidate Admission、hidden controls、restricted data plane或完整Oracle portfolio；
- 新worker protocol、local Docker shortcut、SSH execution path或新execution crate。

若真实build得到`SubjectFailed`，下一slice才以该exact receipt/diagnostic为当前consumer，直接扩展current V1的
Candidate revision lineage；不得在诊断出现前预建通用correction topology。

## 6. Acceptance

- strict proposal reload要求expected typed ID并拒绝noncanonical、non-V1、wrong-domain和identity mismatch；
- materialization逐字保留每个Candidate file并归档exact proposal bytes；paths、parents、modes、size和identity
  继续由现有strong V1 types/`InputBundleV1` fail closed；
- exact image、environment、job、input、command、network、pool、capabilities和capture bounds均可重建；
- tests证明改变proposal或image会改变相应identity、exact profile selectors固定不漂移，且不能把Candidate proposal当receipt；
- normal path复用现有Controller scheduler/material replication/remote worker，无本机执行后门；
- focused tests、no-default-features、Clippy与full `scripts/ci.sh`通过；
- `Accepted`前必须调度一次DEV-012 exact proposal到真实remote `npu-build` worker，并记录job/attempt/receipt、
  terminal outcome、exact environment与restart/recovery事实；
- 只有`SubjectFailed`或`Succeeded`可回答Candidate build；其他terminal/in-doubt结果保持infrastructure evidence，
  slice不误报Candidate conclusion。

## 7. Current operational fact

Controller当前在线且持续接收另一个remote worker heartbeat；`npu-build` worker的最近heartbeat停在2026-08-26。
本地实现与recorded gates可继续，但live acceptance需要operator使原有remote worker按同一managed control path重新
连接。恢复worker是部署操作，不改变本slice contract。

截至2026-08-28，本地产品桥接和live gate已经实现：strict loader要求exact typed proposal ID；materializer逐字
保留proposal/source并生成`npu-build` no-device generic contract；live test只调用Controller scheduler并按
authoritative receipt分类。Focused tests、no-default-features library check、integration compile、Clippy与full
`scripts/ci.sh`均已通过。尚未调度live job，因此本DCR保持`InProgress`，也没有Candidate build结论。
