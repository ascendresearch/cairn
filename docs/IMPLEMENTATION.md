# Cairn 当前实现与下一里程碑

- 状态：当前事实账本
- 日期：2026-09-01
- 作用：只陈述代码与真实运行已经证明什么、尚未证明什么

本文不增加架构要求。目标设计见 `ARCHITECTURE.md`；历史 DEV 记录、试验结果和 superseded 设计通过 Git 历史追溯。

## 1. 结论

Cairn 已经具备可复用的 durable Agent runtime、typed record/replay、Worker scheduling/receipt 和一条部分接通的
CUDA→Ascend C workflow，但首个完整 migration package 尚未建立。

当前最强 live evidence 是：runtime model 面对未知任务产生过 task-generic intent/candidate proposal；exact candidate 曾通过
Controller 调度到远端 Ascend build environment；product-owned native gate 成功排除了 host fallback，但最新 native build
结果仍为 `SubjectFailed`。

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

## 5. 当前关键缺口

按产品价值排序：

1. normal CLI/server/app/workflow 路径上的 native Ascend C build success；
2. exact 950PR run、correctness observation 和 replay；
3. claim-scoped concern applicability/global coherence，避免固定矩阵重复展开和 case inflation；
4. candidate-facing Oracle experiment/mechanism runner，形成真实 Worker receipt→Oracle Admission；
5. qualified Candidate control runner 及真实 receipt→Candidate Admission；
6. `CandidateSearchLoopV1` generation/action/immutable-state protocol 与最小 Evidence/Assurance Graph consumer；
7. observation-bound compile/run/diagnose/repair loop 和 generic revision policy；
8. correctness-first candidate family、host tiling/kernel coupled search 和 target profiler feedback；
9. Development/Qualification separation、epoch invalidation、Candidate lifecycle/promotion 和 hidden exposure closure 的 live consumer；
10. target knowledge pack、actionable diagnostics、performance baseline 和 workload-aware promotion；
11. 至少三个 materially different tasks 的同路径复现；
12. migration package assembly、review commands 和 adoption evidence。

## 6. 下一里程碑：首个真实 package

在此里程碑完成前，暂停新增通用 Agent role、Admission kind、service、crate、完整 graph topology、知识库平台和兼容机制。

### Slice A：normal-path native baseline

- 选择一个此前未知、无 framework 前提的 operator task；
- runtime model 通过正常入口提出 intent 和最简单 candidate；
- ordinary Ascend build Worker 对 exact artifact 获得 native success；
- 不允许 candidate-owned build fallback、fixture branch 或 coding-agent answer；
- 保存 compile diagnostic 和 repair lineage。

Exit：normal path 产生可重放 native build success，且 replay 验证 exact artifact/toolchain binding。

### Slice B：950PR correctness

- 接通 CUDA/reference input generation 与 950PR execution；
- 在昂贵 item development 前完成 claim-scoped concern applicability 和跨 concern coherence；
- 接通 `OracleExperimentRequestV1`→Controller authority→ordinary Worker→trusted observation→原 Agent episode；
- 用最小 public Development Validation 产生真正 candidate-facing executable mechanisms；
- Oracle qualification 接受 honest/correct variants，并拒绝 targeted mutant/negative controls；
- qualified Candidate runner 对 exact candidate 执行已通过 meta-qualification 的 mechanism；
- 把 execution failure、candidate defect、Oracle defect 和 intent ambiguity 分路；
- 产生诚实 admitted/partial/rejected outcome。

Exit：同一 artifact 在 exact 950PR 上运行；至少一个 Oracle mechanism 实际判断 candidate observation，而不是检查 plan prose；
model-free Oracle/Candidate Gates 从 trusted receipts 重算至少一个 required claim。

### Slice C：bounded target search

- 保留 correctness baseline；
- 实现 best-of-N baseline 与一个 bounded population/beam strategy；
- 分别搜索 host tiling/data movement 和 device kernel/schedule；
- 将 compiler/profiler observation 转为结构化、可验证的 action guidance；
- 在冻结 workload 下比较 parent/current，记录 tokens、Worker time、device time 和 plateau。

Exit：至少一个 candidate revision 在 required non-regression 后获得可重复 target-side improvement，或系统诚实证明预算内无改进。

### Slice D：第二、第三任务和 package

使用完全相同的 production path 再运行：

- 一个 numerical-sensitive 或 reduction task；
- 一个 layout/indexing、atomic、stateful 或 concurrency task。

Exit：三个任务无 product-code/prompt special case；至少一个任务形成完整 package；其他任务可以诚实终止为 partial/rejected，
但必须保留 exact failure attribution。

## 7. 之后的优先级

首个 package 后依次：

1. 最小 exact-version Ascend knowledge pack 与 retrieval ablation；
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
