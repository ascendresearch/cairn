# CUDA → Ascend C Oracle 自动生成调研报告

状态：调研结论，尚未形成产品或实现决策
日期：2026-08-27

## 1. 范围

本报告只研究 Cairn 作为 **CUDA → Ascend C 算子移植系统**时的 Oracle 设计与自动生成问题，不将 Cairn 泛化为通用代码生成、通用软件迁移或通用 agent 工程平台。

本报告关注：

- 学术界如何处理 test oracle、编译翻译验证、差分测试、变形测试和数值误差分析；
- 工业算子栈如何组织 reference implementation、输入样例、比较器和后端一致性测试；
- CUDA 与 Ascend C 的编程模型、浮点、并行、内存和设备执行差异对 Oracle 的要求；
- 哪些 Oracle 工作可以自动化，哪些结论不能由生成模型单方面授权。

本报告不提出具体代码改造，不代表当前 Oracle 数据结构、执行图或 admission policy 已经确定。

## 2. 核心结论

对 CUDA → Ascend C，可信的自动 Oracle 不能等同于“自动生成若干输入和 expected output”。它应当是一个具有以下信息的 Oracle 组合：

- 权威来源；
- 适用输入域与前置条件；
- 被检查的语义或安全性质；
- 比较关系与数值接受域；
- 与候选实现的独立性；
- 覆盖度、反例和已知盲区。

软件测试研究长期把自动 Oracle 视为尚未完全解决的问题。现有方法主要由 specification、contract、differential testing、metamorphic testing、formal verification 和 implicit runtime oracle 组合而成，没有单一方法能够覆盖真实算子移植的全部语义。[The Oracle Problem in Software Testing](https://discovery.ucl.ac.uk/id/eprint/1471263/)

生成模型或 LLM 生成的 Oracle 只能作为候选。对神经 Oracle 生成器 TOGA 的大规模复现实验显示，其生成的 assertion 中超过 47% 是假阳性，而正确 assertion 相对已有方法只增加约 0.3% 的故障检测能力。这说明“模型认为答案是什么”本身不构成权威。[Neural-Based Test Oracle Generation: A Large-scale Evaluation and Lessons Learned](https://arxiv.org/abs/2307.16023)

因此，Oracle 自动生成的实质不是让模型凭空创造真值，而是自动发现、组合、执行和审计多个有依据的正确性证据源。

## 3. 对 Cairn 当前实验方案的判断

当前 `matmul-zero-k`、固定 f32、固定 expected bytes 的实验有明确的工程价值。它能够验证：

- 模型可以产出结构化测试材料；
- 输入、期望结果和候选实现可以隔离；
- 执行、比较、归档和 replay 链路可以贯通。

但它证明的是 Oracle 执行管线可以工作，还没有证明 Oracle 能够被可信地自动生成。

零 K 矩阵乘法尤其容易产生虚假的安全感。恒定写零、跳过计算、错误读取被掩盖等实现都可能通过。即使后续增加一个非零 K 固定样例，也只是提高固定样例的强度，没有回答以下根本问题：

- 谁授权 expected value 正确；
- 该答案对哪些 dtype、shape、layout 和数值区域成立；
- 为什么采用精确比较或某个容差；
- 哪些 CUDA 行为是规范语义，哪些只是某次执行的偶然结果；
- Oracle 能否发现 tail、offset、tiling、多核、同步和未初始化错误；
- Oracle 是否会被针对固定输入特化的错误实现绕过。

近期 GPU kernel 生成基准也面临同样的问题。KernelBench 把 kernel 生成评估组织为 PyTorch workload 上的功能正确性与性能评估，但固定或有限输入仍不能代表完整算子契约。[KernelBench](https://proceedings.mlr.press/v267/ouyang25a.html)

Meta/PyTorch 的 BackendBench 已转向复用 OpInfo 边界语义、多样例测试和人工审查，并明确指出单一输入分布可能让恒定输出等错误实现通过。例如，对大规模正态分布向量求均值时，错误实现直接返回零也可能落在宽松容差内。[BackendBench correctness](https://github.com/meta-pytorch/BackendBench/blob/main/docs/correctness.md)

当前 Cairn 方案适合作为 Oracle 执行链路的 spike，但不应直接视为最终的 Oracle 自动生成范式。继续增加固定 matmul 样例不会自然解决 Oracle 的权威性、适用域和充分性问题。

## 4. Oracle 应当由哪些层次组成

对 CUDA → Ascend C，Oracle 至少应包含七个相互区分的层次。

| 层次 | 回答的问题 | 主要候选来源 |
| --- | --- | --- |
| 契约 Oracle | 哪些输入合法，输出、状态和错误行为是什么 | CUDA host 调用、kernel 签名、文档、已有测试、运行轨迹、框架 schema |
| 功能 Oracle | 数学、离散或状态语义应当是什么 | 独立 reference、CUDA 行为、框架实现、形式化语义 |
| 关系 Oracle | 没有精确输出时，哪些输入输出关系必须成立 | metamorphic relation、等价变换、分解与重组性质 |
| 数值 Oracle | 多大数值差异仍然属于正确结果 | dtype、累加精度、运算顺序、误差分析、特殊值政策 |
| 执行与集成 Oracle | 目标 kernel 是否以正确 ABI、配置和设备路径真实执行 | 编译/链接记录、launch trace、设备回执、框架 schema 与 alias 检查 |
| 安全 Oracle | 是否存在越界、竞争、未初始化和同步错误 | CUDA Compute Sanitizer、Ascend msSanitizer、运行时检查 |
| 充分性 Oracle | 当前测试和检查是否具有足够故障检测能力 | 语义分区、tiling 覆盖、历史故障、定向 mutation |

这些层次不能互相替代：输出相等不证明没有越界；sanitizer 无报告不证明数学语义正确；host adapter 成功返回不证明真实 NPU kernel 已执行；高代码覆盖率不证明比较器能观察到错误；mutation score 高也不证明 reference 本身正确。

工业算子栈通常采用类似的组合方式。CUTLASS Profiler 支持 cuBLAS、cuDNN、host/device reference 等不同 verification provider，可以扫描问题空间并分别配置比较方式，而不是只依赖一个输出来源。[CUTLASS Profiler](https://github.com/NVIDIA/cutlass/blob/main/media/docs/cpp/profiler.md)

ONNX 则把算子文档、逐算子 Node Tests、模型级 Tests 和 reference implementation 共同作为后端行为定义。测试同时承担验证后端和消除算子规范歧义的作用。[ONNX Backend Test](https://github.com/onnx/onnx/blob/main/docs/OnnxBackendTest.md)

Ascend C 官方样例通常提供 `gen_data.py` 生成输入与真值、`verify_result.py` 比较结果，并自动化 host 侧调用、编译和 NPU 运行。这说明官方工具链重点提供的是验证脚手架；算子真值函数和精度政策仍需要开发者给出，并没有由工具从 Ascend C 候选实现中自动推导。[Ascend C Kernel Launch Based on a Sample Project](https://www.hiascend.com/document/detail/en/canncommercial/800/opdevg/Ascendcopdevg/atlas_ascendc_10_0056.html)

### 4.1 Oracle 不是线性置信分数，而是证据图

原始信息源之间可能存在依赖，简单地把“通过了三个 reference”计为三份独立证据会高估可信度。例如：

- PyTorch reference 可能在 CUDA 上调用 cuBLAS，而另一个 CUDA provider 也调用同一库；
- CUDA 源实现和模型生成的 CPU reference 可能来自同一段错误的语义解释；
- CPU twin debug 与 NPU binary 使用相同 kernel 源码，但执行调度、指令和内存系统不同；
- expected output、comparator 和候选 Ascend C 实现如果由同一个 agent episode 同时生成，会产生明显的共同错误风险；
- 多个框架实现可能共享同一个底层 vendor library 或算法模板。

因此 Oracle 应保存的不是一个来源字符串或聚合置信分，而是一张证据依赖图。图中至少需要区分：

- 规范、文档、源码、测试、运行观测和人工决策等 provenance class；
- 每项 assertion、case、expected value 和 comparator 由什么材料推导；
- 各 reference 是否共享代码、库、模型推理、数据或执行后端；
- 哪些证据相互支持，哪些证据发生冲突；
- 一项证据失效时，哪些 verdict 随之失效。

“多源一致”只有在来源足够独立且适用域相交时才比单源更强。证据强度更适合使用离散、可审计的类别表达，而不是把来源数量压缩成一个缺乏语义的概率分数。

## 5. CUDA → Ascend C 特有的 Oracle 维度

### 5.1 契约和数据域

Oracle 需要描述和覆盖：

- entry point 与参数顺序；
- pointer 与 scalar 的语义区别；
- 标量宽度、signedness 和取值范围；
- tensor dtype、rank 和 shape 关系；
- layout、stride 和 contiguous 条件；
- pointer alias、in-place 和预分配输出；
- alignment、workspace 和 tiling metadata；
- empty tensor、零维度、非法输入及错误状态。

文档和源码中的约束可以自动抽取，但抽取结果只能是候选契约。DocTer 从 API 文档抽取输入约束并生成合法、非法和边界输入；ACETest 从算子输入检查代码提取约束，帮助测试进入核心逻辑。两者都证明约束抽取能够显著提升输入生成质量，但文档可能过时，代码检查也可能反映实现缺陷而非正确规范。[DocTer](https://www.cs.purdue.edu/homes/lintan/publications/docter-issta22.pdf)、[ACETest](https://arxiv.org/abs/2305.17914)

### 5.2 功能语义

需要根据算子类型显式处理：

- elementwise、reduction、scan、scatter/gather、sort/select；
- 广播、索引和越界规则；
- 重复索引、相等值和 tie-breaking；
- 空集合的 identity 或 rejection；
- 整数 overflow、饱和、截断和类型转换；
- NaN、Inf、signed zero 和 subnormal；
- 随机算子、原子操作和非确定性结果。

CUDA 实际输出可以是重要的源端行为证据，但不能无条件升级为规范真值。CUDA 官方文档明确指出，FMA、求值顺序、非标准数学函数、原子操作调度和硬件差异可能改变浮点结果。[CUDA Programming Guide](https://docs.nvidia.com/cuda/cuda-programming-guide/)

需要首先判断源程序行为属于哪一种情况：

1. 定义良好且确定：可以作为较强差分 Oracle；
2. 定义良好但存在合法结果集合：应采用集合、关系或统计 Oracle；
3. 存在 race、越界或其他未定义行为：不能生成正常功能 Oracle，应报告源端缺陷或要求策略决定；
4. CUDA 行为与外部算子规范冲突：必须由迁移政策决定保留行为还是修复语义。

### 5.3 Ascend C 执行语义

Oracle 必须覆盖 CUDA 与 Ascend C 编程模型之间的差异：

- CUDA grid/block/warp 到 Ascend BlockDim、AI Core、Vector/Cube 的任务划分；
- tile 边界和非整除 tail；
- GM/UB 数据搬运及 offset 计算；
- pipeline 数据依赖和同步；
- SetFlag/WaitFlag 配对；
- double buffering；
- 多核写入冲突；
- atomic 和 reduction 顺序；
- 输出区域是否被完整写入。

华为 Ascend C 最佳实践把同步插入、地址偏移计算和浮点硬件差异列为主要精度风险，并强调每次性能优化后重新检查精度。[Ascend C Normal Accuracy](https://www.hiascend.com/document/detail/en/canncommercial/850/opdevg/Ascendcopdevg/atlas_ascendc_best_practices_10_0005.html)

### 5.4 内存和并发安全

数值结果碰巧一致不能证明 kernel 正确。独立安全 Oracle 至少需要检查：

- out-of-bounds；
- misalignment；
- 未初始化读取；
- 多核和 pipeline contention；
- 同步指令配对；
- 内存泄漏和非法释放；
- 输出写覆盖完整性。

CUDA Compute Sanitizer 提供 memcheck、racecheck、initcheck 和 synccheck，可以作为独立于功能输出的安全 Oracle。[NVIDIA Compute Sanitizer](https://docs.nvidia.com/compute-sanitizer/ComputeSanitizer/index.html)

Ascend msSanitizer 也提供 memory、contention、uninitialization 和 synchronization 检查，但官方同时记录了调用模式、内存池和 mask vector 等检测限制。因此，“工具未报错”必须附带工具版本、运行方式和适用范围，不能被解释为完整正确性证明。[Ascend msSanitizer](https://www.hiascend.com/document/detail/en/canncommercial/850/devaids/optool/atlasopdev_16_0039.html)

### 5.5 数值接受域

不能为所有算子设置一个全局 `atol/rtol`。比较方式需要根据算子、dtype、输入区域和输出语义选择：

- exact 或 bitwise；
- absolute/relative tolerance；
- ULP；
- 集合成员关系；
- permutation 或 multiset；
- invariant/property；
- nondeterministic envelope。

容差需要记录推导依据，例如：

- accumulator dtype；
- reduction 长度和运算顺序；
- FMA 与非 FMA；
- 量化步长和舍入模式；
- 条件数与输入范围；
- 严格误差传播分析；
- 经批准的 reference 结果范围。

仅通过多跑几次、观察最大差异再放宽容差，得到的是经验包络，不是可靠误差界。经验包络可以作为低强度证据，但必须明确标注其性质。

FPTaylor、Daisy 等工具可以对受限的浮点表达式产生严格误差上界，但复杂控制流、循环和并行 reduction 仍有明显适用限制。因此，它们适合作为部分算子的数值子 Oracle，而不是统一比较器。[FPTaylor](https://github.com/soarlab/FPTaylor)、[Daisy](https://link.springer.com/chapter/10.1007/978-3-319-89960-2_15)

### 5.6 真实设备执行与集成契约

原报告遗漏了一个容易被功能测试掩盖的问题：Cairn 必须证明被比较的输出确实来自预期的 Ascend C binary、预期的 NPU 和预期的 launch configuration。

以下结果都不足以单独证明真实目标执行：

- Ascend C 源码能够编译；
- CPU twin debug 或 host 模拟路径输出正确；
- host wrapper 返回成功；
- 输出 buffer 中出现了预期值；
- 性能计时函数返回了一个数值。

其中输出 buffer 可能保留初始化值或旧值，wrapper 可能绕过 kernel，异步错误可能在返回后才暴露，CPU 模拟也不能覆盖 NPU 指令、调度、内存层次和多核行为。华为官方把 CPU twin debugging 与 NPU board debugging、msDebug、msSanitizer 和 profiling 列为不同调试路径；据此应将 CPU 与真实 NPU 结果视为不同强度的证据，而不是相互替代。[Ascend C Debugging and Tuning Overview](https://www.hiascend.com/document/detail/en/canncommercial/850/opdevg/Ascendcopdevg/atlas_ascendc_10_0072.html)

执行与集成 Oracle 至少应观察：

- 编译器、CANN、SoC 和设备身份；
- source、binary 和 launch artifact identity；
- kernel symbol、参数 ABI、参数顺序和字节宽度；
- BlockDim、tiling key、workspace 和 stream；
- launch 是否真正发生以及异步执行是否已同步完成；
- runtime status、device error、timeout、hang 和 crash；
- 输出是否由本次执行完整写入，可使用非平凡 sentinel 检测未写区域；
- 多次执行结果及其确定性包络；
- sanitizer、日志和设备侧证据是否对应同一次 execution。

框架集成还可能要求检查 schema、mutation/alias、预分配输出、抽象 shape 推导和 dispatch 行为。PyTorch 的 `torch.library.opcheck` 会分别检查 schema、autograd registration、FakeTensor/meta 和 AOT dispatch，说明值相等只是自定义算子集成契约的一部分。[PyTorch Custom Operators and opcheck](https://docs.pytorch.org/docs/main/library.html)

性能是正确性通过之后的独立目标。异步计时、warmup、cache 和输入 shape 会显著影响性能结论，但性能优越不能补偿任何功能、安全或集成证据缺失。

### 5.7 覆盖与充分性

测试数量或代码覆盖率不能直接代表 Oracle 充分性。至少需要考虑：

- dtype、shape、layout 和 alias 分区；
- empty、size-one、极值和特殊浮点；
- tile 整除与非整除；
- 单核与多核；
- 不同 tiling key 和 kernel 分支；
- 不同设备、CANN 和编译配置；
- ABI、launch configuration 和真实调用路径；
- 历史真实错误；
- CUDA → Ascend C 特有的定向 mutants。

Mutation testing 的作用是评价测试和 Oracle 能否观察到特定故障类别，例如删除 tail mask、改变 offset、删除同步、改 accumulator dtype 或跳过一次数据搬运。mutation score 衡量故障敏感性，不授权被测实现或 reference 的语义为真。

输入覆盖还需要区分“方便的随机分布”和“语义分区”。只使用均匀分布或正态分布容易遗漏零、极值、稀疏、重复值、强抵消、病态矩阵、特殊浮点以及 tile 边界。合理的自动生成应先建立分区，再在分区内部进行随机、搜索式或约束求解采样；case 数量不是首要充分性指标。

## 6. Oracle 自动生成的方法族

### 6.1 独立 reference

从算子数学定义或框架契约生成 CPU、高精度或朴素 reference，是最直接的 expected-output 来源。其优势是可为大量输入自动计算结果；主要风险是：

- reference 与候选实现共享同一错误理解；
- reference 隐式采用了不同 layout、rounding 或特殊值语义；
- reference 为追求方便而遗漏 alias、状态或错误行为；
- framework reference 本身不是用户 CUDA kernel 的实际契约。

因此 reference 必须记录来源、语义假设和与候选实现的独立性。

expected result 的来源至少应区分以下类别，不能都擦除成相同的字节数组：

1. 由明确规范直接计算或证明得到的 exact result；
2. 由高精度、区间或严格误差分析得到的数值范围；
3. 由独立朴素实现计算的 reference result；
4. 由经批准的框架或 vendor provider 给出的结果；
5. 由 CUDA 源程序执行观察到的 behavioral result；
6. 多个 provider 的一致结果；
7. metamorphic/property relation 所允许的结果集合；
8. 通过重复运行或学习得到的 empirical envelope。

这些类别不是完全线性的强弱排序。例如，一个精确定义的 property 可能比共享底层库的三个差分结果更有独立性；一个严格但只覆盖有限输入域的误差证明，也不能外推到域外输入。

### 6.2 CUDA 差分执行

对相同输入执行 CUDA 源程序和 Ascend C 候选实现，是迁移系统中不可缺少的一类 Oracle。它擅长捕获行为偏差，但存在四个盲区：

- 源程序本身错误；
- 两端存在不同但合法的浮点结果；
- 源端未定义或非确定性行为；
- 固定输入未触发隐藏分支、tail 或 race。

因此 CUDA differential oracle 必须和源端 sanitizer、输入域分区、独立 reference 或关系 Oracle 组合。

对于非确定性 CUDA kernel，单次输出比较尤其危险。应先判断非确定性的来源和合法范围，再选择适当关系：结果集合、permutation/multiset、统计性质、有界误差或重复执行一致性。重复运行只能发现已发生的变化，不能证明尚未观察到的结果不可能发生。

### 6.3 Metamorphic relation

当精确 expected output 难以获得时，可以检查输入变换与输出变换之间的必然关系，例如：

- permutation equivariance；
- 分块与整体计算一致；
- 加零、乘一等 identity；
- 线性或缩放关系；
- transpose/reshape 的对应关系；
- reduction 分解与重组；
- 不相关 padding 不影响有效输出。

变形测试已经用于深度学习算子，并能同时发现实现和精度错误。[A Miss Is as Good as A Mile: Metamorphic Testing for Deep Learning Operators](https://2024.esec-fse.org/details/fse-2024-research-papers/60/A-Miss-Is-as-Good-as-A-Mile-Metamorphic-Testing-for-Deep-Learning-Operators)

但 relation 本身也需要 Oracle：每条关系必须包含前置条件、dtype 限制、特殊值政策和数值比较规则。模型生成的 relation 仍然只能是待验证提案。

### 6.4 约束驱动和生成式输入

NNSmith、NeuRI、FreeFuzz 等工作表明，算子约束、已有测试、文档、真实模型和运行轨迹可以用于生成合法且多样的 tensor/graph 输入。[NNSmith](https://arxiv.org/abs/2207.13066)、[NeuRI](https://arxiv.org/abs/2302.02261)、[FreeFuzz](https://arxiv.org/abs/2201.06589)

这类技术主要解决“测什么输入”，并不自动解决“正确输出是什么”。输入生成器和结果 Oracle 应在模型与类型系统中保持明确区分。

### 6.5 翻译验证和形式化子 Oracle

translation validation 不证明整个 translator 永远正确，而是检查每次实际翻译产物是否保持源程序语义。Alive2 使用 SMT 检查 LLVM transformation 中 target 是否 refine source，并可生成反例。[Alive2](https://github.com/AliveToolkit/alive2)

GPUVerify 可以静态验证 CUDA/OpenCL kernel 的 race freedom 和 barrier divergence 等性质。[GPUVerify](https://fastpl.doc.ic.ac.uk/tools/GPUVerify/IEEE_TSE/)

对 CUDA → Ascend C，目前缺少覆盖两种完整编程模型、内存层次、设备 API 和浮点行为的共同形式语义。因此形式化方法适合：

- 验证受限算术片段；
- 验证索引和循环变换；
- 验证部分内存安全条件；
- 验证可规范化到共同 IR 的子问题。

它们不能在现阶段作为全系统唯一 Oracle。

### 6.6 受限生成与正确性保持变换

代码生成和算子生成领域还有一条重要路线：不在生成任意程序后再完全依赖外部 Oracle，而是把生成空间限制在已知保持语义的变换内。

Ansor 从高层声明式 tensor computation 出发，在受限的 loop transformation 和 schedule 空间内搜索高性能程序，并保存 transformation history；其可行性依赖于变换集合、依赖分析和底层 code generator 的正确性，而不是让搜索模型同时发明计算语义。[Ansor](https://arxiv.org/abs/2006.06762)

这对 CUDA → Ascend C 的启示不是假设两种编程模型可以直接机械等价，而是：

- 如果能把部分 CUDA 语义规范化为明确的 computation/index domain；
- 再通过有前置条件、可追踪的 tiling、partition、layout 和 pipeline 变换生成 Ascend C；
- 那么 transformation history 本身可以成为一类结构化证明义务和 Oracle 来源。

该路线能够减少自由代码生成带来的语义空间，但不能消除验证需求。错误的源语义抽取、错误的变换前置条件、code generator bug、设备数值差异和未建模的并发行为仍需要 differential、formal、safety 和 hardware Oracle。

### 6.7 等价变体与 defined-behavior 生成

编译器测试领域表明，高价值自动测试不仅需要随机性，还需要保证生成程序具有清晰、唯一的语义。Csmith 能有效发现编译器错误的重要前提，是生成有效 C 程序并避免 undefined/unspecified behavior，否则差分结果没有可靠解释。[Finding and Understanding Bugs in C Compilers](https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf)

Equivalence Modulo Inputs（EMI）则从一个程序和已知输入出发，生成在这些输入上应保持等价、但静态结构不同的程序变体，用于刺激不同优化路径。[Compiler Validation via Equivalence Modulo Inputs](https://www.microsoft.com/en-us/research/publication/compiler-validation-via-equivalence-modulo-inputs/)

对 Cairn，这类方法可以转化为候选的关系 Oracle 或充分性测试，例如：

- 改变合法的 CUDA block/grid 配置但保持覆盖域；
- 对输入做 layout-preserving 或 padding 变换；
- 采用不同但语义等价的分块和 reduction 组织；
- 在已知输入上改变不影响结果的未执行路径；
- 生成独立的朴素 Ascend C 或 CPU 变体。

但任何等价变换都必须声明适用输入、数值语义和并发前置条件。整数代数上的等价变换不一定在浮点、原子或存在 overflow 的语义下等价。

## 7. 哪些部分适合自动化

适合较强自动化的工作包括：

- 从 kernel 签名、host launch、断言、文档和已有测试中提取候选约束；
- 生成合法、非法、边界、tail 和特殊浮点输入；
- 构造多组随机但可 replay 的测试；
- 运行 CUDA、CPU reference、框架 reference 和 Ascend C；
- 生成适用于具体算子的 metamorphic relation 候选；
- 调用 CUDA 和 Ascend sanitizer；
- 根据移植风险生成定向 mutants；
- 自动归档 provenance、seed、环境、输出和最小化反例；
- 根据已有证据计算 Oracle 强度和未覆盖区域。

以下内容不能由模型单方面决定：

- intended semantics；
- CUDA 行为与数学或框架规范冲突时采用哪个权威；
- 不同硬件浮点结果的合法边界；
- metamorphic relation 是否成立及其完整前置条件；
- source kernel 本身有 bug 时是保留还是修复；
- 当前证据是否达到可交付或可替换原 CUDA kernel 的风险阈值。

### 7.1 调研归纳出的概念流程

这里描述的是研究结论中的信息流，不是对 Cairn 当前实现的设计决定。

1. **收集权威材料**：CUDA kernel、host launch、调用方、测试、文档、框架 schema、已知算子定义和用户约束；
2. **源端健康检查**：编译并真实运行 CUDA，检查 race、越界、未初始化、同步、确定性和错误状态；
3. **抽取候选契约**：形成参数、shape、dtype、layout、alias、数值和错误行为约束，并保留每条约束的 provenance；
4. **发现冲突**：比较文档、调用方、测试、源码检查和运行观测，不把冲突材料静默合并；
5. **建立输入空间分区**：按合法/非法、边界、tile、布局、特殊浮点、数值病态程度和并发风险划分测试域；
6. **为每个分区选择 Oracle portfolio**：exact reference、high-precision reference、CUDA differential、metamorphic relation、formal condition、sanitizer 或 status oracle；
7. **生成并筛选 case**：使用约束求解、分区采样、搜索、已有测试挖掘和模型提案生成可 replay 输入；
8. **在隔离路径执行各 authority**：防止候选实现读取 expected output，也防止 reference 被候选执行路径替换或绕过；
9. **推导并审查 comparator**：比较方式必须来自算子语义和数值模型，而不是根据候选输出反向调宽；
10. **验证 Oracle 自身**：用已知正确实现、已知错误、历史故障、定向 mutants 和冲突样例检查 precision 与故障检测能力；
11. **真实 NPU 验证**：记录 binary、device、launch、同步、sanitizer 和输出写入证据；
12. **形成分级 verdict**：给出证据类别、适用域、冲突、反例和未覆盖区域，而不是只聚合成 pass/fail。

该流程强调三个分离：

- **输入生成与结果判定分离**：会生成高覆盖输入，不等于知道正确答案；
- **Oracle 提案与 Oracle admission 分离**：能生成 assertion，不等于 assertion 正确；
- **Oracle admission 与候选验证分离**：不能用一个候选实现是否通过来反向证明 Oracle 正确。

## 8. Oracle 自动生成原则

1. **来源优先于模型置信度。** 每个断言、expected result 和比较规则都必须记录权威来源。
2. **保持独立性。** 生成器、候选实现和裁判应尽量避免共享同一实现或同一模型推理链。
3. **只对定义良好的行为生成正常功能 Oracle。** race、越界和其他未定义行为应转为源端缺陷或策略问题。
4. **精确比较必须有精确语义。** 能证明 exact 才使用 exact；容差必须有推导或批准依据。
5. **不确定不是通过。** 证据冲突或覆盖不足时，应输出 unknown、conflict 或 needs-policy，而不是为了自动化率判定 pass。
6. **各类 Oracle 只证明自己的维度。** differential、metamorphic、sanitizer 和 formal proof 不能互相冒充。
7. **Oracle 本身需要 admission。** 应使用历史故障、定向 mutation、独立 reference 和反例搜索评价其故障检测能力。
8. **所有 verdict 必须可 replay。** 需要固定输入、seed、工具链、设备、环境、比较器和 evidence identity。
9. **显式记录盲区。** 通过结论必须说明尚未覆盖的 dtype、shape、layout、设备和语义区域。
10. **LLM 负责提案，不负责自我授权。** LLM 可以生成契约、reference、relation、case 和 comparator 候选，但不能仅凭自身输出建立正确性权威。

### 8.1 Oracle 自身如何被验证

原报告虽然提出 Oracle 需要 admission，但没有展开评价维度。补充调研后，至少需要评价：

- **正确性/precision**：Oracle 是否会把已知正确实现判错；
- **故障检测能力/recall proxy**：是否能杀死与 CUDA → Ascend C 风险匹配的 mutants 和历史故障；
- **适用域**：Oracle 的前置条件是否覆盖待验证 case，是否发生域外使用；
- **独立性**：Oracle 与候选实现、其他 provider 是否存在共享代码、共享模型或共享底层库；
- **稳定性**：相同 replay 条件下 verdict 是否稳定，非确定性是否被显式建模；
- **可观察性**：错误状态能否传播到被比较输出、status、sanitizer 或 trace；
- **抗投机性**：恒定输出、固定 shape 特化、跳过 launch、调用被禁止的原实现等行为能否绕过；
- **执行真实性**：比较材料是否来自声明的 binary、设备和本次运行；
- **可诊断性**：失败时是否能给出最小反例、被违反的 claim 和相关 provenance；
- **成本**：高强度 Oracle 的设备时间和分析开销是否适合 admission、回归或抽样运行。

Oracle admission 至少需要同时包含正控制和负控制：

- 正控制验证 Oracle 不会错误拒绝一个在其适用域内的已知正确实现；
- 负控制验证 Oracle 能够拒绝预期故障类别；
- conflict control 验证多来源矛盾时系统输出 conflict/unknown，而不是任意选择一个来源；
- bypass control 验证没有 launch、输出未写、读取 expected artifact 或 fallback 到原实现时不能通过。

Mutation testing 在这里是敏感度评价工具，不是正确性授权工具。某个 Oracle 杀死了全部设计 mutants，仍然可能共享一项未被建模的错误语义。

## 9. 建议的最终结论形态

Cairn 自动生成的 Oracle 产物最终更应表达为：

> 在给定输入域和执行环境内，依据这些相互独立的权威，对这些功能、数值、安全和集成性质，采用这些比较关系，该 Ascend C 实现获得了这种强度的证据；以下区域仍未证明或存在冲突。

而不应简化为：

> 模型生成了几个输入和 expected bytes，运行相等，因此移植正确。

为了支持前一种结论，概念上的 Oracle 产物至少要能回答：

- **claim**：究竟声称保持了什么语义或安全性质；
- **domain**：该 claim 对哪些 dtype、shape、layout、值域、alias 和环境成立；
- **authority graph**：结论来自哪些来源，来源之间有什么依赖或冲突；
- **case rationale**：每个 case 属于哪个语义分区，为什么选择它；
- **expected relation**：精确值、数值范围、集合、顺序无关关系还是 metamorphic property；
- **comparator rationale**：比较器和容差如何推导；
- **execution evidence**：CUDA 与 Ascend C 分别实际执行了什么 binary、设备和 launch；
- **adequacy evidence**：能够发现哪些历史错误和定向 mutants；
- **verdict strength**：证明、规范派生、独立差分、关系支持、经验支持、未知或冲突；
- **blind spots**：哪些输入域、设备行为和故障类别没有被覆盖。

这些信息共同构成 Oracle。expected bytes 只是其中一种 expected relation 的序列化结果。

## 10. 后续需要讨论的产品政策

在形成实现方案前，至少需要先确定：

1. **正确性权威排序**：外部算子规范、框架 reference、CUDA 源行为、用户测试和模型推断发生冲突时，Cairn 听谁的；
2. **源端缺陷政策**：CUDA kernel 存在 bug、race、未定义行为或数值不稳定时，移植目标是行为保真、语义修复还是阻断并请求人工决策；
3. **结论模型**：Oracle verdict 是否接受 exact/proven、differentially-supported、property-supported、empirical、unknown、conflict 等分级结论，而不是只有 pass/fail；
4. **准入强度**：什么证据组合足以允许 Ascend C kernel 进入后续性能优化、集成验证或替换阶段；
5. **人工边界**：哪些冲突必须由用户批准，哪些可以由预先声明的迁移政策自动处理。

这五项政策会决定 Oracle 自动生成系统的数据模型、agent 职责、admission graph 和硬件验证流程，因而应先于具体实现确定。
