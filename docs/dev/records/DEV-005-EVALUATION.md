# DEV-005 evaluation — cross-task SIR value gate

- 状态：`Accepted / CP0 Go`
- 日期：2026-08-28
- Slice：[`DEV-005`](../SLICE_CATALOG.md#5-dev-005-gono-go)
- 决策边界：保留proposal-only SIR preflight；不授权Admission、Gate或固定Agent topology

## 1. Exact comparison

两个task使用同一个`cairn_migration::run_sir_episode`、prompt、tool schema和control flow。第二次运行没有修改
`sir.rs`，只放宽live预算并让CLI输出本来就已归档的结构化proposal。

| Task | 语义形态 | Live facts |
| --- | --- | --- |
| `reduce-sum-f32` | shared-memory tree reduction；数值顺序、host validation、overlap与runtime status | episode `episode:01a04855-1c39-78b0-897e-ae5ff585c7ed`；5 reads；3 steps；proposal `987885…d825` |
| `compact-above-f32` | atomic append compaction；全局counter副作用；输出顺序不稳定；async stream ABI | episode `episode:01a04865-0ae6-7b90-acd0-e12da52ca622`；3 reads；3 steps；proposal `ba4c8a…2121` |

第二个task的model-visible root只有`source/`。[`TASK.md`](../../../fixtures/cuda-ascend/sir/compact-above-f32/v1/TASK.md)
是为本次synthetic evaluator task编写、并在episode结束后才使用的owner brief，不是来自真实项目用户；它没有
进入task manifest、prompt或tool result。该限制不影响cross-task control-flow检查，但降低value外推强度。

## 2. Second-task live evidence

- model/deployment：`deepseek-v4-pro` / `deepseek-responses`；
- task bundle：`cairn:v1:sha256:migration.sir-task-bundle.v1:ac851b44d57e326ebbabb044ac7b527397afc50a2767df45145328015ed8ac57`；
- proposal：`cairn:v1:sha256:migration.sir-intent-hypothesis-set-proposal.v1:ba4c8a645a01788e2c214e6bf4e977ea970615cdff49bc44766bde8d495e2121`；
- usage：11,888 input、4,309 output、8,832 cache-read tokens；远低于262,144 provider-token limit；
- 3个bounded source reads、1个strict proposal submission、3 steps、`Yielded`；terminal SQLite/CAS restart通过；
- evaluator source使用CUDA 12.4.131成功构建为static library；未执行GPU runtime；
- proposal包含3个竞争hypotheses：精确保留CUDA-visible行为、只保留语义压缩、保留extern-C/static-library
  ABI shape；unknown明确覆盖输出顺序、Ascend runtime映射和output capacity；所有facts/unknowns引用实际
  task-local lines。

## 3. Downstream utility

| 输入路径 | 能得到什么 | 对Oracle/Candidate的影响 |
| --- | --- | --- |
| source-preserving | 复刻`atomicAdd` slot allocation、CUDA stream/error ABI和当前非稳定顺序 | 容易把实现偶然性冻结成迁移contract，并过早排除stable prefix-sum candidate |
| owner-declared | strictly-above-threshold、async、output count、capacity至少`count`、non-overlap；顺序未声明 | 解决capacity/alias边界，但仍不能选择sequence comparator |
| runtime SIR | 用line citations分离“精确保留”与“semantic-only”方案，并指出atomic order与ABI portability冲突 | owner回答顺序前，禁止sequence-sensitive correctness claim；current-source observation只比较count + output multiset；stable与unstable candidate均保留 |

这是可观察的downstream decision，不是prose质量评分。SIR没有替代owner：capacity和non-overlap只能从owner
brief获得；它的价值是把source-preserving会静默冻结的选择显式化并绑定证据。

Reduction proposal与此不同，集中在tree-reduction order、overlap validation、status mapping和同步失败点；
第二个proposal集中在atomic output order、compaction semantics、capacity和stream ABI。Production prompt没有
case branch，输出也没有复用fixture-specific答案。

## 4. Go/no-go

结论：`Go`，但范围只到proposal-only SIR preflight。

- 保留`SirTaskWorkspace`、bounded reads、strict cited proposal和durable episode；
- 不创建Intent Admission、qualification registry、多Agent review或SIR专用crate；
- 下一步只为第一个真实CUDA→Ascend C迁移consumer规划最小intent/Oracle/candidate边界；
- 若真实迁移consumer没有使用proposal改变任何决定，再删除SIR仍是有效后续结论。

Remaining limits：只有2个task、2次live success；第二个owner brief是synthetic而非真实项目用户输入；未证明
广泛泛化、proposal correctness、Ascend build/device结果或成本稳定性。放宽预算只防止过早截断，不构成
quality evidence。
