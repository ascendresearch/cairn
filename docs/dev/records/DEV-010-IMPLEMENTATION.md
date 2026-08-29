# DEV-010 implementation — first qualified local Oracle claim

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-010`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Oracle Exploration`](../../oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md)、
  [`Oracle invariants`](../../oracle/DESIGN_INVARIANTS.md)、
  [`Admission Architecture`](../../design/ADMISSION_ARCHITECTURE.md)
- Requirements：FR-ORACLE-001/002/006/007/008/015/016/027/030/032
- 决策：D-025、D-030、D-032、D-034、D-035、D-042

## 1. Objective

把DEV-009已经可运行、但仍只是candidate-specific comparison evidence的局部机制，冻结成第一个可由
Admission准备、并可在完成Controller发布后供Candidate proposal/search显式消费的局部Oracle capability：

```text
admitted collection-output decision
  → deterministic local Oracle claim proposal
  + actual honest reversed-order implementation receipt (must accept)
  + actual missing-occurrence implementation receipt (must reject)
  + exact mechanism + gate identities
  → mechanical qualification receipt
  → AdmittedCollectionOracleClaimV1 (local/partial only)
```

本slice的产品结果不是多做一组fixture，而是关闭Candidate前置的qualification/type boundary：后续调用必须要求
一个强类型local admitted claim，而不能直接拿intent hypothesis、policy enum、test expectation或raw
comparison冒充Oracle。它还不是已经发布给Candidate的authority；restricted commit/public publish仍必须由
Controller/Admission边界完成。

## 2. Scope and explicit non-goals

只准入一个局部semantic claim：finite-normal-nonzero f32输入上，输出是strictly-above-threshold occurrence的
exact multiset，并且reported count等于selected occurrence count；order unspecified。

明确非目标：

- 不形成`AdmittedOraclePortfolio`、required-claim closure或release资格；
- 不建立Oracle Explorer Agent、Planner、hidden corpus、mutation grid、通用qualification registry或第三人评审；
- 不生成Candidate、不产生Candidate/Migration verdict；
- 不声称zero/subnormal/NaN/Inf、capacity failure、safety、CUDA、Ascend build/NPU、performance或完整domain；
- 不复用historical reduction的全套admission schema来伪装本claim已有那些证据。

## 3. Authority and type boundary

- proposal只能由`CollectionOutputOracleDecisionV1`机械派生，绑定exact intent contract、selection claim、policy、
  domain与DEV-009 mechanism identity；proposal没有admitted字段。
- actual executions必须先经过generic call-adapter capture + authoritative execution receipt validation；raw result、
  test assertion或candidate observation不能代替。
- qualification gate重新物化两条receipt：honest reversed output必须`Equivalent`，不同exact executable产生的
  missing-occurrence output必须为明确mismatch；两个executables必须不同。
- qualification receipt绑定proposal、mechanism、gate、两个invocation/executable/receipt/comparison identities、
  limitations与requalification triggers。
- public admitted claim只暴露opaque qualification receipt identity与claim contract；raw control material可由上层
  authority保持restricted。构造Rust值本身不等于Controller已发布authority。
- local admitted claim与full admitted portfolio是不同类型；任何要求portfolio closure/release的API不能接受它。

## 4. Mechanism qualification scope

DEV-009已有comparator/decoder unit controls与actual honest path。本slice补一个actual fault implementation path，
使首次freeze不只依赖comparator-only mutation：

- honest implementation反序输出全部selected occurrences并报告正确count；unordered policy必须接受；
- fault implementation走同一ABI/process/receipt/materializer path，但丢失一个selected occurrence；必须拒绝；
- existing sequence、duplicate、wrong element、wrong count、count-over-capacity、tamper与strict-V1 controls继续作为
  scoped supporting controls，不被包装成通用考试框架。

任何mechanism/gate/domain/control executable identity变化都触发新的qualification；历史receipt不被原地改写。

## 5. Acceptance

- current-V1 proposal、qualification receipt与local admitted claim均严格反序列化并重跑invariants；
- strong signatures与compile-fail证明intent decision/proposal/raw comparison/local claim不能替代彼此或full
  portfolio；
- actual honest与fault child各自产生authoritative generic execution receipt，gate机械重算一绿一红；
- exact DEV-008 admitted decision无新model call地派生同一local claim proposal；production source无exact private ID/
  hypothesis label/fixture values；
- normal dependency graph不新增`cairn-admission`反向依赖，不复活DEV-002或historical reduction admission依赖；
- focused tests、dependency/vocabulary audit与full `scripts/ci.sh`通过。

## 6. Implementation result

- `cairn-migration`新增严格current-V1的proposal、qualification receipt和local-only admitted claim强类型；
  三者分别绑定exact intent decision、mechanism/gate source identities、actual control receipts和显式
  limitations/requalification triggers。
- qualification gate不信任调用者提供的comparison结论，而是从两个已经验证的generic execution receipts重新
  物化observation：honest reversed-order implementation必须为`Equivalent`，独立missing-occurrence
  implementation必须为`ReportedCountMismatch`且精确少一个expected occurrence。
- 两条control必须绑定同一invocation但不同executable、execution receipt和comparison evidence identities；
  复用同一implementation或伪造closure/limitations会fail closed。
- `cairn-admission`提供production authority入口：它只能从`IntentAdmissionPublicOutcomeV1`重新派生
  exact decision后准备claim；local proposal不能替代admitted intent outcome。
- local claim与完整`cairn_verification::AdmittedOracleV1`保持静态类型隔离，不携带portfolio closure、device、
  safety或release含义。

## 7. Validation evidence

- actual child + authoritative receipt integration：
  `admitted_policy_drives_receipt_bound_collection_materialization` passed；两条child executions均经过generic
  coordinator/capture/receipt路径，honest接受、missing-occurrence拒绝。
- strict/current-V1和negative controls：proposal拒绝non-V1、unknown field和未资格化的sequence policy；
  receipt/claim round-trip重跑结构与exact binding invariants，并拒绝control identity复用和伪造full closure。
- static authority boundaries：`cairn-migration`的2条新增compile-fail和`cairn-admission`的1条新增compile-fail
  通过；proposal不能冒充intent decision/public outcome，local claim不能冒充full portfolio；qualification
  signature只接受validated executions而不接受raw comparison。
- exact private artifact replay：DEV-008 exact live proposal在无provider/model调用下派生相同local claim
  proposal并通过当前mechanism binding。
- dependency/vocabulary audit：normal `cairn-migration` graph不依赖`cairn-admission`；production source不含exact
  private hypothesis ID、label或fixture expected values；未恢复DEV-002 admission framework或兼容路径。
- full `scripts/ci.sh`：通过（fmt、log isolation、locked check、all-target/all-feature clippy、workspace tests、
  doc/compile-fail、link与whitespace checks）。

## 8. Remaining boundary

DEV-010只关闭了一个局部Oracle claim的qualification/type boundary。`AdmittedCollectionOracleClaimV1`由
Admission准备并不等于已经发布：Controller仍须先保证restricted qualification receipt/decision commit，再
发布exact public claim identity，Candidate consumer才可依赖它。下一slice应把这个最小发布动作与首个
Candidate proposal/search consumer放在同一纵向实现中；不为此先建通用portfolio、registry、Planner、hidden
corpus或完整Admission状态机。

该claim仅覆盖host adapter上的finite-normal-nonzero f32、strict `>`、exact occurrence multiset与reported
count。它不声称完整Oracle portfolio、CUDA/Ascend device evidence、safety、numerical edge policy、candidate
verdict或CP1完成。
