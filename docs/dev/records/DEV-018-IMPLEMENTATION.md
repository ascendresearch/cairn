# DEV-018 implementation — first native-feedback follow-up remote ASC build

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-018`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-006、FR-CAND-007、FR-EXEC-001、FR-EXEC-002、FR-EXEC-003、
  FR-FEEDBACK-002

## 1. Objective

把DEV-017 exact immutable native-follow-up publication原样物化，经DEV-016同一个product-owned native ASC harness和
existing Controller/remote no-device Worker执行，取得绑定exact source、primary bytes、environment和contract的
authoritative terminal receipt。

## 2. Exact authority

- follow-up `cairn:v1:sha256:migration.candidate-native-followup-revision.v1:9bc0eeb94474c94c41bae002083d042808b502eb4c30021cba9e83ed1437534a`；
- previous revision与native diagnostic lineage由follow-up V1 envelope固定；
- exact Docker image `sha256:17b6708374ddbde5e36931927aefb2cbcd5596409f3be34244cf43e6de14fb60`；
- existing profile `AscendCann910Beta1Dav3510NoDevice`、Controller scheduler和remote `npu-build` Worker。

## 3. Contract

- follow-up bytes必须以其独立typed publication identity重载并重新执行current-V1 invariants；
- follow-up identity不能替代旧revision identity，prepared native job也保持static distinction；
- private materializer可共享DEV-016 mechanics，但input bundle必须保存exact follow-up envelope和完整model source tree；
- fixed harness从validated typed `primary_source`精确选择source bytes，复制到fixed
  `native/candidate_primary.asc`，不修改任何byte；
- fixed CMake仍只使用`LANGUAGES ASC`、`candidate_native`和`--npu-arch=dav-3510`，不调用Candidate-owned CMake；
- terminal receipt绑定new job/input/environment/contract；`Succeeded`只证明native compilation，`SubjectFailed`只形成
  下一步可选的compiler feedback，两者都不是semantic verdict。

## 4. Non-goals

- 不审核或修改DeepSeek source；
- 不把companion host source加入fixed primary-only native gate；
- 不自动打开下一个Candidate episode或建立arbitrary-depth repair loop；
- 不运行NPU、call adapter、collection Oracle、Candidate Admission、performance或verdict；
- 不改变current V1 schema或建立compatibility path。

## 5. Acceptance

- strict follow-up reloader、distinct native prepared type和compile-fail wrong-domain boundary；
- tests证明exact follow-up envelope/source tree被保留，fixed `.asc` bytes与selected primary逐字节相同；
- wrong identity、noncanonical/non-V1与changed source/material identities fail closed；
- focused、Clippy、compile-fail与full CI通过；
- live remote job取得可重启恢复的terminal receipt，并按实际`Succeeded`或`SubjectFailed`记录；
- 不产生runtime、semantic correctness、performance或verdict claim。

## 6. Implementation

- `validate_archived_collection_candidate_native_followup_revision`以follow-up独立typed identity重载canonical
  current-V1 bytes并重跑constructor invariants；旧revision identity在compile time不能替代；
- `PreparedCandidateNativeFollowupBuildJob`与`PreparedCandidateNativeRevisionBuildJob`保持public semantic
  distinction，分别暴露exact publication/input/environment/contract bindings；
- private native materializer共享DEV-016固定mechanics，但follow-up input保存
  `meta/candidate-native-followup-revision.json`、完整model source tree、fixed harness和逐字节相同的
  `native/candidate_primary.asc`；
- new ignored live test与smoke script继续使用existing Controller scheduler、remote Worker、terminal receipt和restart
  recovery path，没有新增旁路执行器或repair automation。

Focused tests覆盖exact envelope/source/primary preservation、wrong identity、noncanonical/non-V1和changed material
identities；两个compile-fail分别证明旧revision identity与旧native prepared job不能替代follow-up domain。相关Clippy、
live test编译和全仓`scripts/ci.sh`均通过。

## 7. Live remote evidence

2026-08-29 exact DEV-017 follow-up经existing reverse tunnel与remote `npu-build` Worker得到：

| Evidence | Exact fact |
| --- | --- |
| job | `job:01a04c97-e547-77e3-aa90-7db26f8a48af` |
| attempt | `attempt:01a04c97-e596-7b12-a190-fdf112898c09` |
| follow-up | `cairn:v1:sha256:migration.candidate-native-followup-revision.v1:9bc0eeb94474c94c41bae002083d042808b502eb4c30021cba9e83ed1437534a` |
| input | `cairn:v1:sha256:execution.input-bundle.v1:601317aae3c4026794d021511dd8ef5670330acccbd44bcd0465e92a09501c96` |
| environment | `cairn:v1:sha256:execution.environment.v1:6f5324f204951a9b207a20ea9c542afc96f22beb143dd44e25a1c97179b8a803` |
| contract | `cairn:v1:sha256:execution.job-contract.v1:57bc9328cf5f4684f0c83a6da235fc7af5fad3ed0f56b0f2e65fcdb479d09e33` |
| receipt | `cairn:v1:sha256:execution.receipt.v1:30ff2c955085ae812447ac51a2b10d2bab63b89b9929336ff294c85f16d0672c` |
| outcome | `SubjectFailed` |
| trusted evidence | exact observed environment；`docker:accelerator:none` |
| durability | reservation released；Controller stores重开并恢复同一terminal receipt |

Untrusted stderr证明CMake再次选择real `bisheng`、配置fixed ASC project并构建
`candidate_primary.asc.o`。DEV-016的三个source-level errors已不再出现；新的linker divergence是
`compact_above_kernel`没有显式kernel function type attribute，`ld.lld`报告自动类型推导失败。这个变化只说明新source
推进到了下一compiler stage，不证明修复正确或语义成立。

DEV-018 Accepted因为它的目标是让exact follow-up重新经过同一authoritative native compilation gate并取得可恢复
terminal receipt，而不是要求成功。若继续修订，下一slice应把这一个exact receipt派生为bounded diagnostic并由新的
explicit Candidate episode消费；不得自动续接DEV-017或在Cairn中修改source。
