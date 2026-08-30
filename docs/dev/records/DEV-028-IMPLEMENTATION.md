# DEV-028 implementation — strategy-driven Oracle workflow correction

- 状态：`Accepted`
- 日期：2026-08-29
- Slice：[`DEV-028`](../SLICE_CATALOG.md#3-当前critical-slices)
- 架构：D-020、D-043、[`Workflow Architecture`](../../design/WORKFLOW_ARCHITECTURE.md)、
  [`Oracle Exploration Design`](../../oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md)
- 外部执行：无；未调用模型、远端 Worker、Docker、NPU 或互联网

## 1. Objective与考古结论

纠正DEV-025 composition skeleton把当前model-backed synthesis/adversarial profiles误写成产品必经
`Oracle Blue → Oracle Red`阶段的漂移。D-020和Oracle Exploration设计一直规定：Oracle是claim portfolio探索，
Blue/Red只是可选的模型策略实现；policy还可以选择deterministic analyzer/generator、mutation、property/
metamorphic、fuzz或counterexample strategy。真实durable aggregate也只存在通用`AwaitOracleWorkflow`边界，
没有任何production consumer要求固定Blue/Red拓扑。

## 2. 可读业务骨架

`run_controller_workflow`现在只表达：

```text
freeze Controller request
→ SIR Proposal Loop
→ user decision / Intent Admission
→ Oracle Exploration
→ Oracle Admission
→ Candidate Proposal Loop
→ Worker observations
→ Candidate Admission
→ save terminal outcome
```

`ControllerWorkflowStages`只暴露`OracleExplorationProposal`、`run_oracle_exploration`和独立
`run_oracle_admission_gate`。具体strategy数量、实现、模型使用与round policy留在Exploration内部，不再污染
Controller全局拓扑；trait仍无default/no-op成功实现。

## 3. 可选model-backed debate实现

已有dogfood没有被接成强制Oracle workflow，也没有被删除成无consumer的历史残片。它被直接改写为当前V1的
显式可选strategy组合：

- `OracleModelDebatePlanV1`绑定一个synthesis episode和一个adversarial episode；
- `OracleDebateStrategy`、strategy-specific tool catalog、prompt和gateway限制各自capability；
- 两个episode保持不同`EpisodeId`、model/authorship configuration、budget、private context和tool catalog；
- synthesis只提交proposal revision，adversarial只提交attack/variant，二者都不拥有Admission authority；
- plan与prompt content domain直接改为当前model-debate V1，没有legacy reader、alias、converter或版本升级。

该实现是Oracle Exploration可能选择的一个策略组合，不是一般portfolio schema，也不是Controller stage。

## 4. 替代与删除的旧路径

- 删除`OracleBlueProposal`、`OracleRedProposal`、`run_oracle_blue_proposal_loop`和
  `run_oracle_red_proposal_loop` Controller ports；
- 删除`oracle_search.rs`、`oracle_prompt.rs`、`oracle_tools.rs`、`oracle_workflow.rs`模块路径及其旧re-export；
- 删除`OracleSearchPlan*`、`OracleAgentRole`、`OracleRole*`、Blue/Red gateway与tool函数等旧public symbols；
- 删除`oracle_blue_research_live` example和`oracle-blue-dogfood*.json`配置路径；当前路径为
  `oracle_model_debate_live`与`oracle-model-debate*.json`；
- 不保留compatibility alias、dual codec、fallback config key或旧content domain。

历史DEV-025–027 records仍描述当时发生的实现事实；本record和当前baseline明确supersede其固定stage形状。

## 5. Tests与静态controls

- recorded Controller顺序证明`Oracle Exploration → Oracle Admission`且只出现一次；
- unavailable Exploration立即fail closed，任何Admission/Candidate/terminal authority均不会运行；
- model-debate plan strict V1 round-trip、identity binding、distinct episodes/tools/private context与strategy swap controls；
- prompt stable-prefix/restart、strategy tool gateway、proposal/attack authorship与noncanonical feedback controls；
- all-target example编译与旧symbol/path扫描；
- workspace format/check/Clippy/test和`git diff --check`。

Production代码、prompt、content domain和配置没有fixture identity、known answer、fixture-specific branch或通用
untyped content identity。历史reduction integration仍只是evaluation/control consumer，不向runtime strategy提供
restricted expectation。

## 6. 明确非目标

- 不把`AwaitOracleWorkflow`接到真实durable Oracle Exploration aggregate；
- 不预建尚无consumer的通用portfolio/event/schema或strategy registry；
- 不决定下一轮必须使用模型、debate、mutation或其他strategy；
- 不实现Oracle Admission、hidden controls、Worker experiment round-trip或Candidate suffix连接；
- 不改变independent Admission authority，不把adversarial explorer等同于Admission control planner；
- 不调用live model、remote Worker、Docker或NPU，不产生新的runtime、receipt、adequacy或verdict claim。
