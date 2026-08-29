# DEV-011 implementation — publish local Oracle and open Candidate input

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-011`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Admission Architecture`](../../design/ADMISSION_ARCHITECTURE.md)、
  [`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)、
  [`Oracle Exploration`](../../oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md)
- Requirements：FR-ORACLE-001/006/007/008/016/027/030/032，FR-CAND-005
- 决策：D-025、D-030、D-032、D-034、D-035、D-042

## 1. Objective

关闭DEV-010明确留下的authority gap，并立即建立第一个真实consumer：

```text
PreparedAdmittedCollectionOracleClaim
  + exact admitted intent public outcome
  → restricted Oracle decision
  → archive proposal + honest/fault comparisons + qualification receipt + claim + decision
  → CollectionOracleAdmissionPublicOutcomeV1
  → CollectionCandidateSearchInputV1 (local exploration only)
```

public outcome只有在restricted material全部按exact typed identity提交成功后才返回。Candidate侧只能消费这个
outcome，不能拿raw claim、proposal、comparison或qualification receipt绕过发布边界。

## 2. Scope and explicit non-goals

本slice只实现：

- claim-scoped Oracle restricted decision与commit-before-publish顺序；
- 最小公开outcome，重复task、recovery input、intent contract和exact local claim等关键binding；
- 一个非空的Candidate production consumer，把公开authority机械投影成严格V1 search input；
- local-only scope，明确禁止把该输入用于Candidate verdict或release。

明确非目标：

- 不启动DeepSeek或其他Candidate model episode，不生成Ascend C source；
- 不实现Candidate revision、build/run、diagnostic correction、verdict或performance；
- 不形成`AdmittedOraclePortfolio`、required-claim closure、Planner、hidden corpus、registry或通用outbox；
- 不复制restricted execution output、expected values或control executable bytes到Candidate input；
- 不恢复historical reduction Candidate/admission schema作为新产品路径。

## 3. Authority and publication boundary

- `cairn-admission`从exact `IntentAdmissionPublicOutcomeV1`重新派生decision，并验证DEV-010 claim的contract、
  selection claim和decision identities；调用者不能自报这些binding。
- restricted decision绑定exact gate、intent restricted decision、qualification receipt和local claim identities。
- commit按proposal、两条comparison evidence、qualification receipt、claim、restricted decision顺序写入同一个
  Admission-owned store；任一put失败或identity变化都不返回public outcome。
- public outcome不含control observations、expected values、execution receipts、executable identities或
  qualification limitations正文；这些只通过opaque restricted decision/receipt identity追溯。
- Controller仍负责把返回的canonical public outcome归档/发布；本slice不宣称通用workflow event/outbox已经存在。

## 4. Candidate consumer boundary

- Candidate类型留在现有产品crate的独立module；当前没有独立process/security/复用证据，不新增crate。
- `CollectionCandidateSearchInputV1`只接受`CollectionOracleAdmissionPublicOutcomeV1`，保留task、recovery input、
  intent contract、public outcome、local claim、selection claim、domain和strength identities。
- scope固定为`LocalOracleExplorationOnly`；该类型不能传给要求full portfolio或Candidate verdict的API。
- 本输入不包含答案、expected collection、honest/fault outputs或private qualification material；下一slice的
  runtime model只可读取task-scoped public source/context和这个公开contract。

## 5. Acceptance

- restricted decision、public outcome和Candidate search input严格反序列化current V1并重跑invariants；
- DEV-010 qualification path成功commit后才产生public outcome；injected store failure不产生outcome；
- public/restricted stores可按exact identities重建各自允许的artifacts，Candidate input无restricted leakage；
- static boundary证明raw/local claim不能替代published outcome，local search input不能替代full Oracle portfolio；
- normal dependency graph保持`cairn-admission → cairn-migration`单向；产品crate不反向依赖Admission；
- focused tests、exact replay（无model call）、vocabulary/dependency audit和full `scripts/ci.sh`通过。

## 6. Implementation result

- `cairn-admission`新增严格V1的restricted Oracle decision、public outcome和prepared admission；public outcome
  嵌入完整公开`MigrationIntentContractV1`，反序列化时可重新验证claim/contract identity，而不是信任重复的
  task/recovery/contract字符串。
- `commit_collection_oracle_admission`按exact identity提交proposal、honest/fault comparisons、qualification
  receipt、local claim和restricted decision；prepared value不暴露pending public outcome accessor，只有commit
  成功调用才返回可发布outcome。
- Candidate consumer保留在`cairn-migration::candidate_search`产品module，没有创建独立crate。Admission从
  committed outcome机械投影task、recovery input、intent contract、outcome、claim、selection claim、domain与
  strength，生成`CollectionCandidateSearchInputV1`。
- search scope固定为`LocalOracleExplorationOnly`；public Candidate input不含expected、honest/fault comparison、
  execution receipt、executable或qualification正文。

## 7. Validation evidence

- admission composition：generic coordinator产生两条authoritative execution receipts，DEV-010 gate形成一绿一红
  qualification；restricted SQLite commit成功后才返回public outcome并生成Candidate input。
- injected restricted-store failure返回`Storage`错误且没有public outcome；已提交的不可变CAS材料无需回滚，且
  不能被Candidate消费为发布事实。
- strict V1：restricted decision、embedded intent/public outcome、Candidate input canonical round-trip通过；
  non-V1、unknown field和changed recovery-input/contract binding均拒绝。
- static boundaries：raw claim不能传给commit或published-outcome Candidate入口；local Candidate input不能替代
  full admitted portfolio；产品crate normal graph不依赖`cairn-admission`。
- exact private replay：DEV-008 exact live proposal在无provider/model调用下继续派生同一intent decision和local
  Oracle proposal，证明本slice未改变SIR含义。
- full `scripts/ci.sh`：通过（fmt、log isolation、locked check、all-target/all-feature clippy、workspace tests、
  doc/compile-fail、link与whitespace checks）。

## 8. Remaining boundary

DEV-011只打开了一个公开、answer-free、local-only Candidate输入。它没有运行Candidate model、生成Ascend C、
创建revision lineage或执行build/run/verdict。下一slice必须直接复用durable agent runtime启动bounded DeepSeek
Candidate episode，并让模型读取task-scoped public source/context；不在两者之间新增Candidate registry、Planner、
review role或完整portfolio基础设施。

该输入只能支持当前局部Oracle scope内的探索。即使后续生成的candidate通过这个local claim，也不能据此产生
Candidate/Migration verdict、release结论或CP1完成声明。
