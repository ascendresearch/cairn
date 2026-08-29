# Cairn 当前实现详解：从 CUDA atomic compaction 到 Candidate source proposal

- 状态：DEV-004–012 历史 walkthrough；不把目标设计误报为已实现
- 日期：2026-08-29
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 当前基线：[`CURRENT_BASELINE.md`](CURRENT_BASELINE.md)
- 实施记录：[`records/README.md`](records/README.md)

> 注意：本文的逐步样例叙事止于 DEV-012。DEV-013–020 已经继续完成 remote build、native gate、
> diagnostic、DeepSeek repair 与 rebuild；当前事实和下一步分别以
> [`CURRENT_BASELINE.md`](CURRENT_BASELINE.md) 和 [`NEXT_SESSION.md`](NEXT_SESSION.md) 为准。下文
> “proposal 尚未 build/下一步 build”保留为当时的历史说明，不再是当前指令。

## 1. 一句话结论

Cairn 已经把一个 CUDA 算子的“源码与用户声明”推进成了一个经过用户授权、Oracle 资格化、restricted
commit的answer-free Candidate输入，并让真实DeepSeek Candidate按需读取源码后提交了首个typed Ascend C/CANN
source proposal。该proposal仍未build、执行或被Oracle判定。

当前完成的链路是：

```text
用户声明 + CUDA source
        │
        ▼
IntentRecoveryInputV1
        │
        ▼
DeepSeek SIR episode
        │
        ▼
IntentHypothesisSetProposalV1
        │
        ▼
model-free decision request
        │
        ▼
用户 / task authority 选择语义
        │
        ▼
UserIntentDecisionV1
        │
        ▼
独立 Admission process
        │
        ▼
MigrationIntentContractV1
        │
        ▼
contract-selected Oracle policy
        │
        ▼
真实 host child execution + receipt
        │
        ▼
Oracle observation/comparison
        │
        ▼
honest/fault mechanism qualification
        │
        ▼
AdmittedCollectionOracleClaimV1
        │
        ▼
restricted commit
        │
        ▼
CollectionOracleAdmissionPublicOutcomeV1
        │
        ▼
CollectionCandidateSearchInputV1
        │
        ▼
bounded DeepSeek Candidate episode
        │
        ▼
3 次 task-scoped source reads
        │
        ▼
CollectionCandidateProposalV1
        │
        ▼
【当前停在这里：proposal 尚未 build】
        │
        ▼
下一步：选择 exact target/toolchain 的最短 build consumer
```

必须区分三件事：

1. SIR 只提出“高阶意图可能是什么”。
2. 用户或者其他实际 task authority 决定“我们到底要什么”。
3. Oracle 根据已经授权的语义判断 Candidate 行为是否正确。

DeepSeek 不能自己完成第 2 步，也不能自封为 Oracle。

## 2. 样例任务：`compact-above-f32`

CUDA 接口是：

```cpp
cudaError_t launch_compact_above_f32(
    const float* input,
    uint32_t count,
    float threshold,
    float* output,
    uint32_t* output_count,
    cudaStream_t stream);
```

CUDA kernel 的核心逻辑是：

```cpp
const float value = input[index];

if (value > threshold) {
    const uint32_t output_index = atomicAdd(output_count, 1U);
    output[output_index] = value;
}
```

源码见
[`compact_above.cu`](../../fixtures/cuda-ascend/sir/compact-above-f32/v1/source/src/compact_above.cu)。

假设输入：

```text
input     = [1.0, 4.0, 3.0, 2.0]
threshold = 2.0
```

满足 `value > threshold` 的 occurrence 是 `4.0` 和 `3.0`，所以确定无疑的部分是：

```text
输出元素 occurrence multiset = {4.0, 3.0}
output_count                 = 2
```

但存在一个关键问题：

```text
输出必须是 [4.0, 3.0] 吗？
还是 [3.0, 4.0] 也可以？
```

CUDA 实现使用 `atomicAdd` 抢占输出槽。不同线程谁先拿到槽位，可能随调度变化。因此源码表现出来的是一种
调度相关、不稳定的输出顺序。

但是：

```text
“源码当前不保证顺序”
    不自动等于
“用户不关心顺序”
    也不自动等于
“用户要求保留 CUDA 调度产生的顺序行为”
```

这就是 SIR 在该样例中需要处理的问题。

## 3. 冻结任务输入，而不是让模型读取整个仓库

### 3.1 用户明确声明的内容

样例的 caller declaration 明确声明：

- 操作保持异步；
- 复制所有严格大于 threshold 的输入；
- `output_count` 返回复制数量；
- output 容量至少为 `count`；
- input/output 不重叠；
- input/output 重叠行为不在要求范围内；
- 输出顺序是否重要，尚未声明。

完整声明见
[`caller-intent.json`](../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json)。

这里有一个重要的来源边界：

```text
caller claim ≠ source observation ≠ model hypothesis
```

例如，“复制所有 `> threshold` 的值”是 caller claim；“当前 CUDA kernel 使用 `atomicAdd` 分配输出槽”是
source observation。它们不会被揉成一段没有来源的自然语言。

### 3.2 `IntentRecoveryInputV1`

DEV-006 将输入冻结为严格的 `IntentRecoveryInputV1`，它绑定：

```text
task identity
task source bundle identity
caller ABI declaration
caller semantic claims
caller exclusions
caller unknowns
target environment context
authorized evidence
prior feedback
capability manifest
schema version = 1
```

在该样例中，target SoC、toolchain 和 environment 当时都没有选择，所以它们被明确表示为 `not-selected`，
而不是空字符串或模型猜测值。

这体现了强类型原则：

```text
未选择 ≠ 不存在 ≠ 字符串为空 ≠ 模型猜测
```

### 3.3 DeepSeek 能看到什么

模型初始看到：

- caller declaration；
- task-local 文件清单；
- 文件行数和 content identity；
- 可用工具；
- 读取预算和 episode 预算。

模型不能直接遍历整个 Cairn 仓库，也看不到：

- fixture expected answer；
- private evaluator case；
- review receipt；
- Admission policy；
- Oracle expected output；
- Candidate 正确答案；
- restricted store。

如果模型要看源码，必须调用 `sir_read_task_artifact`。该工具限制包括：

- 只能读取冻结的 task root；
- 禁止绝对路径、`..` 和 symlink escape；
- 每次最多 200 行和 32 KiB；
- 总 task 大小有界。

因此 DeepSeek 是迁移任务的 runtime actor，不是一个可以看到全部开发上下文的 repository coding agent。

## 4. DeepSeek 执行 SIR，但只产生 proposal

SIR 是 Semantic Intent Recovery。它不是“自动猜一个唯一正确意图”，而是：

```text
观察事实
→ 区分 caller claim 与 source behavior
→ 提出竞争假设
→ 保留冲突和未知
→ 提议如何消歧
```

### 4.1 真实 live episode

当前 canonical compaction run 是：

```text
episode:01a048a1-7279-7b22-807b-8756963ace78
```

它使用真实 configured DeepSeek，执行了：

- 3 次 bounded source read；
- 首次 structured submission 被 strict gateway 拒绝；
- 模型在同一个 continuation 中修复；
- 第 4 step 提交合法 proposal；
- episode 以 `Yielded` 结束；
- SQLite/CAS 重启后恢复出同一个 terminal result。

strict gateway 没有为了让 live run 看起来成功而放过错误格式。模型必须修复到满足 current V1 contract。

最终 proposal ID 是：

```text
cairn:v1:sha256:migration.sir-intent-hypothesis-set-proposal.v1:
dcedfef6ab58e3dfc7606ed2eab8f21feec81ed6167bb52d99f6fadeb0ed0e35
```

### 4.2 DeepSeek 提取的 source facts

proposal 包含 5 个带源码行号引用的 observed facts，覆盖：

- header 中的完整 ABI；
- host 异步清零 `output_count`；
- `count == 0` 时不启动 kernel；
- kernel launch geometry；
- 每个满足条件的线程通过 `atomicAdd` 获取输出槽；
- `index >= count` 时返回。

最关键的一条是：

```text
f-kernel-atomic-add:
每个 in-bounds thread 读取 input[index]；
若 value > threshold，则通过 atomicAdd(output_count, 1)
获取输出槽，并把 value 写入对应槽位。
```

它绑定源码路径和精确行范围，不是脱离源码的一段评论。

### 4.3 三个竞争假设

#### 假设 A：set-like / multiset-like compaction

ID：

```text
h-compact-set-order-unspecified
```

含义：

```text
必须准确输出所有严格大于 threshold 的 occurrence；
output_count 必须准确；
顺序不属于 contract；
任何 qualifying values 的排列都可以。
```

这是后来被用户选择的假设。

#### 假设 B：保留当前 source scheduling behavior

ID：

```text
h-source-behavior-order-authoritative
```

含义：

```text
迁移结果应保留当前 CUDA 实现的调度相关顺序行为；
输出排列来自线程竞争 atomicAdd 的先后顺序。
```

这个假设代表一种 source-preserving 解释。它不是要求固定一个序列，而是要求保留“顺序由并行调度竞争形成”
这一行为特征。

#### 假设 C：要求 stable input order

ID：

```text
h-stable-input-order-required
```

含义：

```text
输出必须保持输入中 qualifying values 的相对顺序。
```

对于样例输入 `[1, 4, 3, 2]`，稳定输出必须是 `[4, 3]`，而 `[3, 4]` 应当失败。

### 4.4 SIR 为什么不能自己选择

DeepSeek 能从源码看出当前 CUDA 实现使用 `atomicAdd`，因此当前实现不提供 stable ordering。但它无法从源码
证明用户不关心顺序，也无法证明用户要求保留调度相关行为。这属于 desired semantics，不是 source fact。

所以 proposal 明确形成：

```text
unknown:
u-output-order-contract

conflict:
c-output-order-contract
```

并提出消歧问题：

```text
输出顺序是否属于期望 contract？

如果属于：
- 必须稳定保持输入顺序？
- 还是必须保留当前 source scheduling order？

如果不属于：
- 任意 qualifying-value permutation 是否都可接受？
```

这就是 SIR 的实际价值：它没有替用户做决定，而是把一个容易被工程师无意识冻结的实现细节，提升成一个
明确、可授权的问题。

## 5. Model-free process 把 proposal 收敛成用户问题

DEV-007 不再调用模型。独立的 model-free process 从 exact proposal graph 中机械查找：

- `DesiredSemantics` 类型的 unknown；
- 与它绑定的 conflict；
- 同一个 disambiguation experiment；
- conflict 中的 exact hypothesis options。

然后生成 `UserIntentDecisionRequestV1`。这个 request 不是人工摘要，而是绑定：

```text
exact proposal ID
exact recovery input ID
caller unknown context
unknown ID
conflict ID
exact hypothesis IDs
每个 hypothesis 的 layer / claim / domain
允许的响应种类
```

该样例生成的问题包含三个 exact options：

```text
1. h-compact-set-order-unspecified
2. h-source-behavior-order-authoritative
3. h-stable-input-order-required
```

允许的回答不是模糊的 `yes/no`，而是：

```text
select-hypothesis
keep-unknown
provide-authoritative-claim
```

因此用户可以选择已有假设、明确保持未知，或者提供新的 authoritative claim。系统不会为了自动化率强迫
用户从模型给出的选项中选一个。

## 6. 用户作为实际 task authority 做决定

实际选择是：

```text
h-compact-set-order-unspecified
```

并明确了机器可执行语义：

```text
membership:
exact-selected-occurrences

reported_count:
exact-selected-occurrence-count

order:
unspecified-permutation

selection_claim:
copies-strictly-above
```

这不是把 hypothesis prose 标记为 `approved=true`。真正的 `UserIntentDecisionV1` 同时绑定 exact decision
request、exact authority grant、selected hypothesis 和强类型 authoritative claim。

用户确认的机器语义是：

> 对每一个严格大于 threshold 的输入 occurrence，都必须在输出中出现一次；不能丢失、不能新增、不能篡改
> multiplicity；reported count 必须准确；排列顺序不重要。

这里使用 occurrence multiset，而不是数学 set。比如输入 `[4.0, 4.0, 1.0]`、threshold 为 `2.0` 时，正确
结果必须包含两个 `4.0` occurrence，不能因为数学 set 里只有一个 `4.0` 而只返回一个。

## 7. Admission 把用户决定提升为 contract

SIR proposal 没有 authority。User decision 有 task authority，但仍需要 Admission 验证整条 binding 是否完整、
未被替换。

DEV-008 建立了独立的 Admission process：

```text
cairn-sir
    只产生 proposal
    无 restricted store
    无 Admission dependency

cairn-admission
    不链接 model/provider/network runtime
    只读 public artifacts
    可写 Admission restricted store
```

### 7.1 Admission 验证什么

Admission 重新读取并验证：

```text
proposal
recovery input
decision request
authority grant
user decision
```

然后检查：

- task 是否一致；
- proposal 是否绑定该 recovery input；
- decision 是否回答该 exact request；
- authority grant 是否覆盖该 task 和 claim scope；
- selected hypothesis 是否真的在 request options 中；
- authoritative claim 是否与授权 scope 一致；
- canonical bytes 和 typed identity 是否一致；
- schema 是否是 current V1；
- 是否有未知字段或伪造字段。

任何一个环节不一致，都不能生成 contract。

### 7.2 `MigrationIntentContractV1`

成功后形成 `MigrationIntentContractV1`，绑定：

```text
task
recovery input
proposal
decision request
authority grant
user decision
selected hypothesis
admitted authoritative claim
```

它没有改写原 proposal。原 proposal 仍然只是在说“这里有三个竞争解释”；contract 则表示实际 authority 已经
为当前 task 选择 unspecified permutation，并把 exact occurrence membership 与 exact reported count 作为
正式迁移语义。

### 7.3 restricted/public 分离

Admission 内部提交 `RestrictedIntentAdmissionDecisionV1`，它绑定 exact mechanism 与 contract identity。
外部只得到 `IntentAdmissionPublicOutcomeV1`，其中包含公开 contract 和 opaque restricted-decision ID。

SIR process 没有读取 restricted store 的能力。不同 UID smoke 已验证：

- SIR principal 无法读取 restricted directory；
- Admission principal 可以写 restricted store；
- 普通 workspace 用户不能冒充该读取能力。

## 8. Contract 真正改变 Oracle 行为

如果 SIR 只生成分析报告，而不改变下游行为，它就没有形成产品价值。当前实现已经证明，它确实改变了 Oracle
comparator。

Admission 从 contract 机械派生 `CollectionOutputOracleDecisionV1`。由于 contract 中是：

```text
order = unspecified-permutation
```

所以 Oracle policy 必须是：

```text
ExactMultisetAndCount
```

而不能是 `ExactSequenceAndCount`。这不是 caller 随手传一个 enum，也不是 Candidate 自己选择 comparator；它
只能从 admitted contract 派生。

给定：

```text
input     = [1.0, 4.0, 3.0, 2.0]
threshold = 2.0
```

trusted side 独立得到：

```text
expected occurrences = [4.0, 3.0]
expected count       = 2
```

不同 observation 的结果是：

| Observation | 结果 | 原因 |
| --- | --- | --- |
| `[4.0, 3.0], count=2` | 通过 | 元素、multiplicity、count 都正确 |
| `[3.0, 4.0], count=2` | 通过 | 顺序不重要 |
| `[4.0], count=1` | 失败 | 丢失一个 occurrence |
| `[4.0, 4.0], count=2` | 失败 | multiplicity/元素错误 |
| `[4.0, 3.0], count=1` | 失败 | reported count 错误 |
| `[4.0, 3.0, 5.0], count=3` | 失败 | 多出错误元素 |
| 正确 prefix，但 count 超出 output capacity | fail closed | 非法 observation |

如果用户选择的是 `h-stable-input-order-required`，comparator 就会选择 sequence-sensitive policy，`[3, 4]`
将失败。

所以 SIR 的效果不是让文档更漂亮，而是改变了 Candidate correctness space：

```text
允许 stable prefix-scan 实现
允许 unordered parallel compaction 实现
不要求复制 CUDA atomicAdd 的调度行为
```

## 9. 真实 child process，而不只是比较两个内存数组

DEV-009 把 Oracle 从内存 comparator 推进到真实 process observation：

```text
contract-selected Oracle decision
+ input/threshold case
→ call-adapter input
→ 独立 host child process
→ ABI output files
→ generic execution receipt
→ trusted materializer
→ comparison evidence
```

### 9.1 implementation side 看得到什么

call adapter 只得到：

```text
input bytes
threshold bytes
output allocation
reported-count output allocation
invocation identity
decision/contract identity
```

它看不到 expected elements、expected count、comparison result 或 qualification controls。

### 9.2 child process 实际输出

honest adapter 对样例输入故意以反序输出：

```text
values       = [3, 4]
reported_cnt = 2
```

generic execution 层：

- 启动独立 child；
- 捕获 declared outputs；
- 归档 exact bytes；
- 形成 authoritative execution receipt；
- receipt 绑定 executable、job、attempt、output identities。

trusted materializer 从 receipt 指向的 output bytes 重建 `ObservedCollectionOracleOutputV1`，然后使用
contract-selected `ExactMultisetAndCount` 比较，结果是 `Equivalent`。

这里证明的是：用户选择的高阶意图已经影响了一次真实进程输出的解释。但它仍然不是 CUDA GPU 或 Ascend NPU
执行，目前是 host-isolated adapter path。

## 10. 局部 Oracle mechanism qualification

仅仅写了 comparator，并让一个 honest case 通过，还不足以说明该 Oracle mechanism 可以成为后续 Candidate 的
authority。DEV-010 做的是一次很窄、与当前 mechanism 绑定的资格化。

它没有恢复以前庞大的第三人 review framework，也没有建立通用 qualification registry。

### 10.1 honest implementation

输入 `[1, 4, 3, 2]`、threshold 为 `2`，输出：

```text
[3, 4]
count = 2
```

因为顺序 unspecified，所以必须接受。

### 10.2 fault implementation

另一份不同 executable 故意丢失一个 occurrence：

```text
[3]
count = 1
```

它走完全相同的 process、ABI output、generic receipt、trusted materialization 和 comparator 路径，最终必须
被明确拒绝。

### 10.3 qualification receipt 绑定什么

qualification receipt 绑定：

```text
Oracle proposal
mechanism identity
qualification gate identity
honest executable identity
honest invocation/receipt/comparison
fault executable identity
fault invocation/receipt/comparison
limitations
requalification triggers
```

它证明的不只是一个内存断言，而是：

> 当前具体的 reference、materializer、receipt validation 和 comparator 组合，能够在真实 process 路径上
> 接受一个应当接受的实现，并拒绝一个应当拒绝的独立实现。

如果 mechanism source identity 变化，该资格结果不能被悄悄沿用，需要重新运行控制。

### 10.4 当前资格范围

当前只覆盖：

```text
host-isolated adapter
finite normal nonzero f32
strict >
exact binary32 element identity
exact occurrence multiset
exact reported count
reported prefix
unordered comparison
```

它不覆盖：

```text
NaN
Inf
signed zero
subnormal
numerical tolerance
capacity error semantics
CUDA device
Ascend NPU
memory safety
performance
完整 operator domain
```

因此产物是 local-only / partial 的 `AdmittedCollectionOracleClaimV1`，不是 full Oracle portfolio。

## 11. Restricted commit 后才能交给 Candidate

DEV-010 只是准备出了一个 admitted local claim。构造出 Rust value 不等于已经完成 authority publication。
DEV-011 关闭了这个缺口。

### 11.1 restricted commit 顺序

`commit_collection_oracle_admission` 依次归档：

```text
1. Oracle claim proposal
2. honest comparison evidence
3. fault comparison evidence
4. qualification receipt
5. admitted local claim
6. restricted Oracle Admission decision
```

所有材料都按 exact typed identity 写入 Admission-owned restricted store。任何一次写入失败，函数都不会返回
public outcome。

这里采用的是 `commit-before-publish`，而不是先告诉 Candidate “Oracle 已经可用”，然后再尝试保存
qualification evidence。

测试注入了一个必然失败的 `ContentStore`，结果是 Storage error 且没有 public outcome。CAS 中此前可能已经
成功写入的不可变材料不需要回滚，因为它们本身不是 publication authority；没有最终 public outcome，Candidate
就不能消费它们；后续相同 bytes 也可以安全 deduplicate。

### 11.2 public outcome 包含什么

提交成功后返回 `CollectionOracleAdmissionPublicOutcomeV1`，包含：

```text
schema_version = 1
完整 MigrationIntentContractV1
AdmittedCollectionOracleClaimV1
opaque restricted decision identity
```

嵌入完整 intent contract，而不是只重复几个调用者提供的字符串，是为了让反序列化时能够重新检查：

```text
claim.contract_id == embedded_contract.identity()
```

### 11.3 public outcome 不包含什么

它不包含：

```text
expected output
honest output
fault output
comparison evidence 正文
execution receipt 正文
executable bytes/identity
qualification limitations 正文
requalification triggers 正文
```

这些材料仍在 restricted side。public side只知道 Admission 已经对 exact contract 和 exact local claim 做出了
一个可追溯的决定。

当前仍有一个边界：DEV-011 的函数已经做到 restricted commit 后才返回可发布 outcome，但通用 Controller
workflow event/outbox 尚未实现。目前由受信调用者接住 canonical outcome，并继续生成 Candidate 输入。因此
当前是“publishable public outcome 和第一消费者已接通”，不是“完整生产 Controller publication subsystem 已
完成”。

## 12. Answer-free Candidate 输入

成功 commit 后，系统机械生成 `CollectionCandidateSearchInputV1`。其字段形状大致是：

```json
{
  "schema_version": 1,
  "task_id": "...",
  "recovery_input": "...",
  "intent_contract": "...",
  "oracle_outcome": "...",
  "oracle_claim": "...",
  "selection_claim": "copies-strictly-above",
  "domain": "finite-normal-f32-strictly-above-threshold",
  "strength": "exact-occurrence-multiset-and-reported-count",
  "scope": "local-oracle-exploration-only"
}
```

以上是便于阅读的字段示意，不是某个真实 artifact 的逐字 canonical bytes。实现位于
[`candidate_search.rs`](../../crates/cairn-migration/src/candidate_search.rs)。

### 12.1 Candidate 得到什么

Candidate 可以知道：

```text
任务是谁
caller/recovery input 是谁
已经 admitted 的 intent contract 是谁
公开 Oracle outcome 是谁
可依赖的 local claim 是谁
selection semantic 是 strictly-above
当前 claim domain 是什么
当前 evidence strength 是什么
scope 只能用于 local exploration
```

### 12.2 Candidate 没有得到什么

输入字节经过 absence test，不能包含：

```text
qualification_receipt
comparison
execution_receipt
executable
expected
```

DeepSeek Candidate可以知道它生成的Candidate需要满足exact qualifying-occurrence multiset和exact count，且
顺序不重要；但它看不到测试输入、expected `[4, 3]`、honest control `[3, 4]` 或 fault control `[3]`。

这就是 answer-free。

### 12.3 为什么 scope 是 `LocalOracleExplorationOnly`

当前 claim 太窄，不能支持 Candidate verdict、Migration verdict、release 或 CP1 complete。

Rust compile-fail boundary 证明 `CollectionCandidateSearchInputV1` 不能传给要求完整
`AdmittedOracleV1` 的 API。该限制不是注释提醒，而是静态类型不兼容。

### 12.4 真实 Candidate episode 如何消费该输入

DEV-012没有新建一个预想中的Candidate子系统，而是把第一个真实consumer接到现有`cairn-agent` durable
runtime：

```text
exact Candidate search input
+ exact recovery input
+ task bundle manifest（只有路径、行数和identity）
+ fixed Candidate instruction/tool catalog/budget
→ DeepSeek continuation
→ candidate_read_task_artifact（ReadOnly）
→ candidate_submit_collection_proposal（Pure）
→ gateway注入search/model/episode provenance
→ immutable CollectionCandidateProposalV1
```

初始model request不含任何source bytes。真实live episode的第一回合调用了3次read tool，分别读取冻结task
bundle里的3个公开文件；模型不能改读仓库其他路径，也没有hidden/restricted tool。第二回合提交source，第三回合
在看到accepted tool result后`Yielded`。

模型提交了3个canonical relative-path文件：

```text
CMakeLists.txt
include/compact_above.h
src/compact_above.cpp       ← primary source
```

proposal中的设计大意是：保留一个C-linkage host入口，用`aclrtStream`接收调用方stream；host侧先异步清零
`output_count`，然后启动单AI-core Ascend C kernel；kernel顺序扫描input，将严格大于threshold的值写入output，
最后写回count。模型同时明确列出尚未解决的环境假设：CANN headers、`ACLRT_LAUNCH_KERNEL`、GM scalar access
API和device-visible pointer约定是否在实际目标toolchain中可用。

这段解释只能理解为模型的实现意图，不能理解为正确性证据。特别是：

- `candidate_submit_collection_proposal`是`Pure` tool，不执行编译器或设备；
- gateway只检查V1 shape、path/order/size/primary membership和provenance，不审核CANN API是否真实可编译；
- proposal绑定了exact answer-free search input、episode和model configuration，但没有build receipt；
- terminal restart恢复了同一个proposal，只证明durability，不证明source correctness。

live evidence是：

```text
episode  = episode:01a04bb8-3eb5-7c50-9bc8-00f4eddd35b1
proposal = cairn:v1:sha256:migration.candidate-collection-proposal.v1:
           41809ea7233868fc33cfc23c099d80192c4625dc66b9031f00f76e7101055a38
steps    = 3
terminal = Yielded
restart  = recovered
```

因此DEV-012证明的是“真正的Candidate模型已经入场并越过answer-free边界”，不是“Ascend迁移已经成功”。

## 13. 当前角色与权限

| 角色 | 当前职责 | 能看到 | 不能做 |
| --- | --- | --- | --- |
| Repository coding agent | 构建通用应用、类型、runtime、gate、测试 | 仓库代码和开发 fixture | 不能根据 fixture 答案冒充 runtime proposal |
| DeepSeek SIR actor | 面对每个 task 读取 scoped source，提出事实、竞争假设和 unknown | caller public declaration、task-local source、授权 context | 不能 admit intent、读取 restricted evidence、决定用户语义 |
| task authority | 回答 desired-semantics 问题 | exact scoped request 和 options | 不负责伪造 execution evidence |
| Admission process | 验证 authority/binding，形成 contract 和 Oracle admission decision | public authority artifacts、restricted store | 不调用模型、不从 prose 猜语义 |
| execution/coordinator | 执行 adapter 并产生 authoritative receipt | execution input、output allocation | 不决定 desired semantics |
| trusted Oracle | 派生 expected、重建 observation、比较 | contract、trusted reference、receipt outputs | 不向 Candidate 暴露 expected |
| DeepSeek Candidate actor | 已生成首个 Ascend C/CANN source proposal | source/public context、answer-free Candidate input | 不能读取 hidden expected、自判 verdict或宣称build evidence |
| 固定第三人 reviewer | 当前关键路径中没有该角色 | — | 不再作为每个 slice 的强制仪式 |

DEV-002 那种“每件事都要第三人审查”的预建框架已经 superseded，并从 current tree 删除。现在保留的是：

- runtime DeepSeek 是未编写 fixture 的实际 actor；
- task authority 回答只有自己有权回答的语义；
- Oracle mechanism 使用可重复自动控制；
- 只有出现真实独立性或安全需求时，才增加新的 reviewer/process。

## 14. DEV-004 到 DEV-012 的进展

| Slice | 完成内容 | 在样例中的意义 |
| --- | --- | --- |
| DEV-004 | task-generic DeepSeek SIR episode | 证明真实模型可通过 bounded tools 提交 strict proposal |
| DEV-005 | 第二个实质不同的 atomic compaction task | 证明 SIR 不只适用于 reduction，并实际改变 output-order Oracle 选择 |
| DEV-006 | 完整 `IntentRecoveryInputV1` 和 proposal V1 | caller/source/proposal 来源分离；真实 DeepSeek strict repair 成功 |
| DEV-007 | model-free typed decision request | 把整份 proposal 收敛为一个 scoped output-order 问题 |
| DEV-008 | typed user decision、Admission、contract | 用户选择成为强类型 contract，并驱动 unordered comparator |
| DEV-009 | actual child output、receipt、materialization | `[3,4]` 真实进程输出被 contract-selected Oracle 接受 |
| DEV-010 | honest/fault qualification | honest reversed output 通过，missing-occurrence 实现失败，形成 local claim |
| DEV-011 | restricted commit、public outcome、Candidate input | 只有提交 restricted evidence 后，才产生 answer-free Candidate 输入 |
| DEV-012 | bounded real DeepSeek Candidate episode | 3次按需读源码后提交3文件typed proposal；3步yield并通过terminal restart |

DEV-011 对应提交：

```text
8f0bcac feat: publish local oracle to candidate input
```

更细的实施记录见：

- [`DEV-006-IMPLEMENTATION.md`](records/DEV-006-IMPLEMENTATION.md)
- [`DEV-007-IMPLEMENTATION.md`](records/DEV-007-IMPLEMENTATION.md)
- [`DEV-008-IMPLEMENTATION.md`](records/DEV-008-IMPLEMENTATION.md)
- [`DEV-009-IMPLEMENTATION.md`](records/DEV-009-IMPLEMENTATION.md)
- [`DEV-010-IMPLEMENTATION.md`](records/DEV-010-IMPLEMENTATION.md)
- [`DEV-011-IMPLEMENTATION.md`](records/DEV-011-IMPLEMENTATION.md)
- [`DEV-012-IMPLEMENTATION.md`](records/DEV-012-IMPLEMENTATION.md)

## 15. 当前可以跑通什么

当前可以跑通：

```text
完整 caller/source 输入
→ 真实 DeepSeek SIR
→ strict typed proposal
→ model-free 用户问题
→ typed 用户决定
→ 独立 Admission promotion
→ MigrationIntentContract
→ contract-selected Oracle policy
→ actual host child process
→ authoritative execution receipt
→ trusted materialization/comparison
→ honest/fault qualification
→ local admitted Oracle claim
→ restricted commit
→ public Oracle outcome
→ answer-free Candidate search input
→ real DeepSeek Candidate episode
→ bounded task-source reads
→ typed Ascend C/CANN source proposal
```

其中：

- SIR live model 跑过；
- strict repair 跑过；
- SQLite/CAS restart/replay 跑过；
- independent SIR/Admission process 跑过；
- different-UID capability smoke 跑过；
- actual host child process 跑过；
- honest/fault qualification 跑过；
- restricted-store failure control 跑过；
- answer-free live Candidate、pure typed submission与terminal restart跑过；
- 全量 CI、Clippy、doc compile-fail 全部通过。

## 16. 当前不能声称什么

当前还没有：

```text
Candidate revision lineage
Ascend build
CUDA reference device run
Ascend NPU run
cross-platform result comparison
diagnostic correction loop
Candidate verdict
Migration verdict
performance result
完整 Oracle portfolio
完整 domain coverage
CP1 First Migration Outcome
```

因此目前不是“CUDA → Ascend C 迁移已经完成了 80%”。更准确地说：

> 我们已经完成首条迁移 workflow 的前半段 authority 和 Oracle 接线，并让Candidate actor基于安全、answer-free
> 的任务交接面提交了第一份source proposal；现在停在真实build evidence之前。

从产品价值上看，最关键的结果是：Candidate生成代码时拿到的不是fixture答案，也不是repository coding agent
代写的实现，而是一个经过真实SIR、用户授权和局部Oracle admission的迁移contract；实际source则来自configured
runtime model。proposal identity和durability已经成立，但技术正确性仍要由后续build/execution evidence回答。

## 17. 哪些严谨性有实际作用

| 边界 | 防止的问题 |
| --- | --- |
| SIR proposal-only | 防止模型把推测直接变成产品语义 |
| caller/source 类型分离 | 防止把 CUDA 实现偶然性冒充用户要求 |
| user decision request | 防止给用户一整篇报告，却没有定位真正要决定的问题 |
| authority grant | 防止任意字符串身份修改 task semantics |
| Admission promotion | 防止 proposal 或聊天回答直接冒充 contract |
| contract-selected Oracle | 防止 Candidate 或调用者选择对自己有利的判定规则 |
| expected/Candidate 输入隔离 | 防止生成模型看到测试答案 |
| authoritative receipt | 防止普通内存值或测试断言冒充真实执行 |
| honest/fault qualification | 防止只能通过 happy path 的 comparator 被当成可信 Oracle |
| commit-before-publish | 防止没有保存 restricted evidence 就先发布 authority |
| local-only Candidate scope | 防止局部 f32 claim 被外推成完整迁移 verdict |
| compile-fail strong types | 防止不同 authority 因为都是 String/ID 而被混用 |

同时，已经删除或明确没有实施的过度建设包括：

- 固定第三人 reviewer ceremony；
- 完整 qualification registry；
- 七类 Planner；
- 十一位置 Agent catalog；
- 没有 consumer 的 Candidate crate；
- 通用 outbox/recovery framework；
- historical schema 兼容路径；
- 把某个 fixture 的答案固化进 production prompt。

当前方向不是不讲严谨，而是只保留能够阻止真实错误、并且已有下游 consumer 的严谨性。

## 18. 下一步

> 本节记录 DEV-012 当时的下一步，DEV-013–020 已经完成这里描述的 build/repair 路径。当前下一步见
> [`NEXT_SESSION.md`](NEXT_SESSION.md)，不要从本节继续人工打开新 repair。

下一步不是继续扩建SIR、Candidate registry或review，而是让exact DEV-012 proposal遇到第一条真实
target/toolchain build boundary：

```text
CollectionCandidateProposalV1
+ explicit actual target/toolchain/environment facts
→ materialize proposal files
→ existing no-device Ascend build worker/toolchain
→ authoritative build receipt and diagnostics
```

预期边界是：

- 不猜测target SoC、CANN版本或toolchain；这些在recovery input中仍是`not-selected`；
- 先选择能够消费该exact 3-file proposal的最短真实build lane；
- build worker只返回authoritative build fact/diagnostic，不产生Candidate或verdict；
- 首次build失败若发生，才根据真实diagnostic切出最小revision consumer；
- build成功也只证明可构建，不证明CUDA/Ascend语义等价，更不自动形成Candidate verdict；
- 不预建revision topology、完整execution portfolio或performance系统。

整个当前方案可以概括为：

> SIR 负责发现“我们可能想要什么”，用户负责决定“我们确实想要什么”，Admission 把决定变成 contract，
> Oracle 把 contract 变成可执行判定，Candidate 模型随后在不知道答案的情况下生成 Ascend C。当前Candidate
> 已经提交第一份非权威proposal，下一步让它接受真实toolchain事实检验。
