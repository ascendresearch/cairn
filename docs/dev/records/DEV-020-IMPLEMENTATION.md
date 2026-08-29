# DEV-020 implementation — exact native repair remote ASC build

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-020`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-006、FR-CAND-007、FR-EXEC-001、FR-EXEC-002、FR-EXEC-003、
  FR-FEEDBACK-002

## 1. Objective

把DEV-019 exact immutable native repair publication原样物化，经DEV-016/018同一个product-owned native ASC harness、
Controller scheduler和remote no-device Worker执行，取得绑定exact repair、source、primary bytes、environment与contract的
authoritative terminal receipt。

## 2. Exact authority

- repair
  `cairn:v1:sha256:migration.candidate-native-repair-revision.v1:be182db29ba68757e9fcdff6657ef26d3b54e259ba0240d643f168eee4a29b59`；
- root follow-up、immediate parent和repair diagnostic由repair V1 envelope固定；
- exact Docker image `sha256:17b6708374ddbde5e36931927aefb2cbcd5596409f3be34244cf43e6de14fb60`；
- existing profile `AscendCann910Beta1Dav3510NoDevice`、Controller scheduler和remote `npu-build` Worker。

## 3. Contract

- repair bytes必须以repair-specific typed identity重载canonical current V1并重跑constructor invariants；
- repair identity和prepared build type不能被root follow-up、旧revision或generic identities替代；
- input bundle保存exact repair envelope、complete model source tree、fixed harness，以及从validated `primary_source`逐字节
  复制到`native/candidate_primary.asc`的内容；
- fixed harness仍只使用`LANGUAGES ASC`、`candidate_native`与`--npu-arch=dav-3510`，不执行Candidate-owned CMake；
- terminal receipt绑定new job/input/environment/contract；`Succeeded`只证明native compilation，`SubjectFailed`只形成下一轮
  可选feedback，两者都不是semantic verdict。

## 4. Non-goals

- 不审核、解释或修改DeepSeek的`__kernel__`策略；
- 不把host companion加入fixed primary-only gate；
- 不自动打开下一轮repair或build；
- 不运行NPU、call adapter、semantic Oracle、Admission、performance或verdict；
- 不改变current V1或添加compatibility/migration path。

## 5. Acceptance

- repair-specific prepared build type与compile-fail wrong-domain boundary；
- exact envelope/source tree/primary preservation；wrong identity、noncanonical/non-V1与changed material fail closed；
- focused、Clippy、compile-fail和full CI通过；
- exact remote job取得可重启恢复的terminal receipt，并记录实际compiler outcome；
- 不产生runtime、semantic correctness、performance或verdict claim。

## 6. Implementation

- `validate_archived_collection_candidate_native_repair_revision`在任何materialization前重载canonical current-V1 repair并
  重跑root/parent invariant；
- `PreparedCandidateNativeRepairBuildJob`保持repair-specific semantic boundary，暴露exact publication、input、environment和
  contract bindings；compile-fail证明follow-up prepared job不能替代repair prepared job；
- shared private native materializer保存`meta/candidate-native-repair-revision.json`和complete source tree，仅从validated
  primary path复制exact bytes到fixed `native/candidate_primary.asc`；
- product-owned runner/CMake没有变化：只构建fixed ASC project，不读取或执行Candidate `CMakeLists.txt`；
- ignored live test与smoke script复用existing Controller scheduler、remote Worker、receipt recovery与reservation release，
  没有新增旁路执行器或automatic repair行为。

Focused tests覆盖exact repair envelope/source/primary preservation、wrong identity、noncanonical/non-V1和changed material
identities；live test compile与repair-vs-follow-up compile-fail boundary通过。相关all-target Clippy、34个migration doc tests和
全仓`scripts/ci.sh`均通过；这些local checks没有连接remote Worker，也没有产生build receipt。

## 7. Live remote evidence

2026-08-29 exact DEV-019 repair经existing Controller、reverse tunnel和remote `npu-build` Worker得到：

| Evidence | Exact fact |
| --- | --- |
| job | `job:01a04cc6-e951-7a91-b1d8-b36a92256f56` |
| attempt | `attempt:01a04cc6-e98e-7f42-8e92-447b28effad8` |
| repair | `cairn:v1:sha256:migration.candidate-native-repair-revision.v1:be182db29ba68757e9fcdff6657ef26d3b54e259ba0240d643f168eee4a29b59` |
| input | `cairn:v1:sha256:execution.input-bundle.v1:d53254a4a3f6ae3341f97636c202741090d095b1e569cfafa909c7cfaecdc7d6` |
| environment | `cairn:v1:sha256:execution.environment.v1:6f5324f204951a9b207a20ea9c542afc96f22beb143dd44e25a1c97179b8a803` |
| contract | `cairn:v1:sha256:execution.job-contract.v1:9637d47b2db7fc1038442f996a4739f50d71b1eda11444eaafb7d0d7c2c8d690` |
| receipt | `cairn:v1:sha256:execution.receipt.v1:56e3046973d337bd2e1b9f2e3e6ad4a1ff37ef93f3bae28a87fad993dbab04e5` |
| outcome | `SubjectFailed` |
| trusted evidence | exact observed environment；`docker:accelerator:none` |
| durability | reservation released；Controller stores重开并恢复同一terminal receipt |

Untrusted stderr证明fixed CMake再次选择real `bisheng`并开始构建exact `candidate_primary.asc`。Compiler明确报告
`unknown type name '__kernel__'`；随后由于kernel entry没有形成合法void device function，又报告non-void kernel type、
从host调用`__aicore__` constructor、`Init`与`Process`，共5个errors。DEV-019试图解决的linker auto-derivation错误不再是
本次首要failure；这不是修复进展证明，而是real toolchain否定了model-authored `__kernel__`假设。

DEV-020 Accepted因为目标是让exact repair通过同一authoritative native compilation gate并取得可恢复terminal receipt，
不是要求compile success。若继续，下一slice应把这个exact receipt派生为repair-parent-bound diagnostic并打开新的explicit
Candidate repair episode；不得自动续轮，也不得由Cairn修改source。
