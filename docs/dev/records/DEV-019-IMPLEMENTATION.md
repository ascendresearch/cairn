# DEV-019 implementation — explicit repeatable native repair episode

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-019`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)
- Requirements：FR-CAND-006、FR-CAND-007、FR-FEEDBACK-002

## 1. Objective

把DEV-018 exact `SubjectFailed` receipt派生为bounded native diagnostic，由一个新的isolated DeepSeek Candidate
episode消费DEV-017完整source和该diagnostic，提交一份changed full-source repair。与此同时，把后续可能出现的同类
native compiler repair表达为同一个current-V1 typed lineage，而不是继续增加`follow-up-follow-up`专用类型。

## 2. Exact authority

- root follow-up
  `cairn:v1:sha256:migration.candidate-native-followup-revision.v1:9bc0eeb94474c94c41bae002083d042808b502eb4c30021cba9e83ed1437534a`；
- DEV-018 job `job:01a04c97-e547-77e3-aa90-7db26f8a48af`；
- DEV-018 receipt
  `cairn:v1:sha256:execution.receipt.v1:30ff2c955085ae812447ac51a2b10d2bab63b89b9929336ff294c85f16d0672c`；
- exact input、environment、contract、stderr和trusted evidence bindings由receipt与重新prepared的DEV-018 native job验证；
- 新episode使用独立`EpisodeId`和resolved model configuration，不续接DEV-017 private conversation。

## 3. Contract

- `CandidateNativeRepairParentV1`只能是exact root native follow-up identity或exact上一轮native repair identity；
- repair diagnostic绑定parent、input bundle、environment、contract、failed receipt、stderr、trusted evidence和bounded
  applicant-visible compiler text；generic diagnostic与root-revision diagnostic都不能替代；
- repair revision同时绑定root follow-up、exact immediate parent、exact repair diagnostic、独立episode/model configuration和
  complete changed source submission；
- deserialization重跑current-V1 constructor invariants，archival identity要求canonical exact bytes；
- 每轮由外部明确调用打开新episode；terminal failure、submission或yield都不会自动创建下一轮、build或verdict；
- Candidate只负责生成候选source。Cairn负责冻结上下文、强类型谱系、持久化和边界验证，不替Candidate修改代码。

## 4. Non-goals

- 不自动迭代到compile success，也不设隐式token或轮数循环；
- 不在Cairn中解释并修复`compact_above_kernel`，不硬编码本例答案；
- 不创建按深度编号的second/third follow-up artifacts；
- 不在本slice运行remote build、NPU、semantic Oracle、Admission、performance或verdict；
- 不改变任何internal format version，不添加compatibility reader或migration path。

## 5. Acceptance

- strong parent、diagnostic和repair revision types；wrong-domain compile-fail boundary；
- exact receipt/job/evidence、root/parent/search-input、canonical/current-V1和changed-source negative controls；
- recorded episode覆盖bounded reads、complete submit、explicit yield、restart/replay和无自动续轮；
- focused tests、Clippy和full CI通过；
- 在用户明确授权本轮exact external payload后，live DeepSeek episode提交repair并通过terminal restart；
- live结果只形成Candidate source artifact，不产生build、runtime、semantic correctness或verdict claim。

## 6. Implementation

- `CandidateNativeRepairParentV1`保留root follow-up与immediate repair两种typed parent，不用字符串、generic ID或按深度
  扩张artifact type；root-parent repair同时固定同一root identity，反序列化重跑该cross-field invariant；
- repair-specific diagnostic重用private exact failed-native-receipt verifier，但保持独立public semantic type；它重新验证
  follow-up build的job、input、environment、contract、receipt、stderr、evidence、Docker backend和
  `docker:accelerator:none`；
- repair revision固定search input、root、immediate parent、diagnostic、新episode、resolved model configuration和complete
  changed submission；root-bound diagnostic不能授权以repair为parent的下一轮；
- runtime使用独立instruction、policy和typed submit gateway；initial context包含complete immediate-parent source、bounded
  compiler diagnostic与fixed native gate facts，original task source仍只经bounded read tool按需进入；
- terminal submit result显式写出`automatic_next_repair_round=false`；runtime只归档repair并结束，不调度build或下一episode；
- `candidate_native_repair_deepseek --preflight`在provider/credential/network之前重载所有exact live material并打印将发送
  的payload摘要，正式模式使用独立state root并在结束后重开stores验证terminal projection。

Recorded integration覆盖complete submit、explicit yield、terminal restart和byte-exact replay。负向覆盖wrong typed domain、
non-V1、wrong archive identity、unchanged source、root/parent/diagnostic mismatch，以及拿root-bound diagnostic越权打开下一轮。
两个compile-fail证明root revision build与旧native diagnostic不能替代repair domain。

## 7. Local verification and exact external payload

2026-08-29完成focused tests、33个migration doc/compile-fail tests、all-target Clippy和全仓`scripts/ci.sh`。随后运行
local-only `--preflight`，`external_dispatch_performed=false`，得到：

| Material | Exact identity / scope |
| --- | --- |
| root follow-up | `cairn:v1:sha256:migration.candidate-native-followup-revision.v1:9bc0eeb94474c94c41bae002083d042808b502eb4c30021cba9e83ed1437534a` |
| failed receipt | `cairn:v1:sha256:execution.receipt.v1:30ff2c955085ae812447ac51a2b10d2bab63b89b9929336ff294c85f16d0672c` |
| derived repair diagnostic | `cairn:v1:sha256:migration.candidate-native-repair-build-diagnostic.v1:8a3015d59cd30036fdce2879936cc96c523b4ee5f98ad1012f0e3d1027dbd23f` |
| Candidate search input | `cairn:v1:sha256:migration.candidate-collection-search-input.v1:399351d329299d316756afba4b606ae355ade76b5e3cb56553b76b3078e412c8` |
| public task bundle | `cairn:v1:sha256:migration.sir-task-bundle.v1:ac851b44d57e326ebbabb044ac7b527397afc50a2767df45145328015ed8ac57` |

待授权的new external payload只包含：DEV-017五文件complete source；DEV-018 bounded `bisheng`/`ld.lld` stderr（核心是
`compact_above_kernel`缺少显式kernel function type attribute）；public recovery/search/task manifest与fixed gate facts；以及
DeepSeek主动调用bounded read tool时返回的original task source片段。

不会发送raw trusted evidence、Controller/Worker或provider credential、private control set、Oracle expected output/comparison/
verdict、DEV-017 provider continuation/reasoning。不会在这次调用后自动build或自动开启下一轮repair。用户确认这一个
exact payload后才进行了正式provider dispatch。

## 8. Live evidence

2026-08-29 production-path live run得到：

| Evidence | Exact fact |
| --- | --- |
| episode | `episode:01a04cb7-ce3e-7331-8fd4-a345d629e675` |
| root follow-up | `cairn:v1:sha256:migration.candidate-native-followup-revision.v1:9bc0eeb94474c94c41bae002083d042808b502eb4c30021cba9e83ed1437534a` |
| repair diagnostic | `cairn:v1:sha256:migration.candidate-native-repair-build-diagnostic.v1:8a3015d59cd30036fdce2879936cc96c523b4ee5f98ad1012f0e3d1027dbd23f` |
| repair revision | `cairn:v1:sha256:migration.candidate-native-repair-revision.v1:be182db29ba68757e9fcdff6657ef26d3b54e259ba0240d643f168eee4a29b59` |
| model provenance | `deepseek-v4-pro` / `deepseek-responses`；resolved configuration `cairn:v1:sha256:agent.resolved-runtime-model.v1:eee9ebffd5f2d5f86e4d5ba5de53cec4b01cd03bb3487ba72b5df30e2216275a` |
| behavior | first response提出3次bounded reads，其中1次成功、2次被gateway拒绝；第二次response按反馈完成2次合法reads；第三次提交complete changed tree；第四次explicit yield |
| repair shape | 5 files；primary `src/compact_above_kernel.asc`；保持root source tree形状 |
| model strategy | kernel entry与host declaration从`__global__ __aicore__`改为`__global__ __kernel__`；device class methods仍为`__aicore__` |
| usage | 4 responses；input tokens `50081`；output tokens `23595`；reported cache-read tokens `41728` |
| durability | 4 steps；terminal reason `Yielded`；关闭并重开stores后恢复同一episode与repair artifact |
| authority | `automatic_next_repair_round=false`；`build_or_execution_claimed=false`；没有运行native build、Oracle、Admission或verdict |

两次rejected reads证明model不能绕过task manifest/path boundary；随后它使用可见gateway feedback自行纠正，没有人工改写
请求或source。`__kernel__`修改是model-authored repair strategy，不是Cairn认定的正确语法或修复；只有下一次explicit
native build才能判断它是否通过`bisheng` gate，也仍不能形成semantic correctness claim。

DEV-019 Accepted因为exact DEV-018 feedback已经通过一个新isolated、可恢复的Candidate episode产生immutable、typed
root/parent/diagnostic-linked repair，而统一lineage没有引入自动循环。是否构建该repair属于下一slice和下一次显式动作。
