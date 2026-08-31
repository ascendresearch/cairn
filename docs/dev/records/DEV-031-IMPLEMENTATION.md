# DEV-031 — 逐 cell Oracle strategy consumer 与 typed effect observation

> Amended by D-044/DEV-036: the per-cell authority remains; model execution now occurs in a
> Controller workflow step and external capability requests use `WorkflowTool*`→Worker.

- 状态：`Accepted`
- 日期：2026-08-30
- 依赖：D-043、DEV-029、DEV-030
- 外部执行：无；未调用live model、互联网、remote Worker、Docker或NPU

## 1. 目标与最小DCR

把DEV-030停留在`RunOracleExploration`的framework接到第一个task-generic consumer，同时保持“业务流程本身
就是可读的架构骨架”：Controller只选择一个exact claim × concern × role cell，冻结executor与材料，运行一个
strategy，先保存provenance observation，再原子接受strict typed submission。任何一个Agent都不能一次性声称
覆盖全部平面，也不能生成Admission authority或Worker receipt。

本片直接修改current V1，不增加alias、dual reader、converter或format version。

## 2. 实现

### 2.1 无损 admitted claim 与机械obligation

- collection intent的membership、reported-count、output-order contract移入`cairn-migration`共享领域类型；
- `MigrationIntentContractV1`完整body进入Controller event，restart时不依赖临时CAS解释；
- `OracleClaimV1`保存`AuthoritativeIntentClaimV1`，Controller只从admitted contract机械派生claim；
- frozen exploration authority保存完整canonical claims，反序列化重新验证identity与顺序；
- coverage policy继续机械展开每个claim的mandatory concern和required synthesis/adversarial role。

### 2.2 单cell strategy authority

- catalog registration明确选择`Deterministic { implementation }`或
  `AgentEpisode { authorship_model, invocation, tools }`；
- `OracleStrategyRunV1`只绑定一个workspace、work item、strategy与exact executor；
- Controller先提交`OracleStrategyAuthorized`，再允许executor开始；
- deterministic completion携带exact implementation；Agent completion携带完整Host request、terminal与submission；
- ledger只接受当前run/item的contribution、experiment request或explicit unknown，并为每次变化生成parent-linked
  immutable revision。

### 2.3 Oracle proposal step profile

一个Agent request只包含：structured claim、一个work item、一个run、source task、documentation、build/tests、
knowledge、model、budget和current-V1四项tool catalog。通用`run_proposal_loop`仍是唯一runtime lifecycle。

Host-local source read不产生外部authority。external-test search与Worker experiment会产生strict durable yield；
terminal只接受一个cell的typed contribution或explicit unknown。Host重新绑定item/run，拒绝跨cell或自行填写
authority。

### 2.4 effect先成为typed observation

Controller重新打开exact Host journal并验证yielded operation；每个effect在Worker adapter调用前提交operation
authorization/start。可信receipt与public result形成：

```text
WorkflowToolControllerObservationV1
→ OracleObservationPayloadV1
→ OracleExplorationObservationV1(item, run)
→ OracleExplorationLedgerV1 revision
→ same proposal step episode resume
```

model-visible result同时携带Controller重算的typed Oracle observation identity，所以Agent只能引用当前run已投影
的observation。restart读取已完成的`OperationResult`并重算role projection，不重新dispatch effect；Controller
submission归档会canonical decode observation body并核对item/run，不能把别的typed ID或别的cell observation伪装
成本run证据。

### 2.5 独立Admission语义修正

只包含`OracleCoverageGapArtifact`的submission进入distinct `CoverageGap` resolution。即使honest、mutant、hidden、
bypass和mechanism qualification receipts全部passed，Independent Admission仍输出`Partial`；coverage gap与正向
portfolio material混合提交会原子拒绝。

## 3. 替代与删除

本片删除以下superseded fixed topology，不保留compatibility export：

- `oracle_model_debate.rs`；
- `oracle_debate_prompt.rs`；
- `oracle_debate_tools.rs`；
- `oracle_debate_workflow.rs`；
- `examples/oracle_model_debate_live.rs`；
- historical reduction control中对fixed debate plan/blue proposal/red attack的构造。

Oracle的synthesis/adversarial功能保留为logical strategy role；model、deterministic analyzer、generator、mutation、
property、fuzz和counterexample search都通过同一catalog/ledger contract组合，不存在固定episode数量或顺序。

## 4. 测试与审计

- structured claim、work-item plane、catalog/tool/material/request/run drift fail closed；
- Controller start、observation revision、completion、restart与exact command replay；
- recorded Agent external effect验证start-before-adapter、receipt binding、typed Oracle projection、same-episode resume及
  no-redispatch replay；
- contribution只能引用当前active run observation；
- coverage gap在全部control passed时仍为partial；
- all-features workspace compile、full CI、Clippy、format、diff check和旧symbol/path扫描。

Production prompt/type/control flow未包含`reduce-sum-f32`、D-039 identity、fixture expected answer或fixture-specific
branch。所有语义identity继续使用domain-separated `ContentId<T>`，新增claim、run、tool catalog、Controller
observation、Oracle payload、Oracle observation与completion均为distinct type。

## 5. 明确非目标

- 不声称任何live model生成了adequate Oracle；
- 不调用互联网、remote Worker或NPU，不产生live receipt；
- 不实现Oracle Admission的Controller aggregate stage或Candidate衔接；
- 不预建完整production strategy/mechanism registry、Host pool或task catalog；
- 不把coverage、模型共识、测试通过数量或历史fixture行为提升为admitted correctness；
- 不增加internal format V2或任何兼容路径。

下一片应从`OraclePortfolioReady`接入Independent Oracle Admission authority与required control execution。真实NPU
control需要设备时，必须先检查remote Worker registry/lease在线状态，再授权exact experiment。
