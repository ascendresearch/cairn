# CUDA → Ascend C Oracle：工业界与学术界可借鉴方向

状态：调研提炼，供方案讨论使用

日期：2026-08-27

## 1. 文档目的

本文件从工业算子栈、代码生成系统、编译器验证和自动测试研究中，提炼对 Cairn 的 CUDA → Ascend C Oracle 自动生成问题真正值得借鉴的方向、思想和技术点。

它不是文献综述，也不重复《CUDA → Ascend C Oracle 自动生成调研报告》中对 Oracle 问题的完整分析。这里更关心：

- 哪些方法已经在工业系统或研究原型中证明有价值；
- 这些方法解决 Oracle 的哪一部分，而不是被笼统称为“自动测试”；
- Cairn 可以借鉴其什么机制；
- 哪些前提在 CUDA → Ascend C 场景中并不成立，不能直接照搬；
- 哪些方向适合近期采用，哪些应保留为中长期研究能力。

本文件只讨论 Oracle 方向，不构成当前数据模型、agent 划分、admission graph 或实现顺序的决定。

## 2. 选择标准

一个方向被列入本文，至少满足以下一项：

1. 能提高 Oracle 的权威性，而不只是增加测试数量；
2. 能系统扩大 dtype、shape、layout、数值或并行语义覆盖；
3. 能减少差分测试中的共同错误和假阳性；
4. 能独立发现功能比较无法发现的内存、竞争或执行路径错误；
5. 能评价 Oracle 本身是否有故障检测能力；
6. 能把自由代码生成约束为可解释、可验证的变换；
7. 能明确输出证据强度、适用域和盲区。

以下方法即使自动化程度很高，也不会仅凭这一点被认为值得借鉴：

- 只生成更多随机输入；
- 只让 LLM 生成 expected output；
- 只增加一个全局容差；
- 只用代码覆盖率评价测试；
- 只在 CPU 模拟器运行；
- 只记录最终 pass/fail。

## 3. 值得借鉴方向总览

| 方向 | 代表工作 | Cairn 可借鉴的核心 | 建议定位 |
| --- | --- | --- | --- |
| OpInfo 式算子语义画像 | PyTorch OpInfo/opcheck | 元数据驱动的样例族与通用测试模板 | 近期基础能力 |
| 多 provider reference | CUTLASS、ONNX | 多权威差分、问题空间扫描、来源区分 | 近期基础能力 |
| 约束自动抽取 | DocTer、ACETest | 从文档、检查代码和调用方形成候选输入域 | 近期重点 |
| 约束驱动输入生成 | NNSmith、NeuRI、FreeFuzz | 合法、多样、可到达核心逻辑的 case | 近期重点 |
| Metamorphic relation | Meta、MR-Scout | 无精确真值时检查必然关系 | 近期至中期 |
| 真实设备与 sanitizer Oracle | Compute Sanitizer、msSanitizer | 功能、内存、竞争、同步分层验证 | 近期准入门槛 |
| Oracle mutation/admission | Mutation testing | 证明 Oracle 能发现目标故障类别 | 近期至中期 |
| Defined-behavior 与等价变体 | Csmith、EMI | 避免无意义差分，生成结构不同的等价测试 | 中期 |
| Translation validation | Alive2、GPUVerify | 对每次实际翻译产物产生局部证明或反例 | 中长期研究 |
| 正确性保持的受限生成 | Ansor/TVM | 通过受限变换和历史降低自由生成风险 | 中长期架构能力 |
| 严格数值误差分析 | FPTaylor、Daisy | 为部分算子推导而不是猜测容差 | 中长期研究 |
| LLM 只负责提案 | TOGA 复现、LLM Oracle 研究 | 模型生成与权威 admission 分离 | 立即采用的原则 |

这里的“近期”表示不依赖完整共同形式语义，能够直接改善 Cairn Oracle 的可信度；“中长期”表示需要新的语义表示、分析器或较高工程投入。

## 4. 工业界值得借鉴的实现思想

### 4.1 PyTorch OpInfo：把算子知识从测试代码中抽出来

PyTorch 的 OpInfo 不是给每个算子手写一套完全独立的测试，而是用结构化元数据描述算子支持的 dtype、设备、sample inputs、reference inputs、错误输入、变体和 autograd 等能力，再由通用测试模板批量生成测试。

PyTorch 的通用 operator tests 会据此检查 CPU/GPU 对照、NumPy reference、metadata、noncontiguous 输入、变体、autograd、FakeTensor/meta 等不同性质。[PyTorch operator tests](https://github.com/pytorch/pytorch/blob/main/test/test_ops.py)

`torch.library.opcheck` 还把 schema、autograd registration、FakeTensor 和 AOT dispatch 分开检查，说明自定义算子的正确性并不等同于输出值接近。[PyTorch Custom Operators](https://docs.pytorch.org/docs/main/library.html)

#### 值得借鉴

Cairn 可以借鉴“算子语义画像驱动通用测试”的思想。对一个 CUDA kernel，Oracle 生成过程应逐步形成可审计的语义画像，例如：

- 参数角色和 ABI；
- dtype、rank、shape 约束；
- layout、stride、alias 和 mutation；
- 合法、非法和边界输入族；
- 输出结构、状态和错误行为；
- 数值与非确定性政策；
- 可用 reference 和 metamorphic relations；
- 设备、tiling 和并发风险标签。

有了画像，很多测试可以由通用模板生成，例如：

- shape 边界测试；
- noncontiguous/layout 变体；
- 输出预分配和 alias 测试；
- CUDA/Ascend C 差分；
- sentinel 写覆盖；
- sanitizer 执行；
- 多次运行确定性测试。

#### 不能照搬

PyTorch OpInfo 的最终权威是 PyTorch 自己的算子契约。Cairn 面对的是任意用户 CUDA kernel，通常没有现成 OpInfo，也不能默认 kernel 等价于某个 ATen 算子。因此 Cairn 需要“自动抽取并 admission 语义画像”，而不是直接引用框架元数据。

### 4.2 CUTLASS：多 reference provider 和问题空间扫描

CUTLASS Profiler 可以使用 cuBLAS、cuDNN、host reference、device reference 等不同 verification provider，并支持对算子参数范围进行扫描。比较器可以选择 bitwise 或带 epsilon 的比较。[CUTLASS Profiler](https://github.com/NVIDIA/cutlass/blob/main/media/docs/cpp/profiler.md)

#### 值得借鉴

- reference 应是可枚举的 provider，而不是一个无来源的 expected blob；
- 同一个 case 可以由多个 provider 独立求值；
- verification 和 performance profiling 应分开；
- case 应覆盖问题空间，而不是只验证一个方便 shape；
- provider 失败、缺席和不支持也应成为显式结果。

对 Cairn，CUDA source、CPU naive reference、框架 reference、高精度 reference 和 metamorphic relation 可以被视为不同 provider，但必须进一步记录它们是否共享底层库或算法。

#### 不能照搬

CUTLASS 聚焦已有明确数学语义的 GEMM/卷积等算子，reference provider 相对成熟。任意 CUDA kernel 可能包含自定义索引、状态、原子、近似函数和未定义行为，不能直接套用“多个库结果一致即正确”。

另外，CUTLASS 允许配置 epsilon，但 Cairn 不能把“可配置容差”误解为“容差已得到语义证明”。

### 4.3 ONNX Backend Test：测试同时参与定义规范

ONNX Backend Test 将测试分成逐算子 Node Tests 和模型级 Model Tests。Node Tests 不只是验证实现，也和文档一起定义算子的预期行为；新增测试还用于减少 operator definition 的歧义。[ONNX Backend Test](https://github.com/onnx/onnx/blob/main/docs/OnnxBackendTest.md)

#### 值得借鉴

- Oracle case 不只是回归资产，也可以逐步固化已确认的 CUDA kernel 契约；
- 单 kernel 测试和调用图/框架集成测试应区分；
- operator coverage 应按语义属性统计，而不是只按 case 数统计；
- 文档、reference 和 executable tests 应保持一致。

对 Cairn，一个已 admission 的 Oracle corpus 可以成为该 CUDA kernel 当前迁移契约的可执行部分，但前提是每个 case 的来源和适用域清楚。

#### 不能照搬

ONNX 有中心化、显式的 operator specification。Cairn 的源端权威通常是分散的：CUDA 源码、host 调用、测试、文档和用户意图可能冲突。因此 Cairn 不能在未解决冲突前把观察到的行为直接固化为规范。

### 4.4 Triton 与 BackendBench：固定 allclose 的局限和语义样例的重要性

Triton 官方教程通常用 PyTorch/cuBLAS 作为 reference。例如矩阵乘法教程对一个固定 FP16 shape 使用 `atol=1e-2`，FP8 使用更宽容差。这类测试适合教学和基本回归，但本身没有建立完整输入域或严格误差依据。[Triton Matrix Multiplication Tutorial](https://triton-lang.org/main/getting-started/tutorials/03-matrix-multiplication.html)

BackendBench 则明确讨论了 GPU kernel 生成中的测试漏洞：正态分布输入可能让恒定均值实现通过，固定 shape 会鼓励投机特化，异步执行和 warmup 会制造虚假性能结果。它改用 PyTorch OpInfo 的边界样例，并保留人工审查。[BackendBench correctness](https://github.com/meta-pytorch/BackendBench/blob/main/docs/correctness.md)

#### 值得借鉴

- case 必须有语义意图，不能只有 seed 和 shape；
- 输入分布本身可能制造弱 Oracle；
- correctness corpus 与 performance workload 应分开；
- 应显式检测恒定输出、固定 shape 特化、fallback 和绕过执行；
- 最终高价值 kernel 仍需要可审查的独立实现材料和反例。

#### 不能照搬

BackendBench 依赖成熟的 PyTorch OpInfo 和 ATen 契约。Cairn 首先需要恢复用户 CUDA kernel 的契约，不能直接把 PyTorch 结果当作每个 CUDA kernel 的意图。

### 4.5 NVIDIA/Huawei sanitizer：把安全性质变成独立 Oracle

NVIDIA Compute Sanitizer 提供 memcheck、racecheck、initcheck 和 synccheck，可检测越界、misalignment、共享内存竞争、未初始化读取和同步问题。[NVIDIA Compute Sanitizer](https://docs.nvidia.com/compute-sanitizer/ComputeSanitizer/index.html)

Ascend msSanitizer 提供 memory、contention、uninitialization 和 synchronization 检查，覆盖 GM/Local Memory、核内/核间竞争和 SetFlag/WaitFlag 配对等 Ascend C 特有风险。[Ascend msSanitizer](https://www.hiascend.com/document/detail/en/canncommercial/850/devaids/optool/atlasopdev_16_0039.html)

#### 值得借鉴

- 源 CUDA 和目标 Ascend C 都应先通过各自安全 Oracle；
- safety verdict 与数值 verdict 必须分开；
- sanitizer 的工具版本、参数、设备和限制必须进入 evidence；
- 无报告只能表示“在该运行和工具覆盖范围内未观察到问题”；
- race、未初始化和同步错误不应通过放宽数值 comparator 被吸收。

#### 不能照搬

sanitizer 是动态或受限静态检查，覆盖不足时会漏报；工具还存在调用模式、内存池、mask vector 等限制。它们不能证明功能语义，也不能单独证明不存在安全错误。

### 4.6 Ascend C 官方工具链：借验证脚手架，不把脚手架当真值

Ascend C 样例项目通常提供 `gen_data.py`、`verify_result.py`、CPU twin debug、NPU run 和 host 调用框架。msOpST 等工具也能够生成单算子测试工程和报告。[Ascend C Kernel Launch Sample](https://www.hiascend.com/document/detail/en/canncommercial/800/opdevg/Ascendcopdevg/atlas_ascendc_10_0056.html)

#### 值得借鉴

- 标准化输入文件、truth 文件、运行产物和比较报告；
- CPU debug 与 NPU board execution 分层；
- 把编译、host 调用、device run 和结果比较形成可 replay 流程；
- 复用官方工具收集设备侧错误、memory 和 profiling evidence。

#### 不能照搬

官方工具自动化的是工程脚手架和执行流程，`gen_data.py` 中的真值逻辑仍由开发者编写。CPU twin debug 也不是 NPU 指令、调度和多核行为的证明。因此 Cairn 仍需解决真值来源、比较规则和真实 NPU execution attestation。

## 5. 学术界值得借鉴的方向

### 5.1 DocTer 与 ACETest：先恢复有效输入域

DocTer 从自然语言 API 文档中抽取 dtype、shape、参数关系等约束，并据此生成合法、非法和边界输入。[DocTer](https://www.cs.purdue.edu/homes/lintan/publications/docter-issta22.pdf)

ACETest 则识别算子实现中的 input validation code，自动抽取实际检查约束并生成能够通过检查、进入核心逻辑的测试。其评估显示，它能抽取更多复杂约束并发现大量真实缺陷。[ACETest](https://arxiv.org/abs/2305.17914)

#### 值得借鉴

Cairn 不应只分析 CUDA kernel body，还应联合分析：

- host launch 和调用方；
- shape/size 计算；
- assert、status 和错误分支；
- 内存分配和 copy size；
- 现有测试和 fixture；
- 文档、注释和使用样例；
- 真实运行 trace。

这些材料可以共同生成候选 contract，并驱动有效输入、无效输入和边界输入生成。

#### 不能照搬

从代码抽取的是“实现接受什么”，不一定是“规范应接受什么”；从文档抽取的约束可能过时或不完整。多来源冲突不能通过投票或静默合并消失，必须保留 provenance 并进入 conflict/needs-policy。

### 5.2 NNSmith、NeuRI、FreeFuzz：约束驱动的多样输入生成

NNSmith 使用轻量 operator specification 生成合法且多样的 DNN，避免大量无效测试，并结合差分测试发现后端错误。[NNSmith](https://arxiv.org/abs/2207.13066)

NeuRI 从有效和无效执行轨迹中归纳 operator constraints，再结合 symbolic/concrete generation 扩大有效模型空间。[NeuRI](https://arxiv.org/abs/2302.02261)

FreeFuzz 从文档、开发者测试、真实模型和运行 trace 中挖掘参数类型、shape 和取值，再进行变异。[FreeFuzz](https://arxiv.org/abs/2201.06589)

#### 值得借鉴

- 先构造合法域，再在合法域内追求多样性；
- 同时保留 invalid/boundary suite，而不是只测试普通输入；
- 从真实调用中学习高价值 shape，但不能只复现高频 shape；
- 使用约束求解生成 tail、alignment、alias 和 shape interaction；
- 保存 seed、约束解和生成理由，支持 replay 与最小化。

#### 不能照搬

这些方法主要解决 test input generation，不自动提供正确 expected output。归纳出的约束是观察模型，可能过拟合已有 trace，也可能学习到现有实现的 bug。它们应服务于 case proposal，不应直接授权 contract。

### 5.3 Metamorphic testing：把数学和结构关系变成 Oracle

Metamorphic testing 在无法获得精确 expected output 时，通过检查输入变换和输出变换之间的必要关系解决部分 Oracle 问题。面向深度学习算子的 Meta 框架设计了参数和 tensor 层面的 relations，并用于发现实现及精度错误。[A Miss Is as Good as A Mile](https://2024.esec-fse.org/details/fse-2024-research-papers/60/A-Miss-Is-as-Good-as-A-Mile-Metamorphic-Testing-for-Deep-Learning-Operators)

MR-Scout 进一步展示了从已有测试中挖掘 metamorphic relations 的可能性。[MR-Scout](https://doi.org/10.1145/3656340)

#### 值得借鉴

对 CUDA → Ascend C，尤其值得积累的 relation family 包括：

- identity：加零、乘一、空 padding；
- permutation/equivariance；
- reshape、transpose 和 layout-preserving relation；
- 分块计算与整体计算；
- reduction 分解与重组；
- broadcast 扩展；
- scale、linearity 或 monotonicity；
- 无关输入区域不影响有效输出；
- grid/block 或 tiling 改变不改变定义良好的功能结果。

relation 可以同时用于生成新 case 和判断结果，且不必存储完整 expected bytes。

#### 不能照搬

relation 不是天然正确。每条 relation 都需要：

- 适用算子类别；
- dtype 和数值范围；
- shape/layout 前置条件；
- NaN、Inf、overflow 和 alias 政策；
- exact 或 approximate comparator；
- 正控制、反例搜索和 mutation 验证。

整数或实数代数上的关系不一定在有限精度、原子和非确定性语义下成立。

### 5.4 Differential testing：借多实现，重点防共同错误

差分测试是算子和编译器验证中最常见的 pseudo-oracle：对相同输入运行多个实现，结果不一致时至少有一方存在问题。CUTLASS、Triton、TVM 和大量 DL fuzzing 工作都使用这种方法。

DeepREL 的研究特别提醒，不同 API 或后端可能共享设计和实现，因此共同错误可能让差分测试全部通过。[DeepREL](https://lingming.cs.illinois.edu/publications/fse2022b.pdf)

#### 值得借鉴

- 不只比较 CUDA 与 Ascend C 两方；
- 尽量加入独立 CPU、框架、高精度或 property provider；
- 建立 provider dependency graph；
- 不一致时输出 conflict 并保存全部原始结果；
- provider consensus 只在适用域交集内成立；
- status、metadata、shape 和 side effect 也要差分，不只比较数值 tensor。

#### 不能照搬

“多数实现一致”不是规范证明。三个 provider 如果调用同一底层库，只能算一个共同实现家族。CUDA source 本身有 bug 时，目标 faithfully 复制该 bug 也会通过二方差分。

### 5.5 Csmith 与 EMI：保证差分问题本身有意义

Csmith 能有效发现编译器错误，一个关键原因是它生成具有唯一解释的有效 C 程序，并主动避免 undefined 和 unspecified behavior。[Csmith paper](https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf)

Equivalence Modulo Inputs 从一个程序和输入集合生成在这些输入上等价、但静态结构不同的变体，用不同结构刺激编译器优化路径。[EMI](https://www.microsoft.com/en-us/research/publication/compiler-validation-via-equivalence-modulo-inputs/)

#### 值得借鉴

- 在差分前先判断 CUDA source 是否有定义良好的行为；
- source race、越界和未初始化应阻断正常功能 Oracle；
- 为同一语义生成结构不同的 CUDA/reference/Ascend C 变体；
- 使用等价变体检查 translation 对 launch、tiling 和控制结构的敏感性；
- 让反例最小化保持 defined-behavior 前提。

#### 不能照搬

CUDA 与 Ascend C 的设备内存、同步和数值语义比普通 C 更复杂。某个源码重写在顺序实数语义中等价，不代表在 CUDA atomics、浮点 reduction 或 Ascend pipeline 中等价。

### 5.6 Alive2 与 GPUVerify：把形式化验证限定在可建模子域

Alive2 对每次实际 LLVM transformation 检查 target 是否 refine source，使用 SMT 生成证明或反例。它验证的是具体 translation artifact，而不是假设整个 compiler 永远正确。[Alive2](https://github.com/AliveToolkit/alive2)

GPUVerify 面向 CUDA/OpenCL kernel 验证 race freedom 和 barrier divergence 等并发安全性质。[GPUVerify](https://fastpl.doc.ic.ac.uk/tools/GPUVerify/IEEE_TSE/)

#### 值得借鉴

- 把“整个 CUDA → Ascend C 等价”分解成局部 proof obligations；
- 优先形式化索引、边界、循环域、partition 和部分算术；
- 验证每次实际生成产物，而不是只验证 generator 设计；
- proof failure 输出具体 counterexample；
- unsupported semantics 必须显式报告，不能退化为 pass。

#### 不能照搬

目前不存在覆盖 CUDA 与 Ascend C 完整设备模型的共同精确 IR。直接追求全 kernel 形式等价，容易陷入长期语义建模而无法改善近期 Oracle。更现实的借鉴方式是选择可规范化的子域，逐步扩大证明覆盖。

### 5.7 Ansor：限制生成自由度，并保留变换历史

Ansor 从声明式 tensor computation 出发，在分层 schedule/search space 中生成高性能 tensor program。候选程序由受限变换从初始实现导出，并保留 rewriting history；依赖分析和 code generator 用于检查变换合法性。[Ansor](https://arxiv.org/abs/2006.06762)

#### 值得借鉴

- 能由规则生成的部分不要完全交给自由文本代码生成；
- 把 tiling、partition、layout、pipeline 等迁移动作表示成有前置条件的 transformation；
- 保存每个候选的 transformation history；
- 每个 transformation 产生对应 proof obligation 和定向测试；
- 把模型用于选择、组合和补全变换，而不是重新定义计算语义。

#### 不能照搬

Ansor 的输入本身已经是明确的高层 tensor computation，而 CUDA kernel 往往混合索引、同步、共享内存、原子和自定义控制流。Cairn 首先需要可靠抽取 source semantics；否则“正确性保持变换”只是在错误抽象上保持一致。

### 5.8 FPTaylor 与 Daisy：为 comparator 推导误差依据

FPTaylor 使用符号 Taylor 展开等技术给出严格浮点 round-off error bound，并可生成形式化检查材料。[FPTaylor](https://github.com/soarlab/FPTaylor)

Daisy 组合区间、仿射算术、SMT 和不同误差分析技术，能够分析和优化有限精度计算。[Daisy](https://link.springer.com/chapter/10.1007/978-3-319-89960-2_15)

#### 值得借鉴

- comparator 应由算子数值模型推导，而不是从候选误差反向拟合；
- 输入域是误差界的一部分，domain 改变时 comparator 也可能变化；
- 区分 absolute、relative、ULP、range 和 property comparator；
- 记录 accumulator dtype、FMA、reduction order 和量化规则；
- 对不同输入区域使用不同接受域，而不是一个全局容差；
- 将严格 bound 与 empirical envelope 使用不同证据类型表示。

#### 不能照搬

现有严格工具主要适合受限 straight-line arithmetic，对循环、分支、复杂数学函数和并行 reduction 的处理有限。它们应先服务于 elementwise、短 reduction 或可抽取表达式，而不是作为所有算子的统一容差生成器。

### 5.9 Mutation testing：评价 Oracle，而不是授权 Oracle

Mutation testing 向程序注入小型人工故障，并用测试是否能够杀死 mutant 来评价测试与 Oracle 的故障检测能力。它比单纯代码覆盖更接近“Oracle 是否能观察到错误”。[Practical Mutation Testing at Scale](https://arxiv.org/abs/2102.11378)

#### 值得借鉴

Cairn 的 mutation operators 应围绕真实迁移风险，而不是只使用通用语法变异：

- 删除或反转 tail mask；
- 改变 offset、stride 或 tile 起点；
- 少处理/多处理一个 block；
- 删除一次 copy、barrier、SetFlag 或 WaitFlag；
- 改变 queue/buffer 顺序；
- 改 accumulator dtype 或 cast 位置；
- 更改 comparison/tie-breaking；
- 忽略一个输出或只写首个 tile；
- 把多核 partition 退化为重复写同一区域；
- 跳过真实 NPU launch 或返回预初始化输出。

mutation score 适合作为 Oracle admission 的负控制和覆盖指标。

#### 不能照搬

杀死全部 mutants 不代表语义正确。mutant 集合可能遗漏真正故障，reference 和 candidate 也可能共享一项未被变异覆盖的错误理解。Mutation testing 只能证明对已建模故障的敏感性。

### 5.10 TOGA 的复现结果：LLM 适合提案，不适合自我授权

TOGA 等工作尝试从 focal method、test prefix 和文档生成 assertion。后续大规模复现实验发现，TOGA 在生成 assertion 时出现很高的假阳性率，并且相对已有方法新增的实际故障检测能力很低。[Neural-Based Test Oracle Generation](https://arxiv.org/abs/2307.16023)

LLM Oracle 研究也指出，模型可能产生错误 assertion、遗漏重要观察点，还存在训练数据泄漏和评测污染问题。[Test Oracle Automation in the Era of LLMs](https://arxiv.org/abs/2405.12766)

#### 值得借鉴

LLM 很适合：

- 汇总文档、代码和调用方中的候选约束；
- 生成朴素 reference 草案；
- 提议 metamorphic relations；
- 生成边界 case 和定向 mutants；
- 解释 evidence conflict；
- 根据反例修复提案。

但每个产物都应标记为 proposal，并进入独立 admission。

#### 不能照搬

- 不能因为 assertion 可执行就认为其正确；
- 不能让同一个模型同时生成 candidate、expected output 和 comparator 后互相认证；
- 不能用候选是否通过来调宽 Oracle；
- 不能把模型置信度映射为语义证据强度；
- 不能忽略模型可能见过公开 benchmark expected result 的泄漏风险。

## 6. 值得组合成 Cairn 能力的技术点

以下不是实现设计，而是从上述工作共同抽象出的技术能力。

### 6.1 Operator semantic profile

为每个待迁移 CUDA kernel 形成逐步完善的语义画像，包含 ABI、参数角色、domain、side effects、数值政策、并行风险、reference 和未解决冲突。

借鉴来源：PyTorch OpInfo、ONNX operator tests、DocTer、ACETest。

### 6.2 Case intent 与语义分区

每个 case 不只保存输入，还说明它覆盖：

- 哪个 contract 条件；
- 哪个边界或 interaction；
- 哪个 tile/launch 路径；
- 哪种特殊数值；
- 哪个历史故障或 mutant；
- 哪条 metamorphic relation。

借鉴来源：OpInfo、constraint-guided fuzzing、BackendBench、mutation testing。

### 6.3 Oracle claim

把 Oracle 的基本单位从“expected bytes”提升为 claim：

- claim 的性质；
- 适用 domain；
- authority；
- expected relation；
- comparator；
- 所需 execution evidence；
- strength 和 blind spots。

借鉴来源：formal specification、metamorphic testing、translation validation、ONNX tests。

### 6.4 Authority dependency graph

显式保存 reference、模型、库、设备和执行路径之间的依赖，避免把共享 cuBLAS、共享源码或共享模型推理误算为独立证据。

借鉴来源：多 provider differential testing，以及 DeepREL 对共同实现错误的警告。

### 6.5 Comparator family

比较器不是一个全局 `atol/rtol`，而是一组有适用域和依据的关系：exact、abs/rel、ULP、range、set、permutation、property、statistical envelope。

借鉴来源：CUTLASS/PyTorch comparison、FPTaylor、Daisy、metamorphic testing。

### 6.6 Execution attestation

证明结果来自声明的 source/binary、真实设备、launch configuration 和本次执行，并已完成异步同步；同时检查输出写覆盖和禁止 fallback。

借鉴来源：Ascend board debugging、msSanitizer、Compute Sanitizer、BackendBench 的 anti-cheating 与 benchmark footguns。

### 6.7 Oracle admission suite

对 Oracle 本身运行：

- 已知正确实现正控制；
- 历史错误和定向 mutants 负控制；
- 多来源冲突控制；
- no-launch、constant-output、fallback 和 expected-data access 绕过控制；
- replay 稳定性与域外拒绝测试。

借鉴来源：mutation testing、compiler fuzzing、LLM Oracle 复现实验。

### 6.8 分级且局部的 verdict

verdict 应绑定 claim 和 domain，而不是对整个 kernel 给出无条件布尔结论。可能的证据性质包括：

- formally proven in restricted domain；
- specification-derived；
- independently differentially supported；
- metamorphically supported；
- empirically supported；
- unknown；
- conflict；
- invalid source behavior。

借鉴来源：translation validation、test oracle taxonomy、多源差分和数值分析。

## 7. 建议的借鉴优先级

### 7.1 应立即成为方案原则

这些原则不依赖复杂新工具：

1. LLM 只能生成 Oracle proposal，不能自我授权；
2. 输入生成、Oracle admission 和候选验证必须分开；
3. CUDA source 先做 defined-behavior 与 sanitizer 检查；
4. expected value、comparator 和 case 必须记录 provenance 与适用域；
5. host/CPU 成功不能替代真实 NPU evidence；
6. 不确定和冲突不能被压缩为 pass；
7. correctness 与 performance 分开。

### 7.2 最值得近期投入的能力

1. OpInfo 式 operator semantic profile；
2. contract extraction 与 conflict preservation；
3. 语义分区驱动的 case generation；
4. CUDA、CPU/reference、Ascend C 多 provider 执行；
5. comparator family 与 per-domain policy；
6. source/target sanitizer 和 execution attestation；
7. CUDA → Ascend C 定向 mutation；
8. authority dependency graph 和分级 verdict。

这些能力能够显著改善当前固定 expected bytes 方案，而且不要求先完成完整 CUDA/Ascend C 形式语义。

### 7.3 适合中期积累

1. 算子类别化 metamorphic relation library；
2. 从已有测试和代码自动挖掘 relation；
3. 高精度和区间 reference；
4. EMI/等价变体生成；
5. 反例搜索与自动最小化；
6. transformation history 和 proof obligations。

### 7.4 适合长期研究

1. CUDA 与 Ascend C 的共同语义表示；
2. indexing、partition、memory movement 的 translation validation；
3. 并行 reduction 的形式化数值界；
4. 大范围 comparator 自动推导；
5. 正确性保持的 CUDA → Ascend C 受限生成系统。

长期方向应该逐步提高局部 claim 的证明强度，不应以等待“完整形式验证”阻塞近期可获得的多层证据。

## 8. 明确不应照搬的做法

1. **固定 shape + 少量随机输入 + allclose**：适合 smoke test，不足以建立迁移正确性；
2. **CUDA 单次输出即真值**：会固化源端 bug、非确定性和偶然数值结果；
3. **模型同时生成实现和 Oracle**：存在严重共同错误和 reward hacking 风险；
4. **全局容差**：忽略算子、dtype、输入域和 reduction 长度差异；
5. **CPU twin debug 即设备证明**：无法覆盖 NPU 指令、调度、内存和多核行为；
6. **sanitizer 通过即功能正确**：安全检查不证明数学语义；
7. **多数 provider 一致即规范正确**：provider 可能共享底层实现；
8. **mutation score 即正确性概率**：mutation 只评价已建模故障的可检测性；
9. **代码覆盖率即 Oracle 充分性**：执行过的错误如果没有被观察，覆盖率仍然很高；
10. **一个 kernel 一个全局 pass/fail**：掩盖适用域、证据强度和未覆盖区域。

## 9. 总结

工业界最值得 Cairn 借鉴的是：

- PyTorch/ONNX 的结构化算子契约和通用测试模板；
- CUTLASS 的多 reference provider 与问题空间扫描；
- NVIDIA/Huawei 将安全、真实设备和功能比较分离的工具链；
- BackendBench 对固定输入、异步执行和投机实现的防御意识。

学术界最值得借鉴的是：

- DocTer/ACETest/NeuRI 等对输入约束和有效域的自动恢复；
- metamorphic testing 对精确真值缺失的补充；
- Csmith/EMI 对 defined behavior 和等价变体的重视；
- Alive2/GPUVerify 将大问题拆成局部 proof obligations；
- FPTaylor/Daisy 对数值误差依据的严格推导；
- mutation testing 对 Oracle 故障检测能力的评价；
- TOGA 复现所揭示的“模型生成不等于正确性授权”。

这些工作的共同思想不是制造一个万能 Oracle，而是把 Oracle 变成一个分层、可组合、可审计、可反驳的证据系统。对 CUDA → Ascend C，近期最重要的不是继续增加固定 expected output，而是建立语义画像、来源依赖、输入分区、多 provider、真实设备证据和 Oracle admission 之间的闭环。
