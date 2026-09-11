# 动态地形系统研究（build 1.11.1.3.2259）

本文整理 Mechabellum build `1.11.1.3.2259` 的动态地形原生结构、MCFR 表达和已完成的原生采集结果。研究对象是 `RangeItemSystem` 管理的战斗内区域效果，不包括地图静态装饰、部署占位和单位移动碰撞。

## 1. 结论

- 动态地形的权威对象是 `RangeItem`，按 `RangeItemType` 分属不同的 `RangeItemController`。
- `RangeItemController.GetItems()` 返回当前存在的对象集合；MCFR 每 tick 枚举该集合形成 `terrains.parquet`。
- 战场技能和单位科技投射物都能创建 `RangeItem`。前者已静态确认经过 `RangeItemEffectController.PerformEffect -> RangeItemSystem.AddItem`；后者已由投射物移除与地形创建的同 tick、同位置关系完成运行时确认。
- 单位是否受到地形作用由 controller 更新作用成员集合。周期伤害类效果还维护单位级周期时钟。
- 战场护盾既能阻止落点处的地形对象创建，也能把与护盾相交的既有圆形地形转换成裁剪后的网格。
- `terrains.parquet` 的行存在就是对象当前存续事实；生命周期变化由 `terrain_created` 和 `terrain_removed` 表达。
- `terrain_created` 保存地形身份与初始空间属性。投射物归因由 `projectile_removed` 的 tick 和位置与地形事件联合建立。

## 2. 原生系统边界

### 2.1 对象与 controller

`RangeItemSystem` 按 `RangeItemType` 提供 controller。当前 MCFR 枚举覆盖六个原生类型：

| MCFR 类型 | 原生类别 | 已完成原生采集 |
| --- | --- | --- |
| `fire` | 火焰区域 | 战场燃烧弹、猎犬燃烧弹、黏油点燃 |
| `oil` | 黏油区域 | 战场黏油弹、火神黏油弹 |
| `fog` | 烟雾区域 | 战场烟雾弹 |
| `acid` | 酸液区域 | 战场酸液弹 |
| `recovery_zone` | 恢复区域 | 枚举与 Adapter 映射 |
| `fog_sand` | 沙尘/雾沙区域 | 枚举与 Adapter 映射 |

`RangeItemController.GetItems()` 直接返回 controller 保存的 item 列表。MCFR 以该列表为权威成员集合，并用原生指针在单份录像内维持 Terrain 身份。

### 2.2 创建链

战场技能的静态调用关系为：

```text
战场技能落地效果
  -> RangeItemEffectController.PerformEffect(position, index)
  -> RangeItemSystem.AddItem(...)
  -> 对应 RangeItemController 的 item 集合
```

build 2259 的 `RangeItemEffectController.PerformEffect` 直接调用 `RangeItemSystem.AddItem`。静态调用边确定创建入口；具体战场技能运行时配置由各自原生录像确认。

单位科技投射物的运行时链表现为：

```text
ProjectileReleased
  -> Projectile 在 projectiles.parquet 中演化
  -> projectile_removed（最终位置）
  -> 同 tick、同位置 terrain_created
  -> Terrain 进入 terrains.parquet
```

猎犬燃烧弹录像中 5 枚科技投射物均满足该对应关系。火神黏油弹录像中 10 枚科技投射物也都能与落地地形对应。

### 2.3 作用成员与效果时钟

`RangeItemController.Update()` 调用 `UpdateAffectedActorChange()` 更新作用成员。build 2259 静态证据显示该过程包含 `FightActor.IsValidTarget` 和二维范围检查；具体 controller 的效果应用通过虚调用分派。

Adapter 读取 controller 的 `affectedUnits` 形成 `applications`。存在正数 `effectTimeDuration` 时，同时读取每个单位的累计时钟：

```text
applications[]
  unit_id
  periodic_clock
    elapsed
    duration
```

已采集录像中：

- 战场火焰的周期时钟 `duration=4`；
- 战场酸液的周期时钟 `duration=19`；
- 烟雾作用关系使用空周期时钟；
- 持续减速等连续修正同样由作用成员关系表达。

这些值使用游戏原生逻辑计数单位。录像 DurableContext 的 `time_units_per_second=10`。

## 3. 空间表达

### 3.1 圆形模式

普通地形以 `position + radius` 表达圆形作用范围，`grid=null`。现有录像观察到的半径如下：

| 来源 | 类型 | 半径 |
| --- | --- | ---: |
| 战场燃烧弹 | `fire` | 40 m |
| 战场黏油弹 | `oil` | 30 m |
| 战场酸液弹 | `acid` | 40 m |
| 战场烟雾弹 | `fog` | 50 m |
| 猎犬燃烧弹科技 `11028` | `fire` | 12 m |
| 火神黏油弹科技 `11010` | `oil` / 点燃后的 `fire` | 18 m |

文件中的位置与半径保存为 Q32.32 raw integer。

### 3.2 网格模式

圆形区域需要扣除局部空间时，原生对象可以进入 grid mode。MCFR 保存：

```text
grid
  origin_x, origin_y : Q32.32 raw
  size_x, size_y     : u32
  rows               : list<u32>
```

规范行表示按 y 排列的 bit mask，每行低 `size_x` bit 对应有效 x 单元。build 2259 原生 `GridBlockInt.grids` 使用 x 列和 y 高位优先编码，Adapter 在读取边界转置为 MCFR 的 y 行、x 低位优先表示。

## 4. 护盾与地形

`RangeItemEffectLayerGrid.GenerateGrid` 的静态调用边包含：

```text
AdvancedEnergyShieldSystem.GetActiveEnergyShields
FightEnergyShield.GetFightTransform
FightEnergyShield.GetRadius
GridBlockInt.TryDisableGrid
GridBlockInt.GenerateMask
GridBlockInt.Sync
```

这说明地形网格生成会读取当前活动战场盾，并调用 `TryDisableGrid` 扣除盾内单元。

`sticky-oil-red-shield-edge.mcfr` 提供了对应运行时证据：

- 红方护盾世界坐标 `(56, 82)`，半径 70 m；
- 蓝方战场黏油的节点沿 `x=-60..60, z=40` 生成；
- `(0, 40)` 与盾心距离正好为 70 m；
- `x=0/20/40/60` 的节点没有创建 Terrain；
- `x=-60/-40/-20` 创建 Terrain；
- `x=-20` 的 30 m 圆与护盾相交，保存为 `12x12` 网格，共 93 个有效单元。

因此运行时表现由两个阶段组成：落点位于盾边缘或盾内时，地形对象创建被阻止；落点在盾外但作用圆与盾相交时，对象保留，盾内覆盖被 grid 裁剪。

战场盾只参与投射物/区域效果相交，不构成 RVO、单位移动或 layout 部署碰撞体。

## 5. 黏油点燃

火神黏油弹录像记录了完整的油→火演化：

- tick 2–20 发射 10 枚科技投射物，通道为 `skill_slot=1`，`weapon_index=2/3` 交替；
- tick 65–88 投射物落地；
- 6 枚先创建 `oil`，之后在相同位置移除 oil 并创建 `fire`；
- 4 枚落地位置已受到火焰作用，直接创建 `fire`；
- 共出现 6 次 oil 创建和 10 次 fire 创建；
- oil 与 fire 半径均为 18 m；
- 转换后的 fire 逻辑寿命上限为 240 个原生计数单位。

当前 Adapter 对该原生演化记录为 oil 的 `terrain_removed` 与新 fire 的 `terrain_created`。两个对象各自获得 Terrain ID，事件顺序与位置保留转换证据。

## 6. MCFR 状态字段

`terrains.parquet` 每行字段如下：

| 字段 | 含义 |
| --- | --- |
| `tick` | 状态所属逻辑 tick |
| `terrain_id` | Terrain namespace 内稳定身份 |
| `team_id` | 创建或持有该地形的队伍；无 owner 时为 null |
| `terrain_type` | 六种规范地形类型 |
| `position` | 地形中心或节点位置 |
| `radius` | 作用半径 |
| `grid` | 局部裁剪网格；普通圆形为 null |
| `remaining_rounds` | 跨回合余量；具有该原生语义时记录 |
| `logic_lifetime` | 当前逻辑寿命计数 `{elapsed, limit}` |
| `applications` | 当前直接作用到的 Unit 及周期时钟 |

行集合本身表达当前存续对象。对象退出 controller 集合后不再产生状态行，其身份进入 tombstone，继续供生命周期事件和历史 tick 引用。

已观察到的稀疏字段组合：

| 来源 | `remaining_rounds` | `logic_lifetime` |
| --- | --- | --- |
| 战场燃烧弹 | null | `limit=700` |
| 战场黏油弹 | 初始为 `2` | null |
| 战场酸液弹 | null | null |
| 战场烟雾弹 | null | null |
| 猎犬燃烧弹 | null | `limit=200` |
| 火神黏油弹 | null | null |
| 火神点燃地形 | null | `limit=240` |

战场黏油与火神黏油虽然都属于 `oil`，其跨回合字段不同，说明生命周期必须从当前 `RangeItem` 实例读取，不能只按 terrain type 推导。

## 7. 生命周期事件与归因边界

### `terrain_created`

事件记录：

- `object`: 新 TerrainRef；
- `source_team_id`: 创建边界的队伍快照；
- `target`: null；
- `team_id`、`terrain_type`、`position_q32_32`、`radius_q32_32`。

该事件表达 Terrain 首次进入 controller item 集合。投射物归因使用 `projectile_removed` 的 owner、目标、末位置与事件 tick 联合恢复。

### `terrain_removed`

事件记录 Terrain 退出 controller item 集合及最后位置。当前 Adapter 由集合 diff 确认移除，因此 `reason=unknown`。

### `terrain_converted`

共享 MCFR schema 支持同一 Terrain 身份内的类型或归属转换。当前已观察的黏油点燃以旧 oil 移除和新 fire 创建表达。

## 8. 单回合 Layout 表达边界

`layout.yaml` 在所属 `sides.<side>.terrains` 中保存战斗开始时已经存在的跨回合地形。
当前每条记录对应一次 `type: oil` 的黏油弹释放，`positions` 保存两个有序整数控制点。
build 2259 的 `CommanderSkillManager.CalculateAttackPositions` 从这两个点生成七个 Q32.32
中心，因此无需在 YAML 中重复保存五个分数中间点。该 private 方法未出现在运行时反射表，
Adapter 复用它所调用的原生 `FVector3`/`FPoint` 运算，以保持相同的定点舍入。

可选的 `grid_rows` 是 `0..=6` 原生点索引到最终占用网格的映射。省略或空 map 表示七点
全部激活且均为完整 30 m 圆；非空 map 的键集合就是完整激活集合，缺失键表示该点未生成。
映射值 `[]` 表示完整圆，12 个 row 的值表示护盾裁剪后的规范 `12 x 12`、y 行/x 低位优先
掩码。Layout 不保存 `remaining_rounds`，因为它不改变本次单回合战斗的初态。

Simulator 仍拒绝非空 `terrains`。Adapter 通过原生 `RangeItemSystem` 还原；完整闭合要求
将同一 GRBR 回合和试验场复原结果采集为 MCFR，并对照其 Terrain 集合和战斗轨迹。

存在两条互相校验、但不互相依赖的导出路径：

1. `mechcore_document::terrains_from_grbr_round` 直接从 GRBR 的 BinaryFormatter 包装中提取
   `BattleRecord` XML，读取指定 `PlayerRoundRecord`，再把 `activeState` 与扁平
   `gridInfo/ByteMask` 解码为零基激活索引和规范 y-row/x-bit 网格，同时保留原始两个
   `positions` 控制点。这是后续
   `mechcore grbr layout` 命令的纯文件输入函数。
2. 录像采集当前采用 Adapter 路径：在战斗前从 `RangeItemSystem ->
   RangeItemController.GetItems()` 枚举实际恢复后的 `RangeItem`，按共同 provider 聚合，
   从仍存活的零号和六号端点恢复两个控制点，并按 `RangeItem.Index` 导出
   `GridBlockInt`。回放恢复后 manager 已不再保留该 provider 的 release data，因此实时
   路径若缺少任一端点会 fail closed；纯 GRBR 路径没有此限制。这条路径同时检查具体 build
   的原生恢复结果。

试验场恢复先用同一组原生定点运算计算七个精确中心，再按 map 的索引顺序
调用 `RangeItemSystem.AddItem`。非空网格同时覆写原生 `Queue<ByteMask>` 和
`GridBlockInt.grids`，并立即读回检查；不能使用只会在现有网格上继续扣除 cell 的 `Sync`
来反序列化最终状态。值为 `[]` 的点以 `useGrid=false` 创建完整圆。

直接 GRBR 样本的 `activeState=227` 解码为零基索引 `0,1,5,6`；它与四组 `gridInfo` 按
激活索引顺序一一对应。旧的逐圆整数格式会把内部中心四舍五入，留下约 1/3 m 误差；新格式
由原生定点算法重算后，四点中心 raw 分别为
`(103079215104,-47244640256)`、`(28633116167,-40086361513)`、
`(-269151279577,-11453246537)`、`(-343597383680,-4294967296)`，录像和试验场完全一致。

2026-09-03 的交叉验证中，GRBR 直读 terrain 与录像实时导出的 layout 相同；录像和试验场
MCFR 均为 192 ticks，`physics_result_hash=580891840449133a6097087973acc1aaba0fe1339c247ed1af190fff85796b26`，
`content_result_hash=af4605b78120045edf520141c71a07802be3ba941df4a8af897e08f5455d7d60`，
且两个 `.mcfr` 文件 SHA-256 同为
`fb87df0265d25e3482f121e604e2a7dc51edda23a830c8f19e60501b574b64a5`。这确认本样本已闭合，
但其它地形类型或缺少存活端点的录像仍不能由本样本外推。

## 9. 原生采集材料

| 场景 | Layout | MCFR |
| --- | --- | --- |
| 战场燃烧弹 | `tests/layouts/terrain-fire.yaml` | `work/captures/terrain-native-20260901/fire.mcfr` |
| 战场黏油弹 | `tests/layouts/terrain-sticky-oil.yaml` | `work/captures/terrain-native-20260901/sticky-oil.mcfr` |
| 战场酸液弹 | `tests/layouts/terrain-acid.yaml` | `work/captures/terrain-native-20260901/acid.mcfr` |
| 战场烟雾弹 | `tests/layouts/terrain-smoke.yaml` | `work/captures/terrain-native-20260901/smoke.mcfr` |
| 护盾边缘与黏油 | `tests/layouts/terrain-sticky-oil.yaml` | `work/captures/terrain-native-20260901/sticky-oil-red-shield-edge.mcfr` |
| 猎犬燃烧弹 | `tests/layouts/terrain-hound-incendiary.yaml` | `work/captures/terrain-native-20260901/hound-incendiary-vs-crawlers.mcfr` |
| 火神黏油弹 | `tests/layouts/terrain-vulcan-sticky-oil.yaml` | `work/captures/terrain-native-20260901/vulcan-sticky-oil-vs-crawlers.mcfr` |
| GRBR 第二回合遗留黏油 | `tests/layouts/crower-computer-replay-round-2-terrain.yaml` | `work/research/grbr-round2-terrain-grouped-20260903/{replay-round-2-grouped,training-round-2-grouped}.mcfr` |

以上录像均来自 build `1.11.1.3.2259`。MCFR 录像是具体运行时分支和值的证据；反编译索引用于确认静态类型、直接调用关系和候选机制。

## 10. 静态证据与限制

索引根目录：`work/unity-index/1.11.1.3.2259/`

- backend：IL2CPP；
- Unity：`2022.3.62f3`；
- game identity：`5b4248d9af138a94e6a809bb0d529e2ecd8ef0ce0a03d33f2e123cb65370ee58`；
- Cpp2IL：`2022.1.0-pre-release.20+d5260685fddb380f0ee884521d28c8309b2aff7e`；
- 索引规模：350,728 symbols、662,014 call edges、36,361 sources。

主要证据包：

- `commander-range-item-ground-impact.md`：`PerformEffect -> RangeItemSystem.AddItem`；
- `range-item-controller-get-items.md`：controller 权威 item 集合；
- `range-item-affected-selection.md`：作用成员更新；
- `range-item-update-effect-clock.md`：效果虚分派入口；
- `range-item-effect-layer-grid-generate-grid.md`：活动护盾与网格裁剪调用关系。

静态索引没有单独证明具体战斗中采用了哪个动态分派目标或配置值。本文把这些值绑定到对应原生 MCFR 录像；恢复区、雾沙以及 `terrain_converted` 原生 producer 仍需要各自的运行时样本。
