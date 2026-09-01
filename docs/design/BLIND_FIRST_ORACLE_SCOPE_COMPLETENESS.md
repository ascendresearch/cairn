# Blind-First Oracle Scope 设计完整性审计

- 审计对象：[`BLIND_FIRST_ORACLE_SCOPE_DESIGN.md`](BLIND_FIRST_ORACLE_SCOPE_DESIGN.md)
- 审计日期：2026-09-01
- 结论：用户提出的方案要点均已保留；写后检查发现的结构性遗漏已修正；剩余问题均明确标为待实验问题

## 1. 用户方案逐项核对

| 用户要求 | 文档落点 | 结果 |
| --- | --- | --- |
| 首轮不告诉 runtime model 应分析哪些内部维度 | 4.2、5、6 | 已覆盖；隐藏 catalog、identity、示例及间接 taxonomy 泄露 |
| 首轮先报告准备分析的维度 | 6.1、6.2 | 已覆盖；输出 task-specific blind dimensions，而非 generic labels |
| 同时报告支持这些维度的证据 | 6.2 | 已覆盖；要求 exact trigger evidence、risk、domain、uncertainty 和 evidence plan |
| 首轮结果不能在看到补充提示后被改写 | 7、15、21 | 已覆盖；content-addressed freeze、独立 lineage 和 restart 边界 |
| 随后由系统提示补充维度 | 8 | 已覆盖；Coverage Auditor 使用预先 sealed 的完整 policy ledger 产生 challenge |
| 补充不是强制生成全部 item | 3、9、10 | 已覆盖；每项 concern 先 disposition，再合并为最小 obligation graph |
| 可以在“scope prospectus 后”提示 | 16、17.3 | 已设为默认 D1 产品路径 |
| 可以在“完整自主判断后”提示 | 16、17.3 | 已保留为 D2 研究 treatment，并要求额外冻结 blind detailed assessment |

## 2. 架构边界核对

| 边界 | 核对结果 |
| --- | --- |
| Builder/runtime actor | coding agent 不解释 fixture 或手工补维度；runtime roles 提案，Controller 编排 |
| Intent | D 从 admitted Intent 开始；scope evidence 若暴露 Intent 歧义，返回新 SIR/Intent lineage |
| Target | Blind role 仍看到真实 Ascend 950PR/CANN/ABI/capability context，不以“盲”为由隐藏任务事实 |
| 全面性 | sealed task ledger 的每个 policy concern instance 必须恰好一个 typed disposition |
| 非锚定 | catalog 和 challenge manifest 在 blind episode 前冻结、对 blind role 不可见 |
| Novel discovery | catalog 是 coverage floor，不是允许列表；catalog 外发现不能按名称自动删除 |
| 强 identity | distinct claim、dimension、concern、mapping、obligation、case、mechanism 和 revision 不共用 generic ID |
| Worker | capability/evidence class、requesting lineage、receipt projection 和 execution failure 均显式区分 |
| Review | Coverage Auditor、Consolidator、Scope Reviewer、item Reviewer 和 Admission 权限不混合 |
| Admission | model consensus 不产生 authority；required unknown 保持阻塞，最终 Gate 仍 model-free |
| Persistence | blind/challenge visibility 边界、artifact 和 continuation 在 restart 后保持一致 |
| Privacy | 日志只含 identity、计数、状态和失败分类，不写模型正文、源码、stdout/stderr 或 hidden material |
| Pre-release V1 | 未引入 V2、legacy alias、兼容 reader、双写或迁移路径 |
| Product path | 实施验收要求正常 CLI/server/app/workflow/Worker 路径和第二个语义不同任务 |

## 3. 写后检查发现并修正的问题

1. **补充清单的事后选择风险**：初稿只规定 blind 阶段看不到 catalog，没有规定 catalog 何时冻结。现已要求
   `OraclePolicyConcernCatalogV1`、task ledger 和 challenge exposure manifest 在 blind episode 前 sealed commit，防止
   看到模型输出后临时挑选补充维度。
2. **claim × concern 再次膨胀风险**：现已明确 task ledger 不是机械笛卡尔积；policy concern 在 task scope 枚举，
   通过 typed coverage edges 绑定一个或多个 distinct admitted claims。
3. **一项 concern 需要多个义务**：初稿只有 adopt/merge。现已增加 `SplitAcrossObligations`，允许不同 domain/risk
   必须独立验证，同时要求 non-empty edges 和不可合并理由。
4. **错误 blind dimension 的处置**：初稿缺少不丢历史又能排除无证据发现的状态。现已增加仅适用于 blind proposal 的
   `RejectBlindProposalAsUnsupported`。
5. **policy requirement 被模型降级**：现已明确 Consolidator 不能把 sealed requirement 从 required 改成 informational；
   修改 policy 必须走 exact authorized decision。
6. **challenge 只列“遗漏项”的不完整性**：现已要求 challenge 绑定 sealed ledger 并枚举每个 concern instance，
   Controller 能重算完整性；疑似 gap 仍作为重点 challenge，而非唯一内容。
7. **角色 continuation 污染**：现已要求 Auditor 和 Consolidator 使用新 Agent Loop，不恢复 blind continuation；允许同一
   frozen model configuration，但 role、episode、context 和 artifact identity 不同。
8. **未定义的 Blind Scope Review**：初稿在 generic-list 防护中引用了不存在的阶段。现已改为 freeze 前机械 schema
   validation，以及 freeze 后 challenge/consolidation/scope review 的显式语义处置。
9. **scope evidence 倒灌 Intent**：现已定义 `OracleScopeEvidenceObservationV1` 的用途分类；发现 Intent 歧义必须回到新的
   SIR/Intent lineage，不能由 Scope Consolidator 修改 admitted claim。
10. **D2 证据可能丢失**：现已要求 D2 在 policy 可见前冻结包含 provisional decisions、完整 evidence、receipt 和 unknown
    的 `BlindDetailedScopeAssessmentV1`。
11. **指标体系只有名称、没有测量契约**：初稿没有固定分析单位、分母、primary correctness gate、Reviewer/evidence
    边际收益、authority violation、稳定性或 incomplete-run 规则。现已增加独立
    [`D_MEASUREMENT_PROTOCOL.md`](../experiments/reasoning-decomposition-ablation/D_MEASUREMENT_PROTOCOL.md)，并要求与
    hidden evaluator 在正式运行前共同预注册。
12. **SIR/Intent 上游混杂**：D 的直接 treatment 位于 Intent Admission 之后。现已要求主 component experiment 在各 arm
    复用 exact admitted Intent、evidence snapshot 和 upstream receipts；完整 CLI/SIR 路径作为单独 confirmatory
    experiment，不能把 upstream divergence 归因给 D。
13. **`no-evidence` 含义与开发者价值遗漏**：现已明确它只关闭 proposal-visible 新 observation，不关闭共同 hidden
    controls；并增加 developer trace accuracy、failure-state 理解、mechanism replay 和 actionable handoff 指标。
14. **控制上游变量可能绕过产品入口**：现已禁止手写共享 SIR/Intent 或内部 helper。component arms 必须绑定一次真实
    CLI/server/workflow 产生的 immutable upstream snapshot，并保留 typed parent/treatment lineage，继续走正常 ports。

## 4. 有意保留的开放问题

以下内容不是遗漏，而是缺少实验依据，正文第 22 节明确保持开放：

- 是否需要一个同样看不到 taxonomy 的独立 blind Reviewer；
- catalog progressive disclosure 是否能在保持完整 identity 的同时降低上下文成本；
- Consolidator 使用同一还是不同 model configuration；
- `NotApplicable` 的最低 evidence policy；
- property/case 提升规则中机械判断与模型判断的边界；
- typed mechanism compiler 的实际收益；
- D1 相对 B 的 coverage/cost 效果；
- D2 的额外无锚定分析是否值得成本。

这些问题不能在实现时凭便利静默选择。进入相应 slice 前必须冻结 treatment 或形成新的明确设计决策。

## 5. 最终防丢结论

审计后，方案的完整语义为：

> 系统在 blind 运行前冻结但隐藏完整 policy commitment；runtime model 先基于真实代码、Intent、target 和 evidence
> 独立提交 scope；Controller 冻结原始发现；独立 Auditor 再用预先承诺的 policy challenge 补漏和质疑映射；新的
> Consolidator 与 Scope Reviewer 形成可机械核对、保留 novel discovery、没有静默缺口的最小 obligation graph；之后
> 才展开 candidate-facing property、case、mechanism、Worker qualification 和 Admission。

没有把该方案误写成“模型先选什么，系统就只验证什么”，也没有把 A/B/C pilot 误写成已经证明 D 优越。
