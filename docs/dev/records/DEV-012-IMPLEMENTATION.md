# DEV-012 implementation — first bounded Candidate proposal episode

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-012`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Logical Architecture`](../../design/LOGICAL_ARCHITECTURE.md)、
  [`Agent Architecture`](../../design/AGENT_ARCHITECTURE.md)、
  [`System Design`](../../SYSTEM_DESIGN.md)
- Requirements：FR-CAND-001、FR-CAND-005
- 决策：D-025、D-030、D-034、D-035、D-042

## 1. Objective

让第一个真实Candidate runtime actor直接消费DEV-011已经形成的answer-free、local-only authority输入：

```text
CollectionCandidateSearchInputV1
  + exact IntentRecoveryInputV1
  + exact task-local source bundle
  + pinned model/tool/context/budget policy
  → durable Candidate Search episode
  → bounded source reads
  → pure typed source submission
  → immutable CollectionCandidateProposalV1
```

该slice的成功标准是configured DeepSeek通过production path提交一个严格current-V1、可重建、非权威的Ascend C
source proposal。它不要求该source已经build、运行或正确，也不允许repository coding agent根据fixture答案代写
live proposal。

## 2. Role and authority

| Role | 输入/输出 | 能力 | 明确没有 |
| --- | --- | --- | --- |
| Candidate runtime model | frozen public authority + scoped source → proposal | bounded read、pure submit | hidden/restricted、Admission、execution、verdict |
| trusted product gateway | 校验source bundle、current-V1 submission和runtime provenance | 注入exact task/search/model/episode binding | 编写candidate或声明正确 |
| repository coding agent | generic types、prompt、gateway、recorded/live wiring | repository development与测试 | 代替runtime model生成live candidate |
| Admission/Oracle | 本slice只提供已发布public input | 无新决策 | 不向Candidate泄露expected/control material |

`CollectionCandidateProposalV1`是model-authored proposal，不是frozen admitted Candidate、build receipt或
Candidate verdict。Candidate role不能把source proposal、解释文本或自报字段升级为pass。

## 3. Data and visibility

模型初始context只包含：

- exact `CollectionCandidateSearchInputV1`；
- 与其identity一致的`IntentRecoveryInputV1`公开caller/target声明；
- exact task bundle manifest，不含source bytes；
- fixed Candidate instruction、tool catalog、role policy和budget。

source bytes只能经bounded task-artifact read进入后续turn。model-visible context和tool results不得包含：

- expected collection/output；
- honest/fault qualification output、comparison或execution receipt；
- qualification receipt正文、control executable或restricted decision正文；
- hidden corpus、judge policy或fixture expected answer。

模型只提交candidate-relative path、source text、primary source和解释。task、search input、Oracle、episode、model
configuration和content identities均由trusted gateway派生，模型不得重抄。

## 4. Types and current V1

- 新增语义独立的Candidate source path/text/explanation类型，不复用通用`String`冒充不同概念；
- source proposal包含1..N个canonical、唯一、有界文件，一个必须存在于files中的primary source，以及非空解释；
- envelope绑定exact Candidate search input、episode和resolved-model configuration；
- strict Deserialize重跑path、size、ordering、uniqueness、primary membership和V1 invariants；
- proposal与`AdmittedCollectionOracleClaimV1`、完整`AdmittedOracleV1`、Candidate verdict保持静态类型隔离；
- 直接修改current V1，不增加版本、不增加compatibility reader/alias/converter。

## 5. Scope and non-goals

本slice只实现一次initial proposal episode。明确不实现：

- Candidate revision/parent lineage、diagnostic repair loop；
- Ascend build、CUDA/Ascend execution、device receipt或comparison；
- Candidate Admission、Candidate/Migration verdict、performance；
- full Oracle portfolio、Planner、registry、knowledge/skill或新的process crate；
- Candidate对hidden/restricted material的任何读取能力。

FR-CAND-007的多revision lineage在真实rejection/repair consumer出现时直接扩展current V1；本slice不预建空的
revision framework，也不声称已经满足该requirement。

## 6. Acceptance

- strict types、non-V1/unknown/path/size/order/primary negative controls通过；
- scripted与recorded provider走同一durable episode/tool path，terminal SQLite/CAS restart重建相同proposal；
- exact request audit证明初始turn含public authority/manifest但无source bytes或restricted vocabulary，read后才出现源码；
- gateway注入exact search-input/episode/model identity，wrong recovery/task-bundle binding在model call前失败；
- model只能调用fixed read/submit tools，提交后仍必须yield；budget/ambiguous effect继续使用`cairn-agent`语义；
- normal dependency graph不新增Admission反向依赖，不创建Candidate crate；
- focused tests、no-default-features check、Clippy和full `scripts/ci.sh`通过；
- 因本slice改变model-authored output contract，`Accepted`前需要一次用户授权的真实DeepSeek提交与restart证据。

## 7. Remaining boundary

成功proposal只证明真实Candidate actor进入了workflow。下一slice必须依据该source artifact和实际target/toolchain
事实选择最短build consumer；在build evidence出现前，不实现verdict、performance或完整correction topology。

## 8. Implementation and recorded evidence

实现直接复用现有`cairn-agent` durable episode，而没有创建Candidate crate或把Admission引入Candidate正常依赖图：

- `candidate_episode.rs`定义strict current-V1 source submission/proposal、Candidate profile、bounded read与pure submit tool；
- `candidate_search.rs`可从canonical bytes和调用者提供的exact typed identity重载已发布search input；
- model initial request包含exact public authority、recovery input和task manifest，但不含source bytes；源码只有在
  `candidate_read_task_artifact`成功后才进入continuation；
- trusted gateway注入search-input、episode和resolved-model identity；模型只提交relative paths、source、primary
  source和explanation；
- recorded integration覆盖三步read/submit/yield、SQLite/CAS terminal restart和exact model replay；negative controls
  覆盖non-V1、unknown field、path、ordering、size、primary membership与wrong recovery/search binding；
- normal dependency audit确认`cairn-migration`没有新增`cairn-admission`依赖，production prompt/code没有fixture
  answer或private-control vocabulary。

Focused check、两个crate的Clippy、compile-fail和全量`scripts/ci.sh`均通过。

## 9. Authorized live evidence

用户明确授权将exact public recovery/search input及Candidate按需读取的task source发送至DeepSeek API；private
control set、expected output和Oracle判定不在model-visible projection中。2026-08-28的production-path live run得到：

| Evidence | Exact fact |
| --- | --- |
| episode | `episode:01a04bb8-3eb5-7c50-9bc8-00f4eddd35b1` |
| search input | `cairn:v1:sha256:migration.candidate-collection-search-input.v1:399351d329299d316756afba4b606ae355ade76b5e3cb56553b76b3078e412c8` |
| proposal | `cairn:v1:sha256:migration.candidate-collection-proposal.v1:41809ea7233868fc33cfc23c099d80192c4625dc66b9031f00f76e7101055a38` |
| actor/runtime | configured `deepseek-v4-pro` through `deepseek-responses` |
| behavior | first turn made 3 bounded task-artifact reads; second turn submitted one proposal; third turn yielded |
| proposal shape | `CMakeLists.txt`、`include/compact_above.h`、`src/compact_above.cpp`; primary source is the `.cpp` |
| durability | 3 steps; terminal reason `Yielded`; close/reopen recovered the same terminal episode and proposal |
| authority | `build_or_execution_claimed = false`; no build、execution、comparison or verdict occurred |

The model-authored explanation lists unresolved CANN header、launch macro、scalar GM access and device-pointer
assumptions. Its prose that the design intends to preserve the admitted semantics is rationale, not evidence or a pass
claim. The resulting source remains an immutable, non-authoritative, unbuilt proposal. DEV-012 is Accepted because the
real Candidate actor crossed the answer-free boundary and produced a reconstructable typed proposal—not because the
proposal has been shown correct.
