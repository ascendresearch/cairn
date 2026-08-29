# 下一会话启动与交接

- 状态：当前会话交接入口
- 日期：2026-08-29
- 架构基线提交：`c49e16a`（`docs: freeze agent-loop workflow architecture`）
- 当前实现基线：DEV-001–023 已记录；DEV-023 为 `Accepted`

## 1. 下一会话先建立的共同认识

Cairn 已经实际走通一条窄的控制链：

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

DEV-020 的最新 native rebuild 是 `SubjectFailed`。它证明了跨主机 build/feedback/recovery 闭环，不证明
native build success、NPU runtime、semantic correctness、完整 Oracle portfolio、performance 或最终
`MigrationVerdict`。DEV-021已把Candidate native suffix固化为task-owned durable workflow；DEV-022又让同一
generic Proposal Host承载SIR/Candidate role profile并消费persisted workflow request；DEV-023再由active Controller
single-task manager消费durable action并连接Host supervision、Worker scheduler/reconciliation与receipt折回。三片都
只有recorded/local evidence，没有新增live model或Worker事实。

架构已经由 D-043 冻结：

- Controller 拥有一个 durable workflow state machine；
- SIR、Oracle Blue/Red、Candidate 和可选 Planner 是不同 Agent Loop；
- capability-equivalent loop 由通用 Proposal Host 承载，不为 SIR 保留专用长期 service；
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
4. [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 中 DEV-023及其implementation record；
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
rg -n "DEV-023|D-043" docs/dev/SLICE_CATALOG.md docs/DECISIONS.md
rg -n "CandidateWorkflowManagerConfigV1|drive_candidate_workflow_once|ProposalHostBinaryIdentity" \
  crates/cairn-migration crates/cairn-server crates/cairn-proposal-host
```

预期：worktree clean，HEAD 至少包含 `c49e16a`。如果存在用户未提交修改，先审计并保留，不覆盖或清理。

不要在启动审计中连接 DeepSeek、远端 Worker、Docker、NPU 或互联网。外部 effect 只有在一个已确认 slice
明确要求时才运行。

## 4. DEV-023 已闭合的事实

- active Controller可选监管一个配置中明确给出的existing Candidate `TaskId`，没有另一个逐步手工launcher；
- manager逐项消费durable next action，所有Host/Worker effect均先commit exact episode/dispatch IDs；
- `ProposalHostBinaryIdentity`与Worker identity分离；Host要求Controller-prepared invocation marker，并在provider effect前
  保存request-bound start marker，store丢失、binary/model/root drift均fail closed；
- scheduler waiting/reconcile/terminal和Worker receipt进入同一task workflow；SubjectFailed只通过exact build、receipt、
  stderr/evidence构造typed diagnostic，不解释compiler文本；
- `NoCandidate`、expired、NotStarted、Ambiguous和Host failure均typed blocked且不生成replacement IDs；
- 两个materially different task走同一manager path；real Host child replay、missing marker/store controls和local
  controlled receipt折回闭合；
- 四个旧public手工composition helper入口及helper-only test已删除，无alias/dual path；
- 没有调用live model、remote Worker、Docker或NPU，没有新的live receipt或verdict claim。

详细authority、current-V1 contract、tests、删除项与非目标见
[`DEV-023-IMPLEMENTATION.md`](records/DEV-023-IMPLEMENTATION.md)。

## 5. 下一决策点

DEV-023只实现一个已存在Task的Controller manager，没有task intake/global catalog，也没有新增live build事实。
下一片应从真实缺口中审计最短路径：取得native build success后接入最小Candidate Admission/NPU evidence lane，或在
出现真实task-intake consumer时增加task discovery。不要预建Oracle/Planner profiles、Host pool、多租户协议或完整
位置catalog；新增role和supervision topology必须先有真实typed consumer。

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
请核对 Accepted DEV-023 的single-task manager、Host start authority、scheduler/receipt折回、blocked semantics、删除项与CI事实，并审计下一处真实workflow接缝。
先给出最小slice/DCR、consumer、将替代的旧路径、测试与明确非目标；确认没有fixture-specific或generic-ID
漂移后停下来让我确认。先不要调用模型、远端Worker或修改代码。
```

如果用户在新会话中明确指定下一slice，则先完成同样的只读审计与DCR，再按其授权scope实施；遇到normative
conflict、外部授权或会改变slice目标的选择时停下。
