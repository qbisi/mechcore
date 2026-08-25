# MCFR 确定性录像契约

[English](mcfr.md)

## 状态

本文定义原生采集、模拟、比较和播放共同遵循的 `S`、`E` 基础逻辑内容。Schema
version 2 及其有类型列式 HDF5 投影已在 `mechcore-mcfr` 中提供 Rust 参考实现。

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
T(0) = { S(0), E(0) = [] }
T(t) = { S(t), E(t) }，其中 t >= 1
S(t-1) --E(t)--> S(t)
MCFR = D + T(0)..T(n)
```

其中：

- `D` 是持久确定性上下文；
- `S(t)` 是某一逻辑边界上的权威世界快照；
- `E(0)` 为空；`t >= 1` 时，`E(t)` 是从 `S(t-1)` 推进到 `S(t)` 期间直接观测到的
  有序原生事件日志；
- `I(t)` 是可选的临时研究探针，不属于正式确定性契约。

必须在第一次战斗逻辑更新之前的 fighting 进入边界采集 `S(0)`。完成的录像必须标明
战斗从 fighting 变为 over 的唯一帧，不在每一帧重复保存 phase。

快照是连续序列 `S(0)..S(n)`。序列位置就是从零开始的逻辑步，因此不单独保存
`frame_index`、`logic_tick` 或逐帧 `time_seconds`。`D` 规定固定逻辑步时长。
fighting 到 over 的边界只在顶层保存为 `terminal_tick`。来源原生时钟可以作为非规范
证据保留，但不进入 `S` 或正式哈希。

## 坐标系

MCFR 在 Adapter 采集、simulation、比较和播放中统一采用游戏的 Unity 世界轴语义：

- `x/z` 构成战场平面；
- `y` 表示垂直高度；
- 每个 `Vec3` 均按 `(x, y, z)` 排列，绝不把 `y` 重新解释为 layout 的第二个水平坐标。

二维 layout 经过所属方变换后，layout `x` 映射到 MCFR 世界 `x`，layout `y` 映射到
MCFR 世界 `z`。Layout 保留原生二维 `(x, y)` schema；Adapter 和 simulation 只在构造
统一世界模型时执行该规范化。相机视角和屏幕方位不改变这些世界轴定义。

## 原生可观测性硬约束

每个 `S` 字段、每个 `E` 事件及其 payload 值，以及将来的每个 `I` channel 和数值，
都必须有已记录的 Adapter 采集路径，能够在对应逻辑边界或原生操作中直接观测游戏里
的同一事实。Schema 的接纳范围受 Adapter 原生可观测能力限制；仅仅因为 simulation
需要某项数据，或者离线分析能够算出某项结果，不足以使其成为录像字段、事件或探针值。

对于 `S`，直接观测是指在该快照边界读取原生字段、property 或方法返回值。对于 `E`，
必须由原生 callback、delegate、方法 Hook、参数、返回值或同一原生操作内的观测，明确
证明事件确实发生并提供其语义。对于 `I`，profile 必须标明对应的原生 probe；simulation
可以为比较输出同一 channel，但不得引入 simulation 独有或事后处理得到的值。

相邻快照和已记录字段可以用于校验采集是否完整，但其差分不得生成录像状态、事件或
探针值，也不得补全缺失的 cause、reason、outcome、分类或 payload 值。生产者不得用
外部规则、生产者私有状态、常量、默认值、启发式判断或假定的生命周期语义填充未观测值。

允许无损的表达规范化：经过检查的数值拓宽、固定单位换算、原生 enum 的精确映射、
来源中立身份映射、规范排序和物理去重。这些规范化不得引入新的游戏语义事实。每个已
接纳字段、事件类型和探针 channel 都必须在 Adapter 实现或其采集审查中保留原生来源
证据。候选事实没有直接原生来源时，应从对应 schema/profile 删除。采集失败不能代替
一个可采集的 schema，推导也不能作为兜底。

## 持久确定性上下文

`D` 包含采集与模拟共享、但不适合自然表达为逐帧世界快照的来源中立战斗输入。它绑定：

- MCFR schema 版本及其规定的规范化规则；
- 游戏 build；
- 逻辑步时序和整数数值尺度；
- 从一开始计数的战斗回合和 match seed；
- 来源中立的身份契约。

Schema version 2 不允许 fighting 开始后再输入改变战斗逻辑的外部命令。试验场加速投票
只改变墙钟调度，不改变逻辑步输入或战斗结果，因此属于编排信息，不进入 `D`、`S` 或
`E`。Adapter 必须在第一次战斗
更新以及战斗随机流消费之前采集 `S(0)`。无法仅从有类型的回合/seed 字段和 `S(0)`
重建的机制，在 schema 增加可直接采集、来源中立的有类型字段之前，不属于该比较契约。
`D` 不允许保存生产者私有的 RNG JSON。

单位与机制规则是 simulation 选择的外部输入，不嵌入 MCFR，也不在 MCFR 中保存
fingerprint。因此原生采集不需要单位 config 目录；错误的外部规则集通过最终 `S/E`
哈希分歧发现，而不是依赖生产者专用上下文字段。

战斗在 `S(0)` 后对逻辑演化封闭，不允许任何外部动作再影响战斗结果。仅采集 MCFR 时绑定
逻辑更新而非渲染帧或墙钟帧，可以请求只改变墙钟速度的加速投票；带视觉 sidecar 时则可将
下一逻辑更新限制到已完成的渲染边界。无法保证该边界的生产者
不得完成 schema-version-2 录像。

## 消费层级

该格式支持三类逐步增强的消费者：

| 消费者 | 所需逻辑轨道 | 要求 |
| --- | --- | --- |
| Godot 或 Three.js 播放 | `S` | 还原每个逻辑边界上的可见世界。 |
| 原生/模拟对齐 | `S + E` | 同时比较可见演化和规范化的离散机制。 |
| 机制研究 | `S + E + I` | 诊断隐藏输入和原生中间行为。 |

所有消费者还会读取普通的容器元数据。对齐和研究通过 `scenario_hash` 将各自的轨道
绑定到 `D`；上表只列出每类消费者必须解释的时间线轨道。

最小播放器可以忽略 `E`。功能更完整的播放器可以消费直接采集的投射物释放/移除和
伤害事件，以呈现无法保留到 tick-end 快照中的瞬时效果。

## 统一世界快照

`S` 与来源无关。原生指针、运行时对象地址、Adapter 记账信息以及模拟器私有类型均
不得进入 `S`。

Schema version 2 的世界边界只包含：

- 单位；
- 投射物；
- 建筑，包括 Construction 和 Contraption；
- 持久 Status。

Team 和 Formation 是具体世界对象的归属与身份属性，不是逐帧容器实体。Unit 直接
关联其 Team 与 Formation。个人护盾仍然是 Unit 状态。独立区域护盾和动态地形只有在
其必要状态能够完整枚举并通过原生路径直接采集后，才会进入基础实体集合。

Status 是持久 buff 和 debuff 的统一表达，也包括禁用科技的效果。不再并行维护专用
的 `buffs` 和 `technology_disabled` 字段。

Schema version 2 使用 `team_zx_sequential_v1` 身份。Unit、Projectile、Building 和
Status 各自拥有独立 ID 命名空间，每个空间都从 1
开始且不留空洞；Formation 使用另一套从 1 开始的连续命名空间。对于初始 Unit，
build 2227 已有证据支持的顺序是：先按 team-controller 顺序，再在每个 team 内按统一
世界坐标 `z` 升序、`x` 升序。同队 Unit 坐标相同属于非法，不引入虚构 tie-breaker。
该规则只描述坐标，不将其翻译为受相机影响的“左上”等方位。初始非 Unit 对象保留
其游戏注册顺序，战斗中动态创建的对象按规范事件顺序分配。公共 `IdentityAllocator`
提供连续 ordinal；写入器在开始录像时拒绝空洞并校验初始 Unit 顺序。

在满足原生可观测性硬约束的前提下，基础快照只接纳满足以下至少一个条件的字段：

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
| Unit | `unit_id`、`team_id`、`formation_id`、`unit_type_id`、domain；位置、主体旋转、主技能瞄准姿态、当前速度和原生 MotionFSM 状态；碰撞半径；生命、最大生命、alive、active、targetable 和 visibility；个人护盾 active/enabled 状态及当前/最大能量。 |
| Projectile | `projectile_id`、team 和 owner；位置和朝向；目标对象引用；原生缓存目标位置和半径；released 标记及当前/最大投射物生命。不从配置或其它字段推导投射物类别、速度、active、命中结果或移除原因。 |
| Building | `building_id`、team 和建筑类型；位置、旋转和原生 bounds 宽/高；生命、最大生命、alive、destroyed、available、targetable 和 collision-enabled 状态。 |
| Status | `status_id`、原生 Buff 类型、source 和 target；additive stack；原始 `duration_time`、`max_duration_time`、`step_time`、`step_time_config`；finished 和 frozen。上述值保留为原生计数器，不重新解释为 elapsed 或 remaining。 |

以下 Adapter 来源映射是该基础 schema 的规范组成部分：

| 世界对象 | 直接原生来源 |
| --- | --- |
| Unit | Team 归属来自 `FightController.GetTeamControllers` 和 `FightTeamController.GetTeamIndex`；成员和 Formation 来自 `FightTeam.GetMeches` 与 `FightMech.GetMechTeam`；类型/domain 来自 `GetMechID`、`IsFly`；主体与瞄准变换来自 `GetFightTransform`、`GetMainSkill().GetMainTransform()`；速度来自 `MotionController.GetCurrentVelocity`；motion 来自 `MotionController.fsm.GetCurrentState`；半径、gauge 和标记来自 `GetRadius`、`GetLife`、`GetMaxLife`、`IsAlive`、`get_IsActive`、`IsValidTarget(0)`、`GetVisibility`；个人护盾来自 `GetEnergyShieldController`。 |
| Projectile | 枚举与 Team 来自 `ProjectileSystem.projectileControllers` 和 `ProjectileController.GetTeamController`；owner、target、transform、缓存目标信息、released 及 life 直接来自 `FightProjectile.GetOwner`、`GetTarget`、`GetFightTransform`、`GetTargetInfo`、`IsRelease`、`GetLife`、`GetMaxLife`。 |
| Building | 成员/Team 来自 `FightTeam.GetTowers`；类型/顺序、transform、bounds、life 和标记来自 `GetBuildingType`、`GetBuildingIndex`、`GetFightTransform`、`GetBoundsRect`、`GetLife`、`GetMaxLife`、`IsAlive`、`IsDestroyed`、`IsAvaliable`、`IsValidTarget(0)`、`GetBuildingData().get_EnableCollision()`。 |
| Status | 枚举来自 `FightMech.GetBuffManager` 和 `BuffManager.buffs`；类型/source、stack 和标记来自 `Buff.GetBuffID`、`GetSource`、`GetAdditiveStack`、`IsFinish`、`IsFreeze`；四个计时器来自同名 `Buff` 原生字段；target 是其 BuffManager 包含该 Buff 的 `FightMech`。 |

运行时指针只用于将上述原生对象映射为来源中立、从 1 开始的 ID，指针值本身不持久化。

Visibility 是与原生对齐的封闭 enum：`normal`、`disappear`、`stealth`、`hide`。
motion state 精确映射原生 MotionFSM 的 `idle`、`move`、`attack`、`stop` 状态类。
无法直接映射的候选字段或 enum case 应从 schema 排除，而不是用 `unknown`、推导值、
默认值填充，也不应成为拒绝其它本可采集战斗的理由。

索敌缓存、控制器时钟、求解器缓冲等存疑状态，不能仅因旧模拟器保存了它们就进入
基础 S。它们先记录在 `I`；只有原生证据证明其跨逻辑步持续存在，且是产生下一步已
接纳 `S/E` 结果所必需的，才能提升为正式状态。

## 规范化事件日志

`E` 是由原生执行点直接报告的有类型、有顺序战斗日志，不是根据相邻快照重建的变更
日志。每个事件属于目标 tick 的 `E(t)`；事件在数组中的位置就是从 `S(t-1)` 推进至
`S(t)` 期间的规范顺序，不再
重复保存 `event_seq` 或时间戳。

必需的基础事件类别为：

| 类别 | 必需事件及 payload |
| --- | --- |
| 投射物释放 | `ProjectileSystem.AddProjectile` 直接提供投射物身份、原生 owner 和 target；不根据配置或 splash radius 推导投射物类别。 |
| 投射物移除 | `ProjectileSystem.Destroy` 直接提供投射物身份、owner、target、当前位置以及原生 `intercepted` 参数；不推导命中结果或更细的移除原因。 |
| 伤害 | `DamagePerformer.Perform` 的正返回值直接提供 `amount`，其原生 provider 和 target 提供 source 与 target 身份；不由生命/护盾差分合成事件，也不合成治疗和死亡事件。 |

事件必须使用与快照相同的来源中立对象身份。事件类型及 payload 是
`mechcore-mcfr` 实现的封闭 `EventPayload` enum；未知字段和来源专用事件类型会被
拒绝。数组顺序保留这些被 Hook 的原生操作实际发生顺序。

`S` 始终是权威状态。`E` 用于解释状态转换，但不能成为另一套相互冲突的持久状态
来源。

## 临时研究轨道

`I` 只用于闭合尚未解决的机制，可以包含同样满足原生可观测性硬约束的临时原生状态
采样、事件、输入或中间观测。以下示例只有在存在直接原生 probe 时才能接纳：

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
并排除在正式结果哈希之外。不进入正式哈希并不放宽原生来源要求。

参考 API 提供 `InstrumentationSink`。Adapter 采集和 simulation 可以通过同一接口提交
`step`、`channel`、`content_type` 和任意字节 payload。`InstrumentationWriter::record_json`
用于便捷提交规范 JSON；`NoInstrumentation` 可以在不改变生产者控制流的情况下关闭
采集。Sidecar 通过 `scenario_hash` 绑定正式录像，并记录自身的 `profile` 和 `producer`；
它没有正式 state、event 或 result hash。通用字节接口只是传输机制，不表示允许写入
未证实来源或推导得到的 channel；每个 profile 都必须另行记录原生 probe 与 payload
语义。

研究结束时：

- 中间值或诊断值继续留在 MCFR 之外，其采集实现可以删除；
- 经证明为确定性复现所必需的值，必须先提升为 `D`、`S` 或 `E` 中的来源中立字段，
  然后才能删除临时探针。

## 共同生产路径

原生采集与模拟不得分别维护独立的 MCFR 序列化实现。两条路径都直接使用
`mechcore-mcfr` crate 提供的公共数据模型和写入器：

```text
game adapter capture ----\
                          +--> mechcore-mcfr::McfrWriter --> .mcfr
Rust simulation kernel --/
```

`mechcore-mcfr` 统一负责逻辑记录类型、规范排序、验证、哈希、HDF5 序列化及对应的
读取器。Adapter 可以在游戏进程内直接调用该 crate；MCP 可以选择输出路径并编排录像
生命周期，但不是文件序列化的必要中间层。

## HDF5 container version 2

`.mcfr` 文件采用 HDF5。写入器先创建同目录临时文件；只有全部 dataset、元数据和哈希
完成后，才在不覆盖已有文件的前提下发布最终路径。

| 路径 | HDF5 类型 | 含义 |
| --- | --- | --- |
| `/context/data` | 连续 `u8` | 一条规范 `D` 记录。 |
| `/ticks/{unit,projectile,building,status,event}_offsets` | 分块 `u64` | 各 tick 的变长行边界，包含初始零。 |
| `/ticks/hash` | 分块 `u8 [tick,32]` | 每个逻辑 tick 的原始独立 BLAKE3 哈希。 |
| `/states/{units,projectiles,buildings,statuses}/<field>` | 分块有类型列 | 跨 tick 连续保存所有直接观测的快照字段；三分量向量使用 `[row,3]`。 |
| `/events/<field>` | 分块有类型列 | 跨 tick 连续保存有序事件类型、引用和 payload。 |

根属性保存格式标识、container/schema 版本、`tick_count`、`terminal_tick`、
`scenario_hash` 和 `result_hash`。Container version 2 要求 `tick_count >= 1`、
`terminal_tick = tick_count - 1`、每条变长轨道比 tick 多一个 offset，并要求 `E(0) = []`。

逻辑 API 按 tick 提供 AoS 形式，HDF5 物理布局则按字段采用 SoA。每对 offset 直接选择
一个 tick 的行；不使用逐 tick group、HDF5 变长值或逐 tick JSON blob。数值列采用跨多行
chunk，并使用 shuffle + deflate level 1。布尔状态标志做无损 bit-pack。封闭事件 union
共享 payload 列；某事件类型未使用的 payload cell 固定为零且没有逻辑含义。该布局支持
流式追加、按字段读取和直接随机访问某一比较 tick，当前不使用状态差分。

I sidecar 使用 HDF5 格式标识 `mechcore.mcfr.instrumentation`，在 `/records` 下保存
step、channel、content type、payload 字节和 payload offset，并通过根属性保存
`scenario_hash`、`profile`、`producer` 和 `record_count`。

## 规范哈希

正确性定义在规范逻辑内容上，而不是 HDF5 文件的原始字节上。HDF5 库版本、元数据
顺序、chunk 布局、压缩和来源 provenance 都可能改变物理字节而不改变战斗内容。

Schema version 2 使用带 domain separation 的 BLAKE3，并在每条规范记录前加入一个
little-endian `u64` 长度。正式哈希模型为：

```text
scenario_hash = BLAKE3("scenario-v2", canonical D, canonical S(0))
tick_hash(t)  = BLAKE3("tick-v2", little_endian_u64(t), canonical S(t), canonical E(t))
result_hash   = BLAKE3("result-v2", scenario_hash, little_endian_u64(tick_count), tick_hash(0)..tick_hash(n))
```

`D` 只包含有类型字段：schema/build 身份、时序和数值尺度、战斗回合、match seed 与
身份契约。生产者私有 JSON、配置 fingerprint 和来源专用命令不进入正式 scenario hash。

只有 schema 版本和 `scenario_hash` 均相同的两个录像才能比较。

- 首个不相等的 `tick_hash(t)` 就是首个发生分歧的逻辑 tick，可直接读取该 tick 的
  `S(t)/E(t)` 诊断；
- tick hash 彼此独立而非链式，因此较晚 tick 再次相等时可以识别分歧后的重新收敛；
- `result_hash` 相同，表示两个生产者在该场景已接受的 `S/E` 契约范围内一致。

整文件哈希可以另外用于保护传输完整性，但它不是原生/模拟正确性的判据。来源
provenance 作为元数据保留，并排除在 `result_hash` 之外。

规范编码递归排序对象 key；世界对象集合按来源中立身份排序，事件数组保留原生操作
顺序。正式快照数值由 schema 定义为整数；
非有限浮点数不能进入规范 JSON 表达。

## 验证边界

MCFR Reader 校验覆盖容器结构、规范解码、轨道长度、哈希的存在与编码，以及初始规范
身份契约。Writer 生成并持久化相互独立的逐 tick hash；比较时直接扫描这些 hash，只在
首个分歧 tick 读取 `S/E`。Reader 不从 HDF5 轨道重建全部 hash，也不判断游戏状态转换、
引用、数值、gauge 或事件序列在玩法逻辑上是否合法；这些属于特定游戏分析器或
simulation 测试的职责。

以下结论应当明确区分：

- 文件结构有效；
- 文件能够从 `S` 播放；
- 两个文件具有相同的状态演化；
- 两个文件具有相同的规范化事件；
- 无需临时 `I` 证据即可理解某项机制。

## 待完成的规范工作

后续修订必须依次确定：

1. 通过原生证据决定每个存疑 `I` 字段应提升还是排除；
2. 面向播放器的读取和插值契约。

这些细节必须由原生采集可行性、确定性内核需求和播放需求共同推导，不得以某个专题
研究案例单独决定。
