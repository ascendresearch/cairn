# DEV-008 implementation — first admitted intent and Oracle policy

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-008`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Semantic Intent Recovery`](../../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md)、
  [`Admission Architecture`](../../design/ADMISSION_ARCHITECTURE.md)、
  [`Runtime Architecture`](../../design/RUNTIME_ARCHITECTURE.md)
- 决策：D-003、D-025、D-030、D-032、D-034、D-035、D-042

## 1. Objective

- 当前consumer：一个真实collection-output Oracle comparator policy只能从首个
  `MigrationIntentContractV1`选择sequence-sensitive或unordered-multiset-and-count语义。
- 当前authority input：DEV-007 exact request与实际任务authority的
  `h-compact-set-order-unspecified`选择；typed decision将明确声明exact selected occurrences、
  exact reported count与unspecified permutation，不从hypothesis prose猜测机器语义。
- 可观察结果：交换两个正确输出元素仍被新Oracle policy视为等价；丢值、重复值或
  reported-count错误仍拒绝。
- 非目标：不实现七类Admission、hidden corpus、planner/qualification registry、Candidate、
  device execution或通用Oracle portfolio。

## 2. Authority与process边界

```text
separate cairn-sir recorded ingress (proposal-only)
  → Controller-owned public CAS: proposal/request/authority grant/user decision
  → separate cairn-admission process: deterministic exact binding + promotion
  → Admission-owned restricted CAS commit
  → public admitted outcome
  → collection-output Oracle policy
```

- `cairn-sir`只接受canonical typed stdin bundle并输出proposal terminal outcome，不链接SQLite、
  Admission或restricted store；它不能构造admitted type。
- `cairn-admission`只读Controller public store，独占restricted store写权，不链接agent/model/
  provider/network runtime；restricted decision commit后才向stdout发布public outcome。
- local CI证明独立子进程、依赖和capability形状；本机另用不同实际UID的opt-in smoke
  证明SIR无restricted path可达性、Admission可写restricted store。

## 3. Current-V1 contract

- `UserIntentAuthorityGrantV1`由Controller public authority port归档，绑定exact task、authority
  subject和collection-output claim scope；普通自报subject string不构成grant。
- `UserIntentDecisionV1`绑定exact request、grant、selected hypothesis与强类型authoritative
  claim。hypothesis是proposal provenance，authoritative claim才是promotion的机器语义。
- `MigrationIntentContractV1`绑定task/recovery input/proposal/request/grant/decision和exact
  admitted claim；不修改原proposal。
- `RestrictedIntentAdmissionDecisionV1`绑定exact gate mechanism与contract identity；public outcome只暴露
  opaque restricted-decision ID与contract。
- 所有结构直接修改current V1；无version bump、alias、converter或dual reader。

## 4. Acceptance

- exact DEV-006 live proposal经独立SIR process ingress后，DEV-007 request与本次user decision可生成
  一个contract与unordered collection Oracle policy。
- wrong task/request/grant/proposal/recovery input、unoffered hypothesis、scope mismatch、tampered/non-V1
  bytes、proposal直接传入Oracle、restricted commit失败均fail closed。
- normal Admission dependency graph不含`cairn-agent`、`reqwest`、`hyper`、`rustls`或`tokio`；
  SIR ingress dependency graph不含Admission/SQLite/restricted adapter。
- Required lanes：strict/unit、actual child processes、different-UID capability smoke、exact live artifact replay、
  full CI；无新模型调用、CUDA或Ascend device claim。

## 5. 实施结果

- 新增独立 one-shot `cairn-sir` process。它接受有 `SirRunId`、`OperationId`、exact
  implementation identity、完整 task bundle/input/proposal 的 canonical request，只返回仍为 proposal 的
  terminal artifact。该进程没有 store handle，也没有 Admission、agent provider 或 network dependency。
- `cairn-admission` 增加独立 promotion command。它用只读 public store重新装载并机械重算 exact
  proposal/input/request/grant/decision binding；只有成功后才把完整 contract 与 restricted decision归档到
  Admission-owned store，并发布最小 public outcome。
- 用户选择不直接把 hypothesis prose当机器规则。实际 authority decision同时携带强类型
  `CollectionOutputIntentV1`：exact selected occurrences、exact reported count、unspecified permutation。
- Oracle comparator只接受由 admitted public outcome派生的 policy；类型测试禁止 proposal代替contract，也
  区分 trusted expected collection与candidate observation。unordered policy接受正确重排，拒绝缺值、重复值、
  元素错误和reported-count错误。
- public frozen-snapshot SQLite reader使用显式`open_immutable_read_only` API和canonical `file:` URI的
  `mode=ro&immutable=1`。这是different-UID smoke发现的
  必要修复：普通read-only flags在WAL数据库上仍可能创建`-shm`，不满足真实public-store只读权限。
  API名称明确要求Controller先停止写入并冻结snapshot，不把immutable handle误用于并发WAL读取。

## 6. Exact evidence

- DEV-006 live episode：`episode:01a048a1-7279-7b22-807b-8756963ace78`。
- exact proposal：
  `cairn:v1:sha256:migration.sir-intent-hypothesis-set-proposal.v1:dcedfef6ab58e3dfc7606ed2eab8f21feec81ed6167bb52d99f6fadeb0ed0e35`。
- exact request：
  `cairn:v1:sha256:migration.user-intent-decision-request.v1:5cfb90399a26cdcd79131916ebf03fe5f555ebdd86e6d5f97e2275f44de1e72a`。
- exact authority decision：
  `cairn:v1:sha256:migration.user-intent-decision.v1:0a4d26a01a9a7ecf999ab5f1923140e74202267b179bb7276a6c54d474f831c7`；
  selected hypothesis为`h-compact-set-order-unspecified`。
- exact restricted decision：
  `cairn:v1:sha256:admission.intent-decision-restricted.v1:8d5da4a4930b54192abb1b99690c5fedee0ef7a262fcf02479527bea4132edba`。
- live replay使用上述已有private CAS且没有新provider/model调用；typed SIR process、request derivation、
  promotion与Oracle policy全部通过。
- OS-principal smoke使用UID 65534运行SIR、UID 1运行Admission。SIR不能读取restricted目录；Admission只读
  public store并独占写restricted store；普通workspace用户与SIR principal均不能读取restricted结果。
- normal dependency audit通过：Admission无agent/model/network runtime；SIR无Admission/SQLite/store/agent/
  network runtime。
- `scripts/ci.sh`全workspace通过；新authority IDs、schema version、reported count、expected/observed output
  均使用强类型边界，适用的compile-fail tests通过。

## 7. 当前边界

这是第一条窄的authority consumer，不是完整Intent/Oracle平台：

- authority grant当前由Controller归档路径提供；外部身份认证/UI尚未实现；
- public outcome当前由受信父进程捕获exact Admission child stdout；公共workflow event/outbox与重放恢复尚未接入；
- comparator消费已物化的语义元素identity，不等于CUDA reference/Ascend call adapter或device execution；
- 未实现通用required-evidence family、planner、hidden corpus、mechanism registry、Candidate或performance链。

因此DEV-008证明的是首个typed promotion和下游policy消费已经成立，而不是第一条完整迁移已经完成。
