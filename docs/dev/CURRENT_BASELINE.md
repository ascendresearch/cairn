# Cairn 当前开发基线

- 状态：当前事实账本；不把目标设计误报为实现
- 日期：2026-08-29
- 产品范围：仅限 CUDA → Ascend C 算子移植

## 1. 当前结论

Cairn 已有可复用的 durable agent runtime、record/replay、tool loop、provider protocols、worker/scheduler 和
局部验证基础，但新的 CUDA → Ascend C 端到端 workflow 尚未完成。

CP0已经证明DeepSeek可作为runtime actor面对不同task产生task-generic SIR proposal，并在atomic compaction
task上改变一个具体Oracle选择。CP0结论为`Go`：SIR当前以proposal-only capability进入架构，并继续沿第一个
真实consumer扩展。DEV-006已用真实DeepSeek闭合caller/source分离的完整输入与typed proposal contract；首版
submission被strict gateway拒绝后在同一continuation修复成功。DEV-007随后由model-free process从exact
live proposal生成首个output-order request，实际任务authority选择了unordered-set语义。DEV-008已经把该
选择作为exact typed decision，经独立Admission child process机械promotion并restricted commit为首个
`MigrationIntentContractV1`；一个collection-output comparator只能从该contract选择unordered multiset +
reported-count policy。DEV-009已让actual child process经generic authoritative receipt进入可信物化与比较；
DEV-010已用独立honest reversed-order和missing-occurrence实现资格化这一exact mechanism，并把结果限制为
local-only claim。DEV-011又先把exact qualification/claim/decision材料提交到restricted store，再返回嵌入完整
intent contract的public outcome并生成answer-free local Candidate search input。DEV-012已经让真实DeepSeek
Candidate通过3次bounded reads提交首个strict typed Ascend C/CANN source proposal，并在3步后yield且通过terminal
restart。DEV-013–020 已把 exact proposal/revisions 多次经 Controller 调度到远端 no-device Ascend Worker，
取得 generic build、product-owned native ASC build、typed diagnostic、隔离 DeepSeek repair 和 rebuild 的
authoritative receipts。DEV-015 的 generic build 曾成功但暴露 host fallback，DEV-016 起的 fixed `bisheng`
native gate 正确关闭该绕过；DEV-020 的最新 exact repair 仍为 `SubjectFailed`。DEV-021随后把这段反复手工
串接的native build/diagnostic/follow-up/repair suffix固化为task-owned current-V1 durable workflow；recorded
consumer、mid-episode SQLite restart、exact replay/changed-input和typed domain controls均已闭合，旧one-shot
examples、专用smoke scripts和三项手工native ignored tests已删除。DEV-021没有调用模型或远端Worker，因此当前
仍无 native build success、NPU execution、semantic Candidate Admission、performance 或最终 verdict。
DEV-022又以一个generic Proposal Host process接管SIR及Candidate initial/revision/native repair profiles；
DEV-021的persisted episode request现在可由Controller从public CAS物化、经Host运行并把typed publication返回
同一workflow。跨role、strict binding、child-process restart replay均用recorded transport闭合，旧`cairn-sir`
及SIR/Candidate专用one-shot launcher已删除。DEV-022同样没有调用live model或Worker。
DEV-023现已让active Controller中的单任务process manager直接消费该durable next action：native dispatch先提交再
schedule，Worker terminal receipt机械折回typed diagnostic/terminal，Candidate episode先冻结exact Host binary/
model/runtime与operation marker再启动generic Host。`NoCandidate`、expired、ambiguous和Host invocation/process
failure均保留原ID并typed blocked，不会隐式换attempt或episode。两个materially different task、local controlled
receipt及real recorded Host child/restart controls已闭合；本片仍没有调用live model或Worker。
DEV-024又删除了遗留的SIR/Candidate role-specific runner及可绕过generic Host的两套integration test，把所有
现有profile收敛到一个durable Proposal Loop。Host application层现在明确执行freeze request、drive frozen
episode、freeze terminal三段；共同loop在episode打开前冻结model-visible content IDs、tool catalog、budget和
validated capability grant，tool result先归档为`OperationResult`再进入continuation，invalid strict submission
可在同一budget内修复。Host只执行pure/read-only capability，外部effect typed fail closed并保留给Controller。
`run_proposal_loop`顶层只保留open/dispatch/settle/admit/authorized-execute/project/advance步骤，各步骤之间使用
distinct internal typestate。本片没有调用live model、Worker、Docker或NPU。
DEV-025又让Controller的完整十阶段产品顺序成为可读typed composition skeleton。每一阶段只有独立port且没有
default成功实现；recorded control证明顺序，unavailable Oracle Blue control证明不会运行任何下游stage。现有真实
Candidate manager turn同时收敛为recover/select/execute三步，原有effect authority与receipt folding不变。完整
Controller aggregate仍未接通，本片也没有调用live model、Worker、Docker或NPU。
DEV-026现已接入第一个真实Controller prefix：exact SIR Host request和recovery input先归档并冻结，durable start
authority提交后才允许统一Proposal Host运行；terminal/proposal成为typed observation，model-free decision requests
归档后状态明确停在`AwaitingUserIntentDecision`。完整骨架也加入derive decision requests与await user decision两个
独立stage，Intent Admission不能替用户选择。通用Host supervision已从Candidate模块抽出供SIR/Candidate共享；本片
没有运行live model、Intent Admission、Worker、Docker或NPU。

## 2. 可复用基础

| 基础 | 已有事实 | 当前限制 |
| --- | --- | --- |
| Record/protocol | 强类型 V1 codec、CAS/event、durable identity、record/replay、SQLite fault/restart | 不自动具有 product authority 或 restricted capability |
| Agent runtime | OpenAI-compatible/Anthropic paths、DeepSeek deployment、episode/tool/budget/repair、recorded provider | 保持 domain-neutral；旧 Blue/Red 拓扑不是目标产品拓扑 |
| Execution | scheduler/lease/attempt/output、Docker、CUDA/Ascend build 的历史证据 | Worker 不解释 operator intent，不把历史 run 变成当前 claim |
| Product workflow | readable twelve-stage Controller composition skeleton、durable SIR-to-user-decision prefix、task-owned Candidate native suffix aggregate、Controller process drivers、shared generic Proposal Host supervision、统一request/episode/observation/submission/terminal loop、scheduler/reconcile/receipt折回、exact replay | prefix明确等待真实user decision；Intent Admission/Oracle与Candidate suffix尚未并入一个连续aggregate；Host尚无external experiment yield/resume；尚无task intake/catalog、native success或最终migration workflow |
| Verification mechanics | comparison、mutation、receipt binding 和历史 reduction controls | 只有出现真实 Gate consumer 后才按 exact implementation qualification |
| Testkit | DEV-003 provenance/sanitation；DEV-001 clean-room reduction fixture | evaluator-only；production crate 不得依赖或读取 expected/private answer |

## 3. 历史证据的边界

- Blue/Red dogfood 证明 durable model/tool loop 和 artifact-mediated revision 的一部分，不证明 debate 是
  Admission 或固定 Agent topology。
- `matmul-zero-k` 证明一条狭窄 materialization/call-adapter 路径，不代表一般 Oracle coverage。
- historical reduction 证明若干 comparison/mutation blind spot，只作为 control；旧 domain shape 不定义新
  Intent/Oracle schema。
- DEV-001 commit `9dc8243` 和 DEV-003 commit `79a1174` 保留为 current evaluation foundation。
- DEV-002 的 review 在历史上确实发生，但 D-042 已 supersede 其预建 D-040 qualification 方向；对应 code、
  tests、public/private bundle 和 private review record 从 current V1 tree 删除，Git history 足以追溯。

## 4. 当前仍未完成

- 完整 Intent Admission、Oracle portfolio/Candidate authority chain；DEV-008–024 只覆盖一个窄
  host/finite-normal collection claim，从 promotion、Oracle publication 推进到 remote native repair build和
  recorded durable suffix；
- native ASC build success、真实 NPU execution、semantic/safety/performance admission 与最终 verdict；
- 统一 CUDA reference → Ascend build/NPU evidence graph；
- performance、knowledge/skill、feedback 和 platform/release hardening。

目标设计中的完整 Proposal Host pool、多任务catalog、十一位置 catalog、七类 Planner、完整 mechanism registry 和
future crate只是条件设计，不是当前待办或已实现事实；DEV-023只实现一个已存在Task需要的最小Controller supervisor，
DEV-024只统一现有role consumer，不预建新role或Host pool。

## 5. 当前必须停止的外推

- coding agent 根据 fixture 答案生成“模型 proposal”；
- 把 `reduce-sum-f32`、D-039 identity、expected hypotheses 写入 product prompt/type/policy；
- 用更多固定 case 数量代表 SIR 或 Oracle 已泛化；
- 在证明 SIR value 前创建 Admission/qualification/process/role 框架；
- 让 Controller/Proposal runtime 读取 restricted answer；
- 为 superseded development format 保留 alias、reader、converter 或 migration；
- 将 recorded 误报为 live、build 误报为 device run、合理 prose 误报为 correctness。

## 6. 当前输入与近期输出

当前可用输入：

```text
generic durable agent runtime + recorded provider
+ task artifacts and scoped source-inspection tools
+ DEV-001 evaluation fixture (answer visible only after episode)
+ DEV-003 sanitation/provenance controls
```

当前 SIR 输出仍只允许是：

```text
typed cited facts
+ competing intent hypotheses
+ calibrated unknown/conflict
+ durable recorded/live episode facts
```

它本身不是 `MigrationIntentContract`，没有 hidden access、execution、Gate 或 verdict authority。DEV-005已用
同一production path处理atomic compaction task，并证明proposal可阻止把atomic output order误升格为intent；
DEV-008只允许独立Admission消费exact proposal + authority decision后构造contract。

当前 Candidate 纵向交接已经是：

```text
committed public local Oracle outcome
+ exact answer-free CollectionCandidateSearchInputV1
+ exact IntentRecoveryInputV1
+ bounded task-local source bundle
→ real DeepSeek Candidate episode
→ immutable CollectionCandidateProposalV1
→ Controller scheduler → remote Worker build
→ receipt-bound diagnostic → isolated DeepSeek revision/repair episode
→ product-owned native ASC rebuild
→ task-owned durable Candidate suffix state / exact next action
→ generic Proposal Host request / typed publication round-trip
→ single-task Controller manager / scheduler and receipt reconciliation
→ one frozen Proposal Host lifecycle / common durable role loop
```

当前Controller前缀另有一条无live effect的durable接线：

```text
exact SIR Host request + recovery input
→ durable SIR episode start authority
→ typed terminal/proposal observation
→ model-free intent decision requests
→ AwaitingUserIntentDecision
```

它尚未消费真实user decision，也未调用independent Intent Admission。

最新live output仍是 DEV-020 的 authoritative `SubjectFailed` native build receipt，不是 admitted Candidate
或 verdict。最新local product output是DEV-024 common-loop lifecycle controls；DEV-023 recorded/local manager、
generic-Host process replay与receipt折回evidence仍不是新的live build。
DEV-020证明 exact repair 在 exact CANN/`dav-3510` no-device environment 中未通过 `bisheng`；不能把compile
failure外推为语义错误，也不能把recorded workflow或跨主机闭环误报为真实 NPU evidence。

## 7. 当前状态

| Slice | 状态 | 事实 |
| --- | --- | --- |
| DEV-001 | Accepted | reduction evaluator fixture；不供 runtime answer projection |
| DEV-002 | Superseded | 过早 qualification framework 已从 current tree 删除 |
| DEV-003 | Accepted | 最小 fixture provenance/sanitation foundation |
| DEV-004 | Accepted | generic proposal path经recorded replay、full CI和真实DeepSeek episode闭合；只证明runtime workflow接通 |
| DEV-005 | Accepted | reduction与atomic compaction共享production path；SIR改变order-sensitive Oracle选择，CP0 Go |
| DEV-006 | Accepted | 完整`IntentRecoveryInputV1`/`IntentHypothesisSetProposalV1`通过strict、recorded、absence、full CI与真实DeepSeek repair/restart |
| DEV-007 | Accepted | model-free process从exact live proposal机械生成scoped output-order request；实际任务authority选择unordered-set hypothesis；promotion由DEV-008完成 |
| DEV-008 | Accepted | independent SIR ingress与Admission authority process闭合；exact typed decision restricted-commit为首个contract并驱动contract-only collection comparator policy |
| DEV-009 | Accepted | actual host child经双ABI output、generic authoritative receipt进入contract-bound collection materialization/comparison；expected不进入candidate-visible input |
| DEV-010 | Accepted | actual honest/fault implementations资格化首个local-only Oracle claim；不声称portfolio closure或已发布Candidate authority |
| DEV-011 | Accepted | restricted claim/qualification/decision先commit，再发布嵌入完整intent contract的outcome并生成answer-free local Candidate search input |
| DEV-012 | Accepted | 真实DeepSeek Candidate经bounded reads提交strict typed Ascend C/CANN proposal并通过terminal restart；尚无build/run/verdict evidence |
| DEV-013 | Accepted | exact Candidate 经 Controller/remote Worker 得到首个 authoritative `SubjectFailed` build receipt |
| DEV-014 | Accepted | 新隔离 DeepSeek episode 消费 receipt-bound diagnostic，提交 parent-linked full revision |
| DEV-015 | Accepted | exact revision remote build `Succeeded`，同时发现 Candidate-owned CMake 可走 host fallback，故不算 native success |
| DEV-016 | Accepted | product-owned ASC harness 强制 exact primary 进入 `bisheng`/`dav-3510`，得到真实 native `SubjectFailed` |
| DEV-017 | Accepted | 新隔离 DeepSeek episode 消费 native diagnostic 并提交 changed full-source follow-up |
| DEV-018 | Accepted | exact follow-up 重新进入相同 native gate并得到可恢复 `SubjectFailed` |
| DEV-019 | Accepted | 建立显式、可重复但不自动续轮的 native repair lineage；DeepSeek 提交 exact repair |
| DEV-020 | Accepted | exact repair 远端 native rebuild 为 `SubjectFailed`；`__kernel__` 在当前 toolchain 为 unknown type |
| DEV-021 | Accepted | task-owned current-V1 workflow固化native suffix；recorded two-material consumer、restart/replay与旧手工入口删除闭合；无model/remote Worker调用 |
| DEV-022 | Accepted | generic Proposal Host承载SIR/Candidate role profiles并消费persisted workflow request；child restart exact terminal replay与旧专用launcher删除闭合；无live model/Worker调用 |
| DEV-023 | Accepted | active Controller单任务manager消费durable action并连接Host/scheduler/receipt；exact operation marker、blocked no-replacement与旧public helper删除闭合；无live model/Worker调用 |
| DEV-024 | Accepted | SIR/Candidate role-specific runner与旁路测试删除；统一冻结request/profile/capability、durable observation、strict repair与terminal lifecycle；无live model/Worker调用 |
| DEV-025 | Accepted | 完整Controller顺序固化为typed stage-port骨架；unavailable stage fail closed；真实Candidate turn收敛为recover/select/execute；无live effect调用 |
| DEV-026 | Accepted | durable Controller接通exact SIR→decision requests并停在用户决策边界；shared Host supervision、restart/replay/cross-task/model-drift controls闭合；无live effect调用 |

详细历史保留在 Git；当前状态以本表和 [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 为准。
