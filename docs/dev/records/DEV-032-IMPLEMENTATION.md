# DEV-032 — durable Independent Oracle Admission closure

- 状态：`Accepted`
- 日期：2026-08-30
- 依赖：D-043、DEV-030、DEV-031
- 外部执行：无；未调用live model、互联网、remote Worker、Docker或NPU

## 1. 目标与最小DCR

把DEV-031的terminal exploration ledger接到同一个task-owned Controller aggregate中的Independent Oracle
Admission：Controller冻结exact portfolio与strict policy，冻结qualified mechanism inventory并机械派生完整
item × control obligation matrix，只接受exact attempt上的trusted receipts，model-free重算admitted/partial/rejected
claim portfolio，再把typed terminal outcome交给Candidate边界。

本片直接修改current V1，不增加alias、dual reader、converter或format version。业务流程仍以可读步骤表达：

```text
freeze terminal portfolio and policy
→ authorize exact mechanism inventory and control obligations
→ record trusted evidence
→ independently recompute claim outcomes
→ persist terminal Oracle outcome
→ await Candidate workflow
```

## 2. 实现

### 2.1 exact portfolio与policy authority

探索ledger返回`FreezePortfolio`时，Manager不再返回无事件的`OraclePortfolioReady`。它机械构造并归档
`OraclePortfolioProposalV1`与`OracleAdmissionPolicyV1::strict()`，Controller提交
`migration.controller-oracle-portfolio-frozen`。event replay重新从携带的terminal ledger构造二者并核对exact
identity与revision。

### 2.2 mechanism inventory与完整control matrix

`OracleAdmissionMechanismCatalogV1`要求且只允许当前strict policy的五个distinct control family registration：
mechanism qualification、honest、mutant、hidden和bypass。每个registration引用已经归档的
`OracleQualifiedMechanismArtifact`。

`OracleAdmissionAttemptV1`绑定exact proposal、policy、mechanism catalog，并为portfolio中的每个work item机械展开
五项`OracleControlObligationV1`。调用方不能提供、减少或重排required control work。Controller先提交
`migration.controller-oracle-admission-authorized`，再进入receipt边界；该attempt就是所有后续control effect的
durable start authority。

### 2.3 trusted evidence与独立重算

`OracleAdmissionEvidenceV1`只接受exact attempt中存在的item × control × qualified mechanism；duplicate control或
复用同一个trusted receipt identity均原子拒绝。Manager还要求每个`TrustedOracleControlReceiptArtifact`已存在于
Controller CAS。

`recompute_oracle_admission`只读取frozen proposal、strict policy、mechanism catalog、attempt与evidence：全部controls
passed且portfolio cell为positive contribution才admitted；failed control使claim rejected；missing/unavailable receipt
或coverage gap保持partial。模型意见、投票或Candidate表现没有输入位置。

outcome现在显式绑定attempt与evidence identity。Controller在投影
`migration.controller-oracle-admission-recorded`时再次独立重算完整outcome，随后状态进入
`AwaitCandidateWorkflow { outcome }`。

### 2.4 canonical与强类型边界

- proposal、policy、mechanism catalog、attempt、evidence与outcome分别使用distinct `ContentId<T>`；
- mechanism、trusted receipt和work item identity不能以generic ID互换；
- catalog、attempt、evidence与outcome反序列化重新执行current-V1结构校验；
- obligation、receipt、item bucket和claim按typed content identity canonical排序，不依赖fixture或结构偶然顺序；
- aggregate restart从event body重算每一个派生artifact，changed input不能靠CAS identity外观通过。

## 3. 替代与删除

- 删除Manager的无durable event `OraclePortfolioReady`等待路径；
- 以`OraclePortfolioFrozen → OracleAdmissionAuthorized → OracleAdmitted`替代探索与Admission之间的手工接缝；
- 不保留旧status alias、fallback reader或转换逻辑。

fixed Blue/Red路径已在DEV-031删除，本片没有恢复。synthesis/adversarial仍是Exploration strategy role，不是
Admission control authority。

## 4. 测试与审计

- 完整durable SIR→user decision→Intent Admission→multi-cell Oracle Exploration→portfolio freeze→Oracle
  Admission→Candidate boundary linear control；
- 每个剩余claim × concern × role cell都显式终结后才允许freeze；
- portfolio、attempt和terminal outcome restart与exact command replay；
- missing receipts产生partial、failed control产生rejected、coverage gap在全passed controls下仍不能admit；
- unknown item/cross-attempt mechanism、duplicate control与trusted receipt identity reuse fail closed；
- 多item obligation/outcome canonical排序不依赖work-item结构顺序；
- all-features workspace compile、full CI、Clippy、format、diff check与旧status扫描。

Production type、prompt和control flow没有fixture identity、expected answer、`reduce-sum-f32`或D-039 special case；本片
没有用coding agent的task解释替代runtime Agent或Worker evidence。

## 5. 明确非目标

- 不实现具体honest/mutant/hidden/bypass mechanism runner或声称任何mechanism已被真实qualified；
- 不调用live model、GitHub/互联网、remote Worker、Docker或NPU，不产生live receipt；
- 不把Candidate suffix并入同一个aggregate；本片只保存typed Oracle outcome并停在其输入边界；
- 不实现Candidate Search、Candidate Admission或terminal migration verdict；
- 不预建完整mechanism registry、task catalog、Host pool或device experiment adapter；
- 不增加internal format V2或兼容路径。

下一片应从`AwaitCandidateWorkflow { outcome }`审计现有task-owned Candidate suffix的authority input并合并aggregate，
同时保持Candidate不能修改frozen intent、Oracle proposal、attempt、evidence或outcome。若真实control/candidate
experiment需要NPU，必须先检查remote Worker registry/lease在线状态，再为exact operation取得durable start
authority。
