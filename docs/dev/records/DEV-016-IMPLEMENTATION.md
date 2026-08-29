# DEV-016 implementation — product-owned native Ascend Candidate gate

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-016`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-006、FR-CAND-007、FR-EXEC-001、FR-EXEC-002、FR-EXEC-003、
  FR-FEEDBACK-002

## 1. Objective

关闭DEV-015暴露的gate gap：不采用Candidate-owned CMake来判断target compilation，而由product-owned、
content-addressed harness把exact primary source bytes作为固定`.asc`translation unit交给CMake ASC language和
`bisheng --npu-arch=dav-3510`：

```text
exact Candidate revision + exact primary source bytes
  → fixed native harness + fixed .asc material path
  → CMake LANGUAGES ASC + product-selected target/options/includes
  → remote no-device Worker
  → authoritative terminal receipt
```

## 2. Observed toolchain authority

对DEV-015 exact image的只读probe确认：

- `ASCEND_HOME_PATH=/usr/local/Ascend/cann-9.1.0-beta.1`；
- `bisheng`位于该root的`bin/bisheng`；
- `kernel_operator.h`位于toolchain ASC include tree；
- `acl/acl.h`位于`x86_64-linux/include/acl/acl.h`；
- checked-in real Ascend fixture已经通过`find_package(ASC REQUIRED)`、`LANGUAGES ASC`和
  `--npu-arch=dav-3510`取得remote success receipt。

## 3. Contract

- revision publication仍以exact typed identity重载并重跑current-V1 invariants；
- primary source bytes必须从validated revision file set按typed primary path精确选择；
- source tree和revision envelope原样保留；另把相同primary bytes放入fixed
  `native/candidate_primary.asc`，不修改任何source byte；
- fixed `native/CMakeLists.txt`而不是Candidate CMake定义ASC target、include roots和architecture；
- fixed runner只在ASC target成功后输出exact native-gate success line；
- native build prepared type与generic revision build prepared type保持static distinction；
- terminal receipt绑定exact harness/input/environment/contract；`SubjectFailed`是有效native compiler feedback，
  `Succeeded`也只证明native compilation，不证明runtime semantics。

## 4. Non-goals

- 不要求当前revision通过；不由Cairn修正source；
- 不执行host wrapper、NPU kernel、call adapter、collection Oracle或Candidate Admission；
- 不把compiler stderr当trusted evidence；
- 不建立toolchain探测框架、多架构矩阵、自动repair loop或internal format migration。

## 5. Acceptance

- native prepared type、exact primary selection和static wrong-gate boundary；
- input tests证明candidate CMake被保留但不被native runner调用，fixed `.asc` bytes与primary完全一致；
- changed primary/harness/environment改变对应content identities；
- focused、Clippy、compile-fail和full CI通过；
- live remote native job取得可重启恢复的terminal receipt，并按事实记录首个compiler divergence或success；
- slice不产生runtime、correctness、performance或verdict claim。

## 6. Implementation

- `PreparedCandidateNativeRevisionBuildJob`与generic revision build prepared type保持static distinction；
- native materializer从validated revision的typed `primary_source`精确选择source bytes；
- input bundle同时保留完整model source tree、revision envelope，并把同一primary bytes发布为
  `native/candidate_primary.asc`；
- fixed product-owned CMake只声明`LANGUAGES ASC`、`candidate_native` target、source/include roots和
  `--npu-arch=dav-3510`；
- fixed runner只配置`/cairn/work/native`，不会调用保存在`/cairn/work/source/CMakeLists.txt`中的Candidate
  CXX/fallback build；
- unit tests逐字节比较fixed `.asc`与primary，证明generic/native input和contract identity不同，并覆盖changed
  source identity；compile-fail证明generic prepared job不能替代native prepared job。

Focused tests、doc compile-fail、相关crate Clippy和live receipt记录后的全仓`scripts/ci.sh`全部通过。

## 7. Live native gate evidence

2026-08-29 exact DEV-014 revision经同一remote Worker和fixed image得到：

| Evidence | Exact fact |
| --- | --- |
| job | `job:01a04c5a-27cb-7a71-9365-ce9a4779fb5f` |
| attempt | `attempt:01a04c5a-2802-7d72-9f7c-3d2bf0c57e0b` |
| revision | `cairn:v1:sha256:migration.candidate-collection-revision.v1:8f519cb18860127080a4e26560c3c38fcb517dbe21d07fb4b51081c83b3ad39d` |
| input | `cairn:v1:sha256:execution.input-bundle.v1:0aa64b6053f595437e89bb8de7ba5cb62a18c0ce1ef831c629c9fc78b167be88` |
| environment | `cairn:v1:sha256:execution.environment.v1:6f5324f204951a9b207a20ea9c542afc96f22beb143dd44e25a1c97179b8a803` |
| contract | `cairn:v1:sha256:execution.job-contract.v1:a6717afd0912d6fd6161716f9ec3c02b67c878402844d48c3e3260f3daf81e98` |
| receipt | `cairn:v1:sha256:execution.receipt.v1:8565502f4aa842c5b689aa19664a6f4dd2b809cb3a702e9084f6892ace73976e` |
| outcome | `SubjectFailed` |
| trusted evidence | exact observed environment；`docker:accelerator:none` |
| durability | reservation released；Controller stores reopened and recovered the same terminal receipt |

Untrusted stderr同时给出不可由generic CXX gate产生的直接事实：CMake选择
`/usr/local/Ascend/ascend-toolkit/latest/bin/bisheng`，构建`candidate_primary.asc.o`，随后报告：

1. host function不能构造只有`__aicore__` constructor的`CompactAboveKernel`；
2. host `float*`不能绑定到`GM_ADDR`的`__gm__`address space；
3. `ACLRT_LAUNCH_KERNEL`未声明。

这些是bounded applicant-visible native compiler feedback，不是trusted correctness evidence。DEV-016 Accepted因为
product-owned gate已经证明exact primary source确实进入native compiler并取得可恢复receipt；它没有要求revision
通过。下一slice可把该exact failed receipt绑定回一个新的Candidate revision episode，而不是由Cairn修改source。
