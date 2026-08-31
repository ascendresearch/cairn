# DEV-034 — qualified Oracle control runner and exact observation Admission

- 状态：`Accepted`
- 日期：2026-08-30
- 依赖：D-023、D-043、DEV-030–033
- 外部执行：无；未调用live model、互联网、remote Worker、Docker或NPU

## 1. 目标

关闭DEV-032中“Controller已冻结完整Oracle control matrix、但receipt仍由调用者整体注入”的真实缺口。
每个item × control obligation必须绑定catalog中已资格化的mechanism、runner和qualification receipt；Controller
先持久化exact run/dispatch authority，adapter才可执行，Admission receipt只能从exact Worker observation机械投影。

本slice只闭合当前会话要求的CUDA task → SIR → Intent Admission → Oracle Exploration → Oracle Admission前缀。
Candidate失败/partial后的generic revision policy仍是独立下一slice。

## 2. 可读业务骨架

```text
terminal Oracle portfolio + strict Admission policy
→ qualified mechanism + runner + qualification inventory
→ mechanical item × control attempt
→ choose first missing obligation
→ derive OracleControlRunV1
→ adapter prepare exact JobId / AttemptId / JobContractArtifact
→ derive OracleControlDispatchV1 with exact qualified runner
→ durable oracle-control-authorized event
→ adapter execute exact dispatch
→ validate ExecutionReceipt job / attempt / contract / content identity
→ archive TrustedOracleControlObservationV1
→ mechanically derive OracleControlReceiptV1
→ durable oracle-control-observed event
→ repeat, or independently recompute OracleAdmissionOutcomeV1
```

`drive_controller_workflow_once`仍保持recover → select → execute；新增动作只是
`RunOracleAdmissionControls`与`ExecuteOracleAdmissionControl`，没有第二个workflow或隐藏side channel。

## 3. 强类型与authority边界

- `OracleControlRunnerArtifact`、`OracleMechanismQualificationReceiptArtifact`、
  `OracleControlRunArtifact`、`OracleControlDispatchArtifact`和已有
  `TrustedOracleControlReceiptArtifact`保持distinct content domains；没有generic ID bag。
- qualification catalog registration同时冻结control family、mechanism、runner和qualification identity；Manager在
  Admission authorization前canonical decode `OracleMechanismQualificationReceiptV1`，核对三条typed edge及其identity，
  并验证receipt引用的`ExecutionReceiptArtifact` evidence存在；任意opaque bytes不能冒充qualification。
- `OracleControlRunV1`从exact attempt、catalog与其中一个obligation派生，不能自由指定runner或qualification。
- `OracleControlDispatchV1`同时绑定run、qualified runner和Worker job/attempt/contract；compile-fail boundary证明
  runner identity不能替代dispatch identity。
- `TrustedOracleControlObservationV1`保留完整`ExecutionReceipt`，并重新验证dispatch、run、job、attempt、contract和
  receipt content identity。公开Admission receipt只能由该observation投影。
- aggregate reducer保存exact catalog/attempt body来重放验证后续run，不依赖日志或CAS读取；per-control start event不再
  重复整个attempt/receipt history。
- 一个adapter可以按dispatch中的strong runner identity路由多个qualified runner；没有single-runner、fixture-name或
  boolean `supports`捷径。

## 4. 替代或删除的路径

- 删除公开的`record_controller_oracle_admission_evidence`整体receipt入口；
- 删除`AwaitOracleControlReceipts` next action；
- `record_oracle_admission_outcome`只允许从aggregate已经观察到的exact receipts完成，且不再从server crate公开；
- 删除旧`collection_oracle_admission.rs`、`cairn-admission`中的local claim prepare/commit/publication旁路及其
  专属测试；保留的collection comparator只是Intent contract的普通typed consumer，不再发布Oracle authority；
- 不保留legacy alias、dual reader/writer、converter、fallback或V2格式。

新aggregate、prompt、catalog和adapter没有复制旧路径的threshold、dtype、fixture identity或expected answer。

## 5. 日志

遵循D-023和`docs/OBSERVABILITY.md`：

- 所有Controller durable transition提交后记录统一INFO里程碑，字段仅含already-computed task/command/event identity、
  sequence、schema和replay classification；
- Oracle control记录prepare blocked、execution start、blocked和completed，关联run/runner/job/attempt/contract/receipt；
- terminal日志只增加typed result、exit code、elapsed millis和bounded output/receipt counts；
- 不记录CUDA/source、prompt、model request/response、tool body、stdout/stderr、opaque diagnostic、secret或restricted
  Admission material；日志表达式不生成identity/time、不执行effect、不传播`?`。

日志仍不是authority或replay input；关闭subscriber不改变event、CAS identity或Admission outcome。

## 6. 测试与验证

- current-V1 constructor/deserialize及catalog registration更新；
- Controller aggregate recorded control逐项证明start event先于observation event，并从exact ExecutionReceipt lineage
  生成mechanical receipts后完成Oracle Admission；
- restart/replay重新派生run/dispatch/observation/evidence/outcome，changed authority fail closed；
- strong content domains与compile-fail runner/dispatch substitution control；
- `scripts/check-log-isolation.sh`验证日志无span、effect、await或fallible work；
- migration/server all-feature check、focused tests、Clippy和CI作为退出门禁。

## 7. 明确非目标

- 不运行或伪造live model、network、remote Worker、Docker、CUDA/NPU effect；recorded observation不构成设备证据；
- 不定义任何fixture expected answer、固定control recipe或production special case；
- 不让runtime Agent生成qualification receipt、ExecutionReceipt或Admission verdict；
- 不实现Candidate mechanism runner或Candidate revision policy；
- 不把Oracle control结果反馈给SIR/Intent以移动已冻结authority；
- 不增加compatibility、format version、generic-ID abstraction、第二aggregate或日志authority。
