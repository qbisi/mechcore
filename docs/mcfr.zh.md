# MCFR 确定性录像契约

[English](mcfr.md)

## 状态

本文定义原生采集、模拟、比较和播放共同遵循的 `S`、`E` 基础逻辑内容。Schema
version 1 已在 `mechcore-mcfr` 中提供 Rust 参考实现。事件专用的有类型 payload schema
和世界对象最终的列式投影仍待确定。

## 目的

MCFR 使用统一世界模型记录一段连续的单回合战斗。以下两条路径必须产生相同的
逻辑格式：

- 游戏 Adapter 采集权威的原生战斗；
- Rust 内核从相同的初始状态和随机状态出发，确定性地模拟战斗。

Godot 或 Three.js 播放器必须能够从生成的文件还原可见的战斗时间线。原生结果与
模拟结果还必须能够直接比较，不依赖来源专用命令或专题比较格式。

MCFR 不描述部署经济、补给、反应堆核心状态、招募、结算或其它跨回合状态。

## 确定性模型

逻辑状态转换契约为：

```text
D + S(0) -> S(1..n) + E(0..n-1)
```

其中：

- `D` 是持久确定性上下文；
- `S(t)` 是某一逻辑边界上的权威世界快照；
- `E(t)` 是从 `S(t)` 转换到 `S(t+1)` 时产生的有序事件日志；
- `I(t)` 是可选的临时研究探针，不属于正式确定性契约。

必须在第一次战斗逻辑更新之前的 fighting 进入边界采集 `S(0)`。完成的录像必须标明
战斗从 fighting 变为 over 的唯一帧，不在每一帧重复保存 phase。

快照是连续序列 `S(0)..S(n)`。序列位置就是从零开始的逻辑步，因此不单独保存
`frame_index`、`logic_tick` 或逐帧 `time_seconds`。`D` 规定固定逻辑步时长。
fighting 到 over 的边界只在顶层保存为 `terminal_step`。来源原生时钟可以作为非规范
证据保留，但不进入 `S` 或正式哈希。

## 持久确定性上下文

`D` 包含复现战斗所需、但不适合自然表达为逐帧世界快照的全部输入。它至少绑定：

- MCFR schema 版本及其规定的规范化规则；
- 游戏 build；
- 逻辑步时序和数值约定；
- fighting 进入时的随机 seed bundle 或等价 RNG 状态；
- 稳定身份与更新顺序契约；
- fighting 开始后允许发生的任何外部命令。

如果在 `S(0)` 前已经消费过随机流，仅有显示出来的回合 seed 并不足够。复现必须从
相同的有效 RNG 状态开始。

如果战斗在 `S(0)` 后应当是封闭的，则任何未记录的外部动作都不得影响它。任何允许
发生的动作都属于持久上下文，而不是临时研究输入。

## 消费层级

该格式支持三类逐步增强的消费者：

| 消费者 | 所需逻辑轨道 | 要求 |
| --- | --- | --- |
| Godot 或 Three.js 播放 | `S` | 还原每个逻辑边界上的可见世界。 |
| 原生/模拟对齐 | `S + E` | 同时比较可见演化和规范化的离散机制。 |
| 机制研究 | `S + E + I` | 诊断隐藏输入和原生中间行为。 |

所有消费者还会读取普通的容器元数据。对齐和研究通过 `scenario_hash` 将各自的轨道
绑定到 `D`；上表只列出每类消费者必须解释的时间线轨道。

最小播放器可以忽略 `E`。功能更完整的播放器可以消费事件，以呈现无法保留到
tick-end 快照中的瞬时攻击、命中、状态和生命周期效果。

## 统一世界快照

`S` 与来源无关。原生指针、运行时对象地址、Adapter 记账信息以及模拟器私有类型均
不得进入 `S`。

当前候选世界边界包含：

- 单位；
- 投射物；
- 建筑，包括 Construction 和 Contraption；
- 独立区域护盾；
- 动态地形或持久空间区域；
- 持久 Status。

Team 和 Formation 是具体世界对象的归属与身份属性，不是逐帧容器实体。Unit 直接
关联其 Team 与 Formation。个人护盾仍然是 Unit 状态；区域护盾具有独立的空间边界、
能量和生命周期，因此是独立对象。

Status 是持久 buff 和 debuff 的统一表达，也包括禁用科技的效果。不再并行维护专用
的 `buffs` 和 `technology_disabled` 字段。

基础快照只接纳满足以下至少一个条件的字段：

1. 它是推进下一个逻辑步所需的持久状态；
2. 它是内核产生的权威结果；
3. 它是还原可见战斗所必需的；
4. 它用于标识世界对象或对象之间的稳定关系。

物理存储可以对不可变值去重，但读取者看到的逻辑快照必须保持完整。

### 必需的基础状态

不可变的身份、类型、归属和几何信息可以只在实体元数据中保存一次，而不必在每个
逻辑步物理重复；它们仍属于逻辑快照。

| 世界对象 | 必需的逻辑状态 |
| --- | --- |
| Unit | `unit_id`、`team_id`、`formation_id`、`unit_type_id`、可选 `parent_unit_id`、domain；位置、主体旋转、独立变化的瞄准姿态、速度和 motion state；碰撞半径；生命、最大生命、alive、active、targetable 和 visibility；存在个人护盾时保存其 active/enabled 状态及当前/最大能量。 |
| Projectile | `projectile_id`、team、owner/source 和投射物类型；位置及独立变化的朝向或速度；目标对象引用；缓存的目标位置和半径；active，以及适用时的当前/最大投射物生命。不可变的运动、索敌和伤害规则属于 `D` 或实体元数据。 |
| Building | `building_id`、team 及适用时的 formation、建筑类型；位置、旋转和碰撞边界；生命、最大生命、alive、active/available、targetable 和 collision-enabled 状态。 |
| AreaShield | `shield_id`、team 和 owner/source；位置、半径及适用时的高度；当前/最大能量和 active 状态。 |
| DynamicTerrain | `terrain_id`、team、source 和地形类型；位置及区域形状或 grid；active 和已用/剩余生命周期。不可变效果规则属于 `D` 或实体元数据。 |
| Status | `status_id`、Status 类型、source 和 target；stack；elapsed、remaining 和 max duration；active；由该 Status 持有周期效果时保存周期计时器。不可变 Status 规则属于 `D` 或 Status 元数据。 |

组件字段只在对象确实实现该组件时出现。字段不存在表示对象没有该组件；`unknown` 和
`unsupported` 必须采用不同表达。

索敌缓存、控制器时钟、求解器缓冲等存疑状态，不能仅因旧模拟器保存了它们就进入
基础 S。它们先记录在 `I`；只有原生证据证明其跨逻辑步持续存在，且是产生下一步已
接纳 `S/E` 结果所必需的，才能提升为正式状态。

## 规范化事件日志

`E` 是有类型、有顺序的战斗日志。只有当某个离散事实的步内发生、原因或顺序无法从
相邻快照可靠恢复时，才记录为事件。每个事件属于转换 `E(t)`，并拥有该转换内从零
开始的 `event_seq`。转换位置与事件序号共同确定规范顺序，不保存重复时间戳。

必需的基础事件类别为：

| 类别 | 必需事件及 payload |
| --- | --- |
| 对象生命周期 | Unit、Projectile、Building、AreaShield 和 DynamicTerrain 的创建与移除，包括身份、类型、归属/source 和移除原因。这能够保留在同一逻辑步内创建后又移除的对象。 |
| 动作 | 攻击或技能开始与释放，包括执行对象、动作/技能身份，以及已确定时的目标。控制器内部子阶段不是基础事件。 |
| 投射物 | 释放、命中/拦截和移除；按适用情况包含投射物、owner/source、target、命中位置和结果。 |
| 护盾 | 个人或区域护盾受击与停用，包括 source、shield/owner、请求量、实际量及受击前后能量。单纯的包含关系测试不是基础事件。 |
| Status | 施加、刷新/延长和移除/到期，包括 Status 身份/类型、source、target、stack/持续时间变化及原因。 |
| DynamicTerrain | 创建、区域变化、生命周期重置和移除；当动态地形造成伤害、治疗或 Status 变化时，记录有类型的地形效果。内部重叠选择不是基础事件。 |
| 战斗结果 | 伤害、治疗和死亡，包括 provider/source、target、实际量，以及相关生命/护盾的前后状态。 |

事件必须使用与快照相同的来源中立对象身份。确切 enum 编码和有类型 payload 布局仍
待确定，但生产者不得向正式轨道加入来源专用事件类型。

`S` 始终是权威状态。`E` 用于解释状态转换，但不能成为另一套相互冲突的持久状态
来源。

## 临时研究轨道

`I` 只用于闭合尚未解决的机制，可以包含临时状态采样、事件、输入或原生观测。基础
示例包括：

- 目标候选顺序、`nearest_actor`、`motion_target`、`attack_target` 和 lock-target
  缓存，直至证明它们跨步存在且确有必要；
- 攻击/技能控制器内部子阶段、计时器、冷却、cycle 及其它控制器计数器；
- RVO 的 desired target/velocity、next-target point、动态 avoidance radius、邻居、
  求解器缓冲、梯度和迭代过程；
- 随机调用位置和消费的样本；
- 投射物来源姿态缓存和区域护盾包含集合；
- 动态地形重叠候选、选中/粘滞成员关系及控制器 pulse 计数器；
- 观测到的更新顺序、原生 hook 顺序、原生指针和运行时地址。

研究探针默认不得扩展永久 MCFR schema。它应输出为具有明确 profile 的独立伴随文件，
并排除在正式结果哈希之外。

参考 API 提供 `InstrumentationSink`。Adapter 采集和 simulation 可以通过同一接口提交
`step`、`channel`、`content_type` 和任意字节 payload。`InstrumentationWriter::record_json`
用于便捷提交规范 JSON；`NoInstrumentation` 可以在不改变生产者控制流的情况下关闭
采集。Sidecar 通过 `scenario_hash` 绑定正式录像，并记录自身的 `profile` 和 `producer`；
它没有正式 state、event 或 result hash。

研究结束时：

- 中间值或诊断值继续留在 MCFR 之外，其采集实现可以删除；
- 经证明为确定性复现所必需的值，必须先提升为 `D`、`S` 或 `E` 中的来源中立字段，
  然后才能删除临时探针。

## 共同生产路径

原生采集与模拟不得分别维护独立的 MCFR 序列化实现。两条路径都应将相同的规范化
记录提交给同一个 Rust 规范化器和写入器：

```text
game adapter capture ----\
                          +--> Rust canonicalizer/writer --> .mcfr
Rust simulation kernel --/
```

Adapter 负责原生观测。Rust 写入器负责身份规范化、排序、验证、哈希和物理序列化。
这样可以使 HDF5 和比较策略留在注入式 Adapter 之外。

## HDF5 container version 1

`.mcfr` 文件采用 HDF5。写入器先创建同目录临时文件；只有全部 dataset、元数据和哈希
完成后，才在不覆盖已有文件的前提下发布最终路径。

| 路径 | HDF5 类型 | 含义 |
| --- | --- | --- |
| `/context/data` | 连续 `u8` | 一条规范 `D` 记录。 |
| `/states/data` | 分块、deflate 压缩的 `u8` | 依次连接的规范 `S(0)..S(n)`。 |
| `/states/offsets` | append-only `u64` | 包含初始零的状态记录边界。 |
| `/events/data` | 分块、deflate 压缩的 `u8` | 依次连接的规范 `E(0)..E(n-1)`。 |
| `/events/offsets` | append-only `u64` | 包含初始零的转换记录边界。 |

根属性保存格式标识、container/schema 版本、状态与转换数量、`terminal_step` 和四类规范
哈希。Container version 1 要求 `state_count = transition_count + 1`，并且
`terminal_step = transition_count`。

每条逻辑记录都是经过 schema 验证的 Rust 值，并编码为规范 UTF-8 JSON 字节。记录在
数值 dataset 中连续存放，而不是保存为 HDF5 变长 JSON 字符串。由于事件 payload schema
仍在收敛，第一版布局优先支持流式写入和随机记录读取。未来有类型列式投影需要提升
container version，但必须保持相同的逻辑哈希。

I sidecar 使用 HDF5 格式标识 `mechcore.mcfr.instrumentation`，在 `/records` 下保存
step、channel、content type、payload 字节和 payload offset，并通过根属性保存
`scenario_hash`、`profile`、`producer` 和 `record_count`。

## 规范哈希

正确性定义在规范逻辑内容上，而不是 HDF5 文件的原始字节上。HDF5 库版本、元数据
顺序、chunk 布局、压缩和来源 provenance 都可能改变物理字节而不改变战斗内容。

Schema version 1 使用带 domain separation 的 BLAKE3，并在每条规范记录前加入一个
little-endian `u64` 长度。正式哈希模型为：

```text
scenario_hash = BLAKE3("scenario-v1", canonical D, canonical S(0))
state_hash    = BLAKE3("state-v1", canonical S(0)..canonical S(n))
event_hash    = BLAKE3("event-v1", canonical E(0)..canonical E(n-1))
result_hash   = BLAKE3("result-v1", scenario_hash, state_hash, event_hash)
```

`D` 已包含 schema version、RNG 状态和 durable commands，因此实现的 `scenario_hash`
与展开后的需求表达式等价。

只有 schema 版本和 `scenario_hash` 均相同的两个录像才能比较。

- `state_hash` 相同，表示权威基础状态演化相同；
- `state_hash` 相同但 `event_hash` 不同，表示相同快照由不同的已记录机制序列产生；
- `result_hash` 相同，表示两个生产者在该场景已接受的 `S/E` 契约范围内一致。

整文件哈希可以另外用于保护传输完整性，但它不是原生/模拟正确性的判据。来源
provenance 作为元数据保留，并排除在 `result_hash` 之外。

规范编码递归排序对象 key；世界对象集合按来源中立身份排序，动态地形 grid cell 按
坐标排序，事件则必须具有连续的 `event_seq`。正式快照数值由 schema 定义为整数；
非有限浮点数不能进入规范 JSON 表达。

## 验证边界

如果缺少任何必要的确定性输入、帧覆盖不连续、对象身份有歧义、引用无法解析、事件
顺序无效，或者已存储轨道不满足声明的 schema 版本，完成的 MCFR 必须验证失败。

以下结论应当明确区分：

- 文件结构有效；
- 文件能够从 `S` 播放；
- 两个文件具有相同的状态演化；
- 两个文件具有相同的规范化事件；
- 无需临时 `I` 证据即可理解某项机制。

## 待完成的规范工作

后续修订必须依次确定：

1. 每个已接纳事件 enum 的有类型 payload schema；
2. 通过原生证据决定每个存疑 `I` 字段应提升还是排除；
3. 初始对象与动态创建对象的来源中立身份分配；
4. 需要相关语义的字段如何明确表达 unknown/unsupported；
5. 有类型列式 HDF5 投影和压缩 profile；
6. 面向播放器的读取和插值契约。

这些细节必须由原生采集可行性、确定性内核需求和播放需求共同推导，不得以某个专题
研究案例单独决定。
