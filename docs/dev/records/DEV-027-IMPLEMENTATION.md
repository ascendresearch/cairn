# DEV-027 implementation — user decision and independent Intent Admission

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-027`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)
- 外部执行：仅运行本地model-free Admission process control；未调用模型、远端 Worker、Docker、NPU 或互联网

> Current correction：DEV-028已supersede本record中的固定Blue/Red composition占位；durable aggregate仍停在
> 同一个`AwaitOracleWorkflow`边界，当前上层骨架是`Oracle Exploration → Oracle Admission`。

## 1. Objective

把DEV-026停住的真实用户authority输入和已有独立Intent Admission接入同一个task-owned durable Controller
aggregate：Controller只记录actual authority grant/decision，不替用户选择；Admission executable和restricted-store
target必须先获得durable start authority；独立process先提交restricted artifacts，Controller只接收canonical public
outcome，归档带provenance的observation与exact intent contract，然后明确停在Oracle workflow边界。

## 2. 可读业务骨架

顶层driver继续只表达：

```text
recover_controller_turn
→ select_controller_action
→ execute_controller_action
```

durable intent段落一眼可见：

```text
AwaitingUserIntentDecision
→ UserIntentDecisionRecorded
→ AuthorizeIntentAdmission
→ RunIntentAdmission
→ AdmittedIntent
→ AwaitOracleWorkflow
```

用户输入记录、start authority、process supervision、observation archival与aggregate transition各自位于小函数中；
尚未实现的Oracle Blue/Red仍由完整composition skeleton占位，不在本片中伪造成功。

## 3. Authority、process与持久化

- task-owned aggregate从`cairn-migration`移动到拥有Controller composition的`cairn-server`，因此它能直接消费
  migration与Admission的distinct domain types，不制造dependency cycle、generic wrapper或重复domain；
- `record_controller_user_intent_decision`只接受typed grant/decision，重新装载exact batch/request并由aggregate验证
  task、proposal、recovery input、request、grant和decision binding；
- 每个individual decision request与batch一起进入public CAS，独立Admission不会依赖Controller内存或fixture；
- `IntentAdmissionExecutableArtifact`与`IntentAdmissionRestrictedStoreArtifact`是不同typed authority；binary bytes与
  exact restricted database/CAS target在process启动前提交到event stream；
- supervisor在effect前重算两项identity，使用immutable public SQLite/CAS参数和restricted write target启动
  `cairn-admission promote-user-intent`，并限制timeout/stdout/stderr；所有effect paths必须先解析为绝对路径，避免
  相同配置字符串在重启后的working directory中指向另一目标；
- child exit code成功并不等于准入成功：stdout必须是strict、canonical `IntentAdmissionPublicOutcomeV1`；aggregate
  还会验证其中contract对当前task/input/proposal/request/grant/decision的完整绑定；
- Controller不打开restricted store，也不读取restricted decision；它只归档public outcome及其中公开contract。

## 4. Tests与controls

- durable state顺序、SQLite restart recovery与start-authority exact replay；
- cross-task authority grant拒绝、public outcome typed identity drift拒绝；
- Admission executable和restricted-store identities不能互换的compile-fail control；
- restricted-store配置在durable authority后漂移时，子进程启动前返回`InvocationDrift`；
- 成功exit但untyped/noncanonical stdout返回`InvalidOutcome`；
- real `cairn-admission` child process验证public read、restricted contract/decision先提交、canonical public outcome后发布；
- terminal next action严格为`AwaitOracleWorkflow`，本片不会自动运行Oracle；
- feature-off `cairn-migration`重新通过独立编译：`SirTaskWorkspace`只在`agent-runtime`下导出。

测试fixture只用于构造测试输入。production event、API、process args、identity domain和policy没有fixture name、
expected answer、fixture-specific branch或untyped/generic content identity。本片只连接已有collection-output Intent
Admission consumer，不声称已经泛化为完整Intent/Oracle portfolio。

## 5. 替代/删除的旧路径

- 删除`cairn-migration::controller_workflow`及其re-export；唯一current-V1 task aggregate现位于
  `cairn-server::controller_state`，没有legacy alias或dual implementation；
- DEV-026的terminal `AwaitingUserIntentDecision`被真实user-input/Admission transition继续推进，但等待动作本身仍是
  唯一合法authority边界；
- Admission不被内联进Controller，也没有恢复SIR专用process、fallback reader、converter或版本迁移。

## 6. 明确非目标

- 不认证真实UI/HTTP subject；调用方仍须在进入typed grant API前完成用户身份认证；
- 不让Controller、runtime model或proposal step读取restricted artifacts；
- 不接Oracle Blue/Red、Oracle Admission、Candidate suffix或global terminal；
- 不实现Controller↔Host external experiment yield/resume；
- 不扩展现有窄collection-output Intent schema为通用migration intent portfolio；
- 不调用live model、remote Worker、Docker或NPU，不产生新的live receipt、semantic claim或verdict。
