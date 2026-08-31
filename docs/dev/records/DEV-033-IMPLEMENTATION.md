# DEV-033 — task-generic Candidate authority and aggregate merge

> Amended by D-044/DEV-036: Candidate authority/build/Admission remain; the independent proposal
> runtime described below was deleted from current V1.

- 状态：`Accepted`
- 日期：2026-08-30
- 依赖：D-043、DEV-030–032
- 外部执行：无；未调用live model、互联网、remote Worker、Docker或NPU

## 1. 目标

消除Independent Oracle Admission与Candidate之间的手工/collection-only接缝，把Candidate生成、build、
receipt-bound repair、Candidate Admission和terminal outcome逐步并入同一个task-owned Controller aggregate。
Candidate只能消费exact admitted claim portfolio及其公开artifact body，不能读取partial/rejected claim、hidden
Admission controls或修改Intent/Oracle judge。

## 2. 已实现

当前业务骨架已经连续到首个generic Candidate proposal：

```text
independent Oracle outcome
→ derive CandidateOracleContractV1
→ derive CandidateWorkspaceV1
→ materialize exact admitted claim/portfolio artifact bodies
→ freeze Candidate proposal step request
→ durable Candidate episode start authority
→ common run_proposal_loop
→ strict CandidateProposalV1 terminal
→ product-owned CandidateBuildPlanV1
→ durable build start authority
→ Worker ExecutionReceipt observation
→ mechanical claim × item × plane Candidate controls
→ independent Candidate Admission
→ admitted / partial / rejected terminal outcome
```

- `CandidateOracleContractV1`只投影`Admitted` claims，并要求每个entry都是positive `Contributed` resolution；
  coverage gap、partial和rejected work不能反序列化为Candidate authority。
- `CandidateWorkspaceV1`保留task、recovery/admitted intent、Oracle workspace/contract、task bundle及
  docs/build-tests/knowledge的distinct typed edges。
- `CandidateOracleMaterialsV1`要求每个claim body、portfolio element、semantic material kind和exact CAS body
  与contract完整对应；缺失、额外、重排、跨cell、typed identity或body drift全部fail closed。
- generic proposal step新增`CandidateStrategy` role；它只有bounded task read和pure
  `candidate_submit_proposal`，没有Admission、execution、hidden-material或verdict authority。
- Candidate profile的函数主体保持可读架构：validate frozen inputs、archive task、archive prompt、freeze loop、
  open gateways、run common loop、finish typed publication。
- Controller新增request freeze、episode authorization和typed terminal events；restart/replay重新验证
  workspace/contract/material/request/model/episode/publication binding。
- build plan显式冻结image、runner、Worker pools、capabilities、timeout、capture与network policy；Agent不能提交
  build authority，Controller在任何Worker effect前先持久化start authority。
- Candidate Admission从每个admitted Oracle work item的closed plane机械派生source-build、static-analysis、
  execute-observation、semantic-comparison、safety和performance obligations；缺失receipt保持`Partial`，失败
  receipt产生`Rejected`，只有完整通过才是`Admitted`。
- 同一个Controller aggregate保存build observation、Admission attempt/evidence/outcome和terminal status；recorded
  restart control已贯通整个SIR→terminal骨架。
- recorded scatter task通过同一个generic Host profile，证明新路径不依赖现有compact/reduction fixture或旧
  collection Candidate tool。

## 3. 已删除的旧路径

- `MigrationWorkflowV1`、旧Candidate server manager/config与双aggregate启动入口；
- collection-specific Candidate search/proposal/revision/native follow-up/native repair modules与Host roles；
- 固定CMake/ASC/`dav-3510` build profile、对应diagnostic/repair workflow和旧live build测试；
- legacy alias、reader/writer和conversion path均未保留。

失败或partial Candidate outcome的后续重开策略属于下一slice；当前V1不会把失败结果交给旧repair路径，也不会让
Candidate修改已冻结Oracle。下一slice必须以新的generic proposal episode和exact observation lineage设计，而不是恢复
已删除的collection/native三段式。

## 4. 验证

- exact admitted material、body/type drift、coverage-gap exclusion和strict decode control；
- materially different recorded scatter Candidate Host control；
- 完整Controller durable prefix到Candidate terminal的restart/replay control；
- all-feature Clippy `-D warnings`、no-default workspace check、full `scripts/ci.sh`和`git diff --check`通过；
- production新路径扫描无`reduce-sum-f32`、D-039、`FiniteNormalF32StrictlyAboveThreshold`、`dav-3510`或
  `candidate_submit_collection`。

## 5. 非目标

- 本阶段未运行live model、network、Worker、Docker或NPU；recorded receipt只验证authority/replay，不产生
  live build/correctness claim；
- 不让Candidate看到restricted/hidden Admission receipts或用Candidate表现调宽Oracle；
- 不预建兼容格式、V2、legacy adapter或generic ID material bag；
- 不把旧suffix仍存在误报为已删除。
