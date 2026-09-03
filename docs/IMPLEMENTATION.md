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

- typed SDK、Unix-socket App API 和 reference CLI 的 submit/list/status/watch/cancel/review surface，
  提交走一条声明—分块—提交的上传会话，因此源码树的大小不再受单帧上限约束（见 5.1）；
- task-owned Controller aggregate 和可读的 SIR→Intent→Oracle→Candidate stage ordering；
- caller decision request、typed user authority 和 independent intent admission；
- claim×concern×role Oracle ledger、deterministic/Agent strategy consumer 和 evidence experiment request；
- qualified Oracle mechanism runner contract、trusted receipt folding 和 model-free Oracle outcome；
- admitted-only Candidate workspace、product-owned build authority 和 mechanical candidate control matrix；
- Candidate exploration episode：工具面、指令、gateway 与 executor（P3 第 3 项的第一半，见 6.1 之后的 P3 记录）。

**上一版此处把「proposal episode」列为已实现，这是高估。** 直到 2026-09-03，三个 candidate 角色
（exploration / review / revision）的 executor 全部是 `unavailable_role_executor!` 展开的桩，一律返回
`RoleNotImplemented`。也就是说候选阶段从未向模型发出过一次调用：工作流走到 `run_candidate_exploration_loop`
就会失败。按第 2 节的口径这是 **design only**，不是 recorded。exploration 一路现已实现，另两路仍是桩。

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

### 4.2 长期运行部署暴露的 Controller 稳态成本缺陷

一个自 2026-08-26 起持续运行的 Controller 进程（无 Worker 连接、无任务）在 6.8 天内：

| 量 | 观测值 |
| --- | --- |
| 累计 `rchar` | 24.15 TB（平均 41.2 MB/s，全部命中页缓存） |
| 累计读系统调用 | 5.9×10⁹（约 10,000 次/秒） |
| 稳态 CPU | 单个 tokio worker 线程常驻 45–60% |
| durable event store | 42 MB |

`read_bytes` 仅 16 KB，说明并非磁盘 I/O 而是对已缓存事件存储的反复读取。

机制已定位到具体形态，而不是某个函数的缺陷：**控制面投影没有快照，每一次操作都重放整条聚合流。**
`cairn-execution/src/control.rs` 中有 10 处 `read_stream(&stream, None)`，全部以 `None` 作游标。
会话循环按 `min(outbox_poll_interval_ms, authority_poll_interval_ms)`
每 100 ms 推进一次，每次推进都重放一遍 outbox 流。

同一部署的事件计数说明了代价来源：

| 聚合流 | 事件数 |
| --- | --- |
| `execution-worker` | 28,186 |
| `controller-control-outbox` | 9,434 |
| `execution-job` | 44 |
| `worker-enrollment-registry` | 10 |

事件计数的分布确实由心跳主导，但**成本归因不能从计数推断，必须测量**。2026-09-02 用当前构建替换
该进程后重测，结果推翻了先前的归因：

| | 读 syscall | rchar |
| --- | --- | --- |
| 旧构建，单会话 | 10,049 次/秒 | 41.2 MB/s |
| 当前构建，两会话 | 10,545 次/秒 | 43.2 MB/s |

会话游标改动没有改变速率。按字节量重新推算才对上：

| 聚合流 | 事件数 | payload |
| --- | --- | --- |
| `execution-worker` | 28,294 | 7.59 MB |
| `controller-control-outbox` | 9,434 | 2.26 MB |
| 其余全部 | 118 | 0.07 MB |

热点是 **outbox 流的重放**：会话循环每 100 ms 调用一次 `deliver_controller_messages`，
它以 `None` 作游标重放整条 outbox 流。2 会话 × 10 Hz × 2.26 MB = 45.3 MB/s，与实测 43.2 MB/s 吻合。
心跳路径每 30 s 才触及一次 worker 流，改动前的贡献约 0.5 MB/s，占比约 1%。

会话游标那次改动因此是正确但不解决瓶颈的：它移除了心跳的 O(N) 重放，会随 worker 流增长而变得重要，
但当前的 41 MB/s 从来不是它造成的。

按测量结果给 outbox 投影加同一形态的游标后，实测（同为两会话）：

| 构建 | 读 syscall | rchar | 进程 CPU |
| --- | --- | --- | --- |
| 旧构建（单会话） | 10,049 次/秒 | 41.200 MB/s | 45–60% |
| 会话游标（两会话） | 10,545 次/秒 | 43.183 MB/s | 20% |
| outbox 游标（两会话） | 312 次/秒 | 1.271 MB/s | 1.5% |
| 同上，改用无启动窗口重测（两会话） | 2.5 次/秒 | 0.008 MB/s | 1.4% |

依据的不变量是：outbox 投影是该流的纯函数，因此「无事可投」的结论在流前进之前不会改变。
`controller_outbox_position` 只读游标之后的部分，为空即跳过整次重放。

**第三行的测量方法有缺陷，第四行才是同一份代码的稳态。** 前三行的速率取自 `/proc/PID/io`
的累计计数除以进程运行时长，而该窗口包含进程启动时的一次性重放。这个偏差有多大是可直接测量的：
当前进程运行 961 秒、累计 rchar 152.3 MB，而稳态速率（60 秒与 300 秒两个独立窗口互相吻合，
均为 8.6 KB/s）在 961 秒内只能解释 8.0 MB——累计计数的约 95% 是启动重放，与稳态无关。
因此凡按累计量除以运行时长得到的速率，量的是启动，不是稳态。第四行改用两个不含启动的窗口重测。

这个偏差不影响前两行的结论：41 MB/s 那一档由独立的字节量模型佐证
（2 会话 × 10 Hz × 2.26 MB = 45.3 MB/s，与实测吻合），且观测期长达 6.8 天，启动占比可忽略。
受影响的是第三行，以及基于它的那条归因。

**随之作废的归因。** 原先把残余的 1.27 MB/s 归给仍以 `None` 为游标的 `EnrollmentRegistry::load`。
稳态实测推翻了它：会话循环在跑（环回链路 21.5 KB/s，两条 ESTABLISHED 连接），而读 syscall 只有
2.5 次/秒，远低于循环频率——每轮的注册表读取根本没有到达文件层。这与 `journal_mode = WAL`
下 SQLite 用页缓存服务重复扫描一致：outbox 重放停止后，工作集不再把缓存冲掉。
`EnrollmentRegistry::load` 的成本因此是 CPU 与内存，不是 I/O；它仍应加游标，但理由是
随流增长的解析开销，不是这里记错的 1.27 MB/s。

**这是同一类错误的第二次。** 第一次是从事件计数推断成本而没有测量；这一次是测量了，但窗口选错，
把一次性启动算进了稳态速率。两次的共同点是：拿到一个数就用，没有先问这个数在量什么。
对应 `EVALUATION.md` 6.6 的 measurement validity。

### 4.2.1 阻塞：同步存储运行在异步运行时线程上

`ControllerState` 持有 `SqliteEventStore` 与 `SqliteContentStore`，两者都是同步的，而 `schema.rs` 把
`synchronous` 设为 `FULL`，因此每次 append 都是一次 fsync。发现问题时，会话路径的每一次 `read_stream` /
`append` 都直接在 `async fn` 内执行，`cairn-server` 中 `block_in_place` 与 `spawn_blocking` 的出现次数为
**0**；同一工作区的 `cairn-migration-app` 有 11 处。该模式在本仓库内已知，只是没有应用到 Controller。

叠加的第二层是 `tokio::sync::Mutex<ControllerState>`：一把全局锁把所有会话的控制面操作串行化，
共 11 处获取点。这解释了观测现象——负载并未分散到 17 个运行时线程，而是持锁者独自跑满一个线程。

三个设计决定现已全部落地，按依赖顺序：

| 决定 | 处置 | 证据 |
| --- | --- | --- |
| outbox 投影引入游标 | 已实施 | 4.2 表：312 次/秒、1.271 MB/s、CPU 1.5%，实测 |
| 去掉全局锁 | 已实施 | 既有测试 `concurrent_sqlite_placements_cannot_overcommit_quantitative_capacity` 证明并发安全不依赖这把锁：两个 OS 线程、两条连接，落败方由 `RevisionConflict` 拒绝 |
| 同步存储移出运行时线程 | 已实施 | 具名 `on_store` 包裹 Controller 11 处、Worker 16 处同步段，两侧均覆盖整个会话生命周期 |

同一缺陷在 Worker 侧更重，因此一并修：`cairn-worker` 只有 1 处 `spawn_blocking`（执行任务派发），
而它的 journal、content store 与物料落盘全部直接跑在 `async fn` 内。物料落盘尤其关键——
分块追加与 `content.put` 的全量哈希都在会话循环所在的运行时线程上，对一个数 MB 的 bundle
就是整段传输期间占住该线程。P4 的构建流量正是走这条路径。

`spawn_blocking` 在此不可用：这些段持有 `&mut ControllerState`，无法跨越 `'static + Send` 边界，
因此适用形式是 `block_in_place`。它要求多线程运行时，在 current-thread 运行时上会 panic——该隐患经
红验证确认为真实而非理论：把一个测试的 flavor 改为 current_thread 即可立即复现。`main.rs` 没有任何
测试覆盖，两个集成测试各自钉住自己的 flavor，因此 flavor 变更只会在生产暴露；
`scripts/check-product-path.sh` 现在拒绝 `cairn-server` 与 `cairn-worker` 中出现 `current_thread`。
两个 crate 都只被自身 `main`（`#[tokio::main]` 默认多线程）和 `cairn-server` 的多线程集成测试使用，
工作区内其余 current-thread 测试所在的三个 crate 都不依赖它们。

去锁与移出运行时线程在当前空闲负载下**均无可测量差异**，这与预期一致：空闲会话几乎不提交。
进程 CPU 1.5% → 1.4%，落在噪声内；这是 4.2 那组数字里唯一未被窗口选择污染的一项。
可见的变化只有分布：CPU 不再由单个 tokio 线程独占，而是散在多个线程上（0.3/0.2/0.2/0.2/0.1%）。
两者改变的是负载下的行为，不是稳态数字。真正的验证要等 P4 的构建流量。

liveness 已离开 durable 事件流，与 progressing 信号、`last_seen_at` 的语义拆分一并完成，见 4.2.2。

这是单元测试看不见、只有长期真实部署才会暴露的一类缺陷，属于 `EVALUATION.md` 6.4 的 system metrics 范畴。

### 4.2.2 liveness：从算术改为观察

会话此前有两种彼此无关的结束方式。Controller 关闭连接时记录一次 disconnect；与此并行，**每个读者**
各自重算 `last_seen_at + session_timeout`，超过就宣布过期。后者正是把 keepalive 逼进 durable 流的原因：
算术需要一个不断前移的时间戳，于是每次心跳都要追加一个「worker 还在」之外没有任何内容的事件。
一个连续运行一年的 worker 会留下数百万条，而它的每一次完整投影都要重放全部。

改动后 liveness 是一条被记录的事实：会话在有事件说它结束之前一直存活，而只有持有 socket 的 Controller
能看见它结束，因此也只有它写这条记录。心跳若报告的 availability 与日志中已有的一致，则不追加任何事件——
Controller 在自己内存里的会话上记下这次到达，「谁还在场」这件事本来就属于持有连接的进程。
availability 真正发生变化时仍然追加，因为那是 scheduler 要读的证据，不是出勤证明。

崩溃的情形在能回答它的位置回答：新启动的 Controller 对任何早于自身启动的 incarnation 都不持有 socket，
因此日志里所有仍然打开的会话都已经结束，它在绑定监听器之前把这些结束逐一记录下来。

| 删除 | 原因 |
| --- | --- |
| `WORKER_REPLACED` 及其 predecessor-expiry 校验 | 没有算术过期后，前任要么存活（拒绝），要么已结束（普通注册），不存在第三种 |
| `WorkerSessionState::Expired`、`expiry_at` | 过期不再是一种状态 |
| `WorkerSessionTimeoutMillis` 及贯穿注册、恢复、池分配、两个 policy 和 server 配置的 `session_timeout` 参数 | 失去全部消费者 |
| `idle_timeout_ms < session_timeout_ms` 这条时钟关系 | 它存在只因两口钟管同一件事；现在 idle timeout 是唯一的界 |

这条改动带来一个此前隐含、现在承重的前提：**同一份 store 只由一个 Controller 进程服务**。
启动扫除对「早于本进程的 incarnation 一律已结束」的推断，只有在没有第二个进程同时持有 socket 时才成立。

一处实施缺陷由测试抓出，值得记录：扫除最初写在 `ControllerState::open` 里，而该函数是**每条连接**调用一次的。
那样既不会在启动时执行，真执行起来还会在每次有 worker 连入时注销其它所有 worker 的会话。
在此之前没有任何测试覆盖启动扫除——去掉它 CI 依然全绿——这正是本仓库已栽过两次的那类缺陷，
因此补的是一个 staged orphaned session 的集成测试，而不是一个单元测试。

事件载荷形状已变，既有开发期 store 不可读，按 `AGENTS.md` 丢弃重建而不迁移。
该重建已于 2026-09-02 执行：controller store 清空，两台 worker 重新 enrollment，
三个进程升级到 `73326fb`。重建前对 controller store 与两个 worker state 做了完整备份并校验可解包。
worker 的 `worker.json` 原样保留，因此 backends 与 capabilities 未受影响；
清掉的是各自的 identity、journal 与 content——它们引用的 credential 与 incarnation 已随 store 一并作废。
重建后注册表只剩两个在跑的 worker，此前 5 条登记里的 3 条陈旧记录随之消失。

**线上直接验证了这次改动要达到的效果。** 两个 worker 心跳周期均为 30 s，
稳态下 6 分钟观测事件总数 8 → 8，**增长为 0**；旧设计同期会追加 2 × 12 = 24 条心跳事件。
每个 worker 流现在恰好 2 条事件：注册，以及第一次携带新 availability 的心跳。

### 4.2.3 运行时布局：由 bootstrap 拥有

七棵树来自 10.5,但**创建它们的是一条命令,不是运行时的校验**。这一节记录的是这个结论的来路,
因为最初的实现走了相反的方向。

最初我把布局做成一个会在运行时校验自身的子系统:树按角色限制谁能命名、任何一棵不得嵌套在另一棵内、
路径不得逃出所属的树。这些检查合起来 300 多行,而且**都在检查同一份代码刚刚创建出来的东西**。
真正促成它们的那个缺陷——durable state 位于 secret 树之下——恰恰是因为当时没有创建命令、
全靠手敲才出现的。也就是说它是 bootstrap 的证据,不是校验器的证据。

现在的形态:

- `cairn-server bootstrap <目录> <server-name> <control> <enrollment>` 建树、按材料类设权限、
  生成自签 CA 与控制器身份、写出配置,create-only,拒绝并入已有目录。
- `cairn-worker join <bundle> <目录>` 对 worker 做同一件事,树表来自同一处,
  `packs/` 与 `restricted/` 不在 worker 的表里——这就是「被判方不与判官共处一台主机」在这里的落实方式。
- `cairn-layout` 只剩树名与权限位,105 行,没有任何运行时校验。

配置里的相对路径按**配置文件所在目录**解析,绝对路径原样使用。单根部署因此可以整体搬走而不改一行,
系统级安装用绝对路径。这是这个仓库原本就有的规则,中途被换成需要一个 crate 去解释的东西,现在换了回来。

**测的是端到端那一条**:跑真实的 bootstrap 命令,再用它产出的东西跑真实的服务。断言目录存在等于检查
自己刚创建的东西。红验证方式是从树表里去掉一棵,测试立刻失败。

同时删掉的还有 log 树那道门。它断言一棵零文件零消费者的树消费者数为零,今天什么也没保护;
真正在挡诊断正文进日志的是对 tracing 事件本身的隔离检查,那个留着。

一处是设错而不是过度:controller 的 unit 此前只对三棵树开放写入,把 `restricted/` 留成只读,
而 Admission 侧就是 controller,exposure ledger 要写在那里。已修正。

**部署过程中暴露的两个脚本缺陷,都是用它才发现的。**

其一,「不得覆盖未托管进程」的守卫用 pgrep 的全命令模式去匹配配置文件路径,
于是任何命令行里提到该路径的进程都算——包括调用部署脚本的那个 shell。它拒绝了一次完全干净的安装。
改为先按进程名精确匹配再核对命令行,shell 不可能满足。两个方向都验过。

其二,解释上面那次修复的注释,把旧机制写在了反引号里。远端脚本是未加引号的 heredoc,
反引号在其中就是命令替换,于是每次渲染脚本 bash 都真的执行一次 `pgrep -f`。
错误可见但看起来无害,而它悄悄把那条检查清空了。**heredoc 里的注释不是注释。**

还有一处配置认知错误:bootstrap 把给它的地址同时用作监听与对外通告,而这个部署走反向隧道,
两者不同。第一次 worker enrollment 因此指向了 worker 主机上不存在的端口。bootstrap 的提示已说明这一点。

**现状**:三个进程都在 `e5ea2e2` 上,部署由 bootstrap 产出,两个 worker 经 `join` 一条命令建成后
合入各自的 profile。生产重启验证通过:两条会话被记录为结束,两个 worker 在 1.2 秒内重新注册。

### 4.2.4 一处误放的受限语料

`fixtures/cuda-ascend/intent/reduce-sum-f32/v1/` 是一份冻结语料的**公开半边**,在仓库里受版本控制,
含真实 CUDA 源码、`public-corpus.json` 与 `restricted-partitions.public.json`。它的**受限半边**
是 8 个隐藏用例,每个带确切输入位、期望判决与该用例针对的缺陷类型,manifest 上写着
`exposure_policy: sealed-until-disclosure`。

我曾把这 8 个文件搬进生产部署的 `restricted/` 树,依据是它们此前躺在一个叫 `secrets` 的目录下、
而 10.5 说 restricted 不能放在 secret 树里。那是按**目录名与材料类标签**行动,没有先确认它们是什么:
它们的内容类型是 `testkit.*`,唯一消费者在 `cairn-testkit`,运行时没有任何代码读它。
现已移回与公开半边路径镜像的位置并 gitignore:

```
fixtures/cuda-ascend/intent/reduce-sum-f32/v1/     公开半边,在 git 里
.cairn/corpora/cuda-ascend/intent/reduce-sum-f32/v1/   受限半边,不在 git 里
```

**为什么怕它被读,值得写下来**,因为它决定了该建什么控制。这些不是保密意义上的秘密,是**测量意义上的**:
看见输入就能特判;光看见「本用例针对丢输入」就足以让模型避开那一个缺陷而留下整类不被测到;
而且泄露不可逆——私钥泄露了可以轮换,一条用例只能重写并记下旧的已烧毁,这正是设计里要有 exposure ledger 的原因。

据此重排泄露路径后,结论是**目录权限守的是最弱的一条**。候选跑在 worker 主机的容器里
（`--read-only --network none --cap-drop ALL --user 65532`,只有 input 只读挂载和 output 两个挂载）,
它够不着 controller 上的任何东西。真正需要控制的是两处:每个工具绑定的根,以及物化进 bundle 的内容。

### 4.2.5 工具根与物化断言:一个落空,一个落地

**工具根落空了,原因值得记。** 我把 workspace 描述成「Agent 的世界」,再去担心里面放了什么——
这个危险是自己造的:要读原始 CUDA,交出去的应该是 `source/` 而不是 workspace 根。
按这条思路去给工具注册加「根」时才发现:**没有任何工具接受路径参数**。
`ReadTaskArtifact` 按内容标识从 store 读 artifact,`SubmitCandidateRevision` 提交 typed proposal,
一个文件面都不存在,每处 binding 的值都会是 `None`。为不存在的设计搭机制正是要避免的事,所以没有加。
留下的是 `WorkspaceArea`,因为**建立工作区是一件真实的事**,`cairn-server project init` 做这件事:
五个 area 在任何东西写进去之前就存在,理由和部署的七棵树一样——建目录的人决定它是干什么的,
留给第一个写入者决定就等于没人决定。

**物化断言落地了,而且它本来就成立,只是没人说出来。** build bundle 只读挂在候选自己的 `/input` 上,
凡是进了 bundle 的东西就等于交到了候选手里,容器隔离在这里帮不上忙——是我们主动递过去的。
挡住 oracle 材料的是:组装只从两处取料——Controller 提供的构建配方,和候选自己提交的文件,
因为那就是那个函数的全部入参。

现在这条被钉住了:产出的 bundle 里每个文件都必须能被这两个来源之一逐字节解释,
多出第三个来源会变成一个点名路径的失败测试,而不是一次没人注意到的泄露。
红验证方式是往组装里塞一个 oracle 期望值文件,测试立刻抓住并点名。

这正是 10.6 要的「对物化产物的机械断言,而不是一份需要维护的排除清单」:
排除清单必须每次都对,而这个是构造上就对的,测试的作用是让构造保持诚实。

exposure ledger 仍未实现。

### 4.2.6 时钟:一个被抹平的方向,一个不可满足的默认值

部署过程中量到 NPU 主机比 Controller 慢约 93 秒（`chronyd` active 但未同步,共享主机,未擅自步进）。
追下去发现的是 `admitted_resource_observation_time` 的一处不对称:
`resource_clock_skew_tolerance_ms` 只约束 worker **超前**;worker 落后时函数直接返回 Controller 自己的时间。
落后方向不是没有上限,而是**被抹掉**——一次 93 秒前的测量会被盖上「此刻」的时间戳。
对排序而言这可辩护,但它使 quantitative freshness 失去依据:
`match_quantitative_resources` 拒绝在求值时刻已陈旧的证据,而陈旧性此时已不可见。

用 bootstrap 重建部署后碰到了同一个函数的另一面。GPU worker 连不上,
因为示例配置里该容忍度为 `null`,语义是「worker 时钟一毫秒都不许超前」,而它领先 1 到 2 毫秒。
**这个默认值两台机器之间不可能满足**,此前之所以没出问题,是因为旧部署带着一个早于示例的显式取值。
示例与线上均已改为 2000 毫秒。

两面同源:落后被静默抹平,超前的默认值严到无法达成。真正修它应与 device capability 声明（P5）
一并考虑,因为依赖 quantitative freshness 的是那条路径。

### 4.3 设备与工具链现状

通道已于 2026-09-02 恢复并核验。地址、凭据与 enrollment 材料属于 Secret provider，不进入本仓库。

| 事实 | 观测 |
| --- | --- |
| NPU 主机 | 共享，8 张 `Ascend950PR`，驱动 `25.7.rc1.6`；本次观测中 2 张 health 为 `Critical`，多张已被他人占用 HBM |
| NPU 主机 toolchain | 宿主 CANN 8.5.0；构建镜像内为 **CANN 9.1.0-beta.1**，与 4.1 诊断中的版本一致 |
| GPU 主机 | 独立主机，`NVIDIA GB10`，worker 自 2026-08-26 持续在线 |
| 注册 worker | 5 个：`gpu` 2、`npu` 2、`npu-build` 1 |
| Ascend build worker | 已重新上线并注册，backends `docker-v1`，capabilities `execution.role=build`、`toolchain.vendor=ascend`、`toolchain.architecture=dav-3510`、`toolchain.cann=9.1.0-beta.1` |

三个进程已于 2026-09-02 全部由 systemd 管理，并统一部署到同一个提交：

| 进程 | 主机 | scope | unit | 目标环境 |
| --- | --- | --- | --- | --- |
| Controller | 本机 | user（已开 linger） | `cairn-controller-real.service` | x86_64-gnu |
| Ascend build worker | NPU 主机 | system | `cairn-worker-npu-build.service` | x86_64-**musl** |
| CUDA worker | GPU 主机 | user（已开 linger） | `cairn-worker-gpu.service` | aarch64-gnu |

三台主机的布局一致：`<prefix>/versions/<commit>/` 为不可变版本目录，`<prefix>/current` 是符号链接，
回滚是一次链接翻转而不是又一次传输，unit 文件不随版本变化。部署由 `scripts/deploy.sh` 从 release
bundle 完成，见本节末。

此前每个进程都是手工 `setsid` 加重定向启动的：没有东西负责重启，退出原因不被记录，
「哪台机器在跑什么」只存在于 shell 历史里。已实测该性质：对构建 worker 发 `SIGKILL` 后
systemd 在数秒内拉起新进程，`NRestarts` 计为 1，Controller 随即收到重新注册。
本次迁移期间 Controller 观察到的每一次断连都能对应到一个具体操作，worker 重连耗时 2 至 5 秒。

Controller 的 unit 采用 `ProtectSystem=strict` 并只对其 state 目录开放写入；worker 的 unit
**刻意不加固**，因为 worker 要 exec 容器工具并按配置选择物料落盘路径，沙箱指令必须先在真实构建上
验证，未经验证地加上去会以任何测试都覆盖不到的方式破坏构建通路。

两者的 unit 都限制了重启次数。worker 在自己的会话循环内部重连，断连根本不会到达 systemd，
因此进程退出一定是真故障；不限流的话，一个坏部署会表现为一个看起来正在运行的无限循环。

**发布清单与实际部署不符，这次升级把它暴露出来了。** `release/toolchain.toml` 声明 targets 为
两个 gnu 三元组、glibc 下限 2.28，但两台主机要的不是同一样东西：GPU 主机（aarch64、glibc 2.39）
的 worker 配置要求 `aarch64/linux/gnu`，NPU 主机（x86_64、glibc **2.34**）的配置要求
`x86_64/linux/musl`。清单里没有 musl 目标，因此按清单构建产不出 NPU 主机能用的二进制。
已补上 `x86_64-unknown-linux-musl`。

代价是真实的：直接用本机 `cargo build --release`（glibc 2.43）产出的二进制在 NPU 主机上以
`GLIBC_2.38 not found` 立即退出，构建 worker 因此短暂离线，用旧二进制恢复后才走通正确路径。
worker 自带的 `expected_platform` 门在第二次尝试中拒绝了 gnu 构建，这道门是对的——
它把一次「能加载但环境不符」变成了一次明确失败，而不是一次可疑的运行。

**发布通路本身是活的，缺的只是 musl 目标。** `scripts/build-release.sh` 实现了清单，
并接在 `.github/workflows/ci.yml` 与 `release.yml` 上，后者还做两次独立构建并逐字节比对以验证可复现；
`.github/actions/setup-release-toolchain` 按钉住的版本装工具链。该脚本对每个产物校验 machine、
interpreter、glibc 上限与动态依赖白名单，并产出 `BUILD-METADATA.json`、`SHA256SUMS` 与可复现 tar。
它的缺口是显式的：`unsupported release target` 会拒绝 gnu 之外的任何目标，因此产不出 NPU 主机要求的
musl 二进制。清单与脚本彼此一致，只是相对部署都不完整。本次手工拼命令是在绕开这个缺口，
不是在替代一个不存在的脚本。

**反向隧道现在也由 systemd 看管。** 本机两个 user unit（`cairn-tunnel-npu`、`cairn-tunnel-gpu`）
维持到两台 worker 主机的 `-R 7443/7444`。此前它们由交互 shell 起,shell 一走隧道就断,
而断了之后两个 worker 全部离线、控制器侧只看到「没有连接」——一次静默的全系统中断。
已实测自愈:`kill -9` 掉隧道后 systemd 数秒内重建,worker 自行重连。

这两个 unit **不进仓库**:`ARCHITECTURE.md` 10.1 明确说 SSH reverse tunnel 不是产品拓扑,
它们是本地运维脚手架。记在这里是为了让接手的人知道它们存在、以及为什么不在代码里。

两点结论：

- **没有任何 worker 声明 device 执行 capability。** 两个 `npu` 池 worker 的 profile 是 `transport-only`
  且 `execution.mode` 为 `disabled`。因此 950PR 执行的阻塞项不是硬件可得性，也不再是通道，
  而是**尚未创建的 device worker 声明**，属于 P5 范围。
- **当前部署拓扑与 `ARCHITECTURE.md` 10.1 不符。** Controller 仅监听回环地址，worker 经 SSH 反向隧道到达；
  而 10.1 要求 worker 通过 authenticated encrypted control channel 主动连接 Controller，并明确排除
  SSH reverse tunnel 作为产品拓扑。这是一处已知偏离，随 P1 的运行时布局一并纠正。

## 5. 当前关键缺口

按产品价值排序。前三项由本文 4.1 与 4.3 的归因直接决定，与上一版排序不同。第 6 节的阶段划分即这些缺口的施工顺序：

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

## 5.1 从这里继续

**部署现状。** 三个进程都在 `d896d7f`,由 systemd 管理:controller 是本机 user unit
`cairn-controller-real`,部署根 `~/.local/share/cairn`;两个 worker 分别是 NPU 主机的 system unit
`cairn-worker-npu-build`（根 `/opt/cairn-worker/npu-build`）和 GPU 主机的 user unit
`cairn-worker-gpu`（根 `/home/dawei/.local/state/cairn-worker/gpu`）。另有两个隧道 unit,见 4.3。
改动后重新部署的路径是 `scripts/build-release.sh` 产出 bundle,再用 `scripts/deploy.sh` 逐个部署。

**三个会咬人的地方,都咬过。**

发布构建需要钉住版本的 zig,而本机的 shim 装在 scratchpad 里、**不跨会话存活**。
重建方式:`uv venv <scratch>/zigenv --python 3.12`,
`uv pip install --python <scratch>/zigenv/bin/python -r release/zig-requirements.txt`,
再写一个 `exec <scratch>/zigenv/bin/python -m ziglang "$@"` 的 `zig` 放进 PATH。
CI 里由 `.github/actions/setup-release-toolchain` 完成,本地没有等价物。

**配置改动必须和要求它的那次代码改动一起部署。** 这个错误犯过两次:先删了线上配置的字段,
而部署的二进制还是要求它的那一版,systemd 重启五次后放弃,整个系统静默停摆。
线上配置不是可以顺手编辑的东西。

`cairn-server bootstrap` 把给它的地址同时用作监听与对外通告。这个部署走反向隧道,两者不同
（监听 17443/17444,通告 7443/7444),bootstrap 之后要手工改 `enrollment_service.public_tcp_address`
与 `control_endpoint.tcp_address`。

**分块上传已完成。** 任务源码压缩包不再整包塞进一帧。客户端先声明归档的长度与摘要,再按帧上限推导出的
块大小逐块推送,最后在同一条连接上提交。服务端在收下任何字节之前就用声明长度挡掉超出配置上限的归档,
并在完成时用摘要判定重组结果是否正是客户端手里的字节——只看长度会放过一份块块都到、内容却不对的传输。
方向与 worker 的物料通道相反:那条是 Controller 给清单、worker 按偏移拉,这条是客户端往上推。
因此照搬的是形状而不是代码——清单在前、按偏移分块、以内容标识收尾。

**暂存字节挂在连接上,不在共享表里。** 这样「上传被放弃了」由持有 socket 的一方直接观察到,
不需要再引入一口专门判定放弃的上传何时死亡的钟,而 4.2.2 刚把一口这样的钟换成被记录的事实。
偏移因此不是续传游标——连接一断暂存就没了,没有可以续上的东西——它的作用是让重复、乱序或重叠的块
变成一次拒绝,而不是一份被悄悄写坏的归档。服务端也没有单独的每块上限:帧上限已经界定单次请求,
声明长度已经界定整条传输,第三道界什么都不挡。声明本身不预分配,一份声明了却从未发送的归档不占内存。

证据口径是 **local model-free**,没有真实部署运行的证据。四道门各做了一次红验证:把块大小的 envelope
预留改成 0、让客户端整包发一块、在传输中翻转一个字节、把干净 EOF 重新当作错误——四次都如预期失败。

**其中一次红验证先是假通过,值得记下来。** 注入字节翻转的那次改写因为缩进不匹配而一处也没改到,
脚本又没有断言匹配数,于是测试「通过」了——它比较的是未被改动的自身。这正是 `AGENTS.md` 里那条:
重写了零处的变换会让比较同义反复地成立。此后每一次注入都先断言匹配数,四道门才逐一验完。

**P3 已开工,先做的是让候选阶段不再是桩**,记录见 6.1 之后的 P3 小节。

**一处会误导人的现状,顺带记下。** `observe_candidate_on_worker` 会真的把候选调度到 build worker、
拿到 receipt、把 outcome 与 exit code 写进日志,然后**无条件返回 `CandidateMechanismExecutionUnavailable`
并丢弃这次观测**。fail closed 本身是对的——没有 qualified mechanism 就不该发布语义结论——
但它同时意味着 `establish_candidate` 里那个 `loop` 从来没有循环过:第一次构建观测就把整个任务打死。
P3 第 4 项要把构建结果作为**搜索信号**（编译成败与诊断）回灌给循环,同时让 admission 继续 fail closed;
这两件事必须分开,否则要么循环拿不到反馈,要么结构自检被当成语义结论。

**下一步不是 P4,因为 P4 现在跑不了。** 理由与证据见下面 P4 小节:Oracle admission 无条件 fail closed,
候选阶段在它之后,所以正常入口到不了循环。需要 administrator 决定的是先做 P5 第 7 项,
还是把 P4 重新定义。

**这一轮改为验证循环本身,不烧 provider 调用去跑一条已知跑不通的路。** 新增
`CandidateSearchStoreV1`,把循环的持久化面从产品服务里分出来——它是那些服务里唯一可以单独运行的部分,
因为它只需要一个部署的 store,不需要先有 admitted Oracle。集成测试因此能问出那个只有部署后才会失败的问题:
一次转移经过真实 SQLite journal、真实路径、每次调用重新打开、并且不在运行时线程上,还成不成立。
两个测试都做了红验证:让 open 不先 recover、让 loop 身份忽略 task。

同时补了两道门:`check-product-path.sh` 的多线程运行时断言扩展到 `cairn-migration-app`
（它现在也在 `block_in_place` 里开 store）;新增一个测试要求每个任务 fixture 的 caller declaration
可被真正会读它的入口读出来——**它当场抓到了新任务 fixture 的一处缺陷**（claim 未按 id 排序）,
而那个缺陷本来要等到一次真实提交、花掉 provider 调用之后才会暴露。

**profiler 诊断分流（P3 第 4 项的后半）尚未做**:目前回灌的只有编译器诊断,原样回传;
profiler 输出要先经确定性分析器转成结构化建议,那要等到有 device 执行（P5）才有输入。

**未解决的事实,别当成已解决。** exposure ledger 一行代码都没有,而它是 restricted 材料唯一
真正的控制（4.2.4）。资源时钟的两面不对称仍在（4.2.6）。P0 的 Exit 仍未达成:
还没有一个候选经正常入口到达 Ascend build worker 并取回 typed 诊断。

**这一轮里被证明有用的做法。** 每一道门禁都红验证过——把它该抓的东西造出来,确认它真的失败,
再撤回。这一轮有三样东西因为「为还不存在的设计搭架子」而被删掉:布局的运行时校验、工具根绑定、
项目定义与它的服务端子命令;删掉它们比留着便宜。本文记录的是事实与错误,不只是结果,
包括我自己的两次归因错误（4.2）——那些记录比结论更有用。

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

P4 只需要 Ascend build worker，不需要 device，因此 4.3 的通道恢复不进入关键路径。P3 不依赖 P1，可与 P1 并行开工。

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
3. **已完成。** 新增 `scripts/check-product-path.sh` 并接入 `scripts/ci.sh`，三项断言：
   每个 opt-in lane 引用的 test target 必须存在；`cairn-migration` 导出的 `pub fn` 中
   「在定义模块之外没有非测试消费者」的数量必须等于记录基线；`cairn-server` 不得出现
   `current_thread`，因为 4.2.1 的 `on_store` 只在多线程运行时上成立。基线是记录值不是目标值，
   只能经一次显式编辑改变，因此失去消费者与采纳孤儿都成为可见事件。刻意不用名字白名单，
   那种清单会静默变长。三项均已红验：制造悬空 lane 退出 1，摘掉一个真实消费者时计数从 22 变 23
   并列出清单，把一个测试的 flavor 改为 current_thread 时该门退出 1。

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
5. **通道已恢复。** Ascend build worker 已重新上线并注册，见本文 4.3。
   device capability 声明未完成且不属于本阶段：目前没有任何 worker 声明 device 执行能力，
   该声明随 P5 的 950PR 执行一并创建。

规模估计：第 1 项的构建半程与第 2、3、4、5 项已完成；第 1 项的观察半程转入 P5。

已完成部分的附带发现：`scripts/ci.sh` 的行尾空白检查此前使用 `rg`，而该环境未安装 ripgrep，
`if <command-not-found>` 判定为假因而 `status` 保持 0——该检查从未真正运行过。已改为 POSIX `grep`
并把 `AGENTS.md` 纳入覆盖，且按纪律红验：故意引入行尾空白后检查确实失败，撤销后通过。
这是 `ARCHITECTURE.md` 3.6 所说情形的一个实例，也是 P0 第 3 项要覆盖的同一类缺陷。

Exit：一个候选经正常入口到达 Ascend build worker 并取回 typed 诊断（构建半程已具备，
待真实 worker 上验证）；删除任一端到端环节会使 `scripts/ci.sh` 失败；
`cairn-migration` 的公开 API 不再包含 fixture 身份。

### P1 · 运行时布局

纯搬运，不引入新语义。目标是 `ARCHITECTURE.md` 10.5。

1. `CAIRN_HOME` 解析与 10.5 所列**七**棵树的绝对路径配置。两处前提在开工时校正：树是七棵不是六棵；
   现有配置里的相对路径解析的基准是**配置文件所在目录**而不是当前工作目录
   （`cairn-server/src/lib.rs` 与 `cairn-worker/src/lib.rs` 均取 `config_path.parent()`）。
   因此本项要换掉的不是「相对 cwd」，而是「相对配置文件」——后者把布局绑死在配置文件的摆放位置上，
   使 10.5 要求的按归属分树无法表达。
2. 把 `restricted/` 从 `secrets/` 拆出，把 durable state 移出 secret 树。
3. `log/` 不保留任何诊断正文落点；扩展现有日志隔离检查覆盖新布局。
4. 项目定义与 intake 冻结：`project.json`，含 `authored_by_agent` 与 `provided`。
5. 开发数据丢弃重建，不写迁移器（`AGENTS.md` 开发期规则）。
6. 发布脚本扩展到 musl 目标，理由见 4.3——两台主机要求不同的目标环境，
   现有脚本只覆盖 gnu，手工绕开这个缺口已经致过一次停机。

规模估计：第 2、4 项为中，其余为小。

Exit：三类材料分树且权限不同；worker 主机上不存在 `packs/` 与 `restricted/`；从任意工作目录启动解析到同一份状态；
`scripts/build-release.sh` 能产出两台主机各自可运行的 worker 二进制。

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

**P3 的第 1、2 项与第 3、4 项的主体已完成（2026-09-03）。** 候选阶段现在是一条真正的循环：
提案 → 构建 → 诊断 → 修订 → 再构建，由 `CandidateSearchLoopV1` 这个事件溯源聚合拥有每一次转移。
形状照搬 `cairn-execution` 的既有惯例——每模块自带 `project` / `apply` / `fact` / `expected`，
不新造 trait；状态只从自己的事件流重建，因此「下一步该做什么」由流决定，而不是由谁在驱动它的内存决定。

五条迭代策略全部落地，每条都有一个模型看不见而 Controller 看得见的理由：

| 策略 | 落点 | 为什么模型自己做不到 |
| --- | --- | --- |
| 迭代预算 | 观测到的构建次数达上限即停 | actor 不知道还剩几次 |
| 重复动作检测 | 重复的提案不构建，作为 typed notice 回传 | actor 看不见自己在转圈 |
| 空提交按故障计数 | 既无提案也无文本的 episode 计入连续计数，超阈值才停 | 空提交此前是硬错误，直接打死任务 |
| 预算临界通知 | 剩余量低于阈值时注入 typed notice | 让 actor 收敛，而不是被截断 |
| 证据到达即固化 | receipt 折回即追加事件并推进状态 | 流程末尾正是受限 actor 最可能到不了的位置 |

**构建观测现在是搜索信号，不再被丢弃。** `observe_candidate_build` 返回编译成败与 receipt，
循环据此决定是收敛还是再修订；修订 episode 拿到的是编译器原文的 stdout/stderr，而不是退出码。
Admission 仍然 fail closed，且把「没有配置 mechanism 目录」与「没有能执行它的能力」分成两个错误，
因为这是两种不同的运维动作。

**候选评审角色已删除。** 它在流程里没有消费者：admission 用不到它的输出，而把它留在循环里
会让搜索刚成功就死在一个桩上。ARCHITECTURE 把独立评审放在 assurance 一侧，不在搜索循环内。

**Exit 达成情况要说清楚。** 前三条达成：同一 task 内多轮 authorized compile→diagnostic→revision，
重复动作被检测并转为 typed input，空提交计入预算而不被当作完成。**第四条未达成**：聚合本身可以从
事件流恢复（有测试证明），但工作流驱动器没有任何重启恢复——任务来自一个内存 channel，
Controller 重启后没有东西会把任何任务重新驱动起来。这不是本聚合的缺口，是工作流层从来没有过恢复路径，
规模超出 P3。**不要把「聚合可恢复」读成「循环可恢复」。**

证据口径仍是 **local model-free**。四道门做了红验证：关掉迭代预算判断、关掉重复检测、
关掉空提交上限、让测试替身第一次构建就通过——四次都如预期失败。仍然没有任何一次真实模型调用。

`scripts/check-product-path.sh` 的孤儿基线由 22 改为 23，原因写在脚本里：
`recompute_candidate_admission` 失去了调用者，而它**此前也从未可达**，
因为唯一通向它的路径会先无条件返回 `CandidateMechanismExecutionUnavailable`。计数现在说的是本来就成立的事。

**第 3 项的 exploration 一路已完成（2026-09-03）。** 开工时发现的前提比预期严重：三个 candidate 角色
executor 全是桩，候选阶段从未向模型发出过一次调用，因此循环框架无论怎么写都没有东西可循环。
先做的是让 exploration episode 真的存在——三件工具（读任务原文、读 admitted Oracle contract、提交候选）、
一条 task-generic 指令、一个 gateway 和一个 executor，形状照搬既有的 oracle item developer 一路。
每任务材料新增了冻结的 candidate 授权（workspace 与 oracle contract），经 services seam 注册，
并新增 `ExploringCandidate` 这一 task phase，使这一阶段对客户端可见。

三点值得记下来：

- **知识投影是显式的空位。** episode 的 model context 里 `knowledge_snapshot` 写的是 `{"kind":"empty"}`。
  这正是 P2 接上来的地方，写成空位而不是省略，是为了让「还没有知识」是一个可读到的事实。
- **指令里不写平台事实。** 4.1 那次失败的直接原因是编译器无法推导 kernel 类型，而把「用某个 vector 或 cube API」
  写进生产指令，就是把知识塞进 prompt——`AGENTS.md` 说知识以 pack 分发，不进仓库。留空是正确的，
  也正是 P2 存在的理由。
- **没有改动任何中立 crate。** `cairn-agent` 及其以下不知道候选或迁移的存在；新增代码全部落在
  `cairn-migration-app` 的接线层，复用的领域类型早已在 `cairn-migration` 里。

证据口径是 **local model-free**：三道门各做了红验证（撤掉一件工具、让 schema 与领域类型漂移、
把提交的入口点改成确实存在的文件），三次都如预期失败。**尚未有任何一次真实模型调用**，
因此不构成「候选可用」的证据，只构成「这条路不再是桩」。

仍未做：review 与 revision 两路 executor 仍是桩；构建观测仍在 `observe_candidate_on_worker` 里被丢弃
（见下）；第 1、2、4 项尚未开工。

Exit：同一 task 内经过多轮 authorized compile→diagnostic→revision 且不离开 durable state；重复动作被检测并转为 typed input；
空提交计入预算而不被当作完成；Controller restart 后循环从 durable state 恢复。

### P4 · 纵向 A：normal-path native build success

**Oracle 的阻塞已解除（2026-09-03，见下节 P5 第 7 项）。** 以下是解除之前记录的诊断，保留是因为它说明了
为什么那道门当时是对的。

**P4 曾经跑不了，依赖图漏了一项（2026-09-03 查明）。** 工作流到不了候选阶段:
`oracle_control_runner.rs` 里 `candidate_facing_runner_available()` 硬编码返回 `false`,
于是 `qualify` 无条件返回 `SemanticExecutionUnavailable`,`run_qualified_oracle_controls` 用 `?`
向上抛,任务在 Oracle admission 处终止。候选阶段在其之后,因此永远到不了。

这道门本身是对的——3.4 记录它正是为了不让结构自检冒充语义授权——但它同时也是通往候选的唯一道路。
**6.1 的依赖图因此不完整**:P4 不只需要 P0 + P2 + P3,还需要一个 candidate-facing Oracle
mechanism runner,而计划把它放在 **P5 第 7 项**。要么先做 P5 的那一项,要么把 P4 重新定义为
不经 Oracle 的窄纵向;两者都是决定,不是实现细节,所以留给 administrator。

**不能用的做法**:把那个 `false` 翻成 `true`,或者绕过 Oracle admission。前者把结构自检变成假的
语义授权,后者就是 P0 刚删掉的 `continue_after_oracle_admission` 那类只对 builder 开放的旁门。

**部署侧的现状也一并记下,它们同样挡着 P4:**

| 事实 | 观测 |
| --- | --- |
| 迁移产品进程 | **从未部署过**。systemd 里跑的是 `cairn-server`（通用 controller）,不是 `cairn-cuda-migration-server` |
| 产品配置 | 全盘找不到任何 `ProductConfigV1`,正常入口在这个部署上没跑过 |
| 构建配方 | Controller 要提供的 `bin/run` 不存在,仓库里也没有 |
| worker | 两个已登记:`npu-build` 与 `gpu`,构建 worker 在线 |
| 模型凭据 | DeepSeek key 在 `.cairn/secrets/` 下,live 调用具备条件 |

**已经为 P4 备好的**:任务 `scale-clamp-f32`——一个此前不在仓库里的 elementwise 算子
（逐元素乘以标量再夹到闭区间）,含 CUDA 源码、caller declaration 与两个真实的 unknown
（非有限输入、上下界颠倒）。选它的理由就是 P4 说的:把风险集中在通路而不是算法上。

汇合点。使用一个此前未知、无 framework 前提的 **elementwise** 算子：选它的理由是把风险集中在通路而不是算法上，
而通路正是本阶段要证明的对象。

1. 正常入口提交，runtime model 带知识投影，循环给足迭代预算；
2. 不允许 candidate 自有 build fallback、fixture 分支或 coding agent 代答；
3. 保存完整的 compile diagnostic 与 revision lineage。

自本阶段起，按 `EVALUATION.md` 第 6 节记录量测，不等到做对照实验时补记。时间一律从 CLI 提交被接受的 durable 时间戳
起算，包含 provider 排队、Worker 等待与失败尝试，并另报 active model / Worker / device time。

Exit：normal path 产生可重放的 native build success；replay 校验 exact artifact 与 toolchain binding。

### P5 第 7 项 · 判据的可判定性（2026-09-03 完成）

**根因不是那个 `false`，是判据没有可判的东西。** `candidate_facing_runner_available()` 硬编码
返回 `false`，把整条 Oracle 通路堵死。但翻开它并不能解决问题:检查计划的 `pass_condition` 与
`observation` 都是 `bounded_text!`，上限 4096 字节，全部校验只有「非空、无首尾空白」。
没有比较器、没有容差、没有操作数绑定。runner 因此只能 `grep` 它非空——那是关于**计划**的事实，
不是关于任何**候选**的事实。mechanism 至今只是 `(control, runner)` 的哈希，正因为它没有可执行的内容。

**做法:给计划一个可被机器判定的断言。** `OracleCheckAssertionV1` 携带比较器与容差来源:

- 比较器三选一:逐字节相等、binary32 绝对容差、binary32 相对容差；
- 容差以 binary32 **位模式**传递，因为十进制会在 JSON 里被舍入，而一个在传输中变了的阈值不是任何人同意过的那个；
- 容差来源必填（caller-declared / measured-noise-floor / derived-from-arithmetic / not-applicable），
  且与比较器交叉校验:精确比较不得声称来源，容差比较不得没有来源。
  **一个说不出出处的容差是某人随手挑的数**，靠它的判据无法说出候选要错到什么程度它才会叫。

**判据先自证，再裁决。** `calibrate_check_assertion` 实现 `EVALUATION.md` 5.1 的协议:
特异性（参考对自身必须通过）与敏感性（每一个已知错误变体都必须被拒），
并把「没有提供错误变体」与「通过」区分开——**没测过能不能失败，不等于通过**。
校准用的是合成观测，因此**不需要设备**:判据能不能失败是比较器的性质，不是候选的性质。

`candidate_facing_runner_available` 因此从硬编码常量变成对**手上这批计划**的真实探测:
每个计划的断言都要能被本次构建校准，否则照旧 fail closed。

**校准探针当场抓到一个真实缺陷。** 原实现把长度不符判为 `Uncomparable`，
于是「少写了尾部元素」的候选会因为「无法比较」而逃过判定。丢尾是**错误答案**，不是无法判断:
现在只有参考本身不成形（非 binary32 整数组）才是不可比较，候选长度不符一律拒绝。
这个缺陷是探针里那条「丢弃末尾」的变体逼出来的——正是 5.1 要求它存在的理由。

顺带处理的两处:两个 Oracle 角色的检查计划 schema 此前各写一份，已抽成一个共享函数
（两处漂移就意味着模型被告知写的东西会被另一处的校验器拒绝）；
`evaluate_check_assertion` **不导出**，因为它模块外还没有消费者——它的消费者是对真实候选观测求值的
control runner，属于下一步。孤儿门禁当场抓到了这次过早导出。

**随后补上的三项（同日）:** mechanism 有了值类型 `OracleQualifiedMechanismV1`，存进 CAS，
校准结论进入其身份——校准不同的机制就是不同的机制，未校准的无法构造也无法反序列化出来；
五个控制族各自以自己的模式执行一次、各拿一张 receipt（此前是一次执行扇出五份，
那只能证明「合式计划被接受」五遍，证不了 mutant 模式会拒绝 mutant）；
`validate_qualification` 进入生产路径，registration 要与它引用的 receipt 对账。
资格证明绑定的 item 按规范序取，因为要证的是机制而不是某个 item。

**仍未做，别当成已完成:** controls 执行阶段仍在判计划文本，不判候选观测。
断言可求值了，但把它接到 `execute_controls`、让 receipt 承载真实候选判定，需要先有候选观测。

证据口径 **local model-free**。三道红验证:把长度不符改回「无法比较」、去掉容差来源的强制、
关掉敏感性检查——三次都如预期失败，其中第三条同时被领域层与接线层的测试抓到。

### P5 · 纵向 B：950PR correctness

前置项是 worker 通道与 device capability 声明，不是硬件采购（见 4.3）。使用 `compact-above-f32`：
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
