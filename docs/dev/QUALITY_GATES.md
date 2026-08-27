# Cairn 开发质量与晋级 Gate

- 状态：规范性开发计划设计，尚未授权代码实施
- 日期：2026-08-27
- 产品范围：仅限 CUDA → Ascend C 算子移植
- Slice catalog：[`SLICE_CATALOG.md`](SLICE_CATALOG.md)

## 1. Gate 模型

每个 development slice 依次经过：

```text
G0 Design Entry
→ G1 Static Boundary
→ G2 Deterministic Mechanism
→ G3 Workflow/Recovery
→ G4 Applicable External Evidence
→ G5 Backwards Audit
→ G6 Acceptance and Ledger Update
```

某个 Gate 不适用时必须记录 typed reason/scope；不能把“没运行”当通过。真实模型、CUDA、Ascend build、
Ascend NPU、profiling 和 model integration 分别是不同 G4 lane。

## 2. G0 — Design Entry

代码开始前必须具备：

- slice 在 catalog 中存在且状态为 `Ready`；
- 所有 `DecisionBeforeV1Freeze` blocker 已关闭；
- `DesignConformanceRecord` 已审查；
- objective、non-goals、inputs、outputs、authority 和 effect 边界明确；
- exact requirements/decisions/design sections 可追踪；
- 受影响 V1 types、events、CAS domains、process protocols、tests、fixtures、docs 已列出；
- superseded code/fixture 删除列表已列出；
- hardware/model/external-service lanes 与预算已声明；
- dirty worktree 中用户已有变更已识别，不会被覆盖。

以下任一情况使 G0 失败：

- 使用 open question 的某个答案作为“暂定默认”；
- 计划保留 old/new 双读写；
- 用 string role/generic ID/bool 绕过尚未设计的类型；
- applicant 和 gate 在同一权限域；
- 只有 happy-path demo，没有 negative/unknown/tamper control；
- slice objective 是“搭框架供未来使用”而没有当前纵向消费者。

## 3. G1 — Static Boundary

在运行行为前先证明结构不能轻易越界：

- crate dependency allow/deny graph；
- proposal/admitted、public/restricted、plan/decision、observation/receipt 等类型不可互换；
- role/profile/episode/Host/process/model/tool identities 使用强类型；
- 单位、scope、lifecycle、evidence strength 和 outcome 不退化；
- deserialization 重跑 constructor invariants；
- wrong-kind/wrong-role/wrong-store compile-fail 或等价 static test；
- non-V1 input strict rejection；
- 无 compatibility reader、alias、converter、dual event writer；
- generic runtime/Worker 中无 CUDA/Ascend/Oracle product vocabulary；
- Admission binary 无 model transport，Proposal binary 无 restricted store adapter。

仅有运行时 `if role == "..."` 拒绝不满足本 Gate。

## 4. G2 — Deterministic Mechanism

每个新增机制至少验证：

- canonical identity/encoding 和 cross-field invariants；
- honest positive control；
- targeted negative control；
- malformed/tampered/wrong-binding input；
- applicable conflict、unknown、not-applicable、not-executed；
- boundary/property/mutation/fault control；
- mechanism qualification identity 和适用 scope；
- stored `passed`、模型文字或 candidate workspace 不能改变重算结果；
- logger/metrics 开关不改变 durable facts；
- deterministic/recorded path 不访问网络或稀缺设备。

Comparator、adapter、runner、parser、redactor、profiler adapter、gate 和 policy evaluator 都是 mechanism，
不能因代码在 trusted repository 内就跳过 qualification。

## 5. G3 — Workflow、Effect 与 Recovery

涉及 event、process、provider、tool、Worker 或外部 effect 的 slice 必须测试：

- effect authorization 前 crash；
- effect 可能发生后、receipt commit 前 crash；
- receipt commit 后、public projection/publish 前 crash；
- duplicate command/retry/idempotency；
- ambiguous effect 保留且不盲重试；
- cancellation/suspension safe point；
- stale revision/lease/capability；
- process restart 从 durable facts 重建，不读日志补状态；
- event/CAS identity 与 backwards edges 完整；
- capability revoked/expired 后 fail closed；
- 同 Host 多 episode continuation/context/tool result 不串流；
- restricted material 不落 public CAS/log/diagnostic。

同步纯函数 slice 可将 G3 记为不适用，但必须说明没有 durable lifecycle 或 external effect。

## 6. G4 — External Evidence Lanes

### 6.1 Lane 定义

| Lane | 能证明 | 不能证明 |
| --- | --- | --- |
| `OfflineUnit` | pure invariant/mechanism | process、provider、真实编译/设备 |
| `RecordedWorkflow` | durable workflow、projection、replay | live provider/model quality、真实设备 |
| `LiveModel` | exact provider/model/profile 行为与成本 | correctness/admission authority |
| `HostExecution` | host adapter/ABI/capture | CUDA 或 Ascend device execution |
| `CudaBuildRun` | exact CUDA binary/device/launch observation | Ascend candidate correctness |
| `AscendBuild` | exact CANN/compiler/link output | NPU execution |
| `AscendNpuRun` | exact NPU binary/device/launch observation | 未测 workload 的性能/模型效果 |
| `ProfilerMicrobench` | exact environment 的测量与字段 | 其他 SoC/toolchain/workload |
| `ModelIntegration` | exact model/deployment/workload observation | 局部 kernel correctness 归因 |

### 6.2 Lane 规则

- required lane 未运行时 slice 为 `EvidencePending` 或 `Blocked`，不是 `Accepted`；
- optional lane 未运行必须出现在 not-executed scope；
- hardware unavailable 与 test failure 分离；
- 每次 live run 记录 exact environment/model/device/toolchain identity；
- 同一真实 lane 至少按 slice policy重复，且 retry 形成新 attempt identity；
- 外部成功不提升超出其 exact scope 的 evidence strength；
- live result 反驳 recorded fixture 时保留冲突并调查 first divergence，不覆盖 fixture。

## 7. G5 — Backwards Audit

从每个 slice 的正式输出反向遍历，必须能回答：

- 谁/什么 authority 创建它；
- 输入 artifact、policy、profile、mechanism 和 environment 是什么；
- 哪些 evidence 是 public、restricted 或 external reference；
- 哪个 receipt 证明 effect/observation；
- required obligations 如何派生并闭合；
- 哪些 feedback/knowledge/skill 可见，污染关系是什么；
- 哪些 hidden case 暴露或 burned；
- 哪些 blind spots、unknown、conflict、not-executed 保留；
- live/recorded replay 的差异在哪里；
- retraction 或 mechanism refutation 会影响哪些输出。

只保存 summary、stdout、模型最终回答或一个大 JSON bundle 不满足 backwards audit。

## 8. G6 — Acceptance 与事实账本

`Accepted` 前必须：

- G0–G5 的适用部分全部通过；
- repository standard format/lint/unit/integration/doc/link/architecture checks green；
- 新增/修改生产 API 有 negative 和 strict decode tests；
- fixtures 有 provenance、license/data classification；
- requirements、decisions、design、operator docs 和 examples 同步；
- superseded V1 code/tests/fixtures 已删除；
- `git diff --check` 与 secret/body scans 通过；
- `CURRENT_BASELINE.md` 或其引用的状态记录写入 commit、evidence、实际 scope 和 remaining gaps；
- slice conformance record 记录最终偏差；
- 没有把下一 slice 的工作写成当前 slice 已完成。

## 9. Stage 晋级 Gate

Stage 退出额外检查：

- critical slices 全部 `Accepted`；
- stage integration test 从 public entry 到目标 output；
- process/capability matrix 与部署事实一致；
- stage-level first-divergence/impact graph 可查询；
- 一个 negative control 能在正确 authority 边界变红；
- stage 目标在新进程启动、Controller restart 和 recorded replay 后仍成立；
- 下一 stage 的输入是 frozen typed artifact，而不是内部 struct、文件路径或 session memory；
- milestone 文本中所有未执行 plane 显式保留。

## 10. Definition of Done 与常见伪完成

| 看似完成 | 为什么不够 |
| --- | --- |
| crate/build 通过 | 没有行为、authority 或 evidence closure |
| model 生成合理文本 | 没有 typed submission 和 admission |
| Blue/Red 都同意 | proposal agreement 不是 receipt/gate |
| mock worker 返回 pass | 不能证明真实 build/device，且 stored pass 无 authority |
| 真实 NPU 跑过一次 | 未必绑定 exact candidate、launch、output 或 required claims |
| profiler 有数据 | adapter、单位、字段和 device state 可能未 qualification |
| tests skipped because no hardware | 只能证明 lane unavailable |
| 加了 V2 converter | 违反 pre-release V1 直接替换规则 |
| 同时支持 old/new 类型 | 隐藏设计冲突，形成双 authority |
| 日志能恢复发生过什么 | 日志不是 durable truth |
| 总分超过阈值 | 多平面 required failure 不能被平均或性能补偿 |

## 11. Review 强度

按风险决定额外审查，不按代码行数：

| 变化 | 最低额外审查 |
| --- | --- |
| Strong type/schema/identity domain | codec、constructor、mutation、compile-fail、所有消费者审计 |
| Authority/capability/process boundary | threat model、negative permission test、crash/restart |
| Comparator/gate/policy | independent mechanism qualification、mutation/fault control |
| Hidden/diagnostic | exposure/redaction/secret scan、impact audit |
| Live provider/tool | recorded counterpart、budget/effect/replay、data-boundary review |
| CUDA/Ascend adapter | binary/device/ABI/launch/capture、wrong-binding controls |
| Performance | unit/statistics/device state/baseline/roof applicability review |
| Knowledge/skill/feedback | provenance/lifecycle/allowed-use/contamination/retraction review |

## 12. Gate 变更

不能为了让当前 slice green 而降低 gate。若 gate 不合理：

1. 记录反例或成本证据；
2. 更新 requirement/decision/design；
3. 说明历史 evidence 是否失效；
4. 直接修改当前 V1 gate 和 tests；
5. 不保留旧 gate compatibility path。
