# DEV-007 implementation — first typed user-intent decision request

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-007`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Semantic Intent Recovery`](../../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md)、
  [`Admission Architecture`](../../design/ADMISSION_ARCHITECTURE.md)
- 决策：D-025、D-030、D-034、D-042

## 1. Objective

- 当前consumer：一个无model/provider依赖的`cairn-admission` one-shot process从public CAS读取exact
  `IntentHypothesisSetProposalV1`及其绑定的`IntentRecoveryInputV1`。
- 可观察结果：机械遍历proposal graph，把`DesiredSemantics` unknown、同一disambiguation experiment绑定的
  conflicts和至少两个competing hypotheses冻结为`UserIntentDecisionRequestV1`；实际任务authority可以看到
  exact option/provenance，而不是审阅整份SIR报告。
- 非目标：本slice不接受用户回答、不生成`MigrationIntentContract`、不admit任一claim、不读取hidden
  control、不执行实验，也不实现Oracle/Candidate或通用Admission planner/qualification registry。
- 停止条件：必须按operator名、fixture答案或自然语言关键词匹配unknown/conflict；需要让model决定是否
  `NeedsUserDecision`；或无法从DEV-006 exact live proposal生成request。

## 2. Authority与process边界

| Role/process | 输入/输出 | 能力 | 明确没有 |
| --- | --- | --- | --- |
| Existing SIR harness | frozen recovery input → proposal | bounded public reads、pure submit | decision/admitted constructor |
| `cairn-admission` DEV-007 command | exact public CAS proposal/input → typed decision request | public read、canonical stdout | model/network、restricted store、execution、promotion |
| Actual task authority | 后续读取一个scoped request并选择/补充/保持unknown | desired-semantics decision | execution fact或未声明scope authority |
| Repository coding agent | generic graph derivation、process/test wiring | repository development | 替用户选择option |

`UserIntentDecisionRequestV1`是请求，不是用户决定或admitted contract。因为本slice没有promotion edge，
DEV-006的in-process SIR harness仍可保留。第一次真正生成`MigrationIntentContract`的slice必须同时落实独立
SIR process、Admission service principal、typed process protocol和capability reachability；不得把本次
public triage command冒充该authority integration。

## 3. Current V1 contract

- Input identity：typed proposal ID；process从其envelope取得recovery-input ID，再从同一public CAS加载并
  重算两份canonical identity。
- Mechanical closure：只处理`SirUnknownKind::DesiredSemantics`；unknown与conflict必须被同一个
  `SirDisambiguationExperimentV1`引用；conflict必须提供至少两个exact hypothesis options。
- Output：`IntentDecisionRequestBatchV1`及1..N个`UserIntentDecisionRequestV1`；每项绑定proposal、recovery
  input、unknown、caller-declared unknown context、conflict和typed hypothesis IDs/layers/claim/domain。
- 不把hypothesis text升级为truth；option只表示用户可选择的已归档proposal。`keep-unknown`与
  `provide-authoritative-claim`作为不同allowed response，不退化成free-form pass boolean。
- 直接修改current V1；不增加version、不保留旧reader/alias/converter。

## 4. Controls与acceptance

- Positive：从DEV-006 live CAS的proposal
  `cairn:v1:sha256:migration.sir-intent-hypothesis-set-proposal.v1:dcedfef6ab58e3dfc7606ed2eab8f21feec81ed6167bb52d99f6fadeb0ed0e35`
  生成一个output-order desired-semantics request，并由用户确认它是否准确、足够scoped。
- Negative：wrong typed ID、tampered/noncanonical bytes、proposal/input binding mismatch、dangling caller ref、
  无共同experiment closure、少于两个options、source-behavior unknown均不能产生`NeedsUserDecision`。
- Static：unknown、conflict、hypothesis和decision-request identity不可互换。
- Process：normal dependency graph不含`cairn-agent`、`reqwest`或model template；stdout只有canonical V1
  result，diagnostic走stderr且不含proposal正文。
- Required lanes：strict/unit、actual child-process integration、full CI；无新模型调用、CUDA或Ascend设备
  claim。

## 5. Current evidence

- `cargo test -p cairn-admission`：actual child process从read-only public CAS读取typed artifacts，stdout仅有
  canonical V1；wrong content domain失败且stdout为空。
- `cargo test -p cairn-migration intent_admission --all-features`：desired-semantics closure成功；缺共同
  experiment、source-behavior unknown和dangling caller claim均fail closed。
- `cargo check -p cairn-migration --no-default-features --lib`与Admission Clippy通过；
  `cargo tree -p cairn-admission --edges normal`不含`cairn-agent`、`reqwest`、`hyper`、`rustls`或`tokio`。
- exact DEV-006 live proposal在无新模型调用的情况下生成1个request：caller context `output-order`、unknown
  `u-output-order-contract`、conflict `c-output-order-contract`和3个exact hypothesis options。
- 实际任务authority确认该request准确且足够scoped，并明确选择
  `h-compact-set-order-unspecified`：qualifying values与reported count必须正确，输出顺序允许任意排列。
- 该聊天回答只作为下一slice的operator input，不在本slice伪造成typed `UserIntentDecisionV1`或
  `MigrationIntentContract`；DEV-007只证明proposal可以被model-free process收敛为最小人工问题。
- `scripts/ci.sh`通过：workspace locked check、Clippy `-D warnings`、全部unit/integration tests、
  compile-fail doc tests与document/link/whitespace checks无回归；final commit在本记录之后闭合。
