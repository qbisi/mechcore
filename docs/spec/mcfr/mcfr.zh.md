# MCFR v6 格式规范（format 0.5.0）

[English](mcfr.md)

本文描述仓库当前实现的 MCFR v6 逻辑模型、物理容器、Adapter 原生采集来源和 Reader/Writer 校验契约。统一格式标识为：

```text
format = "0.5.0"
```

当前 Adapter 原生字段映射绑定游戏 build `1.11.1.3.2259`。其他 build 可以生成同格式录像，前提是 Producer 已验证所用原生接口与本文语义一致。

## 文件结构

一份 `.mcfr` 是 STORE-only ZIP64，成员集合固定为：

```text
recording.mcfr
├── layout.yaml
├── ticks.parquet
├── units.parquet
├── projectiles.parquet
├── buildings.parquet
├── shields.parquet
├── terrains.parquet
└── events.jsonl
```

| 成员 | 逻辑内容 | 时间覆盖 | 物理编码 |
| --- | --- | --- | --- |
| `layout.yaml` | 可直接重放的规范化场景布局，首行为 `kind: layout` | 录像级 | UTF-8 YAML，LF 结尾 |
| `ticks.parquet` | DurableContext、录像元数据、每帧摘要 | `T(1)..T(n)` | Parquet + Zstd level 6 |
| `units.parquet` | 存活 FightMech 完整状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `projectiles.parquet` | ProjectileSystem 中的弹体完整状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `buildings.parquet` | 各 FightTeam 当前存活的 Crystal/Construction 状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `shields.parquet` | AdvancedEnergyShieldSystem 中仍存在的战场护盾状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `terrains.parquet` | RangeItemSystem 中当前存在的动态战场地形及单位作用关系 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `events.jsonl` | 相邻快照之间的有序离散事件 | `E(1)..E(n)` | UTF-8 JSON Lines，LF 结尾 |

ZIP 层采用 STORE，数据压缩由 Parquet page 的 Zstd 完成。六个 Parquet 成员的 row group 按 128 个逻辑 tick 刷新，单个 row group 的行数上限为 1,000,000。状态表按 `(tick, object_id)` 排序，事件按 `(tick, ordinal)` 排序。

`layout.yaml` 由 Adapter 在 `StartCapture` 的部署期读取游戏对象并缓存，随后进入战斗采样。
两侧来自 `PlayerManager.GetPlayerControllers()`。单位读取 `UnitManager.GetUnits()` 中
`CardElement` 的类型、原生索引、等级、MapElement 位置/朝向、装备以及
`SuperDeploymentSystem.IsTravellingUnit()`；建筑装置读取
`ConstructionManager.GetConstructionElements()`。军官科技来自 `OfficerManager`，单位科技
来自完整单位目录对应的 `TechnologyManager`。研究塔、能量塔、contraption 和战场技能分别
读取其 manager 中的当前部署状态、释放记录和落点。未知类型、重复身份、断裂的升级链或不完整
释放记录均使采集 fail-close。

布局采用规范 YAML：显式记录 `seed`，省略原生值为格式默认值的字段，并按原生 Unit index
保留 `formations` 声明顺序；四种初始防御建筑按 manager 顺序记录在同级
`constructions`。`mechcore doc verify/format` 和 Simulator 在执行前应用完整布局合法性校验。该成员用于自包含重放和
`mechcore fight verify`，不直接进入 `physics_*_hash` 或 `content_*_hash`。

逻辑时间线为：

```text
T(t)     = { S(t), E(t), physics_tick_hash(t), content_tick_hash(t) }, 1 <= t <= n
S(1)     = 第一次原生逻辑更新完成后的状态
```

所有状态、事件和逐 tick 摘要都从 tick 1 开始；部署完成后的 `S(0)` 不落盘。`tick_count` 至少为 1，`terminal_tick` 等于 `tick_count`。

---

# Part I — `ticks.parquet` 与场景上下文

## 1.1 作用

`ticks.parquet` 是容器索引。它保存格式标识、DurableContext、整体摘要以及从 `T(1)` 开始的逐帧哈希。

## 1.2 Parquet schema

```text
tick               : UINT32 required
physics_tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
content_tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
```

`tick` 的合法序列精确为 `1..=tick_count`。`physics_tick_hash` 覆盖同 tick 的稳定战斗物理投影，`content_tick_hash` 覆盖完整 `S(t)` 和 `E(t)`；精确定义见附录 C。

## 1.3 文件元数据

Parquet key-value metadata 的 key 和 value 均为 UTF-8 字符串。

| key | 数据规范 | 含义 |
| --- | --- | --- |
| `format` | 精确值 `0.5.0` | MCFR 逻辑与物理契约版本 |
| `game_build` | 非空 UTF-8 | 采集构建 provenance；Adapter 来自 `UnityEngine.Application.get_version()` |
| `durable_context` | canonical JSON | 单回合保持稳定的上下文 `D` |
| `physics_hash_profile` | 精确值 `battle-physics-v1` | 稳定物理投影版本 |
| `physics_result_hash` | 64 位小写十六进制 | 全部 `physics_tick_hash` 的有序摘要；回归判断依据 |
| `content_hash_profile` | 精确值 `mcfr-content-0.5.0` | 完整内容摘要版本 |
| `content_result_hash` | 64 位小写十六进制 | 全部 `content_tick_hash` 的有序摘要；格式内诊断依据 |
| `tick_count` | `u32` 规范十进制 | 从 `S(1)` 开始记录的逻辑 tick 数 |
| `terminal_tick` | `u32` 规范十进制 | 已确认的最终逻辑边界；当前连续时间线中等于 `tick_count` |

## 1.4 DurableContext 字段

| 字段 | 类型与数据规范 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `logic_step` | `Rational<u32>`，分子分母均大于 0 | 每次逻辑推进的秒数 | 当前 Adapter 固定为 `1/20` |
| `time_units_per_second` | `u32 > 0` | 原生离散时间单位密度 | build 2259 固定为 `2000` |
| `combat_round` | `u32 > 0` | 当前战斗回合 | `CurrentMatch.get_RoundCount()` |

`match_seed` 不写入 `ticks.parquet`。Writer 仍用调用方提供的 seed 校验嵌入布局，Reader 则从规范化 `layout.yaml.seed` 恢复公开 `DurableContext.match_seed`。`game_build` 作为独立文件 metadata 描述采集来源。

---

# Part II — `units.parquet`

## 2.1 行集合与采集边界

每一行表示一个 `LiveUnitState`。Adapter 逐队遍历 `FightTeam.GetMeches()`，选择 `FightMech.IsAlive() == true` 的对象。每个 tick 是完整快照；单位退出存活集合后由 `unit_died` 事件保留离散死亡事实。

## 2.2 顶层 schema 与字段来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `unit_id` | `UINT64 required` | Unit namespace 内稳定 ID | format 0.5.0 身份规则，见附录 B |
| `team_id` | `UINT32 required` | 当前所属队伍 | `FightTeam` controller index |
| `original_team_id` | `UINT32 required` | 首次出现时的队伍 | 首次采样的 `team_id` |
| `formation_id` | `UINT64 required` | 编队身份 | `FightMech.GetMechTeam()` 指针映射 |
| `unit_type_id` | `UINT32 required` | 原生单位类型 | `FightMech.GetMechID()` |
| `domain` | `UINT8 required` | 地面或空中 | `FightMech.IsFly()` |
| `position` | `QVec3 required` | 世界坐标 | FightTransform `GetPositionInt3D()` |
| `body_rotation` | `INT64 required` | 机体朝向，Q32.32 raw | FightTransform `GetRotationInt()` |
| `velocity` | `QVec3 required` | 当前速度 | MotionController 原生速度 |
| `motion_state` | `UINT8 required` | idle/moving/attacking/stopped | 原生运动状态机映射 |
| `mech_lock_target` | nullable `ObjectRef` | FightMech 当前锁定对象 | `FightMech.lockTarget` |
| `collision_radius` | `INT64 required` | 碰撞半径，Q32.32 raw | `FightMech.GetRadius()` |
| `life` | `GaugeI32 required` | 当前/最大生命 | `GetLife()` / `GetMaxLife()` |
| `active` | `BOOLEAN required` | 当前激活状态 | `get_IsActive()` |
| `targetable` | `BOOLEAN required` | 当前目标合法性 | `IsValidTarget(visibility=0)` |
| `visibility` | `UINT8 required` | 原生可见性状态 | `GetVisibility()` |
| `status_mask` | `UINT64 required` | 四个原生布尔状态 | 见 2.3 |
| `buff_modifiers` | nullable struct | BuffManager 非零综合数值修正 | 见 2.4 |
| `unit_dynamic_modifiers` | nullable struct | FightMech 非零单位级动态修正 | 见 2.5 |
| `skill_dynamic_modifiers` | required sparse list | 各技能的非零动态修正 | 见 2.6 |
| `personal_shield` | required struct | 单位个人能量盾状态 | 见 2.7 |
| `weapon_aims` | required list | 主技能与子技能的各武器通道状态 | 见 2.8 |
| `derived` | required struct | 战斗实际读取的那几个数，即所有修正作用之后的结果 | 见 2.9 |

## 2.9 `derived`

上面那几个 modifier 结构说的是**写到**单位身上的东西；这一个说的是 build 由它们**算出来**
的东西。测量一条合成规则时要读的正是这两半，而把两半都记下来意味着读数是"一份录像的一个
tick"，而不是"一场被设计成结果能区分候选假设的战斗"。

| 字段 | 类型 | 原生来源 |
| --- | --- | --- |
| `move_speed` | `INT64 required`，Q32.32 原始值 | `FightMech.GetMoveSpeed()` |
| `attack_range` | `INT64 required`，Q32.32 原始值 | 0 号技能槽的 `FightSkill.GetAttackRange()` |
| `attack_damage` | `INT32 required` | 0 号技能槽的 `FightSkill.GetNormalDamage(0)` |
| `attack_interval` | `INT32 required` | 0 号技能槽的 `FightSkill.GetCurrentAttackInterval()` |

0 号槽就是模拟器所建模的那个技能。各槽不同的单位目前不在闭包内；等它进来时这里会长成
按槽的列表，而分歧会先在内容层暴露出来——那正是这一层的用途。

**攻击间隔记的是 build 自己的整数**，不是它的 property 给出的 `FPoint` 秒——
`RefreshAttackInterval` 就是把 property 除以步长再截断，而整数是两边都能持有、又不必裁定
"谁的舍入算数"的那种形式。它数的是逻辑 tick：一秒二十个。

**这是两个后端明知会答不一样的唯一一个字段。** build 的整数对长弓和铁锤比描述低七个 tick，
对犀牛则与描述齐平，它到底从哪儿开始数还没人读出来。模拟器写的是它自己的间隔、用同样的
tick，所以同一场仗的原生录像和模拟录像在这一项上**有些单位一致、有些差那个偏移**。这个差
是一次读数而不是一个缺陷——它说明描述层漏掉了 build 施加的某样东西——而内容层正是这种东西
该浮出来的地方。

`tests/layouts/modifier/interval-order.mcscript` 正是**在没有定位那个零点的情况下**，用这个
字段量出了合成规则里的顺序：三份 fixture 把那条直线钉死，第四份对着它读。

这些都是内容层字段。物理层不哈希它们，所以加上它们之后，所有已录制的
`physics_result_hash` 一个都没变。

## 2.3 `status_mask`

`status_mask` 保存当前采样边界的四个原生 bit。合法 mask 为 `0x0..=0xF`。

| bit | 名称 | 原生来源 | 规范字段语义 |
| ---: | --- | --- | --- |
| 0 | `invincible` | `BuffManager.IsInvincible()` | 记录原生 `IsInvincible()` 在采样边界的布尔值 |
| 1 | `frozen` | `BuffManager.IsFreeze()` | 记录原生 `IsFreeze()` 在采样边界的布尔值 |
| 2 | `technology_disabled` | `FightMech.IsTechnologyDisabled()` | 单位科技效果禁用 |
| 3 | `recovery_disabled` | `FightMech.IsRecoverDisabled()` | 单位恢复能力禁用 |

这四个 bit 表达 BuffDataInt/单位原生布尔状态的当前值。`invincible` 和 `frozen` 的名称沿用原生 predicate；战斗效果解释采用 build-bound 原生分支证据与受控场景观察。当前研究假设将 `invincible` 指向电磁干扰免疫，`frozen` 的效果映射继续通过运动、攻击、索敌和技能执行观察收敛。数值 Buff 的当前综合效果由 `buff_modifiers` 表达。

## 2.4 `buff_modifiers`

Buff 通道来自 `FightMech.GetBuffManager()` 的综合 getter。每个 Rate 字段保存 Q32.32 raw 的 `{ add: i64, reduce: i64 }`，每个 Value 字段保存 `{ add: i32, reduce: i32 }`。中性值统一为 0，add/reduce 分量均为非负数。

Adapter 每帧读取表中的完整 getter 集合。Parquet 中各 modifier 字段以及 add/reduce 数值叶均为 nullable；null 按 0 读取。全部字段为 0 时，`buff_modifiers` 根 struct 为 null。字段缺失表示已成功采集且当前无对应修正，不表示采集不可用。

原生 reduce getter 返回剩余倍率 `native_reduce_factor`；MCFR 保存其减少量：

```text
reduce = 1.0_q32_32 - native_reduce_factor
```

| 字段 | 类型 | Adapter 原生来源 |
| --- | --- | --- |
| `move_speed_rate` | Rate | `GetMoveSpeedChangeAddRate/ReduceRate` |
| `move_speed_value` | Value | `GetMoveSpeedChangeValue`，按符号拆分 |
| `damage_rate` | Rate | `GetDamageChangeAddRate/ReduceRate` |
| `attack_interval_rate` | Rate | `GetAttackIntervalChangeAddRate/ReduceRate` |
| `extra_attack_interval_rate` | Rate | `GetExtraAttackIntervalChangeAddRate/ReduceRate` |
| `amplify_damage_rate` | Rate | `GetAmplifyDamageAddRate/ReduceRate` |
| `attack_range_value` | Value | `GetAttackRangeAddValue/ReduceValue` |
| `extra_attack_range_value` | Value | `GetExtraAttackRangeAddValue/ReduceValue` |
| `attack_range_rate` | Rate | `GetAttackRangeAddRate/ReduceRate` |
| `extra_attack_range_rate` | Rate | `GetExtraAttackRangeAddRate/ReduceRate` |

`amplify_damage_rate` 表示作用于该单位的承伤倍率修正。Buff 的应用、持续与消退通过连续快照中 `status_mask` 和 `buff_modifiers` 的变化观察。

## 2.5 `unit_dynamic_modifiers`

单位动态通道读取 FightMech 支持的完整 `MechDataChange*` 集合。`i64` 字段保存原生定点 raw，Rate 字段使用 `{add, reduce}`，`i32` 字段保存原生整数。

表内 modifier 字段与数值叶均为 nullable，null 按 0 读取；全部字段为 0 时，`unit_dynamic_modifiers` 根 struct 为 null。Adapter 仍调用完整 enum/getter 集合，再由 MCFR 写入层稀疏编码非零结果。

| 字段组 | 字段 |
| --- | --- |
| `MechDataChangeFloat` / `i64` | `gf_range_value`, `gf_life_time_value`, `mech_group_distance` |
| `MechDataChangeFloatRate` / Rate | `life_rate`, `life_rate_by_kill_count`, `reduce_damage_from_remote`, `move_ability_exit_time_change_rate`, `move_speed_change_rate`, `amplify_damage_rate` |
| `MechDataChangeInt` / `i32` | `move_speed_value`, `reduce_damage_value`, `child_inherit_technology_effect` |

该通道记录单位级运行时变化，与 BuffManager 综合修正保持独立。

## 2.6 `skill_dynamic_modifiers`

列表项 schema：

```text
skill_slot : UINT16 required
modifiers  : SkillDynamicModifierSet required
```

Adapter 遍历 `FightMech.GetSkills()`，以技能槽位排序并读取每个技能支持的完整 `SkillDataChange*` 集合。

`SkillDynamicModifierSet` 的 26 个 modifier 字段及其数值叶均为 nullable，null 按 0 读取。列表只保留至少一个字段非零的 skill；当前没有技能级动态修正时使用空列表。省略的 skill slot 表示该槽位当前无动态修正，不表示单位缺少该技能。

| 字段组 | 字段 |
| --- | --- |
| `SkillDataChangeFloat` / `i64` | `min_attack_range_value`, `attack_range_value`, `attack_air_range_add_value`, `attack_ground_range_add_value`, `attack_interval_value`, `damage_change_rate_ground`, `damage_change_rate_air`, `splash_range_value`, `cb_life_recovery_rate`, `projectile_speed_value`, `attack_point_change_value`, `projectile_duration_value`, `projectile_random_range`, `additional_damage_by_target_life` |
| `SkillDataChangeFloatRate` / Rate | `damage_rate`, `damage_rate_by_kill_count`, `attack_range_rate`, `attack_interval_rate`, `damage_reduce_rate_base`, `projectile_life_rate` |
| `SkillDataChangeInt` / `i32` | `projectile_count_value`, `air_attack_value`, `ground_attack_value`, `attack_range_value_air`, `attack_range_value_ground`, `is_lock_target` |

该通道记录技能/武器级运行时变化。科技、装备和回合增益在原生技能数据中形成的动态变化可由此通道归因。

## 2.7 `personal_shield`

```text
personal_shield = {
  active  : BOOLEAN required,
  enabled : BOOLEAN required,
  energy  : GaugeI32 required
}
```

Adapter 通过 `GetEnergyShieldController()` 读取 `IsActive()`、`IsEnable()`、`GetEnergy()` 和 `GetMaxEnergy()`。

## 2.8 `weapon_aims`

列表项 schema：

```text
skill_slot   : UINT16 required
weapon_index : INT32 required
attack_target: ObjectRef nullable
position     : QVec3 nullable
rotation     : INT64 nullable
```

Adapter 遍历 `FightMech.GetSkills()`，覆盖主技能与子技能，再遍历各技能的 `GetWeapons()`。`skill_slot` 表示技能通道；`weapon_index` 来自 `WeaponData.get_Index()`，表示该技能内的原生武器通道编号。`attack_target` 来自技能的 `GetAttackTarget()`；姿态来自武器 `GetFightTransform()`。武器缺少 FightTransform 时，position 与 rotation 同时为 null。

列表按 `(skill_slot, weapon_index)` 严格升序。`weapon_aims` 独立枚举当前武器通道，因此可以包含未出现在稀疏 `skill_dynamic_modifiers` 中的 skill slot。

---

# Part III — `projectiles.parquet`

## 3.1 行集合与采集边界

每一行表示一个 `ProjectileState`。Adapter 从当前 Fight 的 module 集合定位 `ProjectileSystem`，遍历其 `projectileControllers`，并通过 controller 的 `GetFightProjectile()` 取得弹体。每个 tick 保存系统当前枚举到的完整集合。

## 3.2 schema、字段含义与来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `projectile_id` | `UINT64 required` | Projectile namespace 内稳定 ID | 指针到来源中立 ID 的映射 |
| `team_id` | `UINT32 required` | 弹体所属队伍 | controller `GetTeamController().GetTeamIndex()` |
| `owner` | nullable `ObjectRef` | 发射者 | `FightProjectile.GetOwner()` |
| `position` | `QVec3 required` | 当前世界坐标 | FightTransform `GetPositionInt3D()` |
| `orientation` | `INT64 required` | 当前朝向，Q32.32 raw | FightTransform `GetRotationInt()` |
| `target` | nullable `ObjectRef` | 当前目标 | `FightProjectile.GetTarget()` |
| `cached_target_position` | `QVec3 required` | 弹体缓存的目标坐标 | `GetTargetInfo().GetPosition()` |
| `cached_target_radius` | `INT64 required` | 弹体缓存的目标半径 | `GetTargetInfo().GetRadius()` |
| `released` | `BOOLEAN required` | 原生对象池释放/重置标记 | `FightProjectile.IsRelease()` 返回 `isReleased` |
| `life` | `GaugeI32 required` | 弹体当前/最大生命 | `GetLife()` / `GetMaxLife()` |
| `spawn_containing_shields` | required `list<ObjectRef>` | 弹体创建时已经包含它的活跃战场盾，按 Shield ID 排序 | `ProjectileController.inEnergyShields` |

`owner` 和 `target` 可引用已经退出当前快照的历史对象。引用身份在整份录像内保持稳定。

`released` 属于弹体对象池生命周期：`FightProjectile.Init()` 将 `isReleased` 置为 false，`ProjectileController` 的对象池重置路径调用 `FightProjectile.ResetData()` 后将其置为 true。`projectiles.parquet` 中的活跃弹体通常处于 false；发射事实由 `projectile_released` event 表达。

`spawn_containing_shields` 保存原生投射物相交算法的出生豁免集合：从盾内发射的弹体会跳过该盾，直到离开后再次穿越。空列表表示出生时不在任何活跃战场盾内；列表元素必须是 `ObjectKind::Shield`。

---

# Part IV — `buildings.parquet`

## 4.1 行集合与采集边界

每一行表示一个当前存活的 `BuildingState`。Adapter 对每个 FightTeam 合并以下三个原生集合并按对象指针去重：

```text
FightTeam.GetTowers()
FightTeam.buildings
FightTeam.constructions
```

集合成员通过 `IsAlive()` 进入快照。该枚举覆盖研究塔等塔类 FightCrystal、队伍建筑与 construction，也覆盖速射炮等由队伍持有且以 FightCrystal 参与演化的对象。对象死亡时退出状态集合，并由 `building_destroyed` 事件保存摧毁事实。

## 4.2 schema、字段含义与来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `building_id` | `UINT64 required` | Building namespace 内稳定 ID | 初始按下述采集正则化顺序分配，动态对象顺序追加 |
| `team_id` | `UINT32 required` | 建筑所属队伍 | 当前 FightTeam controller index |
| `building_type_id` | `UINT32 required` | 原生建筑类型 | `FightCrystal.GetBuildingType()` |
| `position` | `QVec3 required` | 世界坐标 | FightTransform `GetPositionInt3D()` |
| `bounds_width` | `INT64 required` | 边界宽度，Q32.32 raw | `GetBoundsRect().size.x` |
| `bounds_height` | `INT64 required` | 边界高度，Q32.32 raw | `GetBoundsRect().size.y` |
| `life` | `GaugeI32 required` | 当前/最大生命 | `GetLife()` / `GetMaxLife()` |
| `available` | `BOOLEAN required` | 原生可用状态 | `IsAvaliable()` |
| `targetable` | `BOOLEAN required` | 当前目标合法性 | `IsValidTarget(visibility=0)` |
| `collision_enabled` | `BOOLEAN required` | 建筑数据启用碰撞 | `GetBuildingData().get_EnableCollision()` |

该 Part 的集合定义同时给出了 v4 中 “building” 的合法来源：队伍演化列表内、当前存活、可分配稳定 Building ID 的对象。

Building 不记录 `rotation`；该字段不在 JSON 状态或 Parquet schema 中，也不参与
canonical tick/result hash。Unit 的 `body_rotation` 与武器姿态的 `rotation` 不受影响。

---

# Part V — `shields.parquet`

## 5.1 行集合与系统边界

每一行表示 `AdvancedEnergyShieldSystem` 全量集合中仍存在的一个独立 `FightEnergyShield`。Adapter 从 `FightTeam.GetFightGroup()` 取得原生 `IFightGroup`，读取 `GetEnergyShields(fightGroup)`；inactive 但仍可在以后重新激活或跨回合回满的对象仍保留在状态轨中。

该状态轨表达与投射物球体相交的战场护盾。单位自身的 `EnergyShieldController` 继续保存在 `LiveUnitState.personal_shield`，不分配 Shield ID。战场盾不参与 RVO、单位移动阻挡或 layout 部署占位。

## 5.2 schema、字段含义与来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `shield_id` | `UINT64 required` | Shield namespace 内稳定 ID | S(1) 按队伍和 active_order 一次性分配，inactive/首 tick 已移除项按确定性键随后分配；动态对象顺序追加 |
| `team_id` | `UINT32 required` | 当前所属队伍 | `GetTeamController().GetTeamIndex()` |
| `source_kind` | `UINT8 required` | 数据源种类 | `get_EnergyShieldData()` 的运行时类型 |
| `owner` | nullable `ObjectRef` | 绑定的 FightActor；定点部署盾为 null | `GetOwner()` |
| `position` | `QVec3 required` | 护盾球心 | FightTransform `GetPositionInt3D()` |
| `radius` | `INT64 required` | 投射物相交球半径，Q32.32 raw | `GetRadius()` |
| `energy` | `GaugeI32 required` | 当前能量与当前有效最大值 | `GetEnergy()` / `GetMaxEnergy()` |
| `round_policy` | `UINT8 required` | 回合结束时的对象策略 | `IsShortLifeTime()` / `IsResetNextRound()` 按原生分支优先级归一化 |
| `active` | `BOOLEAN required` | 当前是否参与投射物拦截 | `get_IsActive()` |
| `active_order` | nullable `UINT32` | 所属 FightGroup 活跃集合内的零基顺序 | `GetActiveEnergyShields(fightGroup)` 的实际下标 |

`source_kind` 的四个值分别对应 `EnergyShieldContraption`、`CS_EnergyShield`、`AdvancedEnergyShieldController` 和 `SpawnAdvancedShieldController`。未知数据源使 build 2259 Adapter fail-close。

`round_policy` 按下列有序规则保存：short-lived 对象为 `destroy_at_round_end`；其余 reset-next-round 对象为 `reset_to_max`；其余为 `retain_state`。`reset_to_max` 使用回合结束时数据源提供的当前有效最大值，因此护盾专家对既有普通盾造成的最大能量刷新可以直接体现在 Gauge 变化中。

## 5.3 活跃顺序与约束

原生系统分别维护全量集合与活跃集合。`ActiveEnergyShield()` 会把重新激活的对象追加到活跃集合，包括跨回合重新激活的既存盾。因此两种集合的顺序可以不同，`active_order` 不能由 `shield_id` 与 `active` 一般性推导。

合法状态满足：

- `active = false` 时 `active_order = null`；`active = true` 时必须有值；
- 当前 Training Ground 的每个 FightGroup 内非 null `active_order` 连续覆盖 `0..k-1`；
- `radius` 为正；
- 快照按 `shield_id` 严格升序；
- owner 可以引用已退出当前状态集合、但仍具有录像级稳定身份的对象。

`system_order` 只用于初始 ID 分配与 Adapter 一致性检查，不进入文件。

---

# Part VI — `terrains.parquet`

## 6.1 行集合与系统边界

每一行表示 `RangeItemSystem` 某个 `RangeItemController.GetItems()` 当前集合中的动态地形。Adapter 按原生 `RangeItemType` 枚举 controller，并为集合成员分配 Terrain ID；尚未实例化的 controller 与返回 null 的空 item 集合都表示该类型当前行集合为空。对象退出集合时，其最后状态与身份继续供事件引用。

`TerrainType` 覆盖原生六种类型：`fire`、`oil`、`fog`、`acid`、`recovery_zone`、`fog_sand`。其中四个可部署战场技能的对应关系为燃烧弹 `fire`、粘油弹 `oil`、烟雾弹 `fog`、酸液弹 `acid`。

## 6.2 schema、字段含义与来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `terrain_id` | `UINT64 required` | Terrain namespace 内稳定 ID | `RangeItem` 指针首次观察顺序 |
| `team_id` | nullable `UINT32` | 创建/持有该地形的队伍；无 owner 地形为 null | `RangeItem.GetTeamController()` |
| `terrain_type` | `UINT8 required` | 六种原生 `RangeItemType` | 所属 controller 与 `RangeItem.GetRangeItemType()` 交叉校验 |
| `position` | `QVec3 required` | 地形中心或线段节点位置 | `RangeItem.GetPosition()` |
| `radius` | `INT64 required` | 作用半径，Q32.32 raw | `RangeItem.GetRange()` |
| `grid` | nullable struct | 网格地形的原点、尺寸与逐行 bit mask | `IsGridMode()` / `GetGridBlock()` |
| `remaining_rounds` | nullable `UINT32` | 当前边界剩余的跨回合次数 | `GetDuration() - get_Round()`，仅跨回合地形写值 |
| `logic_lifetime` | nullable struct | 逻辑帧内寿命计数 `{elapsed, limit}` | `RangeItem.time/lifeTime`；`FightGroundFire` 使用其同名字段 |
| `applications` | required list | 当前直接作用到的单位及可用周期时钟 | controller 的 `affectedUnits` 与 `affectedUnitTimes` |

`grid` schema 为：

```text
origin_x : INT64 required
origin_y : INT64 required
size_x   : UINT32 required
size_y   : UINT32 required
rows     : list<UINT32> required
```

`origin_x/origin_y` 保存 Q32.32 raw。`rows` 长度等于 `size_y`，每行低 `size_x` bit 表示该行有效网格；当前原生 `GridBlockInt` 的宽高上限均为 32。

`applications` 列表项为：

```text
unit_id        : UINT64 required
periodic_clock : struct<elapsed: INT32, duration: INT32> nullable
```

列表按 `unit_id` 严格升序。`unit_id` 必须引用同 tick 的存活 Unit。controller 提供正数
`effectTimeDuration` 时写入 `periodic_clock`；持续减速、减射程等没有周期触发器的作用关系使用
null。时钟中的两个整数都以逻辑步为单位，并通过 `DurableContext.logic_step` 换算为秒；它们不使用
`time_units_per_second` 的尺度。

`remaining_rounds` 是一个稀疏派生字段。当前 1v1 原生场景中粘油是四种可部署地形里跨回合保留的类型；字段直接保存相减结果，Reader 使用这个单值解释跨回合余量。普通燃烧、酸液和烟雾行使用 null。

## 6.3 身份、生命周期与事件关系

初始 Terrain 按 `(terrain_type, controller item index)` 分配 ID，战斗中新出现的对象按首次观察顺序追加。退出集合的原生指针进入 tombstone，同一录像内不能继承旧 Terrain ID。

每 tick 的 `terrains.parquet` 行存在即表示该 Terrain 属于 controller 权威 item 集合。`terrain_created` 与 `terrain_removed` 保存相邻边界之间的集合成员变化。当前 Adapter 的成员 diff 记录所有至少跨越一个快照边界的实例；实现了原生 lifecycle hook 的 Producer 还可记录同一逻辑推进内完成创建与移除的瞬时实例。

---

# Part VII — `events.jsonl`

## 7.1 文件与排序规范

`events.jsonl` 使用 UTF-8、LF 行尾和每行一个 JSON object。空事件流对应零字节文件。每行字段按 Writer 固定顺序和紧凑 JSON 编码输出。

同一 tick 的 `ordinal` 从 0 连续递增。全文件按 `(tick, ordinal)` 严格升序，tick 合法范围为 `1..=tick_count`。

## 7.2 公共字段

| 字段 | JSON 类型 | 含义 |
| --- | --- | --- |
| `tick` | number (`u32`) | 事件归属的推进时刻 |
| `ordinal` | number (`u32`) | 同 tick 内的观测顺序 |
| `type` | string | 事件种类 tag |
| `object` | `ObjectRef` 或 null | 事件主体 |
| `source` | `ObjectRef` 或 null | 直接来源对象；记录该引用的事件类型固定包含此字段 |
| `source_team_id` | number (`u32`) 或 null | 事件来源或归属对象在事件边界的队伍快照 |
| `target` | `ObjectRef` 或 null | 直接目标对象 |

JSON 中的 `ObjectRef.id`、`formation_id` 和 Q32.32 raw 均使用规范十进制字符串，确保跨语言精确读取。`ObjectRef.kind` 使用 `unit`、`projectile`、`building`、`shield`、`terrain` 字符串。

## 7.3 事件种类与 payload

| `type` | 必要引用 | payload 字段 | 语义 |
| --- | --- | --- | --- |
| `projectile_released` | `object` | `skill_slot: u16|null`, `weapon_index: i32|null` | 弹体创建并加入 ProjectileSystem 的发射事实及其武器通道；两个通道字段同时有值或同时为 null |
| `projectile_removed` | `object` | `position_q32_32: QVec3`, `intercepted: bool`, `absorbed_by: ObjectRef|null` | 弹体从系统移除；`absorbed_by` 指明直接吸收它的战场盾 |
| `damage` | `target` | `amount: i32` | 单个实际 target 的一次正数伤害结果；Actor 使用原生 `damageReal` |
| `unit_created` | `object` | `team_id: u32`, `formation_id: u64`, `unit_type_id: u32`, `position_q32_32: QVec3` | Unit 生命周期起点 |
| `unit_died` | `object` | `position_q32_32: QVec3` | Unit 死亡边界 |
| `building_destroyed` | `object` | `position_q32_32: QVec3` | Building 摧毁边界 |
| `unit_team_changed` | `object` | `previous_team_id: u32`, `new_team_id: u32` | Unit 当前阵营变化 |
| `shield_created` | `object` | `team_id: u32`, `source_kind: string`, `position_q32_32: QVec3` | Shield 加入全量集合并取得身份 |
| `shield_destroyed` | `object` | `position_q32_32: QVec3`, `reason: string` | Shield 从全量集合移除，生命周期结束 |
| `terrain_created` | `object`；省略 `source` | `team_id: u32|null`, `terrain_type: string`, `position_q32_32: QVec3`, `radius_q32_32: i64` | Terrain 加入 controller item 集合并取得身份 |
| `terrain_removed` | `object` | `position_q32_32: QVec3`, `reason: string` | Terrain 退出 controller item 集合，生命周期结束 |
| `terrain_converted` | `object` | `position_q32_32: QVec3` | 同一 Terrain 发生原生类型/归属转换 |
| `healing` | `target` | `amount: i32` | 一次正数恢复结果 |

`projectile_removed` 的合法原因组合为：原生拦截系统移除使用 `intercepted=true, absorbed_by=null`；战场盾吸收使用 `intercepted=false, absorbed_by=ShieldRef`；其他移除使用两者均为空/false。`absorbed_by` 只能引用 Shield。

`shield_destroyed.reason` 使用 `energy_depleted`、`owner_destroyed`、`round_end`、`scripted` 或 `unknown`。Producer 只在原生销毁入口能确定原因时写具体值；当前 Adapter 由全量集合消失确认生命周期结束，因此写 `unknown`。

`terrain_removed.reason` 使用 `time_expired`、`round_expired`、`grid_depleted`、`cleared` 或 `unknown`。当前 Adapter 由 controller item 集合消失确认生命周期结束，因此写 `unknown`。

`terrain_created` 表达 controller 集合成员的生命周期起点，其 JSON object 由 Terrain 身份、队伍、类型、位置和半径完整确定。投射物因果关系通过同 tick、同位置的 `projectile_removed` 与 Terrain 状态变化联合观察。

## 7.4 当前 Adapter 原生事件来源

共享 MCFR model 与 JSONL codec 实现上表事件。build 2259 Adapter 当前事件 producer 覆盖下列原生观测：

| 事件 | 当前原生来源 |
| --- | --- |
| `projectile_released` | `ProjectileSystem.Create` 到 `AddProjectile` 的嵌套 trace；同时解析 owner、target、skill slot、weapon index |
| `projectile_removed` | ProjectileSystem 销毁路径 trace，采集末位置与 intercepted；同一伤害链命中 Shield 时补充 `absorbed_by` |
| `damage` | `DamagePerformer.Perform` 提供 provider 归因作用域；每次 `FightController.OnActorHitted(HitDamageInfo)` 提供 Actor target 与 `damageReal`；战场盾伤害 helper 提供逐 Shield 实际结果 |
| `unit_died` | `FightMech.OnDead` trace |
| `building_destroyed` | `FightCrystal.OnDead` trace |
| `shield_created` | 相邻采样边界间首次进入 `GetEnergyShields(fightGroup)` 全量集合 |
| `shield_destroyed` | 相邻采样边界间从全量集合消失，当前 reason 为 `unknown` |
| `terrain_created` | 相邻采样边界间首次进入所属 `RangeItemController.GetItems()` 集合 |
| `terrain_removed` | 相邻采样边界间从所属 controller item 集合消失，当前 reason 为 `unknown` |

`unit_created`、`unit_team_changed`、`terrain_converted` 与 `healing` 具备共享 schema、canonical 表达和 Reader/Writer 支持，供能够直接观测对应演化来源的 Producer 写入。

Buff 观测位于 Unit 状态轨道：布尔状态进入 `status_mask`，综合数值修正进入 `buff_modifiers`。相邻快照的字段变化构成 Buff 状态演化证据。

---

# Part VIII — 写入、读取与验证

## 8.1 Writer 生命周期

Writer 的公开生命周期为：

```text
create(game_build, context, layout_yaml)
append_tick(S(1), E(1))
...
append_tick(S(n), E(n))
finish()
```

Writer 在目标同目录创建临时成员和 `.zip.part`，完成 Parquet footer、JSONL、ZIP 封装后，以 `McfrReader` 重新打开并复核结构及持久化哈希元数据，最后通过 `persist_noclobber` 发布目标文件。目标路径在创建时保持空闲，发布具有防覆盖语义。

每次写入前，WorldSnapshot 先按规范顺序 canonicalize，再执行对象 ID、列表顺序、状态 bit 和 modifier 约束校验。一次局部写入失败会使 Writer 进入 poisoned 状态，完整文件只由成功的 `finish()` 发布。

## 8.2 Reader 验证

Reader 在打开容器时验证：

- ZIP 成员集合、STORE method、ZIP64 可读性与成员唯一性；
- `layout.yaml` 的 UTF-8、共享结构、规范化表示以及 round 与 DurableContext 一致性；
- 六个 Parquet schema、required/nullable 结构及 Zstd column compression；
- `game_build`、DurableContext canonical JSON、元数据类型和格式标识；
- tick 连续性、状态表排序、事件排序与 ordinal 连续性；
- ObjectRef、enum tag、初始身份顺序、列表顺序、`status_mask` 保留位和 modifier 分量；
- 两类 tick hash 行的连续性、宽度和编码，两类 result hash 及 profile 的编码。

Reader 信任 MCFR 自身持久化的两类 tick/result hash。`McfrReader::open()` 不会为了校验哈希而逐 tick 重建 `S(t)`/`E(t)`，也不重新计算整条时间线的哈希；`first_divergence()` 直接比较持久化的 `physics_tick_hash`。定位差异后，调用方可按需读取对应 tick 的完整内容和 `content_tick_hash`。

合法录像至少具有一个 `T(1)`，不存在 tick 0 状态或事件，并在 `terminal_tick` 处完整结束。

---

# 附录 A — 公共数据类型与 enum tag

## A.1 Q32.32

空间、旋转以及原生定点 modifier 使用有符号 `i64` raw bits：

```text
real_value = raw / 2^32
```

`QVec3 = { x: i64, y: i64, z: i64 }`。Parquet 使用三个 required `INT64` 子字段；JSONL 使用十进制字符串。position、radius 和 bounds 的量纲为米，velocity 为米/秒，rotation/orientation 为度。

## A.2 Gauge 与 Rational

```text
GaugeI32 = { current: i32, maximum: i32 }
Rational = { numerator: u32, denominator: u32 }
```

Gauge 保留原生有符号值。Rational 的分子分母均大于 0。

## A.3 ObjectRef

```text
ObjectRef = { kind: ObjectKind, id: u64 }
```

每个 ObjectKind 使用独立 ID namespace，ID 从 1 开始，在录像生命周期内保持稳定。

## A.4 enum tag

| 类型 | Parquet tag |
| --- | --- |
| `ObjectKind` | `0=unit`, `1=projectile`, `2=building`, `3=shield`, `4=terrain` |
| `Domain` | `0=ground`, `1=air` |
| `MotionState` | `0=idle`, `1=moving`, `2=attacking`, `3=stopped` |
| `Visibility` | `0=normal`, `1=disappear`, `2=stealth`, `3=hide` |
| `ShieldSourceKind` | `0=contraption`, `1=commander_skill`, `2=owner_advanced`, `3=spawned_temporary` |
| `ShieldRoundPolicy` | `0=destroy_at_round_end`, `1=reset_to_max`, `2=retain_state` |
| `TerrainType` | `0=fire`, `1=oil`, `2=fog`, `3=acid`, `4=recovery_zone`, `5=fog_sand` |

# 附录 B — 身份与排序约定

## B.1 format 0.5.0 身份规则

format `0.5.0` 采用 `team_zx_sequential_v1`。初始 Unit 按 `(team_id, position.z, position.x)` 严格升序排列，再依次分配 `unit_id = 1..N`。同一队伍的初始单位具有唯一 `(z, x)`，因此 Adapter 与 Simulator 可从相同场景构造相同编号。

战斗期间首次出现的 Unit 按首次观察顺序取得当前 Unit namespace 的下一个连续编号。Unit namespace 从 1 开始单调递增；历史引用持续使用对象首次取得的编号。

`formation_id` 由上述初始 Unit 顺序首次遇到的 formation 依次分配。因此初始
`formation_id` 的首现顺序必须严格为 `1..F`；它表达成员按世界 `z/x` 排序后的首现编号，
不等同于布局 formation 声明索引。布局声明顺序保留原生 Unit index，因为 formation
成员散布使用 `match_seed + unit_index`。动态 Unit、Projectile 和 Building 在各自 namespace
中按首次观察顺序追加。ID 生命周期覆盖其退出快照后的历史引用。

初始 Building 按 `(team_id, building_type_id, position.x, position.y, position.z)`
严格升序分配 `building_id = 1..N`，位置使用 Q32.32 raw 整数；相同键的多个对象使
采集失败，不能用原生指针或创建计数器作为不稳定的 tie-breaker。一项 construction
可对应多段城墙，每段按自己的位置获得独立 ID。layout 不保存 construction index；
原生 building/construction index 和 layout 声明顺序都不参与 MCFR Building ID 分配。
两种游戏模式采用同一规则。分配后的 ID 及所有 ObjectRef 在整场保持稳定，动态对象
按首次观察时的同一排序追加；死亡或移位不会重新编号。

初始 Shield 在 S(1) 按队伍分组：活跃盾按 `active_order` 升序，同队 inactive 盾随后。首 tick 已移除但事件仍可能引用的盾排在全部 S(1) 对象之后，再按队伍分组，确保初始状态中的 ID 从 1 连续。inactive/已移除组内按 `(source_kind, owner, position.x/y/z, radius, round_policy, energy.maximum, energy.current)` 排序，已移除项使用最后观测状态；无法区分的相同键使采集失败。编号仅规范化一次，并同步转换 E(1) 与缓存引用，不能每 tick 用 active_order 重新编号。此后新盾按首次观察顺序追加，ID 不因失活、重激活或 active_order 变化而改变。初始 Terrain 按 `(terrain_type, native controller item index)` 分配，动态 Terrain 按首次观察顺序追加。Shield 与 Terrain 从各自权威集合移除后，原生指针进入 tombstone 并保持历史 ID 唯一。

状态快照最终统一按对象 ID 排序；`skill_dynamic_modifiers` 按 `skill_slot`，`weapon_aims` 按 `(skill_slot, weapon_index)`，投射物 `spawn_containing_shields` 按 Shield ObjectRef 排序。

## B.2 状态与事件的同帧约定

`E(t)` 表示从 `S(t-1)` 推进到 `S(t)` 期间由 hook 直接观测的事件。`S(t)` 是该次推进结束后的权威完整状态。因而同一 tick 的死亡/摧毁事件可以与对象退出 `S(t)` 同时出现。

# 附录 C — 分层哈希规范

## C.1 公共编码

Hasher 为 BLAKE3。每个摘要先输入固定前缀和 domain；前缀、domain 以及后续每个输入段都编码为：

```text
LE_u64(byte_length) || bytes
```

固定前缀为 `mechcore.mcfr.canonical\0`。整数均按指明宽度使用小端补码原始位；布尔值使用单字节 `0/1`；`Option<T>` 先写单字节存在位，再在存在时写 `T`；列表长度使用 `LE_u64`。所有公开摘要均编码为 64 位小写十六进制，Parquet tick 列保存原始 32 bytes。

## C.2 稳定物理层 `battle-physics-v1`

`physics_tick_hash` 不对完整 MCFR schema 做摘要，而只对版本冻结的战斗物理投影做摘要。新增 MCFR 观测字段不会改变该投影；若投影字段、单位、精度、顺序或编码语义必须改变，应发布新的 profile，不能就地修改 `battle-physics-v1`。

WorldSnapshot 在哈希前按附录 B 规范化。对象列表保持按稳定 ID 的顺序，事件保持原生 `ordinal` 顺序，武器姿态保持 `(skill_slot, weapon_index)` 顺序。Q32.32 保留 `i64` raw bits；角度在哈希前按 `360 << 32` 取欧几里得模，使相差整数圈的角度等价。`logic_step` 先约分。

每个 tick 由三个独立 lane 摘要组成：

| lane/domain | 纳入字段 |
| --- | --- |
| `battle-physics-kinematics-v1` | Unit：`unit_id, position, body_rotation, velocity`，以及仅含有效 `pose` 的 `skill_slot, weapon_index, pose.position, pose.rotation`；Projectile：`projectile_id, position, orientation`；Building：`building_id, position`；Shield：`shield_id, position, radius`；Terrain：`terrain_id, position, radius, grid(origin_x, origin_y, size_x, size_y, rows)` |
| `battle-physics-vitals-v1` | Unit：`unit_id, unit_type_id, team_id, domain, collision_radius, life, personal_shield.active/energy`；Projectile：`projectile_id, team_id, owner, released, life`；Building：`building_id, building_type_id, team_id, bounds_width, bounds_height, life, available, targetable, collision_enabled`；Shield：`shield_id, team_id, owner, radius, energy, active`；Terrain：`terrain_id, team_id, terrain_type, radius` |
| `battle-physics-interactions-v1` | 每项先纳入 `ordinal, subject, source, source_team_id, target`，再纳入事件类型及其物理 payload：弹体释放通道；弹体移除位置/拦截/吸收盾；伤害量；单位生成的队伍/类型/位置；单位死亡位置；建筑摧毁位置；单位换队；护盾生成队伍/位置；护盾摧毁位置；地形生成队伍/类型/位置/半径；地形移除或转换位置；治疗量 |

显式不纳入的内容包括：`game_build`、布局文本、seed、回合号和其他文件元数据；Unit 的 `original_team_id, formation_id, motion_state, mech_lock_target, active, targetable, visibility, status_mask`、所有 modifier、个人盾 `enabled`、武器 `attack_target` 和无姿态通道；Projectile 的 `target`、缓存目标位置/半径和生成时包含盾列表；Shield 的 `source_kind, round_policy, active_order`；Terrain 的剩余回合、逻辑寿命和单位应用内部时钟；事件的 `formation_id`、护盾来源类别、护盾/地形移除原因。这些字段仍由 `content_*_hash` 检测。

定义：

```text
K(t) = H_battle-physics-kinematics-v1(kinematics projection)
V(t) = H_battle-physics-vitals-v1(vitals projection)
I(t) = H_battle-physics-interactions-v1(interactions projection)

physics_tick_hash(t) = H_battle-physics-tick-v1(
    LE_u32(reduced_logic_step_numerator),
    LE_u32(reduced_logic_step_denominator),
    LE_u32(time_units_per_second),
    LE_u32(t),
    K(t), V(t), I(t)
)

physics_result_hash = H_battle-physics-result-v1(
    LE_u32(tick_count),
    physics_tick_hash(1)..physics_tick_hash(n)
)
```

因此物理回归仍精确引用逻辑时间、Q32.32 位置/角度/速度、生命与护盾以及伤害等相互作用，但不会因增加纯诊断字段而要求重录。

## C.3 完整内容层 `mcfr-content-0.5.0`

完整状态和事件先编码为 canonical JSON：UTF-8、递归字典序排列 object key、紧凑编码和 schema 定义的数组顺序。它覆盖 format 0.5.0 的全部 `S(t)`/`E(t)` 字段，用于同格式内的捕获完整性诊断；它不包含布局、DurableContext 或其他文件元数据。

```text
content_tick_hash(t) = H_content-tick-0.5.0(LE_u32(t), JSON(S(t)), JSON(E(t)))
content_result_hash  = H_content-result-0.5.0(
    LE_u32(tick_count),
    content_tick_hash(1)..content_tick_hash(n)
)
```

`mechcore fight compare` 与 `mechcore fight verify` 以物理层决定 `equal` 和首个分歧，同时单独返回 `content_equal`。这允许格式增加观测能力后保持物理回归身份，又不会掩盖同格式中的完整内容差异。

# 附录 D — 物理编码约定

- ZIP member 顺序固定为 `ticks`, `units`, `projectiles`, `buildings`, `shields`, `terrains`, `events`。
- ZIP member 使用 STORE 和固定时间戳，Parquet column chunk 使用 Zstd level 6。
- Parquet 全局 dictionary 关闭；team、类型、enum、固定半径/上限等低基数字段按列启用 dictionary。
- 各 Parquet 的 `tick` 列使用 `DELTA_BINARY_PACKED`。
- required list 使用空列表表达当前对象没有对应项；nullable struct 表达 `Option<T>`。
- modifier 数值叶为 nullable，null 的规范值为 0；Buff 和单位动态根 struct 全零时为 null，技能动态列表只保存非零 skill slot。
- 状态哈希使用展开后的零默认值；skill 全零项在 canonicalize 时移除，因此稀疏物理编码可重建相同的规范状态。
- 状态表每个 tick 保存完整当前集合，读取任一 tick 可直接重建 `S(t)`。
- JSONL 字段集合由事件 type 精确确定；Reader 逐行执行 canonical 类型、必要引用与字段集合校验。

# 附录 E — 原生 modifier 映射示例

粘油减速进入 BuffManager 的 `move_speed_rate` 综合值。光子投射产生的承伤变化进入 `amplify_damage_rate`，其 `IsInvincible()` 当前值进入 `status_mask.invincible`。剑齿虎科技副炮等子技能在 `skill_dynamic_modifiers` 和 `weapon_aims` 中使用各自 `skill_slot`；回合 `+15` 射程增益形成的技能级动态变化保留在对应 skill modifier 字段。

这些例子说明三个采集通道的归因边界：BuffManager 综合效果、FightMech 单位级动态修正、FightSkill 技能级动态修正分别持久化，原生字段归属保持可观察。
