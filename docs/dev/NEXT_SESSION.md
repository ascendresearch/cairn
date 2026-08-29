# 下一会话启动与交接

- 状态：当前会话交接入口
- 日期：2026-08-29
- 架构基线提交：`c49e16a`（`docs: freeze agent-loop workflow architecture`）
- 当前实现基线：DEV-001–021 已记录；DEV-021 为 `Accepted`

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
`MigrationVerdict`。DEV-021没有新增live evidence；它已把Candidate native suffix固化为task-owned durable
workflow，并以recorded two-material consumer、mid-episode restart、exact replay和typed negative controls闭合。

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
4. [`SLICE_CATALOG.md`](SLICE_CATALOG.md) 中 DEV-021及其implementation record；
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
rg -n "DEV-021|D-043" docs/dev/SLICE_CATALOG.md docs/DECISIONS.md
rg -n "MigrationWorkflowV1|CandidateEpisodeRequestV1|prepare_candidate_native|schedule_candidate_native" \
  crates/cairn-migration crates/cairn-server
```

预期：worktree clean，HEAD 至少包含 `c49e16a`。如果存在用户未提交修改，先审计并保留，不覆盖或清理。

不要在启动审计中连接 DeepSeek、远端 Worker、Docker、NPU 或互联网。外部 effect 只有在一个已确认 slice
明确要求时才运行。

## 4. DEV-021 已闭合的事实

- `MigrationWorkflowV1`由existing `TaskId`拥有，product fold位于`cairn-migration`，server只做CAS/native
  materialization与existing scheduler composition；
- authority、publication、diagnostic、dispatch、episode request、revision budget和terminal outcome均使用
  distinct strong types；全部scheduler IDs在effect前持久化；
- exact command replay返回current recovered state，changed input冲突；in-doubt只进入reconcile action；
- recorded consumer用两份不同source material完成revision→follow-up→repair→terminal链；
- episode request在未消费时可经SQLite重开恢复；wrong diagnostic domain和publication ID混用fail closed；
- two one-shot DeepSeek examples、two configs、three native smoke scripts和three direct native ignored tests已删除；
- low-level typed materializers、diagnostics、repair lineage、scheduler和历史records保留；
- 没有调用model、remote Worker、Docker或NPU，没有新的live receipt或verdict claim。

详细authority、current-V1 contract、tests、删除项与非目标见
[`DEV-021-IMPLEMENTATION.md`](records/DEV-021-IMPLEMENTATION.md)。

## 5. 下一决策点

workflow现在产生真实`CandidateEpisodeRequestV1` consumer seam。若下一片选择通用 Proposal Host，应先审计
现有SIR/Candidate launcher与durable agent runtime，并至少满足：

1. 让至少 SIR 与 Candidate 两种 role profile 通过同一 Host implementation、不同 isolated episode/grant
   运行，证明它不是重命名后的专用 SIR process；
2. 保留不同 context、continuation、budget、tool result、namespace 和 capability；
3. 所有实验仍通过 Controller→Worker，不给 Host 本地 Docker/Worker credential；
4. production SIR 被 Host 接管后，直接删除当前 `cairn-sir` one-shot path，不保留双 launcher；
5. 再沿真实 workflow consumer 推进 Oracle qualification、native success 和 NPU Candidate Admission。

具体 slice 编号和scope由下一次只读审计与用户确认决定，不在本交接中预建DEV-022以后清单。也可以先沿
其他真实workflow缺口切片；不得因为Host出现在目标架构中就默认它是下一项已授权实现。

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
请核对 Accepted DEV-021 的aggregate、recorded consumer、删除项与CI事实，并审计下一处真实workflow接缝。
先给出最小slice/DCR、consumer、将替代的旧路径、测试与明确非目标；确认没有fixture-specific或generic-ID
漂移后停下来让我确认。先不要调用模型、远端Worker或修改代码。
```

如果用户在新会话中明确指定下一slice，则先完成同样的只读审计与DCR，再按其授权scope实施；遇到normative
conflict、外部授权或会改变slice目标的选择时停下。
