# DEV-036 — proposal 回归 Controller 主 workflow

- 状态：`Accepted`
- 日期：2026-08-31
- 依赖：D-043、D-044、DEV-035
- 外部执行：无；未调用runtime模型、外部网络、Docker、managed Worker workload或NPU（测试包含本地loopback/control）

## 1. 目标

纠正把 proposal 误建模为独立 Host 的架构漂移。SIR、Oracle 和 Candidate proposal 都是 Controller 主
workflow 中的 typed Agent step，不拥有独立进程、二进制、service identity、OS principal、私有数据库或
execution authority。需要编译、运行、Docker、host adapter 或设备能力时，Agent 只提出 typed tool request，
Controller 再生成并授权 `JobContract`，调度到 capability-matched managed Worker。

本片直接修改 current V1，不保留旧 process protocol、reader、alias、configuration fallback 或版本升级。

## 2. 可读业务骨架

```text
Controller workflow selects exact proposal step
→ freeze domain input / model / tool catalog / budget
→ drive domain-neutral model and pure/read-only tool transitions in cairn-server
→ external capability needed?
   → no: archive domain proposal and advance Controller aggregate
   → yes: derive typed WorkflowToolRequest
        → Controller authorizes JobContract
        → scheduler selects capability-matched cairn-worker
        → exact job / attempt / contract observation
        → project result back to the same typed workflow step
→ independent Admission remains model-free
```

## 3. 已删除和替代

- workspace 删除独立 proposal process crate及其stdin/stdout process protocol与process-boundary tests；
- Server 删除child supervisor、executable/state-root/process-timeout/stdout/stderr配置；
- proposal runtime删除binary digest与invocation marker；
- 删除每episode私有SQLite/CAS、start/terminal checkpoint和process failure/reconciliation state；
- `proposal_loop.rs`及其Host命名/authority外壳删除；更早已存在的domain-neutral Agent episode/model/tool
  operation primitives保留，Controller内共享的runner组合归入`cairn-agent`；
- process-specific external-effect yield删除，改为Controller step返回typed `WorkflowToolRequest`；
- Controller直接以自己的event/content stores驱动Agent step。

旧开发数据库因current-V1定义直接变化而废弃重建；没有converter或dual reader。

## 4. 强类型与 authority

- SIR、Oracle strategy和Candidate仍使用各自领域input/output及distinct content identities；
- `WorkflowToolRequest`、dispatch、Worker binding、Worker observation和Controller observation保持不可互换；
- proposal output不能进入admitted constructor；Worker observation不能由模型或调用者自由构造；
- model runtime recovery不授予execution、Admission、policy或user authority；
- 外部effect必须沿Controller authority→Worker job/attempt/contract→observation闭合。

## 5. 日志

保留Controller workflow里程碑和`cairn-agent`安全的model/tool状态日志。禁止记录source、prompt、model
request/response、tool arguments/results、stdout/stderr、secret或restricted material。日志不是transition或replay
input。

## 6. 测试与退出门禁

- workspace check与全测试target编译；
- in-process SIR/Oracle/Candidate recorded step与restart/replay；
- Workflow tool start-before-Worker、exact receipt lineage和no-direct-execution controls；
- 删除proposal process crate/config/symbol/path的静态扫描；
- current normative docs、baseline、catalog与D-044一致；
- format、Clippy、log-isolation、diff check。

实际结果：`cargo check --workspace`、`cargo test --workspace --no-run`、
`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo fmt --all -- --check`、`scripts/check-log-isolation.sh`与`git diff --check`全部通过。
静态扫描确认workspace、Cargo lock、current代码和规范文档不再含独立proposal process/crate/supervisor/
binary identity或`AwaitingController` process-yield contract。

## 7. 明确非目标

- 不把proposal代码、编译或设备验证移入Controller；
- 不把模型输出提升为Worker receipt或Admission verdict；
- 不增加knowledge/skill库；
- 不实现Candidate suffix或revision policy；
- 不声称当前App API已接入真实local Worker；该缺口在本片闭合后继续处理；
- 不保留任何旧Host数据兼容路径。
