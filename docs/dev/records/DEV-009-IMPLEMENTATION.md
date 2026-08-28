# DEV-009 implementation — contract-bound collection observation

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-009`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Oracle Exploration`](../../oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md)、
  [`Oracle invariants`](../../oracle/DESIGN_INVARIANTS.md)、
  [`Runtime Architecture`](../../design/RUNTIME_ARCHITECTURE.md)
- Requirements：FR-INTENT-012、FR-ORACLE-002/006/007/013/016/029/030
- 决策：D-025、D-030、D-032、D-034、D-035、D-042

## 1. Objective

把DEV-008的首个`MigrationIntentContractV1`从“选择内存比较策略”推进到一个真实execution consumer：

```text
admitted collection-output decision
  + finite-normal-nonzero f32 input/threshold case
  → isolated call-adapter input (expected不可见)
  → actual child process writes values ABI output + reported-count ABI output
  → authoritative generic execution receipt
  → trusted materializer reconstructs observed collection
  → contract-selected multiset-and-count comparison evidence
```

可观察结果：一个候选进程把所有strictly-above-threshold值以反向顺序写出并报告正确count时通过；缺值、
重复值、错误元素、错误reported count、越界count、tampered capture/receipt均拒绝或产生明确非等价结果。

## 2. Scope

本slice只覆盖第一个局部claim和一个窄、无数值歧义的case domain：

- 输入、threshold和selected values均为finite normal nonzero binary32；
- element equality为该domain上的exact binary32 identity；signed zero、NaN、Inf、subnormal policy不在本slice
  声称范围内；
- values output按reported count截取有效prefix，capacity tail不成为集合元素；
- output order只来自DEV-008 admitted policy，不从CUDA atomic observation或candidate输出反推。

明确非目标：不形成`AdmittedOraclePortfolio`或candidate verdict，不实现Oracle Planner/hidden corpus/
qualification registry，不运行CUDA/Ascend设备，不声称domain coverage、safety、numerical allowance或完整
operator correctness。

## 3. Authority与类型边界

- `cairn-admission`继续独占proposal + user decision → contract的promotion；本slice不增加promotion入口。
- contract identity、collection policy/expected/observed/comparison是产品语义而非Admission capability，移到
  `cairn-migration`供执行consumer使用；旧定义直接从current V1移除，无alias或dual path。
- candidate call-adapter只读取input、threshold、output allocation和exact decision/invocation identity；expected
  elements由trusted materializer独立派生，不进入candidate input bundle。
- `ValidatedCallAdapterExecution`与exact `ExecutionReceipt`只证明实际process output；它不能决定desired order。
- comparison record绑定exact contract/decision、invocation、receipt、expected/observed identities和mechanism
  identity；它是Oracle evidence，不是admission或candidate verdict。

## 4. Mechanism scope

首次参与真实comparison的collection reference/materializer/comparator按FR-ORACLE-030绑定exact source
identity。资格证据只围绕本实现与风险：

- honest reversed-order implementation必须通过unordered policy；
- stable-order policy对同一reversal必须失败；
- missing/duplicate/wrong/count-over-capacity/tampered inputs必须fail closed；
- expected bytes/IDs不出现在candidate-visible bundle；
- call-adapter request、captured files、result manifest、job contract和receipt binding复用现有真实执行控制。

这不是通用mechanism registry，也不引入第三人评审。

## 5. Acceptance

- generic integration test通过actual child adapter与generic execution receipt完成双output observation；
- exact DEV-008 live proposal/decision replay生成同一contract-bound policy并消费当前V1 materializer；无新model call；
- compile-fail/static boundary证明proposal、raw call-adapter result和candidate observation不能替代expected或
  admitted contract decision；
- normal migration dependency graph不因本slice增加Admission依赖；production code不含exact live case ID、
  hypothesis label或fixture expected values；
- focused tests、mechanism negative controls、full `scripts/ci.sh`通过。

## 6. Mechanism qualification lifecycle

本slice不建立通用qualification registry，也不要求新的第三人评审。当前mechanism qualification是与实现
同仓、可重复执行的窄控制集：

- exact mechanism identity覆盖`collection_oracle.rs`的reference/materializer/comparator与
  `call_adapter.rs`的receipt/capture binding；actual candidate fixture不被伪装成trusted reference；
- honest reversed-order child process必须经generic coordinator receipt后通过unordered policy；
- sequence policy、missing、duplicate、wrong element、wrong count与count-over-capacity分别由局部negative
  controls覆盖；common call-adapter controls继续覆盖request/result/output/receipt tamper；
- 任一被identity覆盖的source发生变化都会产生新mechanism identity，必须重新运行本控制集和full CI；控制失败
  时该mechanism不能作为后续verdict input；
- 当前资格只覆盖host-isolated adapter、finite normal nonzero f32、strict `>`、exact bit element identity、
  reported-prefix与sequence/multiset-and-count比较。它不覆盖设备runner、NaN/Inf/zero/subnormal、numerical
  tolerance、capacity error semantics或完整operator domain。

## 7. Implementation result

- `cairn-migration`现在持有current-V1 contract identity与collection expected/observed/policy/comparison强类型；
  `cairn-admission`只保留proposal + authority decision到contract/policy的promotion，旧定义已直接删除。
- 新collection invocation固定使用input、threshold、values output、reported-count output四个typed ABI位置；
  invocation绑定exact decision/contract/selection claim，且不含expected elements。
- generic integration实际启动独立host child。child从input和threshold计算strictly-above结果，反序写values
  prefix与count；generic executor归档三个declared outputs并生成receipt，trusted materializer从receipt内容重建
  observation，unordered policy判定equivalent。
- comparison evidence绑定mechanism、decision、contract、selection claim、invocation、receipt、expected与observed
  identities及明确failure class；它仍是Oracle evidence，不是candidate verdict。
- exact DEV-008 private CAS在无provider/model调用下重放；真实
  `h-compact-set-order-unspecified` admission decision驱动当前materializer并接受反序actual output。

## 8. Validation evidence

- collection mechanism unit controls：3 passed；覆盖expected leakage、numeric domain、order policy、
  missing/duplicate/wrong/count与capacity bound。
- actual child + authoritative receipt integration：
  `admitted_policy_drives_receipt_bound_collection_materialization` passed。
- exact private artifact replay：
  `exact_live_proposal_crosses_process_and_drives_first_admitted_oracle_policy` passed；沿用DEV-008 exact proposal，
  未发起新模型调用。
- dependency/vocabulary audit：normal `cairn-migration` graph不依赖`cairn-admission`；production source不含exact
  private hypothesis ID或fixture expected values；无compatibility alias、converter、dual reader/writer或version bump。
- full `scripts/ci.sh`：通过（fmt、log isolation、locked check、all-target/all-feature clippy、workspace
  tests、doc/compile-fail tests、link与whitespace checks）。

## 9. Remaining boundary

DEV-009只证明“一个已admit高阶意图确实改变真实process observation的解释方式”。它没有把这个case升级为
全局fixture或以它驱动所有迁移任务，也没有建立Oracle portfolio、Planner、hidden tests、device execution、
Candidate Search或最终Admission verdict。下一slice应继续选择对首条端到端迁移最短的consumer，而不是扩建SIR
或qualification基础设施本身。
