# DEV-030：多平面 Oracle Exploration 与独立 Admission 内核

- 状态：Accepted
- 日期：2026-08-29
- 外部 effect：无；未调用模型、远端 Worker、Docker、NPU或互联网

## 目标

把“Oracle 是 claim-scoped、多平面 portfolio”从设计文字变成 current-V1 生产约束。Controller 不再把
“请全面考虑”交给一个 Agent，而是确定性展开每个 claim × concern × logical strategy role，并以 durable
immutable ledger revision逐项推进。

## 已实现

- `OracleCoveragePolicyV1`从 correctness/performance profile机械派生 mandatory concerns；persisted policy不能
  删除其中一项。correctness覆盖 observable semantics、valid/boundary/invalid domain、numerical、shape/layout/type、
  aliasing、memory/effects、failure、determinism，并强制 cross-plane invariant和uncatalogued-risk discovery。
- `OracleAdversarialPolicyV1`决定是否为每个 concern增加独立 adversarial obligation；synthesis始终存在。
- `OracleStrategyCatalogV1`支持 deterministic analyzer、model-backed synthesis/adversarial、generator、mutation、
  property、fuzz和counterexample search。executor显式区分 deterministic implementation与model/tool binding；
  缺少任何 required cell consumer时 exploration不能打开。
- `OracleWorkspaceV1`冻结 admitted intent、SIR input/task bundle、source、documentation、build/tests、knowledge、
  bounded research tools、experiment tools、capability grant、policy、strategy catalog和budget的distinct typed edges。
- `OracleExplorationLedgerV1`以parent-linked immutable revision表达 pending → running → experiment proposed →
  Controller authorized → observation projected → contributed/unknown。Agent不能删除cell、签发experiment authority
  或把unknown标为success。
- strategy run不是裸digest：`OracleStrategyRunV1`绑定workspace、exact work item、strategy及catalog-resolved
  deterministic/model executor；`OracleExperimentRequestV1`绑定item、run、experiment tool catalog、operation和exact
  arguments；`TrustedOracleWorkerReceiptV1`再绑定request、generic Worker job contract和execution receipt。
- Worker observation绑定 exact work item、strategy run、experiment request、trusted Worker receipt和payload；
  research/deterministic observation保留不同provenance class。
- `OraclePortfolioElementV1`保留 domain refinement、case、reference、property、source plan、valid-family plan、
  observation plan、comparator、execution/safety和coverage gap的typed区别，并绑定产生它的cell/run/observations。
- `run_oracle_exploration`和`run_independent_oracle_admission`分别成为短小、可读的业务骨架。Admission机械派生并
  执行 mechanism qualification、honest、mutant、hidden和bypass controls，再从exact trusted receipts重算
  admitted/partial/rejected claim portfolio；不存在模型投票入口。
- Controller公共骨架把中间产物命名为`OraclePortfolioProposal`，Admission端口改为
  `run_independent_oracle_admission`。task-owned aggregate现在从`AwaitOracleExplorationWorkspace`接收并归档
  exact policy、strategy catalog、workspace、claims和Controller重算的initial ledger，durable推进到
  `RunOracleExploration`；Manager只发布ready authority，不擅自选择或运行strategy。
- Manager在提交opening event前验证workspace引用的source、documentation、build/tests、knowledge、research/
  experiment tool catalogs和capability grant都真实存在于typed content store；缺一项即失败，不产生可运行状态。
- verification current-V1 corpus provenance删除`Blue`/`Red`枚举，改为logical synthesis/adversarial strategy来源。

## Authority与失败边界

- Strategy只贡献proposal material、unknown或experiment request。
- Controller start authority必须先于Worker effect；observation provenance不能替换或跨cell/run/request投影。
- policy waiver需要distinct authority artifact；budget exhausted、missing strategy、unknown和unsupported都不能变成
  contributed或admitted。
- Admission只接受与exact portfolio/work item绑定的trusted receipt；missing controls保持partial，failed control使
  claim rejected，未知work-item receipt原子拒绝。
- current V1反序列化重新执行policy、plane/concern、strategy/executor、revision lineage和admission outcome invariants。

## 替代/删除

- 替代“一个Oracle Exploration proposal”的模糊Controller命名，使用claim-scoped portfolio proposal。
- 删除verification公开provenance中的Blue/Red二元角色来源；不提供legacy alias或dual decoder。
- `AwaitOracleWorkflow`模糊等待状态由`AwaitOracleExplorationWorkspace`直接替代。

## 验证

- 每claim、每mandatory concern、每required role均有work item；cross-plane/discovery不可省略。
- persisted policy删项、work-item plane漂移、missing strategy、未完成portfolio freeze均失败。
- exact experiment authority/receipt/observation round trip形成六个连续ledger revisions；request drift失败。
- strategy executor漂移、experiment tool-catalog漂移和receipt/request漂移在ledger推进前失败。
- forged admission status、unknown-item receipt、missing controls和failed controls分别失败/partial/rejected。
- readable exploration与Controller skeleton order tests。
- admitted intent→Oracle opening durable transition、restart、exact replay、claim/workspace/ledger typed binding controls。
- focused tests、all-target/all-feature Clippy与完整`./scripts/ci.sh`。

## 明确非目标

- 本slice不替业务调用方解释`MigrationIntentContractV1`并生成claims，也不自动选择strategy；它只接受调用方提供的
  task-generic typed claims/workspace，并由Controller重算、归档和持有initial ledger authority。
- 没有选择某个具体strategy作为默认答案，没有运行model-backed debate，也没有具体tool→Worker adapter/live receipt。
- 没有声称Oracle portfolio已经admitted、Candidate suffix已连接或端到端MigrationVerdict已完成。
