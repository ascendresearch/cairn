# Cairn 当前实现与下一里程碑

- 状态：当前事实账本
- 日期：2026-09-01
- 作用：陈述代码与真实运行已经证明什么、尚未证明什么，以及下一里程碑的阶段划分与 Exit

本文不增加架构要求。目标设计见 `ARCHITECTURE.md`；历史 DEV 记录、试验结果和 superseded 设计通过 Git 历史追溯。

## 1. 结论

Cairn 已经具备可复用的 durable Agent runtime、typed record/replay、Worker scheduling/receipt 和一条部分接通的
CUDA→Ascend C workflow，但首个完整 migration package 尚未建立。

当前最强 live evidence 是：runtime model 面对未知任务产生过 task-generic intent/candidate proposal；exact candidate 曾通过
Controller 调度到远端 Ascend build environment；product-owned native gate 成功排除了 host fallback，但最新 native build
结果仍为 `SubjectFailed`。

**该 live 路径已不在当前代码树中，且是分两步失去的。** 2026-08-30 的 `ea25dcf` 移除了 native repair 路径，
同时引入通用的 `CandidateBuildPlanV1` 并在 `cairn-server` 中接通了它。次日的 `d699412`（refactor: make migration
workflow own agent loops）从 `cairn-server` 净删除约 9,500 行，把那两个消费者一并移除，
`CandidateBuildPlanV1` 自此没有调用者，`authorize_candidate_build` 只剩测试替身实现。

因此当前代码树在端到端能力上严格弱于 2026-08-29 的代码树，而 `scripts/ci.sh` 全绿，
因为真实 build/device 通道都是 `--ignored` 的显式 opt-in。这正是 `ARCHITECTURE.md` 3.6 所说的情形：
没有反向审计的绿灯不构成能力证据。失去通路的是一次以「转移所有权」为目的的重构，不是一次删除决定，
这也是 P0 第 3 项要把通路可达性做成 CI 断言的直接理由。

因此当前没有以下产品声明：

- native Ascend C build success；
- 950PR NPU execution 或 correctness；
- semantic、numerical、safety 或 performance Candidate Admission；
- qualified candidate family 或 dispatch；
- 可采用、可重放的端到端 migration package。

## 2. 证据口径

| 口径 | 含义 | 可以证明 | 不能证明 |
| --- | --- | --- | --- |
| live | 真实 model/provider、toolchain 或 Worker 执行 | exact run 的行为 | 其他 target/task 或完整产品 |
| recorded | 冻结 adapter/receipt 驱动 deterministic workflow | protocol、replay、failure handling | 新的模型质量或硬件事实 |
| local model-free | 本地 Gate、codec、store、runner control | mechanical policy 和 authority boundary | Candidate/Oracle 内容质量 |
| design only | 只存在类型、port、文档或 test skeleton | 预留的边界 | capability 已经可用 |

任何状态报告必须使用上述口径，不把 recorded 冒充 live、build 冒充 device run、合理 prose 冒充 correctness。

## 3. 已实现基础

### 3.1 Strong types、codec 与 record

- validated current-V1 identities、wire/storage codec 和严格反序列化；
- append-only event、content-addressed artifacts、SQLite persistence 和 replay/fault controls；
- task、episode、operation、job、attempt、receipt 和 revision binding；
- public/restricted material 的类型与入口边界；
- 日志 isolation 和稳定 operational fields。

### 3.2 Agent runtime

- domain-neutral model turn/tool operation/episode lifecycle；
- OpenAI-compatible、Anthropic 和 DeepSeek integration paths；
- structured submission rejection/repair、budget、continuation 和 restart；
- Controller workflow step 内共享 runtime，而不是独立 proposal service；
- exact tool request→durable operation authority→result→episode resume。

### 3.3 Execution foundation

- Worker enrollment、capability/resource facts、scheduler、lease、attempt 和 receipt；
- Docker、CUDA 和 Ascend build 的历史 live execution evidence；
- candidate-writable workspace 与 Worker evidence channel 的边界；
- product-owned Ascend build plan 能阻止 Candidate 通过 CMake host fallback 假装 native success。

已知失效项：`CandidateBuildPlanV1` 在当前树中无调用者（消费者由 `d699412` 移除），这是 P0 第 1 项。
引用已删除 test target 的三个 Ascend 冒烟脚本已一并移除；该 lane 由 P0 第 1 项与其脚本一起重建，
Git 历史保留其原调用契约。这两项都不在 `scripts/ci.sh` 的覆盖范围内，因此绿灯不代表通路存在。

### 3.4 Migration workflow pieces

- typed SDK、Unix-socket App API 和 reference CLI 的 submit/list/status/watch/cancel/review surface；
- task-owned Controller aggregate 和可读的 SIR→Intent→Oracle→Candidate stage ordering；
- caller decision request、typed user authority 和 independent intent admission；
- claim×concern×role Oracle ledger、deterministic/Agent strategy consumer 和 evidence experiment request；
- qualified Oracle mechanism runner contract、trusted receipt folding 和 model-free Oracle outcome；
- admitted-only Candidate workspace、proposal episode、product-owned build authority 和 mechanical candidate control matrix。

这些环节中相当一部分只由 recorded 或 local model-free controls 验证。App API 尚未把完整 normal path 组合到真实 local Worker、
qualified Candidate control runner 和 950PR execution。

当前 Oracle control runner 的旧实现只验证 plan digest、item binding、schema 和方法字段，不执行 mechanism 对 candidate 的
observation。该路径已改为 `SemanticExecutionUnavailable` 并映射为独立的
`OracleSemanticMechanismUnavailable`，因此不会再把结构自检发布成 semantic qualification。这是已关闭的不安全成功路径，
不是已经实现的 Oracle execution。

## 4. 已发生的 live 纵向证据

1. DeepSeek 对不同 task 读取 source/caller 后产生过 cited intent proposals；strict gateway 曾拒绝无效 submission，并在原
   continuation 中修复。
2. 一个此前未知的非 `vectorAdd` task 通过 normal CLI→server→migration workflow，运行了 SIR、administrator intent 和多轮
   Oracle development/review；Review 找到了行列索引交换、不可执行 observation、错误 launch assumption 和 tautological
   metamorphic comparison 等具体缺陷。
3. 该 dogfood 同时证明固定 claim×concern 展开缺少 applicability：不同 concern 产生重叠 items，而局部 Review 看不到跨 concern
   重复；结构 control 也不能执行 candidate-facing mechanism。系统因此 fail closed，没有形成 Oracle acceptance。
4. caller-authoritative decision 经 independent Intent Admission 形成过 exact contract；source observation 没有自动提升为
   desired semantics。
5. runtime model 在另一条历史 live lineage 中产生过 strict Ascend C/CANN candidate proposal，并通过 restart/continuation
   control。
6. exact candidate/revisions 经 Controller 和 remote no-device Ascend Worker 多次进入 native build/diagnostic/repair。
7. 一次 generic build 的 success 暴露了 host fallback；product-owned native gate 随后 fail closed。
8. 最新 exact native repair 仍为 `SubjectFailed`；它只证明当前 artifact 在 exact build environment 中没有编译通过，不证明
   semantic defect，也不构成 NPU evidence。

### 4.1 该次 `SubjectFailed` 的已归因根因

诊断正文可归因，结论不依赖推测：

- 链接器报告 kernel function 类型自动推导失败，要求显式标注 function type attribute；
- candidate 的 kernel 只使用 `GlobalTensor` 的标量取值与写值，未使用任何 vector 或 cube API，编译器因此无法推导
  该 kernel 属于哪一类核。**「最简单的正确性基线」这一策略本身直接产生了该编译失败**；
- repair episode 把原本正确的 `__aicore__` 改为不存在的写法，并在提交说明中自陈其 API 假设未经证实；
- 该 repair episode 只有「读任务原文」与「提交一次修改」两个工具，`execution_authority` 与 `automatic_iteration`
  均为 false，无文档访问、无 toolchain 头文件访问、无编译器访问、单轮不迭代。

因此该失败的 failure class 是 **platform fact gap 加 iteration budget**，不是 candidate semantic defect，
也不是 build plan defect。归因证据保存在对应 episode 的 durable store 中。

### 4.2 设备与工具链现状

- Ascend build worker 已 enroll 并多次真实执行，toolchain 为 CANN 9.1.0-beta.1，目标 arch 为 `dav-3510`；
- 该 worker 为 **build-only**：未声明 device 执行 capability，因此不存在任何 NPU 执行 evidence；
- 部署侧的 NPU 通道指向一台真实 950PR 主机，enrollment bundle 已签发，但最近一次连接为超时。
  因此 950PR 执行的阻塞项是**通道与 capability 声明**，不是硬件可得性。这一条改变阶段排序，见第 6 节。

## 5. 当前关键缺口

按产品价值排序。前三项由本文 4.1 与 4.2 的归因直接决定，与上一版排序不同。第 6 节的阶段划分即这些缺口的施工顺序：

1. `CandidateSearchLoopV1` 的 generation/action/immutable-state protocol 与 `ARCHITECTURE.md` 6.2 的 iteration policy；
   当前不存在 observation-bound 的 compile/run/diagnose/repair 循环；
2. target knowledge 与 skill 层（pack 导入、trust ledger、exposure 绑定、按 target 的投影）；
   代码树中不存在任何 Ascend C 领域知识，而 4.1 的失败正是 platform fact gap；
3. 恢复 normal path 上的 candidate 构建能力，并使「端到端通路断裂」成为 CI 可见的失败，而不是需要读 git 历史才能发现；
4. NPU worker 的 device capability 声明与 exact 950PR run、correctness observation 和 replay；
5. `ARCHITECTURE.md` 10.5/10.6 的运行时布局与项目工作区：三类材料分树、绝对路径、候选修订的 git 投影；
6. candidate-facing Oracle experiment/mechanism runner，形成真实 Worker receipt→Oracle Admission；
7. claim-scoped concern applicability/global coherence，避免固定矩阵重复展开和 case inflation；
8. qualified Candidate control runner 及真实 receipt→Candidate Admission；
9. 最小 Evidence/Assurance Graph consumer；
10. correctness-first candidate family、host tiling/kernel coupled search 和 target profiler feedback；
11. Development/Qualification separation、epoch invalidation、Candidate lifecycle/promotion 和 hidden exposure closure 的 live consumer；
12. actionable diagnostics、performance baseline 和 workload-aware promotion；
13. 至少三个 materially different tasks 的同路径复现；
14. migration package assembly、review commands 和 adoption evidence。

## 6. 下一里程碑：首个真实 package

在此里程碑完成前，暂停新增通用 Agent role、Admission kind、service、完整 graph topology 和兼容机制。
knowledge 与 skill 层不在暂停范围内：它是 P2，理由见本文 4.1。其 crate 边界见 P2。

阶段划分的依据是本文 4.1 的归因。该次失败的 failure class 是 platform fact gap 加 iteration budget，
因此在补上知识与迭代之前重跑纵向，只会重复产生同一类失败。规模标注是估计，不是已证明的事实。

### 6.1 阶段依赖

知识层与循环框架基本独立：前者是存储与门禁子系统，后者是工作流子系统，二者只在「循环把知识投影进 episode」
一处相接，因此排成两条并行轨道，在 P4 汇合。P0 是两条轨道共同的前置，因为当前代码树无法构建候选（见 3.3、4.1）。

```text
P0 止血 ─┬─ P1 运行时布局 ─┬─ P2 知识与 skill 层 ──┐
         │                 │                       ├─ P4 纵向 A ─ P5 纵向 B ─ P6 工作区 ─ P7 搜索与第二三任务
         │                 └─ P3 循环框架 ─────────┘
         │
         └─ 通道恢复（运维，外部依赖，只阻塞 P5）
```

P4 只需要 Ascend build worker，不需要 device，因此 4.2 的通道恢复不进入关键路径。P3 不依赖 P1，可与 P1 并行开工。

### P0 · 止血与清帐

当前代码树在端到端能力上弱于 2026-08-29 的代码树，而 `scripts/ci.sh` 全绿。先使该情形不可再现。

1. **构建半程已完成。** `prepare_generic_candidate_build_job` 现有非测试消费者
   `CandidateBuildRunnerV1`；`authorize_candidate_build` 物化并归档 exact build job，
   `observe_candidate_on_worker` 把它调度到 ordinary Worker 并折回 trusted receipt，
   产出带 job / attempt / request / receipt identity 与 outcome、exit code 的 typed observation。
   构建配方由 Controller 配置提供而非 Candidate 产出：Candidate 只贡献 `source/` 下的文件，
   `bin/run` 来自部署。Candidate 因此无法选择自己的构建路径，这正是区分 native build 与
   冒名的 host fallback 的那道控制。

   **观察半程仍 fail closed。** 拿到构建 receipt 之后，没有任何 qualified Candidate mechanism
   可以对已构建产物作出判断，因此返回 `CandidateMechanismExecutionUnavailable`。
   这与 Oracle 侧的 `OracleSemanticMechanismUnavailable` 同源，属于 P5 范围。
   在它接通前，本阶段能证明的只有「exact artifact 在 exact 环境中编译与否」，不构成任何语义结论。

   `record_terminal_outcome` 仍为 `NotImplemented`：候选终态相位尚未定义，
   在定义前记录终态会产生假报告。

   已先行完成的一步：删除 `continue_after_oracle_admission` 及其全部相关逻辑。
   该分支允许工作流在 Oracle Admission 之后直接产出终态而完全不经过 candidate，
   由 `MigrationCompletionTargetV1` 这一仅为测试 SIR 与 Oracle 而引入的开关驱动。
   它使 `run_cuda_migration` 对「完整产品工作流」的声明不成立，也是 `AGENTS.md`
   所禁止的那类只对 builder 开放的旁门。随之删除 `OracleWorkflowDispositionV1`、
   终态变体 `OracleAccepted` 及其构造器、server 配置中的 `completion_target` 字段。
   `MigrationTerminalOutcomeV1` 现在只有 `CandidateAccepted` 一个变体；
   它会在 `ARCHITECTURE.md` 11 的 fail-closed 终态落地时重新变宽。
   `record_terminal_outcome` 随之改为 `NotImplemented`，与其两个 candidate 侧同族方法一致——
   在候选通路接通之前，把候选终态标成 Oracle 相位是不诚实的。
2. **已完成。** 三个引用已删除 test target 的 Ascend 冒烟脚本随之移除；重建 lane 时脚本与 test target 一起产生。
3. **已完成。** 新增 `scripts/check-product-path.sh` 并接入 `scripts/ci.sh`，两项断言：
   每个 opt-in lane 引用的 test target 必须存在；`cairn-migration` 导出的 `pub fn` 中
   「在定义模块之外没有非测试消费者」的数量必须等于记录基线。基线是记录值不是目标值，
   只能经一次显式编辑改变，因此失去消费者与采纳孤儿都成为可见事件。刻意不用名字白名单，
   那种清单会静默变长。两半均已红验：制造悬空 lane 退出 1，摘掉一个真实消费者时计数从 22 变 23 并列出清单。

   开工时发现该缺陷的规模比预期大：仓库的三道文本门此前**全部是死的**。
   `ci.sh` 的行尾空白检查与 `check-log-isolation.sh` 的两项检查都依赖未安装的 `rg`，
   而写法均为 `if <command-not-found>`，条件为假因而静默通过。
   现已全部改用 POSIX 工具，并为每道门加上「依赖工具缺失即以 2 退出」的自检。
   复活 `check-log-isolation.sh` 当即查出 9 处在其失效期间累积的真实违规：
   `tracing::` 事件内的可失败 `?` 调用，分布在 `app_api.rs`、`oracle_control_runner.rs` 与
   `lib.rs`。已把这些计算提升到日志调用之前，而不是放宽该门。

   当前基线本身是一项事实：41 个导出函数中有 22 个没有跨模块的非测试消费者。
   `d699412` 让构建通路失去消费者不是孤例，而是这个 crate 的常态。
4. **已完成。** fixture 专用代码按 `AGENTS.md` 的裁决删除，而非迁移：reduction 四个模块、collection oracle、
   随之失去全部消费者的 historical-failure 记录模块、其测试与九个 fixture 二进制，以及 `call_adapter` 中
   fixture 形状的 `CollectionOutput` 变体及其三个函数。通用枚举的其余四个变体不受影响。
   净减约 7,400 行、82 个公开类型；`cairn-migration` 的公开 API 不再包含 fixture 身份。
   同时把误置于 fixture 模块中的通用类型 `MigrationIntentContractArtifact` 移入 `intent_claim`。

   一项连带删除需要记录去向：regression fixture `historical-false-reject` 的历史源指向被删测试，
   它记录的是单样本精确比较误拒合法求和顺序、以及 mutation grid 存在依赖用例的盲区。
   该 fixture 随其机制删除，但这两条教训已由 `EVALUATION.md` 5.1 的校准协议以规则形式承载
   （特异性、敏感性、最小可捕获误差量级）。丢失的是一个机器可检的控制，不是这条知识。
5. 恢复 NPU worker 控制通道并声明 device capability（运维项，与 P1–P3 并行）。

规模估计：第 5 项为运维。第 1 项的构建半程与第 2、3、4 项已完成。

已完成部分的附带发现：`scripts/ci.sh` 的行尾空白检查此前使用 `rg`，而该环境未安装 ripgrep，
`if <command-not-found>` 判定为假因而 `status` 保持 0——该检查从未真正运行过。已改为 POSIX `grep`
并把 `AGENTS.md` 纳入覆盖，且按纪律红验：故意引入行尾空白后检查确实失败，撤销后通过。
这是 `ARCHITECTURE.md` 3.6 所说情形的一个实例，也是 P0 第 3 项要覆盖的同一类缺陷。

Exit：一个候选经正常入口到达 Ascend build worker 并取回 typed 诊断（构建半程已具备，
待真实 worker 上验证）；删除任一端到端环节会使 `scripts/ci.sh` 失败；
`cairn-migration` 的公开 API 不再包含 fixture 身份。

### P1 · 运行时布局

纯搬运，不引入新语义。目标是 `ARCHITECTURE.md` 10.5。

1. `CAIRN_HOME` 解析与六棵树的绝对路径配置，取代当前相对当前工作目录的解析。
2. 把 `restricted/` 从 `secrets/` 拆出，把 durable state 移出 secret 树。
3. `log/` 不保留任何诊断正文落点；扩展现有日志隔离检查覆盖新布局。
4. 项目定义与 intake 冻结：`project.json`，含 `authored_by_agent` 与 `provided`。
5. 开发数据丢弃重建，不写迁移器（`AGENTS.md` 开发期规则）。

规模估计：第 2、4 项为中，其余为小。

Exit：三类材料分树且权限不同；worker 主机上不存在 `packs/` 与 `restricted/`；从任意工作目录启动解析到同一份状态。

### P2 · 知识与 skill 层（轨道 A）

目标是 `ARCHITECTURE.md` 7.4。本里程碑中最大的新建部分。

crate 边界已定：新建 **`cairn-knowledge`**，与 `cairn-agent`、`cairn-execution` 同为第 2 层的领域无关子系统，
依赖 `cairn-codec` / `cairn-protocol` / `cairn-record`。判据是该子系统的全部概念——entry、claim、provenance、
trust state、target 约束、exposure、pack revision——都不含迁移领域词汇；Ascend 相关内容以 pack 数据形式存在，不是代码。
`cairn-migration` 消费它做按 target 的投影，`cairn-cli` 直接消费它做 pack 与 trust 操作。

证据解析（判断一份 receipt 是否存在并支撑某条 claim）是一个 trait port，由 `cairn-migration-app` 注入实现。
`cairn-knowledge` 不得依赖 `cairn-execution`，否则该子系统会被抬到第 3 层并绑定到执行模型。
`ContentId<T>` 位于 `cairn-protocol` 且泛型于 `ContentType`，因此本 crate 可自带证据引用的标记类型。

1. pack manifest 类型与导入器：摄入 CAS、铸 pack revision、逐条摘要校验；启动时自动导入未入库的包，
   钉住的摘要与目录不符时报告漂移并 fail closed。运行时不从 pack 目录供应内容。
2. entry 类型 `platform` / `fact` / `case` / `skill`；`recipe` 只留槽位不实现。
   投影接口按 target 与 exposure 授权返回，不暴露全量枚举；索引与摘要常驻，正文按 content id 按需读取。
3. 信任账本：四态、三种证据形式、claim-scoped 裁决、绑定 content identity。
4. `applies_to` 按 `TargetPlatformContextV1` 过滤投影。
5. exposure 由 Controller 按 policy 绑定，pack 只能请求默认值。
6. 四个反向审计，读取端与审计端共用同一状态推导函数。
7. CLI：`pack import/list/verify/export`、`kb query`、`skill show`、`trust audit/set`。
8. 导入真实内容：外部 skill 快照、实测 target 事实、平台常量、裁决表、核心 skill、已验证参考内核作为 case。
   第 8 项受本节末第二个阻塞决定约束。

规模估计：第 3 项为大，第 1、2、6、7 项为中，第 4、5 项为小，第 8 项为搬运。

Exit：知识按 target 过滤后投影进 episode；任一 entry 字节变化使其裁决失效并退回未验证；每一道门可反向运行；
首批导入全部停在 `Unaudited` 与 `Reviewed`，这是预期状态而非缺陷。

### P3 · 循环框架（轨道 B）

目标是 `ARCHITECTURE.md` 6.2，包括其 Iteration policy。

1. `CandidateSearchLoopV1` 的 durable 状态机：immutable projection → episode → typed proposal → 验证
   → authorized effect → receipt 折回 → 下一 immutable state。
2. 五条迭代策略：迭代预算、重复动作检测、空提交按故障重试、预算临界通知、证据到达即固化。
3. **候选工具面**：读任务原文、读知识投影、在 build worker 上检索工具链头文件、提交修订、请求构建。
   最后两项直接对应 4.1 的已归因根因。
4. 诊断回灌分流：编译器诊断原样回传；profiler 输出先经确定性分析器转为结构化建议（`ARCHITECTURE.md` 7.3）。

规模估计：第 1 项为大，其余为中。

Exit：同一 task 内经过多轮 authorized compile→diagnostic→revision 且不离开 durable state；重复动作被检测并转为 typed input；
空提交计入预算而不被当作完成；Controller restart 后循环从 durable state 恢复。

### P4 · 纵向 A：normal-path native build success

汇合点。使用一个此前未知、无 framework 前提的 **elementwise** 算子：选它的理由是把风险集中在通路而不是算法上，
而通路正是本阶段要证明的对象。

1. 正常入口提交，runtime model 带知识投影，循环给足迭代预算；
2. 不允许 candidate 自有 build fallback、fixture 分支或 coding agent 代答；
3. 保存完整的 compile diagnostic 与 revision lineage。

自本阶段起，按 `EVALUATION.md` 第 6 节记录量测，不等到做对照实验时补记。时间一律从 CLI 提交被接受的 durable 时间戳
起算，包含 provider 排队、Worker 等待与失败尝试，并另报 active model / Worker / device time。

Exit：normal path 产生可重放的 native build success；replay 校验 exact artifact 与 toolchain binding。

### P5 · 纵向 B：950PR correctness

前置项是 worker 通道与 device capability 声明，不是硬件采购（见 4.2）。使用 `compact-above-f32`：
它已有 admitted intent 与一次已归因的失败构建，且其输出顺序未被 caller 声明，正适合检验「source 不是 specification」。

1. 声明并验证 NPU worker 的 device 执行 capability，恢复控制通道；
2. 接通 CUDA/reference input generation 与 950PR execution；
3. 差分 oracle 走 `EVALUATION.md` 5.1 的校准协议：噪声地板、退化地板检测、地板准确度上界、特异性、敏感性；
4. 在昂贵 item development 前完成 claim-scoped concern applicability 与跨 concern coherence；
5. 接通 `OracleExperimentRequestV1`→Controller authority→ordinary Worker→trusted observation→原 Agent episode；
6. Oracle qualification 接受 honest/correct variants 并拒绝 targeted mutant/negative controls；
7. qualified Candidate runner 对 exact candidate 执行已通过 meta-qualification 的 mechanism；
8. failure class 分流：execution failure、candidate defect、Oracle defect、platform fact gap、intent ambiguity。

Exit：同一 artifact 在 exact 950PR 上运行；至少一个 Oracle mechanism 实际判断 candidate observation，而不是检查 plan prose；
model-free Oracle/Candidate Gate 从 trusted receipt 重算至少一条 required claim；测量携带设备占用状态
（`EVALUATION.md` 6.6）。

### P6 · 工作区与交付面

目标是 `ARCHITECTURE.md` 10.6。

1. 候选修订 DAG 单向物化为 git commit DAG，trailer 携带 task / candidate revision / parent revision / episode identity；
2. 分叉检测：工作区被外部改动时物化拒绝而不是覆盖；
3. 测量与 profiling 投影，不进入 commit tree；
4. 导出：migration package、patch、replay bundle。

Exit：从任一 commit 能重组 bundle 并重跑；`git diff` 精确等于该次候选的代码改动；物化产物不含 qualification oracle，
且该断言是对产物的机械断言而不是一份排除清单。

### P7 · 搜索与第二、第三任务

1. 保留 correctness baseline；先把串行深度与知识做够，再引入种群搜索；
2. best-of-N 与一个有界 population/beam 的同预算对照，按 `EVALUATION.md` 4.2 记录；
3. 分别搜索 host tiling/data movement 与 device kernel/schedule，将 compiler/profiler observation 转为可验证的 action guidance；
4. 第二个任务为 reduction 或数值敏感算子，第三个为 layout/indexing、atomic、stateful 或 concurrency 算子；
5. package 组装、review 命令与 adoption evidence。

Exit：至少一个 candidate revision 在 required non-regression 后获得可重复的 target-side improvement，或系统诚实证明预算内无改进；
三个任务无 product-code 或 prompt 特例；至少一个任务形成完整 package，其余可诚实终止为 partial 或 rejected，
但必须保留 exact failure attribution。

### 本里程碑的自我约束

两条约束针对本轮评审识别出的两种复发模式，适用于全部阶段：

- **知识层不得扩张为平台。** 一条知识是否结晶，先过四问：是否已被记在对的读者会看到的地方、是否跨任务复现过、
  对的读者是否会在对的时刻读到、是否值得它占用的噪声代价。任一问模棱两可就留在 `case` 里。
  语义检索、自动聚类与 `recipe` 层在本里程碑内一律不做。
- **未执行过的阶段先不铸类型。** 强类型守在有 receipt 的边界上：identity、content id、capability、receipt、revision。
  没有运行时数据流经的类型不构成验证，且会在该阶段首次真实运行时被重写。
- **Exit 必须可证伪。** 一条读起来「怎样都算过」的 Exit 与一道不能被诚实满足的 gate 是同一件事的两面：
  前者总会被宣布达成，后者总会被绕过去（`ARCHITECTURE.md` 3.6）。发现这样的 Exit 应当先改写它，而不是先开工。

### 阻塞决定

以下四项需要 administrator 决定；在决定作出前，对应工作不得按便利默认推进。

已决：知识层新建 `cairn-knowledge`，理由与边界见 P2。暂停令中的「新增 crate」约束的是过早泛化领域机制，
不约束被领域消费的中立子系统；该形状与既有且承重的 `cairn-agent`、`cairn-execution` 一致。

| 决定 | 阻塞对象 | 保守边界 |
| --- | --- | --- |
| 外部 skill 快照的再分发条款与发布位置 | P2 第 8 项对外发布 | 决定前只能本地导入，不对外分发 |
| fixture 专用代码迁出后若通用机制依赖它 | P0 第 4 项范围 | 按 `AGENTS.md`，该依赖本身即「机制尚未 task-generic」的证据；若牵动过深则单独立项，不阻塞 P0 其余项 |
| P4 使用哪个算子 | P4 开工 | 建议此前未知的 elementwise 算子；续用 `compact-above-f32` 会在首次证明通路时引入非必要的算法难度 |

## 7. 之后的优先级

首个 package 后依次：

1. knowledge 各 trust state 与 retrieval 策略的消融（pack 本身已提前到 P2，见第 6 节）；
2. hidden/mutant/source-defect controls，证明 assurance 相对简单 harness 的增量价值；
3. 20-task evaluation corpus 与 strong baselines；
4. adaptive co-design 与 up-front structured workflow 的同预算比较；
5. 50–100 task 扩展、生产 workload traces 和 adoption/merge evidence；
6. 有真实 consumer 后再扩展多 target、多 kernel graph 或应用级 migration。

## 8. Quality gates

普通提交必须通过：

```bash
scripts/ci.sh
```

聚焦 migration 变更至少运行：

```bash
cargo test -p cairn-migration -p cairn-migration-app --all-targets --no-fail-fast
cargo clippy -p cairn-migration -p cairn-migration-app --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

真实 provider、CUDA、Ascend build 和 950PR lanes 显式 opt-in。没有对应 capability 时终止为
`RequiredCapabilityUnavailable`，不能使用 shell、recorded receipt 或 simulator 代替。

## 9. 更新规则

- 每次合并 material product slice 时直接更新本文的事实、缺口和下一里程碑；
- 不新增 DEV-NNN Markdown、完成性审计、session handoff 或历史结果目录；
- 详细实现原因写入清晰的 commit message、代码、tests 和 durable run artifacts；
- 实验原始 artifact 留在 runtime store/CAS 或外部 artifact bundle，不提交到 `docs/`；
- superseded 事实直接删除或改写，历史由 Git 保存；
- 本文不能把目标设计、recorded control 或测试 helper 写成已实现产品能力。
