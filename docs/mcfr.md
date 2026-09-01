# MCFR v4 格式规范（format 0.1.0）

本文描述仓库当前实现的 MCFR v4 逻辑模型、物理容器、Adapter 原生采集来源和 Reader/Writer 校验契约。统一格式标识为：

```text
format = "0.1.0"
```

当前 Adapter 原生字段映射绑定游戏 build `1.11.1.3.2259`。其他 build 可以生成同格式录像，前提是 Producer 已验证所用原生接口与本文语义一致。

## 文件结构

一份 `.mcfr` 是 STORE-only ZIP64，成员集合固定为：

```text
recording.mcfr
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
| `ticks.parquet` | DurableContext、录像元数据、每帧摘要 | `T(1)..T(n)` | Parquet + Zstd level 6 |
| `units.parquet` | 存活 FightMech 完整状态 | `S(0)..S(n)` | Parquet + Zstd level 6 |
| `projectiles.parquet` | ProjectileSystem 中的弹体完整状态 | `S(0)..S(n)` | Parquet + Zstd level 6 |
| `buildings.parquet` | 各 FightTeam 当前存活的 Crystal/Construction 状态 | `S(0)..S(n)` | Parquet + Zstd level 6 |
| `shields.parquet` | AdvancedEnergyShieldSystem 中仍存在的战场护盾状态 | `S(0)..S(n)` | Parquet + Zstd level 6 |
| `terrains.parquet` | RangeItemSystem 中当前存在的动态战场地形及单位作用关系 | `S(0)..S(n)` | Parquet + Zstd level 6 |
| `events.jsonl` | 相邻快照之间的有序离散事件 | `E(1)..E(n)` | UTF-8 JSON Lines，LF 结尾 |

ZIP 层采用 STORE，数据压缩由 Parquet page 的 Zstd 完成。六个 Parquet 成员的 row group 按 128 个逻辑 tick 刷新，单个 row group 的行数上限为 1,000,000。状态表按 `(tick, object_id)` 排序，事件按 `(tick, ordinal)` 排序。

逻辑时间线为：

```text
scenario = { D, S(0) }
T(t)     = { S(t), E(t), tick_hash(t) }, 1 <= t <= n
S(t-1) --E(t)--> S(t)
```

`S(0)` 写入五个状态 Parquet 的 tick 0 行。`ticks.parquet` 和 `events.jsonl` 从 tick 1 开始。`tick_count` 至少为 1，`terminal_tick` 等于 `tick_count`。

---

# Part I — `ticks.parquet` 与场景上下文

## 1.1 作用

`ticks.parquet` 是容器索引。它保存格式标识、DurableContext、整体摘要以及从 `T(1)` 开始的逐帧哈希。

## 1.2 Parquet schema

```text
tick       : UINT32 required
tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
```

`tick` 的合法序列精确为 `1..=tick_count`。每行的 `tick_hash` 覆盖同 tick 的 `S(t)` 和 `E(t)`。

## 1.3 文件元数据

Parquet key-value metadata 的 key 和 value 均为 UTF-8 字符串。

| key | 数据规范 | 含义 |
| --- | --- | --- |
| `format` | 精确值 `0.1.0` | MCFR 逻辑与物理契约版本 |
| `game_build` | 非空 UTF-8 | 采集构建 provenance；Adapter 来自 `UnityEngine.Application.get_version()` |
| `durable_context` | canonical JSON | 单回合保持稳定的上下文 `D` |
| `scenario_hash` | 64 位小写十六进制 | `format`、DurableContext 与 `S(0)` 的摘要 |
| `result_hash` | 64 位小写十六进制 | `scenario_hash` 与全部 `tick_hash` 的摘要 |
| `tick_count` | `u32` 规范十进制 | `S(0)` 后记录的推进次数 |
| `terminal_tick` | `u32` 规范十进制 | 已确认的最终逻辑边界；当前连续时间线中等于 `tick_count` |

## 1.4 DurableContext 字段

| 字段 | 类型与数据规范 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `logic_step` | `Rational<u32>`，分子分母均大于 0 | 每次逻辑推进的秒数 | 当前 Adapter 固定为 `1/20` |
| `time_units_per_second` | `u32 > 0` | 原生离散时间单位密度 | build 2259 固定为 `2000` |
| `combat_round` | `u32 > 0` | 当前战斗回合 | `CurrentMatch.get_RoundCount()` |
| `match_seed` | `i32` | 原生战斗随机种子 | `CurrentMatch.GetRandom().GetSeed()` |

`durable_context` 的完整 canonical JSON 是场景哈希输入。`game_build` 作为独立文件 metadata 描述采集来源，使同一逻辑场景在不同等价 build 下保持相同摘要。

---

# Part II — `units.parquet`

## 2.1 行集合与采集边界

每一行表示一个 `LiveUnitState`。Adapter 逐队遍历 `FightTeam.GetMeches()`，选择 `FightMech.IsAlive() == true` 的对象。每个 tick 是完整快照；单位退出存活集合后由 `unit_died` 事件保留离散死亡事实。

## 2.2 顶层 schema 与字段来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `unit_id` | `UINT64 required` | Unit namespace 内稳定 ID | format 0.1.0 身份规则，见附录 B |
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
| `building_id` | `UINT64 required` | Building namespace 内稳定 ID | 初始按 `(team_id, building_index)` 分配，动态对象顺序追加 |
| `team_id` | `UINT32 required` | 建筑所属队伍 | 当前 FightTeam controller index |
| `building_type_id` | `UINT32 required` | 原生建筑类型 | `FightCrystal.GetBuildingType()` |
| `position` | `QVec3 required` | 世界坐标 | FightTransform `GetPositionInt3D()` |
| `rotation` | `INT64 required` | 朝向，Q32.32 raw | FightTransform `GetRotationInt()` |
| `bounds_width` | `INT64 required` | 边界宽度，Q32.32 raw | `GetBoundsRect().size.x` |
| `bounds_height` | `INT64 required` | 边界高度，Q32.32 raw | `GetBoundsRect().size.y` |
| `life` | `GaugeI32 required` | 当前/最大生命 | `GetLife()` / `GetMaxLife()` |
| `available` | `BOOLEAN required` | 原生可用状态 | `IsAvaliable()` |
| `targetable` | `BOOLEAN required` | 当前目标合法性 | `IsValidTarget(visibility=0)` |
| `collision_enabled` | `BOOLEAN required` | 建筑数据启用碰撞 | `GetBuildingData().get_EnableCollision()` |

该 Part 的集合定义同时给出了 v4 中 “building” 的合法来源：队伍演化列表内、当前存活、可分配稳定 Building ID 的对象。

---

# Part V — `shields.parquet`

## 5.1 行集合与系统边界

每一行表示 `AdvancedEnergyShieldSystem` 全量集合中仍存在的一个独立 `FightEnergyShield`。Adapter 从 `FightTeam.GetFightGroup()` 取得原生 `IFightGroup`，读取 `GetEnergyShields(fightGroup)`；inactive 但仍可在以后重新激活或跨回合回满的对象仍保留在状态轨中。

该状态轨表达与投射物球体相交的战场护盾。单位自身的 `EnergyShieldController` 继续保存在 `LiveUnitState.personal_shield`，不分配 Shield ID。战场盾不参与 RVO、单位移动阻挡或 layout 部署占位。

## 5.2 schema、字段含义与来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `shield_id` | `UINT64 required` | Shield namespace 内稳定 ID | 初始按队伍与全量集合顺序分配，动态对象顺序追加 |
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

原生系统分别维护全量集合与活跃集合。开战时两者使用同一排序，但 `ActiveEnergyShield()` 会把战斗中重新激活的对象追加到活跃集合。因此 `active_order` 不能由 `shield_id` 与 `active` 一般性推导。

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

列表按 `unit_id` 严格升序。`unit_id` 必须引用同 tick 的存活 Unit。controller 提供正数 `effectTimeDuration` 时写入 `periodic_clock`；持续减速、减射程等没有周期触发器的作用关系使用 null。

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
create(game_build, context)
set_initial_state(S(0))
append_tick(S(1), E(1))
...
append_tick(S(n), E(n))
finish()
```

Writer 在目标同目录创建临时成员和 `.zip.part`，完成 Parquet footer、JSONL、ZIP 封装后，以 `McfrReader` 重新打开并复核结构与哈希，最后通过 `persist_noclobber` 发布目标文件。目标路径在创建时保持空闲，发布具有防覆盖语义。

每次写入前，WorldSnapshot 先按规范顺序 canonicalize，再执行对象 ID、列表顺序、状态 bit 和 modifier 约束校验。一次局部写入失败会使 Writer 进入 poisoned 状态，完整文件只由成功的 `finish()` 发布。

## 8.2 Reader 验证

Reader 在打开容器时验证：

- ZIP 成员集合、STORE method、ZIP64 可读性与成员唯一性；
- 六个 Parquet schema、required/nullable 结构及 Zstd column compression；
- `game_build`、DurableContext canonical JSON、元数据类型和格式标识；
- tick 连续性、状态表排序、事件排序与 ordinal 连续性；
- ObjectRef、enum tag、初始身份顺序、列表顺序、`status_mask` 保留位和 modifier 分量；
- `scenario_hash`、全部 `tick_hash` 与 `result_hash` 的重新计算结果。

合法录像具有一个 `S(0)`、至少一个 `T(1)`，并在 `terminal_tick` 处完整结束。

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

## B.1 format 0.1.0 身份规则

format `0.1.0` 固定采用 `team_zx_sequential_v1`。初始 Unit 按 `(team_id, position.z, position.x)` 严格升序排列，再依次分配 `unit_id = 1..N`。同一队伍的初始单位具有唯一 `(z, x)`，因此 Adapter 与 Simulator 可从相同场景构造相同编号。

战斗期间首次出现的 Unit 按首次观察顺序取得当前 Unit namespace 的下一个连续编号。Unit namespace 从 1 开始单调递增；历史引用持续使用对象首次取得的编号。

`formation_id` 由初始 Unit 顺序首次遇到的 formation 依次分配。动态 Unit、Projectile 和 Building 在各自 namespace 中按首次观察顺序追加。ID 生命周期覆盖其退出快照后的历史引用。

初始 Building 按 `(team_id, native building_index)` 排序后分配。初始 Shield 按 `(team_id, native full-list index)` 分配，战斗中新进入全量集合的 Shield 按首次观察顺序追加。初始 Terrain 按 `(terrain_type, native controller item index)` 分配，动态 Terrain 按首次观察顺序追加。Shield 与 Terrain 从各自权威集合移除后，原生指针进入 tombstone 并保持历史 ID 唯一。

状态快照最终统一按对象 ID 排序；`skill_dynamic_modifiers` 按 `skill_slot`，`weapon_aims` 按 `(skill_slot, weapon_index)`，投射物 `spawn_containing_shields` 按 Shield ObjectRef 排序。

## B.2 状态与事件的同帧约定

`E(t)` 表示从 `S(t-1)` 推进到 `S(t)` 期间由 hook 直接观测的事件。`S(t)` 是该次推进结束后的权威完整状态。因而同一 tick 的死亡/摧毁事件可以与对象退出 `S(t)` 同时出现。

# 附录 C — Canonical encoding 与哈希

Canonical JSON 使用 UTF-8、递归字典序排列 object key、紧凑编码和 schema 定义的数组顺序。哈希输入的每一段使用：

```text
LE_u64(byte_length) || bytes
```

Hasher 为 BLAKE3，并以固定前缀 `mechcore.mcfr.canonical\0` 开始。三个 domain 为：

```text
scenario-0.1.0
tick-0.1.0
result-0.1.0
```

摘要定义：

```text
scenario_hash = H_s(format, DurableContext, S(0))
tick_hash(t)  = H_t(LE_u32(t), S(t), E(t))
result_hash   = H_r(scenario_hash, LE_u32(tick_count), tick_hash(1)..tick_hash(n))
```

元数据中的 scenario/result hash 使用 64 位小写十六进制；`ticks.parquet.tick_hash` 保存原始 32 bytes。

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
