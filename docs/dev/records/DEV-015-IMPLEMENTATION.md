# DEV-015 implementation — first exact Candidate revision remote build

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-015`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-006、FR-CAND-007、FR-EXEC-001、FR-EXEC-002、FR-EXEC-003、
  FR-FEEDBACK-002

## 1. Objective

把DEV-014的exact immutable revision作为独立于initial proposal的强类型publication，原样物化为generic execution
input/environment/contract，并经现有Controller scheduler送到remote no-device Ascend build Worker：

```text
exact Candidate revision publication
  → revision-specific validation and materialization
  → generic content-addressed execution contract
  → Controller scheduler → remote Worker → Docker build
  → authoritative terminal receipt + trusted evidence + untrusted stdout/stderr
```

## 2. Exact authority

- revision `cairn:v1:sha256:migration.candidate-collection-revision.v1:8f519cb18860127080a4e26560c3c38fcb517dbe21d07fb4b51081c83b3ad39d`；
- parent与diagnostic lineage由revision V1 envelope固定；
- exact Docker image `sha256:17b6708374ddbde5e36931927aefb2cbcd5596409f3be34244cf43e6de14fb60`；
- existing profile `AscendCann910Beta1Dav3510NoDevice`和existing remote `npu-build` Worker。

## 3. Contract

- revision bytes必须以exact typed publication identity重载并重新执行current-V1 invariants；
- revision identity不得被initial proposal identity替代，两个public build API保持semantic distinction；
- input bundle包含model-authored source bytes和exact revision envelope，不重写path/source/CMake；
- environment、placement、capture、network和command policy与DEV-013同一closed profile一致；
- terminal receipt必须绑定new job/contract/input/environment，trusted evidence必须确认no-device；
- build成功只代表该exact environment中的compile/link gate成功，不代表semantic correctness或verdict。

## 4. Non-goals

- 不审核、修正或删除模型选择的fallback；
- 不运行semantic Oracle、candidate admission、NPU execution、performance或final verdict；
- 不建立通用revision chain、多轮自动repair或compatibility/migration path。

## 5. Acceptance

- strict revision reloader和distinct typed/static build boundary；
- identity、canonical bytes、source/material binding与wrong-domain controls fail closed；
- focused、Clippy、compile-fail和full CI通过；
- live remote job取得可重启恢复的authoritative terminal receipt；
- outcome按事实记录，`Succeeded`和`SubjectFailed`都不被升级为semantic verdict。

## 6. Implementation

- `validate_archived_collection_candidate_revision`以typed revision identity重载canonical current-V1 bytes并重跑
  invariants；proposal identity在compile time不能替代revision identity；
- `PreparedCandidateRevisionBuildJob`与`PreparedCandidateBuildJob`保持public semantic distinction，只在private
  materializer共享input/environment/contract mechanics；
- revision input bundle包含`meta/candidate-revision.json`、exact model-authored source tree和现有generic runner；
- 新live test和smoke script沿用Controller scheduler、reservation、remote Worker、terminal receipt与restart recovery
  path，没有为revision建立旁路执行器。

Focused tests、doc compile-fail、两个相关crate的Clippy和live事实记录后的全仓`scripts/ci.sh`全部通过。

## 7. Live remote evidence

2026-08-28 exact DEV-014 revision经现有reverse tunnel到remote Worker，在固定Docker image中得到：

| Evidence | Exact fact |
| --- | --- |
| job | `job:01a04c50-1d34-7da1-ac48-0965d34891dc` |
| attempt | `attempt:01a04c50-1d76-7c33-8bfc-3dd601878413` |
| revision | `cairn:v1:sha256:migration.candidate-collection-revision.v1:8f519cb18860127080a4e26560c3c38fcb517dbe21d07fb4b51081c83b3ad39d` |
| input | `cairn:v1:sha256:execution.input-bundle.v1:b4a9e2164d06ecd90f2e0dc8e520e816d6f7d0b0524179a4a28c17704ffd12bf` |
| environment | `cairn:v1:sha256:execution.environment.v1:6f5324f204951a9b207a20ea9c542afc96f22beb143dd44e25a1c97179b8a803` |
| contract | `cairn:v1:sha256:execution.job-contract.v1:a303c40431a4f3bdcda97c05ec7152d45729eee7d57b2f99250195000a6cfb3f` |
| receipt | `cairn:v1:sha256:execution.receipt.v1:ca80172cd70fcd7e90939931e3719544863815038956bccf3b54c87f2e34b4a4` |
| outcome | `Succeeded`；exit zero；stdout exact gate `PASS candidate-build=complete device=none` |
| trusted evidence | exact observed environment；`docker:accelerator:none` |
| durability | reservation released；Controller stores reopened and recovered the same terminal receipt |

## 8. What the success does and does not mean

该revision的CMake仍以`LANGUAGES CXX`构建`.cpp`。当CANN headers未被普通CXX include discovery找到时，模型
选择其local ACL/kernel shims与detached host thread fallback；因此本次`Succeeded`证明的是“该exact source tree能在
selected container的current generic CMake build gate中生成静态库”，不是“native Ascend C translation unit被ASC
compiler接受”。静态库阶段也不会解析所有外部runtime symbols。

所以本receipt是有效build evidence，同时也是有效的gate-gap observation：后续流程若需要声称target compilation，
必须由product-owned contract显式要求ASC language/target artifact或以trusted evidence证明native branch，不能根据
文件名、模型解释或`Succeeded`自行推断。它更不提供local collection semantics、ABI runtime behavior、NPU behavior或
Candidate verdict。DEV-015按其窄目标Accepted；该gap进入下一slice的设计输入。
