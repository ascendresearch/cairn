# DEV-021 implementation — Controller-owned Candidate workflow spine

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-021`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：无；本片未调用模型、远端 Worker、Docker或NPU

## 1. Objective

把DEV-016–020反复人工串接的Candidate native build/diagnostic/follow-up/repair suffix固化为
Controller-owned、current-V1、可恢复且可精确重放的product workflow spine。当前consumer是recorded
Candidate episode adapter：它消费持久化的typed episode request，返回严格绑定parent、diagnostic和episode的
follow-up/repair publication，再由同一状态机选择下一次native build或terminal outcome。

本片的停止条件是recorded consumer能够完成：

```text
revision build SubjectFailed
→ native follow-up request/publication
→ follow-up build SubjectFailed
→ repair request/publication
→ repair native build terminal
```

并证明SQLite中途重开、exact command replay、changed command input、wrong diagnostic domain和illegal transition
均按明确语义恢复或fail closed。

## 2. Role与authority

| Role | Input/output | Capability | 明确没有 |
| --- | --- | --- | --- |
| Coding agent/builder | architecture、current code、recorded test material | 实现aggregate、typed events、Controller composition与机械controls | fixture解释权、runtime proposal或verdict authority |
| Runtime model/proposal | 后续可消费`CandidateEpisodeRequestV1`并返回对应typed publication | 只在未来proposal step slice中提出source artifact | admitted intent、Oracle、receipt、调度或terminal authority |
| Recorded consumer | exact episode request → exact follow-up/repair V1 | 离线证明同一production boundary有真实consumer | 模型能力、hidden answer或live evidence claim |
| Controller/execution | durable workflow、CAS、scheduler IDs、verified receipt | commit-before-effect、materialization、schedule/reconcile、terminal classification | 修改Candidate source、解释compiler文本或授予semantic verdict |

`MigrationWorkflowV1`由existing strong `TaskId`拥有；`StreamId`/`AggregateId`只在private record adapter边界
从Task identity导出。`cairn-migration`拥有product state/fold，`cairn-server`只组合CAS materialization与existing
scheduler，不在server中复制workflow policy。

## 3. Data与effects

- `CandidateWorkflowAuthorityV1`冻结exact task、Candidate search input、admitted intent、Oracle public outcome和
  admitted local claim；search input identity重新从canonical bytes推导，不能由调用方任意配对。
- `CandidateNativePublicationV1`分别携带revision、native follow-up和native repair的不同typed `ContentId`；
  `CandidateNativeDiagnosticV1`分别携带首轮native diagnostic与repair diagnostic。
- `CandidateNativeBuildDispatchV1`持久化publication、job、input bundle、environment、contract及全部attempt、
  placement、reservation、assignment、lease、control-message和scheduler command identities。调度重试只能复用这组身份。
- `CandidateEpisodeRequestV1`是最小proposal step seam；请求在任何模型effect前commit，并固定kind、episode、
  authority、parent、diagnostic和revision round。
- `SubjectFailed`只允许打开受budget约束的下一轮；`TimedOut`/`InfrastructureFailed`、integrity failure、cancel、
  native compilation success与revision-budget exhaustion是不同terminal variants。in-doubt build先进入
  `NativeBuildReconciliationRequired`，保留完整dispatch，不能创建替代attempt。
- 本片只执行本地CAS/SQLite recorded tests；没有model-visible prompt变化、restricted/secret读取或外部effect。

## 4. Types与current V1

新增或修改的current-V1边界包括：

- `MigrationWorkflowV1`、`CandidateWorkflowStateV1`、`CandidateWorkflowNextActionV1`；
- `CandidateWorkflowAuthorityV1`、`CandidateNativePublicationV1`、
  `CandidateNativeDiagnosticV1`、`CandidateEpisodeKindV1`；
- positive `CandidateRevisionRoundLimit`与distinct `CandidateRevisionRoundCount`；
- `CandidateNativeBuildScheduleV1`、`CandidateNativeBuildDispatchV1`、
  `CandidateEpisodeRequestV1`和closed terminal outcome enums；
- current-V1 event payloads及strict canonical fold；
- later repair round使用`PreparedCandidateNativeRepairBuildJob`生成
  `CandidateNativeRepairParentV1::Repair` diagnostic的typed API。

所有wire/storage payload都strict V1、deny unknown fields或通过validated deserializer重跑invariant。静态
compile-fail controls证明generic Candidate proposal identity不能替代native revision publication，follow-up build
不能替代later repair diagnostic parent。没有format version bump、legacy alias、fallback reader、dual path、converter或
migration machinery。

## 5. Replay、failure与consumer controls

- 每个command先按command ID、V1 schema、canonical payload和`ObservedAtUnixMillis`查找历史；完全相同返回
  current recovered state，任一输入变化返回`CommandConflict`。
- event fold逐项复核parent event、publication kind、authority、revision round、dispatch和terminal category；
  non-V1、noncanonical或illegal ordering失败。
- subject/terminal receipt在command boundary重新推导typed receipt ID，并复核job、attempt、contract和outcome；
  terminal event自身也保存原dispatch供restart fold校验。
- recorded consumer使用`matrix-layout`与`stream-window`两份不同source material走同一生产状态机，没有product branch、
  expected answer或prompt变化。
- SQLite restart test在episode request已commit、consumer尚未运行时重开store，恢复byte-equivalent request并精确重放。
- wrong diagnostic domain被拒绝；changed old command input返回command conflict；compile-fail验证publication ID域不混用。

## 6. Superseded current paths

已直接删除、未保留compatibility alias：

- `candidate_native_followup_deepseek.rs`与`candidate_native_repair_deepseek.rs` one-shot examples；
- 对应两个专用example JSON配置；
- revision/follow-up/repair三条`real-candidate-native-*-asc-build-smoke.sh`手工脚本；
- `real_ascend_build_worker.rs`中直接读取外部state dir并手工调度native revision、follow-up和repair的三项ignored tests。

保留低层typed publication validators、native materializers、receipt-bound diagnostic constructors、repair lineage、
generic scheduler与DEV-013–020历史records。它们是新workflow composition的实现mechanics或历史事实，不是平行launcher。

## 7. Tests与acceptance

通过：

- `cargo check -p cairn-migration --no-default-features --lib`；
- `cargo check -p cairn-server --no-default-features --lib`；
- focused recorded/restart/domain-drift tests；
- focused migration/server all-target all-feature Clippy；
- `scripts/ci.sh`完整workspace check、all-target all-feature Clippy、tests和doc/compile-fail tests；
- current-tree superseded-path扫描、fixture token扫描和typed-ID审计；
- `git diff --check`（最终handoff前执行）。

CI中的live GitHub、Docker、GPU和Ascend Worker lanes保持显式ignored；本片没有调用它们，也没有新增live receipt。

## 8. 明确非目标与remaining unknown

- 不实现完整CUDA→Ascend-C top-level workflow、generic proposal step service或多role Host迁移；
- 不调用DeepSeek或任何runtime model，不证明recorded consumer具有模型质量；
- 不调用远端Worker，不重新证明DEV-016–020 live native gate；
- 不自动解释或修复compiler diagnostic，不嵌入当前compiler文本或fixture-derived recipe；
- 不执行NPU、semantic/safety/performance Admission，不产生`MigrationVerdict`；
- 不扩展SIR authority/topology，不创建future empty crate；
- 不建立compatibility、schema migration或V2。

下一slice只能从已存在的`CandidateEpisodeRequestV1`真实consumer继续切片；若建立generic proposal step，必须让至少
两种role profile共享同一Host implementation，并删除被接管的专用launcher，不能以空service或新手工repair替代proof。
