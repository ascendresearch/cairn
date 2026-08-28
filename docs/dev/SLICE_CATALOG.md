# Cairn Development Slice Catalog

- 状态：当前近期slice catalog；future work按runtime evidence再切片
- 日期：2026-08-28
- 产品范围：仅限 CUDA → Ascend C 算子移植
- 路线图：[`ROADMAP.md`](ROADMAP.md)

## 1. Catalog规则

只登记近期、可证伪且有当前consumer的slice。不再为尚无runtime evidence的完整目标架构预先编号。
Fixture或类型本身不是vertical result；一个Agent能力必须包含实际runtime-model/recorded episode消费者。

`DesignConformanceRecord`只在slice改变authority、restricted/secret visibility、external effect、public API或
persisted/wire contract时需要。其他slice使用本表objective、tests和scope note即可。

## 2. 已完成与被纠正的基础

| ID | 状态 | 当前意义 | Current evidence / disposition |
| --- | --- | --- | --- |
| `DEV-001` | Accepted | D-039 clean-room reduction evaluator fixture | commit `9dc8243`；只供evaluation，不是runtime knowledge或product recipe |
| `DEV-002` | Superseded | 曾预建D-040十项qualification exam/review framework | D-042判定为value proof前的过早机制；current code/tests/public/private bundle删除；历史commit保留在Git，不构成authority |
| `DEV-003` | Accepted | 最小fixture provenance/sanitation基础 | commit `79a1174`；可被新的evaluation fixture复用 |

DEV-002的supersession不撤销历史上review曾发生的事实，但其artifact不再是current V1输入、entry gate或future
mechanism authority。不得通过compatibility path复活。

## 3. 当前critical slices

| ID | 状态 | Objective | 依赖 | 专属退出证据 |
| --- | --- | --- | --- | --- |
| `DEV-004` | Accepted | 复用现有`cairn-agent`建立task-generic DeepSeek SIR proposal episode | current agent runtime；DEV-001作为evaluation-only input；D-042；implementation note `be2985a` | commit `9e4711e`；recorded/full CI green；live episode `episode:01a04855-1c39-78b0-897e-ae5ff585c7ed`提交strict proposal并通过terminal restart |
| `DEV-005` | Accepted | 用同一production path运行reduction和一个实质不同CUDA task，并做SIR go/no-go | DEV-004 | [`DEV-005 evaluation`](records/DEV-005-EVALUATION.md)；atomic compaction live成功；SIR改变order-sensitive Oracle选择；CP0 Go |
| `DEV-006` | Accepted | 用完整caller/source分离的`IntentRecoveryInputV1`驱动同一runtime episode，并提交可供后续claim admission消费的完整typed hypothesis set | DEV-005 Go；[`DEV-006 DCR`](records/DEV-006-IMPLEMENTATION.md) | strict V1、recorded、absence与full CI green；live episode `episode:01a048a1-7279-7b22-807b-8756963ace78`严格修复后提交完整proposal并通过terminal restart |
| `DEV-007` | Accepted | 由model-free process消费exact public SIR proposal，机械生成首个claim-scoped `UserIntentDecisionRequestV1` | DEV-006；[`DEV-007 DCR`](records/DEV-007-IMPLEMENTATION.md) | strict process/current-V1/negative controls green；exact live proposal生成output-order request，实际任务authority选择`h-compact-set-order-unspecified` |
| `DEV-008` | Accepted | 将exact typed user decision机械promotion为首个`MigrationIntentContractV1`，并仅用它驱动collection-output Oracle policy | DEV-007；[`DEV-008 DCR`](records/DEV-008-IMPLEMENTATION.md) | separate SIR/Admission child process、different-UID authority smoke、restricted commit、exact live replay与contract-only Oracle comparator green |
| `DEV-009` | Accepted | 让DEV-008 admitted collection contract约束现有call-adapter的真实ABI observation与comparison evidence | DEV-008；[`DEV-009 record`](records/DEV-009-IMPLEMENTATION.md) | expected隔离、双ABI output、authoritative receipt、unordered controls、exact replay与full CI已闭合；不扩展portfolio/planner/hidden/device |

DEV-004 implementation、recorded lane、full CI和用户明确授权的live DeepSeek lane均已闭合。Live使用同一
product runner，经5个bounded reads提交strict cited proposal，3 steps后yield，并通过terminal restart；它不需要
另一轮private fixture review。该结果只解除DEV-005依赖，不证明SIR已有downstream value。

## 4. DEV-004边界

允许：

- 在当前product crate中增加最小SIR profile/context/proposal adapter；
- 复用`cairn-agent`的durable episode、DeepSeek protocol、tool loop和recorded/live transport；
- scoped task-artifact inspection tools；
- task-generic typed proposal与strict decode；
- test harness在episode结束后读取DEV-001 expected results做evaluation。

禁止：

- production代码或prompt出现`reduce-sum-f32`、D-039 identity/domain/expected hypotheses；
- runtime proposal episode读取restricted corpus或review receipt；
- coding agent直接写出fixture正确答案冒充DeepSeek结果；
- 创建`cairn-sir`、`cairn-admission`、`cairn-proposal-host`空crate；
- 实现`MigrationIntentContract`、Mechanical Gate或mechanism qualification registry；
- 为未来Oracle/Candidate/Performance提前增加无consumer abstraction。

## 5. DEV-005 go/no-go

Go需要同时满足：

- 两个task共享相同profile schema、tool boundary和production control flow；
- runtime outputs引用各自task artifacts；
- unknown/conflict不会被强制折叠成一个答案；
- SIR对至少一个下游选择有可观察价值，或相对user-declared intent减少实际工作；
- cost/reliability在声明预算内。

No-go后：SIR离开critical path，保留最小task-generic extension seam；近期迁移由user-declared intent、静态
事实和自动verification驱动，待端到端架构稳定或真实consumer出现再扩展。No-go是合法优先级结论，不以扩
benchmark或增加评审延后，也不等于永久否定SIR。

DEV-005结论为Go；SIR当前成熟度是proposal-only seam，不是永久能力上限。它没有admit intent：第二个task的
proposal使downstream Oracle在owner回答顺序问题前禁止sequence-sensitive correctness claim，并把
current-source observation限定为count + multiset。容量与non-overlap仍来自owner brief，证明SIR不能替代
用户声明。

## 6. Future backlog（未切片）

DEV-005已经Go，DEV-008已闭合第一条窄的Intent authority→Oracle policy consumer。后续仍只按下一个真实consumer拆分：Oracle materialization、Candidate、CUDA/Ascend execution、
restricted evidence、mechanism qualification、performance、knowledge/skill、feedback和platform/release。
目标设计中的概念清单不是开发待办，也不自动获得ID或entry gate。
