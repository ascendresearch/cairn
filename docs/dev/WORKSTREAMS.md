# Cairn 开发 Workstreams 与集成设计

- 状态：规范性开发计划设计，尚未授权代码实施
- 日期：2026-08-27
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 路线图：[`ROADMAP.md`](ROADMAP.md)
- Code organization：[`../design/CODE_ORGANIZATION.md`](../design/CODE_ORGANIZATION.md)

## 1. 目的

Workstream 是开发协作和代码所有权视角，不是运行时 Agent、OS process 或新的产品 authority。一个人或
编码 Agent 可以承担多个 workstream；多个执行者也可以在同一 workstream 内工作。正式 authority 仍由
系统设计和运行进程决定。

## 2. Workstream Catalog

| Workstream | 主要责任 | 主要代码区域 | 禁止承担 |
| --- | --- | --- | --- |
| `WS-DOMAIN` | CUDA→Ascend C task、Intent、Oracle、Candidate、Verdict types/policy | `cairn-cuda-ascend` | provider/Worker adapter、restricted gate implementation |
| `WS-ADMISSION` | required evidence、plan validation、qualified Gate、restricted store | `cairn-admission`、verification mechanics | model transport、修改 applicant |
| `WS-AGENT` | domain-neutral episode/runtime 与 Proposal Host adapters | `cairn-agent`、`cairn-proposal-host` | product policy、admitted constructor |
| `WS-SIR` | SIR process、static/behavior analysis、hypothesis proposal | `cairn-sir` + product SIR contracts | Intent Admission、hidden corpus |
| `WS-RECORD` | event/CAS/projection/replay、typed visibility ports | protocol/codec/record/store | verdict semantics |
| `WS-EXECUTION` | Job/Attempt/Worker/Scheduler、CUDA/Ascend adapters | execution/server/worker adapters | operator semantics、Gate outcome |
| `WS-PERFORMANCE` | hardware facts、microbench、profiler、roofline | product hardware modules + qualified adapters | correctness compensation、unscoped peak claims |
| `WS-KNOWLEDGE` | knowledge/skill lifecycle、retrieval、feedback/revalidation | product registry/feedback modules | origin-based trust、capability expansion |
| `WS-QUALITY` | testkit、architecture checks、fixture provenance、CI/evidence audit | `cairn-testkit`、tests、scripts/docs | production shortcuts、mock authority |
| `WS-PRODUCT` | Controller process managers、API、CLI、stage integration | server/app-server/cli | hidden bytes、Gate reimplementation |

Workstream 名称不进入 production protocol 或 runtime role catalog。

## 3. Contract Ownership

每个跨边界 contract 只有一个语义 owner；adapter owner 不能复制或修改其规则：

| Contract | Semantic owner | Implementing/consuming workstreams |
| --- | --- | --- |
| `IntentRecoveryInput` / `IntentHypothesisSet` | WS-DOMAIN + WS-SIR | WS-SIR、WS-PRODUCT、WS-RECORD |
| `RequiredIntentEvidenceSet` / Intent outcome | WS-DOMAIN + WS-ADMISSION | WS-ADMISSION、WS-PRODUCT |
| Agent profile/invocation/interaction | WS-DOMAIN | WS-AGENT、WS-SIR、WS-PRODUCT |
| `AgentEpisodeSpec` / native continuation | WS-AGENT | WS-SIR、Proposal Host、WS-RECORD |
| `JobContract` / receipt | WS-EXECUTION | WS-PRODUCT、WS-ADMISSION、device adapters |
| Oracle claim/portfolio | WS-DOMAIN | WS-ADMISSION、WS-AGENT、WS-QUALITY |
| Mechanism qualification | WS-ADMISSION | comparator/adapter owners、WS-QUALITY |
| Hardware fact/roofline/performance outcome | WS-PERFORMANCE + WS-DOMAIN | WS-ADMISSION、WS-EXECUTION |
| Knowledge/skill/feedback | WS-KNOWLEDGE + WS-DOMAIN | WS-AGENT、WS-SIR、WS-PRODUCT |
| Public/restricted/secret store ports | WS-RECORD + WS-ADMISSION | WS-PRODUCT、WS-AGENT、WS-EXECUTION |

“共同 owner”表示必须共同审查不同语义部分，不表示可以建立一个 generic type 消除差异。

## 4. 并行化单位

可以安全并行的单位是独立 change set + frozen contract fixture，而不是共享可变设计：

```text
Contract owner
  → canonical V1 fixture + invariants + negative cases
    → adapter/runtime consumers in parallel
      → shared contract suite
        → integration increment
```

允许的并行示例：

- WS-SIR 实现 process adapter，同时 WS-ADMISSION 实现 restricted process shell，双方消费已经冻结的
  product types；
- CUDA adapter 与 Ascend build adapter消费同一个 opaque Job/Receipt contract；
- recorded provider lane 与 live provider lane 共享同一个 product gateway；
- profiler adapter qualification 与 deterministic Hardware Fact domain types 并行；
- fixture sanitation 与 architecture test 增量并行。

禁止的并行方式：

- 两个分支分别定义同名 V1 schema，合并时写 converter；
- 用 `String`/JSON blob 暂时跨 contract 未定的边界；
- 复制 strong types 到多个 crate 后再“统一”；
- Proposal 与 Admission 同时各实现一套 required obligations；
- live adapter 先定义行为，recorded fixture 事后迎合；
- 多个执行者共享一个未冻结 hidden corpus 或可变 truth file。

## 5. Critical-path 协作

### 5.1 ST0/ST1

主 owner：WS-DOMAIN、WS-SIR、WS-ADMISSION、WS-RECORD。

集成顺序：

1. WS-DOMAIN 冻结首个 task/intent proposal/admitted contracts；
2. WS-QUALITY 提供正负/冲突/unknown fixtures；
3. WS-RECORD/ADMISSION 建立 public/restricted capability；
4. WS-SIR 接入 process proposal；
5. WS-ADMISSION 接入 required set、receipts 和 Gate；
6. WS-PRODUCT 完成 process manager 和 Oracle claim handoff；
7. WS-QUALITY 做 backwards audit 和 restart integration。

不得为了并行先让 SIR 返回正式 `MigrationIntentContract`，之后再“拆 Admission”。

### 5.2 ST2/ST3

主 owner：WS-DOMAIN、WS-AGENT、WS-ADMISSION、WS-QUALITY。

先冻结 `RequiredOracleClaimSet` 和 proposal schema，再并行 synthesis/adversarial strategies 和 mechanism
qualification。Admission Gate 最后消费 frozen proposal/receipts，不消费 strategy internal state。

### 5.3 ST4/ST5

主 owner：WS-EXECUTION、WS-DOMAIN、WS-ADMISSION、WS-PERFORMANCE。

CUDA reference、Ascend build、NPU run、profiler lane 使用不同 adapter/evidence identity。稀缺 NPU 预约由
WS-EXECUTION 管理，WS-DOMAIN/ADMISSION 只能提出 typed requirement/job，不直接占设备。

## 6. Change Set 边界

一个 slice 可以有多个 change sets，但推荐按以下顺序独立审查：

1. contract + constructors + codec + compile-fail tests；
2. pure policy/mechanism + unit/property/mutation tests；
3. process/port adapter + contract tests；
4. workflow integration + crash/replay tests；
5. live model/hardware adapter/evidence；
6. docs/examples/operations + fact ledger closure。

这不是保留半成品到主分支的授权。若 repository policy 要求每次合并 green，则 enabling change set 必须
被现有消费者覆盖、处于不可调用内部状态，或与最小消费者一起合并。

## 7. Integration Rules

- 以 small non-interactive commits 保留 review/rollback 能力；
- 任何 schema change 同一 change set 更新所有当前 V1 consumers/fixtures；
- 不把 unrelated formatting/refactor 混入 authority slice；
- dirty worktree 中已有用户变更视为外部约束，冲突时先停下；
- adapter branch 不改 semantic owner 的 policy；
- contract fixture identity 改变时所有消费者明确更新，不用 fallback；
- integration branch 失败按 first divergence 定位，不以“哪个分支最后合入”归因；
- live device/provider evidence 只在 deterministic/recorded gates green 后运行；
- 合并顺序不能跳过 slice entry/exit gate。

## 8. Review Matrix

| Change | Required review roles |
| --- | --- |
| Intent/Oracle/Candidate domain type | WS-DOMAIN + WS-ADMISSION + WS-QUALITY |
| Agent profile/context/capability | WS-DOMAIN + WS-AGENT + WS-RECORD |
| Restricted/hidden path | WS-ADMISSION + WS-RECORD + WS-QUALITY |
| Worker/device adapter | WS-EXECUTION + WS-QUALITY + consuming domain owner |
| Comparator/Gate/policy | WS-ADMISSION + independent WS-QUALITY reviewer |
| Hardware/profiler/roofline | WS-PERFORMANCE + WS-ADMISSION + WS-EXECUTION |
| Knowledge/skill/feedback | WS-KNOWLEDGE + WS-DOMAIN + WS-ADMISSION |
| Public API/release | WS-PRODUCT + all exposed semantic owners |

“Independent reviewer”是开发审查职责，不等同于运行时独立 evidence authority。

## 9. Hardware 与外部资源协调

设备和 provider 被视为有预算的 execution capabilities：

- 先声明 exact lane、input、expected receipt 和 stopping policy；
- 不为调试临时扩大网络、secret 或 filesystem 权限；
- 同一设备上 build、correctness、sanitizer、microbench、profiling 分任务记录；
- shared Ascend device busy 时保持 unavailable/draining，不抢占或把未运行标 green；
- live model 调用先有 recorded counterpart 和 token/tool/wall budget；
- 失败保留 provider/device/infrastructure/candidate/mechanism 的分类；
- 成本与 evidence gain 进入事实账本，为后续 invocation/scheduling policy 提供数据。

## 10. 开发自动化 Agent 的使用

编码 Agent 可以辅助不同 workstream，但必须遵守与人工相同的 slice contract：

- 只获得当前 slice 所需目录、工具和上下文；
- 先读 applicable AGENTS/rules、requirements、design 和 conformance record；
- 不自行关闭 open question、扩 scope、启动 live provider/device 或创建兼容路径；
- 并行 Agent 通过已提交 change set、typed fixture 和审查反馈协作，不共享未审计临时“事实”；
- Agent 自报 tests passed 不是证据，调用者/CI 以实际命令输出和 artifacts 验证；
- 多 Agent 共识不替代 code review、mechanism qualification 或 product admission。

开发多 Agent 协作是工程执行策略，不改变产品内的
[`../design/AGENT_ARCHITECTURE.md`](../design/AGENT_ARCHITECTURE.md)。

## 11. 阻塞与重新规划

Workstream 遇到 blocker 时记录：

- exact slice/obligation；
- blocker 类型：decision、contract、authority、qualification、fixture、environment、budget、defect；
- 已完成且仍有效的 evidence；
- 可安全并行的其他工作；
- 解除条件与 owner role；
- 是否影响 critical path/stage milestone。

不得用另一个 workstream 的 mock、私有 fork 或临时权限绕过 blocker。若新证据改变架构，先更新上位
规范和 roadmap，再重切 slice。
