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
| `DEV-004` | Proposed | 复用现有`cairn-agent`建立task-generic DeepSeek SIR proposal episode | current agent runtime；DEV-001作为evaluation-only input；D-042 | typed proposal含citations/competing hypotheses/unknown；recorded replay；opt-in live DeepSeek；model-visible context无fixture answer；无Admission/hidden/production special case |
| `DEV-005` | Blocked | 用同一production path运行reduction和一个实质不同CUDA task，并做SIR go/no-go | DEV-004 | 无production branch/prompt结构变化；与source-preserving/user-declared baseline比较；至少一个downstream utility或明确停止SIR |

DEV-004尚未进入`Ready`。开始前只需一个精简implementation note明确exact files、model-visible projection、
typed output、recorded/live commands、预算和删除路径；它不需要另一轮private fixture review。

## 4. DEV-004边界

允许：

- 在当前product crate中增加最小SIR profile/context/proposal adapter；
- 复用`cairn-agent`的durable episode、DeepSeek protocol、tool loop和recorded/live transport；
- scoped repository read/search tools；
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

No-go后：删除SIR product path，保留domain-neutral agent runtime；后续迁移由user-declared intent、静态事实和
自动verification驱动。No-go是合法产品结论，不以扩benchmark或增加评审延后。

## 6. Future backlog（未切片）

只有DEV-005 Go后才按第一个真实consumer拆分：Intent authority、Oracle、Candidate、CUDA/Ascend execution、
restricted evidence、mechanism qualification、performance、knowledge/skill、feedback和platform/release。
目标设计中的概念清单不是开发待办，也不自动获得ID或entry gate。
