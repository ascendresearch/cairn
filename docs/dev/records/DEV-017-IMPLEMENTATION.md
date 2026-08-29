# DEV-017 implementation — first native-feedback Candidate follow-up

- 状态：`InProgress`
- 日期：2026-08-29
- Slice：[`DEV-017`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-003、FR-CAND-005、FR-CAND-006、FR-CAND-007、FR-AGENT-023、
  FR-FEEDBACK-002

## 1. Objective

把DEV-016的exact native `SubjectFailed` receipt变成bounded applicant-visible diagnostic，并交给一个新的isolated
DeepSeek Candidate episode。模型读取DEV-014 complete revision与三个`bisheng` errors，提交一份完整changed source
tree；trusted code写入previous revision、diagnostic、episode和model provenance。

## 2. Contract

- native diagnostic artifact与DEV-013 generic proposal diagnostic保持distinct type；
- trusted preparation必须验证exact native prepared job、receipt、stderr/evidence和no-device observation；
- model initial context包含complete previous revision、native diagnostic和public search/recovery/task manifest；
- original task source仍只通过bounded read tool按需进入continuation；
- model只提交complete sorted files、primary source和explanation，不能填写identity/provenance/build/verdict；
- trusted gateway拒绝unchanged submission，产生新的native-follow-up revision artifact；
- new episode不得复用DEV-014 continuation/reasoning，也不得把DEV-015 generic success当native success。

## 3. Non-goals

- 不建立arbitrary-depth revision graph或automatic repair loop；
- 不要求模型本轮通过native gate，不由Cairn修source；
- 不运行下一次build、NPU execution、semantic Oracle、admission或verdict；
- 不把compiler stderr升级为trusted evidence。

## 4. Acceptance

- strict native diagnostic和follow-up revision artifact、typed/static boundaries；
- wrong revision/receipt/contract/input/environment/stderr/evidence、non-`SubjectFailed`与unchanged source fail closed；
- recorded episode证明exact projection、submit/yield、restart/replay；
- 用户按external payload明确授权后运行live DeepSeek episode；
- focused、Clippy、compile-fail与full CI通过；
- 只声称产生native-feedback-linked revision，不声称build或correctness。

## 5. Implemented local boundary

- `CollectionCandidateNativeBuildDiagnosticV1`与generic proposal build diagnostic保持独立semantic domain；
- diagnostic preparation重新推导exact DEV-016 native job，并验证revision、input、environment、contract、receipt、
  stderr、evidence、`SubjectFailed`、Docker backend和`docker:accelerator:none`；
- `CollectionCandidateNativeFollowupRevisionV1`显式绑定search input、exact previous revision、native diagnostic、new
  episode和resolved model configuration；模型只能提交complete changed source tree；
- native follow-up runtime建立独立instruction、policy、tool registration和episode state，不读取或续接DEV-014
  provider continuation/reasoning；
- initial model context只包含public search/recovery/task manifest、complete DEV-014 revision、bounded native diagnostic和
  product-owned gate facts；original task source仍只能通过bounded read tool按需读取；
- `candidate_native_followup_deepseek`在任何provider dispatch前从revision store和Controller store重新验证全部exact
  identity binding，并在独立state root执行、关闭和重开terminal episode；它不调度下一次build。

Recorded integration覆盖typed submit、explicit yield、terminal restart和byte-exact replay。负向测试覆盖wrong receipt、
stderr/evidence、no-device observation、job/contract/outcome、previous revision与unchanged submission；compile-fail证明
generic build diagnostic不能替代native diagnostic。

## 6. Local verification

2026-08-29完成：

- `cargo test -p cairn-migration candidate_native_followup`；
- `cargo test -p cairn-migration --test candidate_episode`；
- `cargo test -p cairn-migration --doc`；
- `cargo clippy -p cairn-migration --all-targets --all-features -- -D warnings`；
- `scripts/ci.sh`，包括全仓tests、examples和doc compile-fail。

这些检查没有调用provider或remote Worker；因此当前仍没有DEV-017 model-authored follow-up revision，也没有新的build
receipt。

## 7. Pending exact external payload authorization

live调用将发送至`https://api.deepseek.com/v1/responses`，model-visible projection严格限于：

- exact DEV-014 complete revision
  `cairn:v1:sha256:migration.candidate-collection-revision.v1:8f519cb18860127080a4e26560c3c38fcb517dbe21d07fb4b51081c83b3ad39d`；
- exact DEV-016 receipt
  `cairn:v1:sha256:execution.receipt.v1:8565502f4aa842c5b689aa19664a6f4dd2b809cb3a702e9084f6892ace73976e`
  派生的bounded native `bisheng` diagnostic及fixed gate facts；
- public recovery/search/task manifest，以及模型主动调用bounded read tool时返回的original task source片段。

不会发送private control set、Oracle expected output/comparison/verdict、Controller/Worker credential、raw trusted
evidence、DEV-014 private continuation/reasoning或generic DEV-015 success。收到用户对这次新增external payload的明确授权
后，才运行live episode并记录exact outcome。
