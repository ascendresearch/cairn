# DEV-004 implementation note — generic DeepSeek SIR proposal

- 状态：`EvidencePending`；不是 DCR，不授权 Admission 或其他 authority 实现
- 日期：2026-08-28
- Slice：[`DEV-004`](../SLICE_CATALOG.md#3-当前critical-slices)
- 决策：[`D-042`](../../DECISIONS.md#d-042--runtime-models-reason-per-task-fixtures-evaluate-but-do-not-define-the-product)

## 1. 这次只证明什么

让 configured DeepSeek 通过现有 durable `cairn-agent::AgentEpisode` 检查一个此前未知的 CUDA migration
task，并提交一个可严格解码、带源码引用、保留竞争假设和 unknown 的 proposal。

它只证明真实 runtime-model workflow 接通，不证明 proposal 正确、SIR 已泛化或任何 Intent 已 admitted。
DEV-005 才比较第二个 task 和 downstream utility。

## 2. 最小 workflow

```text
task-local source root
→ bounded task manifest (path/kind/line count/content identity)
→ generic SIR instructions + manifest-only initial context
→ DeepSeek AgentEpisode
→ bounded read-task-artifact tool calls
→ submit-intent-hypotheses tool call
→ trusted gateway validates citations/references and binds episode/task/model provenance
→ immutable IntentHypothesisSetProposalV1
```

DeepSeek 不直接读取 repository。Task loader 只打开调用者显式给出的 task root，拒绝 symlink、absolute path、
parent traversal、非 UTF-8、文件/总字节超限。Model 初始 context 只看到 task-local path、kind、line count 和
identity；源码由 bounded read tool 返回。Fixture 父目录名、claims、corpus、review receipt 和 expected answer
不进入 projection。

## 3. Product types

全部放在当前 `cairn-migration`，不建新 crate：

- `SirTaskArtifactPath`、`SirSourceLineNumber`；
- `SirTaskBundleV1`：有序 task-local paths、line counts 与 exact content identities；
- `SirSourceCitationV1`：artifact path + inclusive line range；
- `SirFactStatement`、`SirHypothesisSummary`、`SirUnknownQuestion`：三个不可互换的bounded text types；
- `SirCitedFactV1`：statement + 至少一个 citation；
- `SirIntentHypothesisV1`：summary + 直接内嵌的supporting/counter facts，不建立中间ID graph；
- `SirUnknownV1`：unresolved question + 可选的相关source citations；
- `SirProposalSubmissionV1`：model-authored body，不允许自填 episode/model/task authority；
- `IntentHypothesisSetProposalV1`：gateway 注入 exact task bundle、episode 和 resolved-model identity 后形成的
  proposal artifact。

Current-V1 constructor/deserialize 必须保证：paths canonical；bounded text非空；collections非空且有界；
至少两个hypotheses和一个unknown；citation path存在且line range有效。Proposal type没有
admitted constructor、confidence score、pass/verdict或 hidden identity 字段。

## 4. Tools 与 prompt

只实现一个 `ReadOnly` source tool 和一个 `Pure` proposal tool：

1. `sir_read_task_artifact`：输入 task-local path、start line、line count；每次最多200行/32 KiB，只能读 frozen
   bundle；返回带行号文本和exact artifact identity。
2. `sir_submit_intent_hypotheses`：输入 `SirProposalSubmissionV1`；gateway按bundle校验citation和引用，成功后
   归档proposal；它不产生Admission或execution effect。

首片不实现search：当前task很小，bounded read已经足够；只有DEV-005第二个task证明需要时才增加literal
search。Prompt只描述调查协议：检查source/host/ABI，区分observable facts与inference，提交至少两个竞争
hypothesis并保留unknown。不得出现operator名、D-039、reduction domain值或expected hypothesis。

## 5. Exact change inventory

| Path | Change |
| --- | --- |
| `crates/cairn-migration/src/sir.rs` | task bundle、minimal proposal types、generic prompt/projection、两个tool gateways和episode runner |
| `crates/cairn-migration/src/lib.rs` | 只导出当前consumer需要的SIR API |
| `crates/cairn-migration/examples/sir_deepseek.rs` | live入口调用recorded/live共用的product runner；SQLite/CAS终态重开；JSON summary不打印hidden reasoning |
| `config/sir-deepseek.example.json` | current-V1 model alias、output/episode/tool/time/byte limits；无task/fixture答案 |
| `crates/cairn-migration/tests/sir_episode.rs` | strict types、tool scope、context absence、malformed submission、recorded replay/restart |

预计不修改 `cairn-agent`、`cairn-testkit`、Admission、verification或server。若实现发现通用runtime缺口，暂停
并单独说明，不能把产品 workaround 塞进 `cairn-agent`。

## 6. Recorded 与 live lanes

Recorded required command：

```text
cargo test -p cairn-migration --test sir_episode
```

测试先经同一 native codec/episode/tool path 捕获 exact request/response exchanges，再在新durable state中用
`RecordedModelTransport` 对exact request identity逐步重放；原episode关闭SQLite/CAS后必须恢复同一terminal
projection。Scripted response只证明protocol，不能计作DeepSeek quality。

Opt-in live command：

```text
cargo run -p cairn-migration --example sir_deepseek -- \
  fixtures/cuda-ascend/intent/reduce-sum-f32/v1/source \
  config/sir-deepseek.example.json
```

默认限额：最多8个model steps、24个tool operations、65,536 observed provider tokens、单turn 16,384 output
tokens、900秒、32个task files、256 KiB task bytes。Live lane记录model/deployment、episode、task bundle、
proposal、request/response、usage、completion和restart facts，但不打印provider raw response或chain-of-thought。

## 7. Acceptance 与停止

DEV-004 接受必须同时满足：

- recorded episode、tool loop、strict proposal和restart replay通过；
- model-visible request absence test证明没有fixture answer/restricted material；
- 至少一次受预算约束的真实DeepSeek run提交有效proposal；
- proposal引用实际task-local source lines，含竞争hypotheses和unknown；
- production module/prompt没有fixture-specific branch/vocabulary；
- full CI green。

如果真实run无法形成有效proposal，DEV-004保持`EvidencePending`并记录first divergence，不增加Admission、
qualification或多Agent review。若实现本身失败，删除上述5个新增/修改入口；若DEV-005 No-go，删除
`sir.rs`、example/config/tests和exports，保留未修改的domain-neutral `cairn-agent`。

## 8. Current evidence

- `cargo test -p cairn-migration --test sir_episode`：通过，3 tests；
- `cargo clippy -p cairn-migration --all-targets -- -D warnings`：通过；
- `scripts/ci.sh`：沙箱内mTLS local-socket test被OS拒绝；在允许local socket的环境重跑full CI通过；
- live DeepSeek：`NotExecuted`。外部调用在process启动前被安全审查拒绝，没有provider request发生；需要用户
  明确授权把所选task source发送给配置的外部DeepSeek endpoint并使用private API credential。
