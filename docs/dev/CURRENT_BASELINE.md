# Cairn 当前开发基线

- 状态：当前事实账本；不把目标设计误报为实现
- 日期：2026-08-30
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
DEV-025又让Controller的完整产品顺序成为可读typed composition skeleton。每一阶段只有独立port且没有
default成功实现；DEV-028纠正了其中把具体Blue/Red策略误写成必经stage的漂移。当前recorded control证明
`Oracle Exploration → Oracle Admission`顺序，unavailable Oracle Exploration control证明不会运行任何下游stage。现有真实
Candidate manager turn同时收敛为recover/select/execute三步，原有effect authority与receipt folding不变。完整
Controller aggregate仍未接通，本片也没有调用live model、Worker、Docker或NPU。
DEV-026现已接入第一个真实Controller prefix：exact SIR Host request和recovery input先归档并冻结，durable start
authority提交后才允许统一Proposal Host运行；terminal/proposal成为typed observation，model-free decision requests
归档后状态明确停在`AwaitingUserIntentDecision`。完整骨架也加入derive decision requests与await user decision两个
独立stage，Intent Admission不能替用户选择。通用Host supervision已从Candidate模块抽出供SIR/Candidate共享；本片
没有运行live model、Intent Admission、Worker、Docker或NPU。
DEV-027继续把actual typed authority grant/decision和independent Intent Admission接入同一个task-owned aggregate：
individual request先进入public CAS，Admission executable与restricted-store target先获得durable start authority，
child使用immutable public store并先提交restricted contract/decision，Controller只接受canonical public outcome并
完整验证task/input/proposal/request/grant/decision binding。aggregate通过`AwaitOracleExplorationWorkspace`接收
Oracle输入，但不会自动运行strategy。Controller aggregate已从migration领域crate移到server composition root，旧模块/re-export直接删除；没有
compatibility path。本片只运行本地model-free process control，没有调用live model、remote Worker、Docker或NPU。
DEV-029补齐了generic Proposal Host的external experiment round trip：current-V1 Host outcome现在可以是terminal或
request/episode/step/model-attempt/operation/exact-arguments绑定的durable yield；Controller重新核对Host journal并在
Worker adapter调用前提交operation authorization/start，job/attempt/contract一致的execution receipt与observation
成为canonical `OperationResult`后，同一episode从原native continuation继续。recorded control证明Host零external
effect、yielding model turn不重发及terminal child restart；本片没有新增具体experiment tool，也未调用live Worker。
DEV-030把Oracle完备性直接固化为current-V1生产类型：Controller按claim×concern×role机械展开mandatory
work item，冻结source/docs/build/tests/knowledge/research/experiment capability的workspace，以parent-linked ledger
保存strategy、Controller-authorized experiment、provenance observation和typed portfolio element，并由独立Admission
从qualified mechanism与honest/mutant/hidden/bypass receipts重算claim outcome。当前尚未从admitted intent构造第一个
production claim解释器或strategy consumer，也没有运行model或Worker。Controller Manager现在会验证workspace引用的
source/docs/build-tests/knowledge/tool catalogs/capability均已归档，再归档exact policy、catalog、workspace、claims及
机械重算的initial ledger，并把task aggregate推进到`RunOracleExploration` ready authority。
DEV-031已接通该strategy consumer。Controller从完整admitted intent body重建structured claim，选择一个exact
cell与catalog executor，先提交durable run authority；deterministic strategy提交带implementation provenance的
strict result，Agent strategy则由generic Proposal Host冻结exact source/docs/build-tests/knowledge、model、tool catalog
和budget。Agent外部effect先yield，Controller在start authority后接收receipt-bound result，将其重算并归档为
distinct Controller observation、Oracle payload和run-bound Oracle observation，再更新immutable ledger并恢复同一
episode。fixed model-debate modules/example/tests已删除；coverage gap保持terminal但Admission永远视为partial。本片
只使用recorded adapter，没有调用live model、网络、remote Worker、Docker或NPU。
DEV-032继续把terminal ledger机械冻结为exact portfolio与strict Admission policy，在同一个Controller aggregate中
冻结qualified mechanism inventory和完整item × control attempt，只接受exact trusted receipt provenance并于event
replay时model-free重算admitted/partial/rejected claim portfolio。DEV-033继续冻结admitted-only Candidate
contract/workspace、exact typed public Oracle bodies、Candidate Host request/start/terminal、product-owned build plan、
durable build start与Worker receipt observation，再机械展开claim × item × plane Candidate controls并独立重算terminal；
没有运行真实control mechanism、model、Worker或NPU。

## 2. 可复用基础

| 基础 | 已有事实 | 当前限制 |
| --- | --- | --- |
| Record/protocol | 强类型 V1 codec、CAS/event、durable identity、record/replay、SQLite fault/restart | 不自动具有 product authority 或 restricted capability |
| Agent runtime | OpenAI-compatible/Anthropic paths、DeepSeek deployment、episode/tool/budget/repair、recorded provider | 保持 domain-neutral；Oracle Agent由catalog分配到单个claim × concern × role cell，不存在fixed debate topology |
| Execution | scheduler/lease/attempt/output、Docker、CUDA/Ascend build 的历史证据 | Worker 不解释 operator intent，不把历史 run 变成当前 claim |
| Product workflow | readable Controller composition skeleton、单一task-owned SIR→terminal aggregate、逐cell deterministic/Agent executor、typed effect observation projection、independent Oracle/Candidate Admission、generic Candidate Host、product-owned build authority、Worker receipt折回与exact replay | 尚无production Oracle/Candidate control runner、失败/partial后的generic revision policy、GitHub/NPU adapter、task intake/catalog、native success或live terminal verdict |
| Verification mechanics | comparison、mutation、receipt binding 和历史 reduction controls | 只有出现真实 Gate consumer 后才按 exact implementation qualification |
| Testkit | DEV-003 provenance/sanitation；DEV-001 clean-room reduction fixture | evaluator-only；production crate 不得依赖或读取 expected/private answer |

## 3. 历史证据的边界

- 已删除的model-backed debate只保留Git历史事实，不是current product path；当前证据来自generic逐cell Host和
  recorded effect adapter，不证明live model quality或完整Oracle portfolio adequacy。
- `matmul-zero-k` 证明一条狭窄 materialization/call-adapter 路径，不代表一般 Oracle coverage。
- historical reduction 证明若干 comparison/mutation blind spot，只作为 control；旧 domain shape 不定义新
  Intent/Oracle schema。
- DEV-001 commit `9dc8243` 和 DEV-003 commit `79a1174` 保留为 current evaluation foundation。
- DEV-002 的 review 在历史上确实发生，但 D-042 已 supersede 其预建 D-040 qualification 方向；对应 code、
  tests、public/private bundle 和 private review record 从 current V1 tree 删除，Git history 足以追溯。

## 4. 当前仍未完成

- qualified Oracle/Candidate control mechanism runner，以及把真实receipt接入现有mechanical attempt/evidence；
- Candidate失败/partial后的generic、observation-bound revision policy；不得恢复已删除的collection/native三段式；
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

当前 Candidate 产品交接已经是：

```text
independently admitted claim portfolio
→ admitted-only CandidateOracleContractV1
→ exact public claim/Oracle bodies + task workspace
→ frozen generic Candidate Proposal Host episode
→ strict CandidateProposalV1
→ product-owned CandidateBuildPlanV1
→ durable Worker start authority + ExecutionReceipt observation
→ mechanically expanded claim × item × plane controls
→ trusted control evidence + independent Candidate Admission
→ admitted / partial / rejected terminal outcome
```

同一Controller aggregate的完整可读骨架是：

```text
exact SIR Host request + recovery input
→ durable SIR episode start authority
→ typed terminal/proposal observation
→ model-free intent decision requests
→ actual typed user decision
→ independent Intent Admission
→ AwaitOracleExplorationWorkspace
→ verify and archive exact Oracle workspace/policy/catalog/claims
→ Controller-derived initial claim × concern × role ledger
→ per-cell deterministic/Agent strategy and typed observations
→ terminal portfolio + strict Admission policy freeze
→ qualified mechanism inventory + mechanical control attempt
→ trusted evidence + independent claim outcome
→ admitted-only Candidate contract/workspace + exact public Oracle bodies
→ generic Candidate Proposal Host request/start/typed terminal
→ product-owned build authority + Worker receipt observation
→ mechanical Candidate control matrix + independent Admission
→ admitted / partial / rejected terminal
```

旧`MigrationWorkflowV1`、collection/native Candidate suffix与固定`dav-3510` build profile已删除。具体Oracle/Candidate
control mechanism runner与失败后的generic revision policy仍须后续slice实现。

最新live output仍是 DEV-020 的 authoritative `SubjectFailed` native build receipt，不是 admitted Candidate
或 verdict。最新local product output是DEV-033 recorded Candidate terminal closure；所有DEV-021–033 recorded/local
workflow、generic-Host replay与receipt折回evidence仍不是新的live build。
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
| DEV-027 | Accepted | actual user decision与independent Intent Admission接入同一durable aggregate并停在Oracle边界；executable/restricted-store authority、real local child、restart/replay/cross-task/drift controls闭合；无live model/Worker调用 |
| DEV-028 | Accepted | Controller主骨架纠正为Oracle Exploration→Admission；旧Blue/Red公开路径删除并收窄为可选model-backed debate strategy；文档、示例、配置、tests与静态漂移controls闭合；无live effect调用 |
| DEV-029 | Accepted | Proposal Host external effect改为typed durable yield；Controller start-before-Worker、receipt provenance和same-episode/no-redispatch resume闭合；无live effect或具体experiment adapter |
| DEV-030 | Accepted | claim-scoped多平面obligation、durable exploration ledger、typed portfolio、independent admission内核与task-owned initial-ledger opening闭合；停在strategy consumer，无live model/Worker |
| DEV-031 | Accepted | structured admitted claims、逐cell deterministic/Agent strategy、typed effect observation→ledger、strict completion provenance与fixed debate删除闭合；无live model/network/Worker/NPU |
| DEV-032 | Accepted | terminal portfolio/policy、qualified mechanism inventory、机械control attempt、trusted evidence与independent claim outcome接入同一Controller aggregate；停在Candidate边界，无live model/network/Worker/NPU |
| DEV-033 | Accepted | generic Candidate Host、product-owned build authority、Worker receipt observation、机械plane controls、independent Candidate Admission与terminal接入同一aggregate；旧collection/native suffix删除，无live model/network/Worker/NPU |

详细历史保留在 Git；当前状态以本表和 [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 为准。
