# 下一会话启动与交接

- 状态：当前会话交接入口
- 日期：2026-08-31
- 架构基线：D-044、DEV-036 proposal-in-workflow correction
- 当前实现基线：DEV-001–034、DEV-036 已记录；DEV-035 为 `InProgress`

## 1. 下一会话先建立的共同认识

Cairn 曾经实际走通一条窄的控制链：

```text
DeepSeek SIR proposal
→ scoped user decision
→ independent Intent Admission
→ local Oracle qualification/publication
→ DeepSeek Candidate source
→ Controller scheduler / remote Worker build
→ product-owned native ASC build
→ receipt-bound compiler diagnostic
→ isolated DeepSeek revision/repair
→ remote native rebuild
```

DEV-020 的最新 native rebuild 是 `SubjectFailed`。它证明了当时的跨主机 build/feedback/recovery 闭环，不证明
native build success、NPU runtime、semantic correctness、完整 Oracle portfolio、performance 或最终
`MigrationVerdict`。DEV-021–030逐步建立generic proposal step、可读Controller骨架、task-owned durable
SIR/Intent/Oracle authority与external-effect接缝；这些slice只有recorded/local evidence，没有新增live model或
Worker事实。DEV-033已经删除当时的独立`MigrationWorkflowV1`及collection/native Candidate suffix，current path
不再以历史三段式repair作为产品架构。

D-044/DEV-036进一步废弃了上述链路中的独立proposal process假设。Current V1中proposal是Controller主
workflow的typed Agent step；没有proposal binary、supervisor、OS principal、私有journal或process protocol。
旧dogfood数据库不可兼容读取，必须在local Worker runner接通后经正常CLI→server入口重建。
DEV-031现已删除其后仍保留的fixed model-debate实现、example与测试接缝；synthesis/adversarial只是catalog中的
strategy kind，不再对应固定的双episode产品路径。
DEV-029的旧process-specific yield/journal已由DEV-036删除；current边界是`WorkflowToolRequest`经Controller
authority转换为Worker job，并从exact observation恢复Agent step。
DEV-030已把Oracle多平面完备性编码为current-V1 production contract：Controller按claim×concern×role机械展开，
workspace冻结source/docs/build/tests/knowledge/research/experiment capability，ledger逐项保存strategy/experiment/
observation/proposal lineage，Independent Admission从qualified mechanisms与honest/mutant/hidden/bypass receipts重算。
DEV-031继续把admitted intent的完整structured claim、每个独立cell的deterministic/Agent executor、exact
proposal step request/terminal completion和immutable ledger revision接入task aggregate。Agent只提出typed Worker
request，再由Controller形成receipt-bound typed Oracle observation并投影到同一active cell；原episode随后恢复且
不会重发已完成的model turn。coverage-gap保持显式partial，不能被全通过control receipts提升为admitted。
DEV-032把terminal ledger机械冻结为exact portfolio与strict policy，冻结qualified mechanism inventory并展开完整
item × control attempt，只接受exact trusted receipt provenance；Controller在durable replay时独立重算
admitted/partial/rejected claim portfolio，现停在typed Candidate输入边界。没有运行真实mechanism、model或Worker。
DEV-034关闭了其中的receipt注入缺口：每个registration现在冻结distinct mechanism/runner/qualification，Controller
逐项持久化runner-bound run/dispatch后才允许adapter effect，并只从exact ExecutionReceipt observation机械生成
Admission receipt。recorded aggregate control闭合到Oracle accepted；没有运行live model、network、Worker、Docker或NPU。
旧collection-local Oracle Admission prepare/commit/publication旁路已删除，没有compatibility入口。

架构已经由 D-043 冻结：

- Controller 拥有一个 durable workflow state machine；
- SIR、Oracle Exploration、Candidate 和可选 Planner 是不同 Agent Loop；
- Oracle Exploration按policy选择synthesis、adversarial、analyzer、mutation、property或counterexample strategy，
  model-backed synthesis/adversarial debate不是必经拓扑；
- proposal step在Controller主workflow内执行，不映射为独立进程、binary或service；
- Admission 是独立、model-free authority；
- 所有代码/toolchain/Docker/device 实验经 Controller 调度到 managed Worker；
- Worker 在 operator 已有 VPN/可路由私网上直接 outbound mTLS/WSS 连接 Controller；
- single-lab Controller control/enrollment listener 分别监听 `0.0.0.0:7443/7444`，发布 VPN 可达地址；
- SSH tunnel、Controller 反向拨号和 Cairn 自建 VPN 不属于目标架构；
- Candidate feedback 不得在同一 lineage 内移动 Intent 或 Oracle judge。

## 2. 必读顺序

不要从聊天摘要或 DEV-020 末尾的局部“再修一轮”建议直接续跑。按以下顺序读取：

1. 根目录 [`AGENTS.md`](../../AGENTS.md)；
2. [`WORKFLOW_ARCHITECTURE.md`](../design/WORKFLOW_ARCHITECTURE.md)；
3. [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md)；
4. [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 中 DEV-027–034及其implementation records；
5. [`ARCHITECTURE_OVERVIEW.md`](../design/ARCHITECTURE_OVERVIEW.md) 和
   [`RUNTIME_ARCHITECTURE.md`](../design/RUNTIME_ARCHITECTURE.md)；
6. 只有准备修改对应边界时，再读
   [`AGENT_ARCHITECTURE.md`](../design/AGENT_ARCHITECTURE.md)、
   [`ADMISSION_ARCHITECTURE.md`](../design/ADMISSION_ARCHITECTURE.md) 和 focused Oracle/SIR design；
7. 用 Git 和代码验证事实，不把 target design 当作已经实现。

`CURRENT_IMPLEMENTATION_WALKTHROUGH.md` 的详细样例叙事只覆盖到 DEV-012 Candidate proposal；它可用于
理解早期数据流，但不能作为当前 next-step authority。DEV-013–020 以 `CURRENT_BASELINE.md`、slice catalog
和各 implementation record 为准。

## 3. 启动时的只读审计

在提出改动前运行：

```bash
git status --short
git log -5 --oneline --decorate
rg -n "DEV-036|D-044" docs/dev/SLICE_CATALOG.md docs/DECISIONS.md
rg -n "run_controller_workflow|run_oracle_exploration|WorkflowTool|MigrationWorkflowV1" \
  crates/cairn-agent crates/cairn-migration crates/cairn-server
```

预期：worktree clean，HEAD 包含DEV-034 implementation record，production code不再命中
`MigrationWorkflowV1`。如果存在用户未提交修改，先审计并保留，不覆盖或清理。

不要在启动审计中连接 DeepSeek、远端 Worker、Docker、NPU 或互联网。外部 effect 只有在一个已确认 slice
明确要求时才运行。

## 4. DEV-027–036 边界事实

- `run_controller_workflow`显式表达freeze、SIR、derive decision requests、await user decision、Intent Admission、
  Oracle Exploration、Oracle Admission、Candidate、Worker observations、Candidate Admission和terminal；
- `ControllerWorkflowStages`为每一环定义distinct associated artifact type及async port，无default/no-op成功实现；
- `ControllerWorkflowV1`表达exact SIR proposal→Intent Admission→Oracle Exploration/Admission→Candidate
  proposal/build observation→Candidate Admission→terminal；
- Admission executable和restricted-store target必须先获得distinct typed durable authority；Controller只接收
  restricted commit之后的canonical public outcome，不读取restricted artifacts；
- active Controller以recover/select/execute表达一个完整aggregate；任何外部effect都在durable start authority之后；
- DEV-036删除proposal process crate/supervisor/binary identity/private journal；SIR/Oracle/Candidate由Controller
  主workflow直接驱动；
- cross-task、restart、exact replay/changed-input、model/store/outcome drift、no-auto-decision/no-auto-Oracle controls闭合；
- Controller接口、stage order和current production tree不再出现fixed Blue/Red或model-debate executor；Oracle catalog
  只表达logical strategy kind、eligible role/concern与deterministic/Agent executor；
- 只运行本地model-free Admission process control；没有调用live model、remote Worker、Docker或NPU，没有新的
  live receipt或verdict claim。
- Oracle Agent一次只处理一个structured claim × concern × role cell；source、docs、build/tests、knowledge和exact
  tool catalog均进入冻结request，不能跨cell提交或自封Admission结论；
- external-capability tool call形成strict `WorkflowToolRequest`；Controller先提交operation/job authority，再允许
  capability-matched Worker执行；receipt-bound result被重算为distinct Controller observation、Oracle payload与
  run-bound observation，进入ledger后恢复同一Agent step；
- Oracle policy机械包含observable/domain/numerical/interface/memory-effect/failure/determinism/cross-plane/discovery
  concerns；performance profile显式增加resource/performance。每个cell独立要求synthesis并按policy要求adversarial，
  missing strategy不能打开exploration；
- exact code、docs、build/tests、knowledge、research/experiment tools、capability、policy/catalog/budget均由distinct
  workspace edge冻结；portfolio material保留typed kind，不退化为generic ID bag；
- independent admission只从exact qualified mechanism与honest/mutant/hidden/bypass receipts重算
  admitted/partial/rejected；missing receipt保持partial，模型共识没有入口。
- terminal portfolio、strict policy、qualified mechanism inventory、完整item × control attempt、trusted evidence与
  independent outcome都已进入同一个Controller event stream；restart重新派生并核对每一条authority edge，terminal
  状态只暴露typed Candidate outcome输入。
- Oracle mechanism registration同时绑定strong mechanism、runner与qualification identity；每个control先提交exact
  run/dispatch start event，再接受job/attempt/contract/content-bound Worker observation，调用者不能整体注入receipt。
- admitted-only Candidate contract/workspace/public bodies、generic Candidate proposal step、product-owned build plan、
  Worker receipt observation、mechanical Candidate control matrix和independent Candidate terminal也在同一event stream；
  旧collection/native suffix及固定`dav-3510` build profile已经删除。

详细authority、current-V1 contract、tests、删除项与非目标见
[`DEV-027-IMPLEMENTATION.md`](records/DEV-027-IMPLEMENTATION.md)和
[`DEV-028-IMPLEMENTATION.md`](records/DEV-028-IMPLEMENTATION.md)、
[`DEV-029-IMPLEMENTATION.md`](records/DEV-029-IMPLEMENTATION.md)和
[`DEV-030-IMPLEMENTATION.md`](records/DEV-030-IMPLEMENTATION.md)、
[`DEV-031-IMPLEMENTATION.md`](records/DEV-031-IMPLEMENTATION.md)和
[`DEV-032-IMPLEMENTATION.md`](records/DEV-032-IMPLEMENTATION.md)和
[`DEV-033-IMPLEMENTATION.md`](records/DEV-033-IMPLEMENTATION.md)和
[`DEV-034-IMPLEMENTATION.md`](records/DEV-034-IMPLEMENTATION.md)。

## 5. 下一决策点

DEV-034已把qualified Oracle mechanism runner与exact observation lineage接入同一aggregate。下一步优先在两个独立
候选中选择最小slice：qualified Candidate mechanism runner，或失败/partial outcome的generic、exact-observation-bound
revision policy；不得恢复旧三段式repair、让模型生成receipt或按Candidate表现调宽judge。只有真实control需要设备时才
检查远端Worker registry/lease并运行对应effect。

## 6. 网络与部署启动规则

[`../../config/controller.example.json`](../../config/controller.example.json) 已配置：

```text
control listen    = 0.0.0.0:7443
enrollment listen = 0.0.0.0:7444
```

真实部署只需把 advertised control/enrollment endpoint 和 TLS server name 配成现有 VPN 内可达的 Controller
DNS/IP，并由 Worker 直接连接。不要恢复历史 reverse tunnel；历史 DEV records 保留当时事实，但不再定义
运行方案。不要把私有 PKI、enrollment bundle、Worker state、provider credential 或远端绝对路径提交仓库。

## 7. 验证与提交

普通本地实现按风险运行 focused tests、Clippy、compile-fail/static boundary 和：

```bash
scripts/ci.sh
git diff --check
```

完整 CI 的 mTLS loopback 测试需要允许本地 socket；受限沙箱出现 `Operation not permitted` 时，应在获准
环境重跑同一个 CI 命令，不能删除/跳过测试。Live model/remote Worker/NPU lane 保持显式 opt-in，并分别
记录 exact evidence 和未覆盖范围。

## 8. 可直接用于新会话的启动消息

```text
请先读取 AGENTS.md、docs/dev/NEXT_SESSION.md、
docs/design/WORKFLOW_ARCHITECTURE.md、docs/dev/CURRENT_BASELINE.md 和
docs/dev/SLICE_CATALOG.md，并用 Git/代码核对交接事实。先不要调用模型、远端 Worker 或修改代码。
请核对D-044/DEV-036：proposal是Controller主workflow中的typed Agent step，不存在独立proposal process、
binary、supervisor、private journal或OS isolation。需要编译、运行、Docker或设备能力时，必须走
WorkflowToolRequest→Controller JobContract authority→capability-matched cairn-worker→exact observation。
继续闭合local Worker qualified Oracle runner后，再从正常CLI→server入口重做vectorAdd dogfood。
先给出最小slice/DCR、将替代的旧路径、测试与明确非目标；确认没有fixture-specific或generic-ID漂移后停下来
让我确认。先不要调用模型、远端Worker或修改代码。
```

如果用户在新会话中明确指定下一slice，则先完成同样的只读审计与DCR，再按其授权scope实施；遇到normative
conflict、外部授权或会改变slice目标的选择时停下。
