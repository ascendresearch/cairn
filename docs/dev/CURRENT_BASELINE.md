# Cairn 当前开发基线与迁移分类

- 状态：当前事实基线与开发状态账本；以仓库代码、测试、commit 和 durable artifacts 为证据
- 日期：2026-08-27
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 说明：本文件不把目标设计误报为实现

旧的完整 Phase A–G 实施流水、Blue/Red prompt 设计和 dogfood 记录保存在 Git commit `688b637` 及其
历史中。本文件只保留当前开发决策所需的事实摘要；需要逐条追溯时使用 Git，不在当前文档集中复制
历史叙事。

## 1. 基线结论

Cairn 已有较强的 record、agent protocol、worker、scheduler、generic execution 和局部 Oracle 控制基础，
但新的端到端架构尚未开始。当前最重要的开发动作不是继续扩充固定 `matmul-zero-k`，而是建立
SIR proposal → Intent Admission → admitted intent → Oracle claim proposal 的第一条 authority-safe 纵向路径。

## 2. 分类规则

现有内容分为五类：

| 类别 | 含义 | 后续动作 |
| --- | --- | --- |
| `ReusableFoundation` | 符合新边界的领域中立机制 | 保留，补 contract/architecture tests |
| `ProductEvidence` | 已证明某个产品机制，但范围较窄 | 迁入新模块并保留 exact scope |
| `HistoricalControl` | 能揭示回归或风险，但不是目标业务模型 | 净化后保留为 fixture/control |
| `PartialPath` | 部分工作可复用，但 authority/closure 不完整 | 拆解复用，不沿旧顺序续写 |
| `SupersededDirection` | 与新设计冲突或会造成漂移 | 在消费方切换后直接删除，不留兼容路径 |

## 3. 可复用基础

### 3.1 Record 与协议

- 强类型 V1 protocol/codec；
- append-only event、CAS、record/replay 基础；
- durable identity、native continuation 和 semantic projection；
- provider request/response、tool operation 和 usage/cache facts；
- SQLite store 与 fault/restart 控制。

复用条件：补齐 public/restricted/secret typed ports，任何现有通用 content handle 不得被带入 Admission
authority。

### 3.2 Agent runtime

- OpenAI Responses、Chat Completions、Anthropic Messages protocol paths；
- model template/deployment/protocol/credential 分层；
- durable episode、tool、budget、repair 和 recorded provider 基础；
- Blue/Red 独立 continuation 与 artifact-mediated revision 的部分证据。

复用条件：runtime 保持 domain-neutral；产品 profile catalog、invocation policy、interaction validator 和
Proposal Host 仍需新建。

### 3.3 Execution 与 Worker

- worker enrollment、credential rotation、resource probing；
- scheduler、reservation、lease、assignment 和 reconciliation；
- opaque `JobContract`、attempt、output capture 和 authoritative receipt；
- Docker execution、CUDA-capable Worker 和 Ascend build Worker 的真实记录；
- x86-64/AArch64 构建与发布基础。

复用条件：Worker 不加入 Intent/Oracle/Candidate 业务判断；restricted hidden job 需要新的 one-time
capability data path。

### 3.4 Verification mechanics

- numerical allowance provenance/assurance 的一部分强类型；
- generic comparison、mutation grid、receipt binding；
- historical reduction correct/wrong variants 与 blind-spot control；
- hardware-free admitted-oracle/candidate-verdict 控制。

复用条件：先由 DEV-002 冻结 independent qualification controls，再由 owning implementation slice 对
exact source/dependency 完成 qualification，任何 mechanism 在此前不得进入 Gate；产品专属 policy 移回
`cairn-cuda-ascend`，generic crate 只保留真正共享的 mechanics。

## 4. 产品证据与历史控制

### 4.1 Blue/Red dogfood

它证明：

- bounded research tool 可以进入 durable model loop；
- retrieved upstream bytes 可保持 research provenance；
- malformed submission 可在原 role continuation 修复；
- frozen proposal、attack finding、changed revision 和 re-review 可以 artifact-mediated；
- provider cache usage 可观测但不形成 authority。

它没有证明：

- Blue/Red 是永久 Agent 拓扑；
- debate convergence 等于 Oracle Admission；
- 完整 Oracle portfolio、hidden controls 或 receipt closure 已存在；
- Candidate、真实 Ascend NPU 或 performance 已完成。

分类：`ProductEvidence + PartialPath`。

### 4.2 `matmul-zero-k` materialization

它证明模型 authored typed body 可被验证、物化、归档并经现有 call-adapter/host fixture 比较，也暴露了
signed-zero authority 边界。

它不能覆盖 nonzero computation、完整 claim set、数值域、真实 CUDA/Ascend C 对照、anti-bypass 或
performance。分类：`HistoricalControl + ProductEvidence`。保留为 transport/materialization fixture，
不得继续把增加固定 case 当作 Oracle 自动生成路线。

### 4.3 Historical reduction

它证明 measured-family allowance、correct/wrong controls、mutation blind spot、receipt recomputation 和
candidate comparison 的若干机制。其旧 domain/admitted shapes 不是新 Intent/Oracle portfolio 的默认
schema。分类：`HistoricalControl`；按 D-041 生成新的净化 fixture，不修改旧证据后冒充 digest 未变。

## 5. 目标设计尚未实现

- `cairn-cuda-ascend` 对 `cairn-migration` 的直接 V1 替换；
- 独立 `cairn-sir` 与 `cairn-admission` 进程；
- generic `cairn-proposal-host`；
- public/restricted/secret storage capability 闭合；
- `IntentHypothesisSet`、`RequiredIntentEvidenceSet` 和 `MigrationIntentContract` 纵向路径；
- Oracle claim portfolio、required claim graph 和独立 Oracle Admission；
- 产品侧 11-position Agent-capable catalog 与七类 typed Planner profiles；
- Candidate Search 正式 profile 和新架构 Candidate Admission；
- 真实 CUDA reference → Ascend C build → Ascend NPU execution 的统一证据图；
- Hardware Performance Model、microbench、profiler qualification、conditional roofline；
- Knowledge/Skill registry、typed feedback、contamination 和 revalidation；
- hidden corpus exposure/burn/replenishment；
- 第二个语义形态不同的 CUDA kernel 端到端路径。

## 6. 必须停止的自然外推

- 不继续用固定 `matmul-zero-k`/nonzero-K case 数量代表 Oracle coverage；
- 不把旧 `AdmittedOracle`/candidate `Pass` 直接扩字段冒充多平面新 contract；
- 不把 Blue/Red debate status 连接到 admitted constructor；
- 不在 `cairn-agent` 中加入 CUDA、Ascend、Oracle 或 Planner kind 分支；
- 不为旧 `cairn-migration` schema 增加 compatibility reader、alias 或 converter；
- 不让 Controller 暂时读取 restricted hidden bytes；
- 不让 Worker 因当前 fixture 便利理解 product role；
- 不在真实硬件可用前把 skipped lane 记为 passed。

## 7. 开发起点

D-039、D-040、D-041 已于 2026-08-27 接受，关闭了三个 P0 设计选择。D-041 sanitized fixtures、manifest、
scan profile和current-V1 `cairn-testkit` contract已由DEV-003 commit
`79a1174ad9767ab528c808a39511ada91e8129f9`接受。D-039 source/corpus bytes和D-040 qualification
contract/independent controls尚未生成；DEV-001因此进入`Ready`，DEV-002仍为`Proposed`，下游代码slice保持
`Blocked`。Qualification receipt不能在verdict-relevant implementation、dependency和calibration
environment存在前预填；十项exact receipts按catalog mapping由DEV-100/102/103/104在首次使用前生成，
DEV-104负责set closure。

DEV-001 与 DEV-003 的首版 `DesignConformanceRecord` 已写入
[`records/`](records/README.md)，精确列出了计划路径、authority/data boundary、controls、外部 lane、历史
材料 disposition 和删除时机。用户于2026-08-27审查并接受review package commit `fe88f4e`。DEV-003已
通过G1–G6 repository evidence并以`79a1174`接受；DEV-001的设计评审和依赖回填均已完成，状态为`Ready`。
当前尚未生成CUDA source或public/restricted Intent corpus。为避免重复fixture schema，DEV-001必须直接消费
DEV-003已接受的`cairn-testkit` contract，不另建并行V1 fixture manifest。

首个新 increment 的输入不是某段旧 Phase G 未完成代码，而是：

```text
materialized D-039 source + public/restricted corpus
+ D-040 qualification contracts + independent golden/mutation/fault controls
+ sanitized D-041 fixtures + provenance manifests
+ current reusable runtime/record/execution foundations
+ current normative architecture
```

输出限定为：

```text
IntentHypothesisSet
→ independently derived RequiredIntentEvidenceSet
→ separate-process Intent Admission
→ MigrationIntentContract or explicit non-success outcome
→ one Oracle claim proposal
```

在该闭环完成前，不启动 Candidate Search，不以真实 NPU 等待掩盖软件 authority 缺口。
