# DEV-006 implementation — typed Intent Recovery contract

- 状态：`Accepted`
- 日期：2026-08-28
- Slice：[`DEV-006`](../SLICE_CATALOG.md#3-当前critical-slices)
- 设计：[`Semantic Intent Recovery`](../../oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md)
- 决策：D-003、D-025、D-034、D-042

## 1. Objective

- 当前consumer：现有`run_sir_episode`与configured DeepSeek strategy；它必须消费冻结
  `IntentRecoveryInputV1`，不能再从裸task manifest猜测caller declaration。
- 可观察结果：同一recorded/live tool loop提交来源分离的observed facts、竞争hypotheses、conflicts、
  unknowns、invariants、optimization freedoms、source dispositions和disambiguation proposals；trusted
  gateway绑定exact recovery input、episode与model provenance。
- 非目标：不生成`MigrationIntentContract`，不实现Intent Admission/Gate、hidden controls、Oracle或
  Candidate，不把当前in-process harness升级为authority，也不创建空`cairn-sir` crate。
- 停止条件：新schema需要fixture答案、operator branch、模型自填trusted provenance，或不能由recorded与
  live runtime提交strict proposal。
- Superseded V1：直接删除manifest-only prompt contract、内嵌fact的minimal proposal shape及其测试；不保留
  alias、dual reader、converter或migration。

## 2. Role与authority

| Role | Input/output | Capability | 明确没有 |
| --- | --- | --- | --- |
| Coding agent/builder | generic types、prompt/tool gateway、recorded/live test | repository write与测试执行 | runtime conclusion authority、fixture answer projection |
| Runtime model/proposal | frozen caller declaration、target、task bundle、allowed evidence/capability → proposal body | bounded task reads、pure submit | admitted/hidden/execution/verdict authority |
| Evaluator | episode完成后检查strict shape、absence与utility | public test assertions、opt-in live classification | product prompt/type mutation authority |
| Admission/execution | 本slice不存在 | 无 | proposal self-certification |

## 3. Data与effects

- Model-visible projection：caller ABI roles/dtypes/shapes、declared semantic claims/exclusions/unknowns、target
  context、task manifest、`NoPriorFeedback`或允许feedback refs、exact capability manifest。
- 来源隔离：caller claim、observed source fact、SIR hypothesis、conflict/unknown和trusted provenance使用不同
  types/edges，不合并成无出处字符串。
- Restricted/secret exclusions：fixture expected/restricted corpus、review identity、Admission policy与candidate
  material不进入input、prompt或tool result。
- External effects：proposal工具仍为Pure，source工具仍为ReadOnly；live provider调用沿现有transport预算与
  ambiguous-effect规则。
- Required lanes：strict/unit + RecordedWorkflow；因修改model-authored output contract，接受前需要一次明确
  授权的LiveModel提交。CUDA/Ascend hardware均`NotExecuted`且与本objective无关。

## 4. Types与current V1

- 新增/替换：`IntentRecoveryRequestV1`、`IntentRecoveryInputV1`、caller argument/claim/exclusion/unknown、
  target context、prior feedback、capability manifest；distinct local observation/hypothesis/conflict/unknown/
  invariant/freedom/disposition/experiment identities和references。
- `IntentHypothesisSetProposalV1`继续明确是proposal envelope，但其current-V1 body直接替换为完整shape并绑定
  `IntentRecoveryInputArtifact`，不只绑定task bundle。
- Constructor/strict decode重跑集合有界、canonical order/uniqueness、引用闭包、citation范围、caller/source
  provenance与protected-invariant约束。
- Static boundary：caller claim ID、observed fact ID、hypothesis ID和invariant ID不可互换；proposal仍不能
  作为未来admitted contract。
- 不增加version number；所有schema仍为1，无alias、converter或dual path。

## 5. Controls与acceptance

- Positive：recorded runtime读取caller declaration与源码，提交完整strict proposal，CAS/restart/replay保持
  exact identity。
- Primary negative：dangling/cross-kind reference、非法citation、non-V1、重复identity和trusted provenance
  spoofing均fail closed。
- Absence：初始请求包含caller declaration但不包含source bytes、fixture answer/restricted/reviewer材料；source
  只能经bounded read进入后续turn。
- Generalization：production schema、prompt和control flow不含reduction/compaction答案或operator分支。
- Remaining：Intent Admission、`NeedsUserDecision` artifact、独立SIR process和Oracle consumer仍未实现；它们
  只有在DEV-006 runtime contract通过后进入下一authority slice。
- Local evidence（2026-08-28）：
  - `cargo test -p cairn-migration --test sir_episode`：4 passed；覆盖完整contract、strict boundary、
    model-visible absence、CAS/restart/recorded replay与dangling/citation rejection；
  - `cargo clippy -p cairn-migration --all-targets --all-features -- -D warnings`：通过；
  - `scripts/ci.sh`：通过，包括workspace tests与`SirObservationId`/`SirHypothesisId` compile-fail；
  - 用户明确授权后执行
    `cargo run -p cairn-migration --example sir_deepseek -- fixtures/cuda-ascend/sir/compact-above-f32/v1/source fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json config/sir-deepseek.example.json`；
  - live episode `episode:01a048a1-7279-7b22-807b-8756963ace78`先做3次bounded read；第一版submission被
    strict gateway拒绝，模型在同一continuation修复后提交有效proposal，4 steps后`Yielded`并通过terminal
    restart；这证明repair没有绕过schema或gateway；
  - recovery input `cairn:v1:sha256:migration.intent-recovery-input.v1:a102178b4362fec2261cfb2a2b4a86105f66aea2b63c22820ef53c7d375497d0`；
    proposal `cairn:v1:sha256:migration.sir-intent-hypothesis-set-proposal.v1:dcedfef6ab58e3dfc7606ed2eab8f21feec81ed6167bb52d99f6fadeb0ed0e35`；
  - accepted body含5个cited observed facts、3个competing hypotheses、1个conflict、2个unknown、4个
    invariants、2个optimization freedoms、3个source dispositions和1个disambiguation experiment；
  - 4次provider dispatch累计input/output/cache-read tokens为52,677 / 14,277 / 35,072；raw provider
    response与reasoning未打印。
