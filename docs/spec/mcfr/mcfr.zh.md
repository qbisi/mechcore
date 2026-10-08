# MCFR 格式规范（format 0.18.0）

[English](mcfr.md)

本文描述仓库当前实现的 MCFR 逻辑模型、物理容器、Adapter 原生采集来源和 Reader/Writer 校验契约。统一格式标识为：

```text
format = "0.18.0"
```

当前 Adapter 原生字段映射绑定仓库在 `GAME_VERSION` 钉住的游戏版本。其他版本可以生成同格式录像，前提是 Producer 已验证所用原生接口与本文语义一致。

本文只规定一个哈希，覆盖时间线里的全部内容。两份录像当且仅当每个 tick 的每个字段都一致时才是同一场仗：哈希不会为了省一次重录而放过任何差异。它读哪些量逐项准入，见[哈希的准入](#c3-哈希的准入)。

## 文件结构

一份 `.mcfr` 是 STORE-only ZIP64，成员集合固定，另加录像请求过的 instrument 通道：

```text
recording.mcfr
├── layout.yaml
├── ticks.parquet
├── units.parquet
├── projectiles.parquet
├── buildings.parquet
├── shields.parquet
├── terrains.parquet
├── statistics.parquet
├── formations.parquet
├── events.parquet
└── instrument/<channel>.parquet   零个或多个
```

instrument 通道是研究要看的战斗内部过程（技能状态机、一次决策方法的每次调用），
每个通道一个成员 `instrument/<channel>.parquet`，哈希不读它，所以要不要通道
不影响任何钉子。通道的行是一个 Rust 类型，Arrow schema 从类型推出；第一列是
`tick`（`u32`），其后是类型自己的字段。请求了但没观测到行的通道也会发布为空成员；
没请求的通道不存在，读者得到 `None`。每个通道每 tick 的量不超过实体数的常数倍。
字段定义以 [English](mcfr.md#instrument-channels) 为准。

| 成员 | 逻辑内容 | 时间覆盖 | 物理编码 |
| --- | --- | --- | --- |
| `layout.yaml` | 可直接重放的规范化场景布局，首行为 `kind: layout` | 录像级 | UTF-8 YAML，LF 结尾 |
| `ticks.parquet` | DurableContext、录像元数据、每帧摘要 | `T(1)..T(n)` | Parquet + Zstd level 6 |
| `units.parquet` | 存活 FightMech 完整状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `projectiles.parquet` | ProjectileSystem 中的弹体完整状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `buildings.parquet` | 各 FightTeam 当前存活的 Crystal/Construction 状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `shields.parquet` | AdvancedEnergyShieldSystem 中仍存在的战场护盾状态 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `terrains.parquet` | RangeItemSystem 中当前存在的动态战场地形及单位作用关系 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `statistics.parquet` | 本场战斗到目前为止的原生伤害与击杀计数 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `formations.parquet` | 每个编队的经验 | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `events.parquet` | 相邻快照之间的有序离散事件 | `E(1)..E(n)` | Parquet + Zstd level 6 |

从 `units.parquet` 到 `events.parquet` 的八张表只在有行时才写入，缺失的表按空表读取。成员都不内嵌 Arrow schema；`ticks.parquet` 的元数据是 Parquet 文件的 key/value 元数据；列统计只按 column chunk 记录，不写页级统计和页索引。

ZIP 层采用 STORE，数据压缩由 Parquet page 的 Zstd 完成。逐 tick 的表 row group 按 1024 个逻辑 tick 刷新，单个 row group 的行数上限为 1,000,000。状态表按 `(tick, object_id)` 排序，事件按 `(tick, ordinal)` 排序。

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
`constructions`。`mechcore verify/format` 和 Simulator 在执行前应用完整布局合法性校验。该成员用于自包含重放和
`mechcore verify`，不进入哈希。

逻辑时间线为：

```text
T(t)     = { S(t), E(t), tick_hash(t) }, 1 <= t <= n
S(1)     = 第一次原生逻辑更新完成后的状态
```

所有状态、事件和逐 tick 摘要都从 tick 1 开始；部署完成后的 `S(0)` 不落盘。`tick_count` 至少为 1，`terminal_tick` 等于 `tick_count`。

---

# Part I — `ticks.parquet` 与场景上下文

## 1.1 作用

`ticks.parquet` 是容器索引。它保存格式标识、DurableContext、result hash 以及从 `T(1)` 开始的逐帧哈希。

## 1.2 Parquet schema

```text
tick       : UINT32 required
tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
```

`tick` 的合法序列精确为 `1..=tick_count`。`tick_hash` 覆盖同 tick 的完整 `S(t)` 和 `E(t)`，`result_hash` 按顺序覆盖全部 `tick_hash`；精确定义见附录 C。

## 1.3 文件元数据

Parquet key-value metadata 的 key 和 value 均为 UTF-8 字符串。

| key | 数据规范 | 含义 |
| --- | --- | --- |
| `format` | 精确值 `0.18.0` | MCFR 逻辑与物理契约版本 |
| `producer` | `game` 或 `simulator` | 录像由谁写出：经 Adapter 的游戏，或模拟器 |
| `game_build` | 非空 UTF-8 | 采集构建 provenance；Adapter 来自 `UnityEngine.Application.get_version()` |
| `durable_context` | canonical JSON | 单回合保持稳定的上下文 `D` |
| `hash_profile` | 精确值 `mcfr-content-0.7.0` | 哈希定义，以其 domain 字符串的版本命名 |
| `result_hash` | 64 位小写十六进制 | 全部 `tick_hash` 的有序摘要；回归判断依据 |
| `tick_count` | `u32` 规范十进制 | 从 `S(1)` 开始记录的逻辑 tick 数 |
| `terminal_tick` | `u32` 规范十进制 | 已确认的最终逻辑边界；当前连续时间线中等于 `tick_count` |

## 1.4 DurableContext 字段

| 字段 | 类型与数据规范 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `logic_step` | `Rational<u32>`，分子分母均大于 0 | 每次逻辑推进的秒数 | 当前 Adapter 固定为 `1/20` |
| `time_units_per_second` | `u32 > 0` | 原生离散时间单位密度 | 游戏固定为 `2000` |
| `combat_round` | `u32 > 0` | 当前战斗回合 | `CurrentMatch.get_RoundCount()` |

`match_seed` 不写入 `ticks.parquet`。Writer 仍用调用方提供的 seed 校验嵌入布局，Reader 则从规范化 `layout.yaml.seed` 恢复公开 `DurableContext.match_seed`。`game_build` 作为独立文件 metadata 描述采集来源。

`producer` 是录像关于其来源的唯一说明：Adapter 采集的为 `game`，模拟器写出的为 `simulator`。两者对同一场战斗写出相同时间线，因此它与 `game_build` 一样不进哈希，同一场战斗的两种来源录像共享全部哈希。读者据此区分游戏行为的证据与模拟器的计算结果。

---

# Part II — `units.parquet`

## 2.1 行集合与采集边界

每一行表示一个 `LiveUnitState`。Adapter 逐队遍历 `FightTeam.GetMeches()`，选择 `FightMech.IsAlive() == true` 的对象。每个 tick 是完整快照；单位退出存活集合后由 `unit_died` 事件保留离散死亡事实。

## 2.2 顶层 schema 与字段来源

| 字段 | Parquet 类型 | 含义 | Adapter 原生来源 |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | 状态所属逻辑时刻 | Adapter 逻辑帧计数 |
| `unit_id` | `UINT64 required` | Unit namespace 内稳定 ID | format 0.18.0 身份规则，见附录 B |
| `team_id` | `UINT32 required` | 当前所属队伍 | `FightTeam` controller index |
| `original_team_id` | `UINT32 required` | 首次出现时的队伍 | 首次采样的 `team_id` |
| `formation_id` | `UINT64 required` | 编队身份 | `FightMech.GetMechTeam()` 指针映射 |
| `unit_type_id` | `UINT32 required` | 原生单位类型 | `FightMech.GetMechID()` |
| `domain` | `UINT8 required` | 地面或空中 | `FightMech.IsFly()` |
| `position` | `QVec3 required` | 世界坐标 | FightTransform `GetPositionInt3D()` |
| `body_rotation` | `INT64 required` | 机体朝向，Q32.32 raw | FightTransform `GetRotationInt()` |
| `turret_rotation` | `INT64 nullable` | 炮塔朝向，Q32.32 raw；没有身体的单位为 null | `FightMech.mechBody` 的 FightTransform `GetRotationInt()` |
| `velocity` | `QPlanar required` | 地面上的当前速度，世界 `x` 与 `z` | MotionController 原生速度；本版本垂直分量恒为 0，采集遇到非 0 即拒绝 |
| `motion_state` | `UINT8 required` | idle/moving/attacking/stopped，见 2.8 | 原生运动状态机映射 |
| `mech_lock_target` | nullable `ObjectRef` | 单位**本体**指向的对象，见 2.8 | `FightMech.lockTarget` |
| `collision_radius` | `INT64 required` | 碰撞半径，Q32.32 raw | `FightMech.GetRadius()` |
| `life` | `GaugeI32 required` | 当前/最大生命 | `GetLife()` / `GetMaxLife()` |
| `active` | `BOOLEAN required` | 当前激活状态 | `get_IsActive()` |
| `targetable` | `BOOLEAN required` | 当前目标合法性 | `IsValidTarget(visibility=0)` |
| `visibility` | `UINT8 required` | 原生可见性状态 | `GetVisibility()` |
| `status_mask` | `UINT64 required` | 四个原生布尔状态 | 见 2.3 |
| `modifiers` | required sparse list | 单位、其技能与其 buff 上的全部非零修正 | 见 2.4 |
| `personal_shield` | required struct | 单位个人能量盾状态 | 见 2.5 |
| `move_speed` | `INT64 required` | 战斗移动单位所用的速度，所有修正之后，Q32.32 原始值 | `FightMech.GetMoveSpeed()` |
| `skills` | required list | 单位持有的每个技能、其状态与武器 | 见 2.6 |

## 2.3 `status_mask`

`status_mask` 保存当前采样边界的四个原生 bit。合法 mask 为 `0x0..=0xF`。

| bit | 名称 | 原生来源 | 规范字段语义 |
| ---: | --- | --- | --- |
| 0 | `invincible` | `BuffManager.IsInvincible()` | 记录原生 `IsInvincible()` 在采样边界的布尔值 |
| 1 | `frozen` | `BuffManager.IsFreeze()` | 记录原生 `IsFreeze()` 在采样边界的布尔值 |
| 2 | `technology_disabled` | `FightMech.IsTechnologyDisabled()` | 单位科技效果禁用 |
| 3 | `recovery_disabled` | `FightMech.IsRecoverDisabled()` | 单位恢复能力禁用 |

这四个 bit 表达 BuffDataInt/单位原生布尔状态的当前值。`invincible` 和 `frozen` 的名称沿用原生 predicate；战斗效果解释采用 build-bound 原生分支证据与受控场景观察。当前研究假设将 `invincible` 指向电磁干扰免疫，`frozen` 的效果映射继续通过运动、攻击、索敌和技能执行观察收敛。数值 Buff 的当前综合效果由 `modifiers` 的 `buff` 通道表达。

## 2.4 `modifiers`

```text
channel    : UINT8 required    (0=buff, 1=mech_float, 2=mech_float_rate, 3=mech_int,
                                4=skill_float, 5=skill_float_rate, 6=skill_int)
skill_slot : UINT16 nullable   技能在 GetSkills() 中的下标，仅技能通道有
field      : UTF8 required     原生枚举成员名的 snake_case，拼写照原生
part       : UINT8 required    (0=value, 1=add, 2=reduce)
value      : INT64 required    float/rate 为 Q32.32 raw，int 为整数本身
```

一个单位的修正是一张稀疏列表，只记原生为非零的项，按 `(channel, skill_slot, field, part)` 严格升序。没有内容设置的字段不占任何空间，原生新增的成员不改格式即可记录。

六个 `DataSet` 通道以原生枚举为键，Adapter 启动时枚举每个枚举的全部成员并逐一读取：`mech_float`（`MechDataChangeFloat`，`GetDataFloat`）、`mech_float_rate`（`MechDataChangeFloatRate`，`GetDataFloatAddRate/ReduceRate`）、`mech_int`（`MechDataChangeInt`，`GetDataInt`）、`skill_float`（`SkillDataChangeFloat`，`FightSkill.GetData`）、`skill_float_rate`（`SkillDataChangeFloatRate`）、`skill_int`（`SkillDataChangeInt`）。成员名按原生拼写转 snake_case：`CBLifeRecoveryRate` 为 `cb_life_recovery_rate`，`DamageChagneRateGround` 保留原生拼写错误。技能通道对 `GetSkills()` 返回的每个技能读取。

rate 分两部分：`add` 为增益之和，`reduce` 为减益扣掉的部分；原生 reduce getter 返回剩余倍率，MCFR 存 `reduce = 1.0_q32_32 - native_reduce_factor`。两者都非负。float 与 int 为一个有符号 `value`。

`buff` 通道为 `FightMech.GetBuffManager()` 上对全部生效 buff 的综合 getter，与 `DataSet` 通道分开保存，效果来自哪一处存储因此可以区分。字段：rate 类 `move_speed_rate`、`damage_rate`、`attack_interval_rate`、`extra_attack_interval_rate`、`amplify_damage_rate`、`attack_range_rate`、`extra_attack_range_rate`（add、reduce）；value 类 `move_speed_value`、`attack_range_add_value`、`attack_range_reduce_value`、`extra_attack_range_add_value`、`extra_attack_range_reduce_value`，均为原生有符号值（录像中出现过 `GetAttackRangeReduceValue` 为 -20）。`amplify_damage_rate` 表示作用于该单位的承伤倍率修正。Buff 的应用、持续与消退通过连续快照中 `status_mask` 与 `buff` 通道的变化观察。

## 2.5 `personal_shield`

```text
personal_shield = {
  active  : BOOLEAN required,
  enabled : BOOLEAN required,
  energy  : GaugeI32 required
}
```

Adapter 通过 `GetEnergyShieldController()` 读取 `IsActive()`、`IsEnable()`、`GetEnergy()` 和 `GetMaxEnergy()`。

## 2.6 `skills`

```text
skill_slot : UINT16 required   技能在 FightMech.GetSkills() 中的下标
enabled    : nullable struct   FightSkill.IsEnable() 为 false 或单位旅行中时为 null
  lock_target             : ObjectRef nullable   FightSkill.lockTarget
  attack_target           : ObjectRef nullable   FightSkill.GetAttackTarget()
  state                   : UINT8 required       SkillStateController 状态
  attack_phase            : UINT8 nullable       SkillAttackController 阶段
  attack_time             : INT32 required       FightSkill.attackTime
  current_attack_interval : INT32 required       FightSkill.GetCurrentAttackInterval()
  attack_count            : INT32 required       FightSkill.GetAttackCount()
  attack_range            : INT64 required       FightSkill.GetAttackRange()，Q32.32 原始值
  attack_damage           : INT32 required       FightSkill.GetNormalDamage(0)
  weapons                 : required list
    weapon_index : INT32 required    WeaponData.get_Index()
    position     : QVec3 nullable    武器 FightTransform 的 GetPositionInt3D()
    rotation     : INT64 nullable    武器 FightTransform 的 GetRotationInt()
```

列表包含 `GetSkills()` 返回的每个 `FightSkill`，按 `skill_slot` 严格升序。分组单位的技能就是其分组的各槽：恶灵四个，带浮游炮阵时八个。额外武器的技能排在主技能之后，分组行的每个槽各占一个：带电磁弹幕与能量散射的熔点，0 号槽是主光束，1 号是弹幕，2 到 5 号是四道散射光束。

**被关闭的技能只保留槽位。** 禁用科技的 buff 会关掉其科技加上的额外武器技能（`ExtraSkillProvider.DisableSkill`、`FightSkill.Disable`），在此期间该技能的 `enabled` 为 null，其间它持有的一切都不进哈希。build 在技能关闭期间照常推进它的计时，重新开启后从计时所在处接着走，所以关闭期间漂移的计时会在它回来的那个 tick 暴露出来。

**旅行中单位的技能不读取。** `FightMech.IsSuperDeployment` 成立期间，每个技能的 `enabled` 都为 null，无论技能开着与否：旅行中的单位不更新（`FightCoreSystem.TeamUpdate`），它的技能持有的一切都不改变它做什么，全部从抵达那个 tick 起显现。额外武器在旅行期间开着与否取决于对局放置单位与研究科技的先后，任何 layout 都不陈述它（[`super_deployment.md`](../../rules/super_deployment.md)），否则同一场仗只因此不同的两份录像就会哈希不同。

`state` 是 `SkillStateController` 当前状态的类：`0=idle`（`SkillIdleState`）、`1=prepare`、`2=attack`、`3=cooling`、`4=reloading`、`5=lock`（`SkillLockState`）。`attack_phase` 是 `SkillAttackController` 当前的阶段控制器：`0=before` 等待攻击点，`1=attacking` 正在释放，`2=after` 后摇；没有出手进行中时为 null，攻击状态的大部分时间都是如此：两次出手之间，以及在一次更新里开始又结束的出手。

`attack_time` 数自上次出手开始以来的逻辑 tick，到达 `current_attack_interval` 时技能出手。**这个间隔不是描述里的那个：** 每个周期都会从队伍随机流里抽一次错开，三只长弓在第 1 tick 读作 55、65、56（描述是 62），[`combat.md`](../../rules/combat.md) 量了这次抽取。间隔记的是 build 自己的整数，不是 property 的 `FPoint` 秒。`attack_count` 是进入攻击状态以来开始的出手数减一，攻击状态之外为 `-1`。

`attack_range` 与 `attack_damage` 是技能自身 property 在所有修正之后的答案，与写入它的 `modifiers` 并列：一份录像在一个 tick 里就能回答一条修正如何合成。

`weapons` 按 `weapon_index` 严格升序。武器缺少 FightTransform 时，position 与 rotation 同时为 null。

## 2.8 单位指向什么

三个字段回答关于单位意图的三个不同问题，把其中一个当成另一个，就会读错这场仗。

| 字段 | 回答 | 归属 |
| --- | --- | --- |
| `mech_lock_target` | 单位**本体**指向什么 | 机甲 |
| `skills[].enabled.attack_target` | 每个**技能的武器**朝什么开火 | 该技能 |
| `motion_state` | 本体是在行进、停下攻击、空闲还是停止 | 机甲的运动状态机 |

**`mech_lock_target` 是本体的目标。** 它是机甲自己的搜索找到的对象；成组技能则是它
最近一次分配给某个武器位的目标。`moving` 时单位朝它走，有身体的单位在 `attacking`
时身体也一直朝着它。武器位先被分配一个目标，再朝挡在这个目标前面的东西开火，所以挡
在射线上的对象永远不会出现在锁定里。它不说明正在打什么；机甲没
有目标时为空。

**`attack_target` 是武器的目标。** 它是拥有该通道的技能让这个通道朝之开火的对象，
可以和 `mech_lock_target` 不同：技能会把挡在射线上的对象交给武器，而机甲保持自己的
锁定；同一个成组技能的各个通道也可以各持不同的目标。没有身体的单位除了武器之外没有
自己的朝向，所以它的 `body_rotation` 跟着攻击目标，而不是锁定。

**`turret_rotation` 是有身体的单位瞄准的方向。** 这种单位在底盘上带着炮塔
`FightMech.mechBody`：`body_rotation` 停在底盘朝向，`turret_rotation` 按单位的转速转向
攻击目标，攻击角从它量起。一台停在 0.68° 的 Fortress，等炮塔转到离 32° 外的 Crawler
不足 20° 时开火。没有身体的单位没有炮塔，该字段为 null。

**`motion_state` 跟着攻击目标，不跟锁定。** `attacking` 表示某个武器的攻击目标在射
程之内、本体为它停了下来；`moving` 是本体唯一会行进的状态，行进方向是
`mech_lock_target`。所以只要前方有能打的东西在射程内，单位就可以在锁定目标够不着的
情况下处于 `attacking`。

采集按原生对象的报告原样记录这三项，不由其中一个推出另一个，这样读者才能拿它们互
相比较。哈希读这三项，所以单位移动和命中都一致、却在这三项上不同的两场仗，是两场
不同的仗。

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
| `target` | nullable `ObjectRef` | 当前目标 | `FightProjectile.GetTarget()` |
| `cached_target_position` | `QVec3 required` | 弹体缓存的目标坐标 | `GetTargetInfo().GetPosition()` |
| `cached_target_radius` | `INT64 required` | 弹体缓存的目标半径 | `GetTargetInfo().GetRadius()` |
| `life` | `GaugeI32 required` | 弹体当前/最大生命 | `GetLife()` / `GetMaxLife()` |
| `spawn_containing_shields` | required `list<ObjectRef>` | 弹体自身命中检测会看的、创建时已包含它的盾，按 Shield ID 排序 | `ProjectileController.inEnergyShields` |

`owner` 和 `target` 可引用已经退出当前快照的历史对象。引用身份在整份录像内保持稳定。

活跃弹体在这里没有朝向和释放标记：它从不旋转（`GetRotationInt()` 恒为 0），`isReleased` 只在回收进对象池时由重置路径置位，活跃弹体读到的总是 false；采集遇到其它值即拒绝。发射事实由 `projectile_released` event 表达。

`spawn_containing_shields` 是弹体的出生豁免：它出生时就在其中、因此永远不会命中的那些盾。`ProjectileController.Init` 在弹体有了炮口位置之后用 `GetInEnergyShields` 填一次：按弹体 `GetEffectTargetType()` 所指的阵营（`Self` 为本队，`Opponent` 为对手，`Both` 为全部）取出半径包含该位置的盾。命中检测 `IsProjectileHitEnergyShield` 重新求出此刻包含弹体的盾，取第一个不在列表里的；弹体回收进对象池之前没有任何地方修改这个列表，所以豁免持续弹体的整个寿命，而不是离开盾之后就结束。因此只有从自身命中检测会看的盾内发射时列表才非空，对伤害弹体而言就是射手站在敌方护盾里；几乎每一行都为空。列表元素必须是 `ObjectKind::Shield`。

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

`source_kind` 的四个值分别对应 `EnergyShieldContraption`、`CS_EnergyShield`、`AdvancedEnergyShieldController` 和 `SpawnAdvancedShieldController`。未知数据源使 Adapter fail-close。

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

# Part VII — `statistics.parquet`

每个快照一份 `BattleStatisticManager` 当前回合的计数：每个记录者一行，按 `(tick, team_id, recorder, recorder_id)` 严格升序。`FightController.OnActorHitted` 在逻辑 tick 内更新它，所以它是战斗状态，尽管战斗里没有任何地方回读它。

| 列 | Parquet 类型 | 含义 |
| --- | --- | --- |
| `tick` | `UINT32` required | 快照 |
| `team_id` | `UINT32` required | 持有该项的队伍字典；单独计数的单位为它此刻效力的一方 |
| `recorder` | `UINT8` required | `0=formation`（`MechTeam`）、`1=construction`（`FightConstructionCombination`，或不属于组合的单个建造物）、`2=unit`（没有 `MechTeam` 的单位：召唤物，或被精神控制的单位） |
| `recorder_id` | `UINT64` required | 编队的 `formation_id`；建造物组合中最小的 `building_id`；单位的 `unit_id` |
| `damage` | `INT32` required | `DamageMax`：每一下伤害经过全部减免之后、按目标剩余生命截断之前的值之和 |
| `damage_real` | `INT32` required | `DamageReal`：这些伤害实际扣掉的生命或个人护盾能量 |
| `kills` | `INT32` required | `KillCount`：打完之后目标不再存活的次数 |
| `damage_taken` | `INT32` required | `DamageTaken`：记录者成员受到的伤害，只计入增伤、未经任何减伤 |

一下伤害记给 `HitDamageInfo.sourceSkillOwner` 的记录者，并记入目标的记录者。单位的记录者是它的编队；建造物的是它的组合；防御塔不是记录者，打它的一方照样记分，它自己不记承伤。没有技能拥有者的伤害（地面火、buff 持续伤害、指挥官技能、空投）不记给任何人。已经死亡的目标在计数之前就被跳过。每个编队和建造物组合从部署起就有一行，初值为零，编队全灭后仍保留。召唤物没有编队，从它计入的第一下伤害起单独计数；被精神控制的单位也单独计数，在回合的临时字典里，记在它此刻效力的一方。计数是原生的 `int`，每场战斗从零开始。

## 7.1 `formations.parquet`

每个快照一份每个编队的经验：录像编过号的每个 `MechTeam` 一行，存活与否都在，按 `(tick, formation_id)` 严格升序。列为 `tick`、`formation_id`、`team_id`（首个单位的原始队伍）、`experience`（`MechTeam.expFloat`，FPoint raw，首次获得经验之前为 -1.0）、`max_experience`（`MechTeam.maxExpFloat`，FPoint raw，经验到此为止）。`ExpSystem` 在逻辑 tick 内、击杀当时分配经验，所以它是战斗状态；一次击杀给出多少、给谁，见 [`docs/rules/unit_experience.md`](../../rules/unit_experience.md#what-a-kill-hands-out)。战斗结束时 `BattleSystem.OnFightOver` 把每个编队的经验截为整数，最后一个快照已经是截断后的值。

# Part VIII — `events.parquet`

## 8.1 表与排序规范

`events.parquet` 每个事件一行。同一 tick 的 `ordinal` 从 0 连续递增。全表按 `(tick, ordinal)` 严格升序，tick 合法范围为 `1..=tick_count`。

## 8.2 公共列

| 列 | Parquet 类型 | 含义 |
| --- | --- | --- |
| `tick` | `UINT32` required | 事件归属的推进时刻 |
| `ordinal` | `UINT32` required | 同 tick 内的观测顺序 |
| `type` | `UINT8` required | 事件种类 tag，按下表行序从 0 编号 |
| `object` | `ObjectRef` nullable | 事件主体 |
| `source` | `ObjectRef` nullable | 直接来源对象 |
| `source_team_id` | `UINT32` nullable | 事件来源或归属对象在事件边界的队伍快照 |
| `target` | `ObjectRef` nullable | 直接目标对象 |

下表每个 payload 字段各有一列可空列，类型与字段相同；一行只设置其事件类型携带的字段，其余 payload 列为 null。Reader 拒绝设置了本类型不携带字段、或缺少必需字段的行。列名与类型以 [English](mcfr.md#events) 为准。

## 8.3 事件种类与 payload

| `type` | 必要引用 | payload 字段 | 语义 |
| --- | --- | --- | --- |
| `projectile_released` | `object` | `skill_slot: u16|null`, `weapon_index: i32|null` | 弹体创建并加入 ProjectileSystem 的发射事实及其武器通道；两个通道字段同时有值或同时为 null |
| `projectile_removed` | `object` | `position: QVec3`, `intercepted: bool`, `absorbed_by: ObjectRef|null` | 弹体从系统移除；`absorbed_by` 指明直接吸收它的战场盾 |
| `damage` | `target`；有弹体时 `object` 为该弹体 | `amount: i32`, `skill_slot: u16\|null` | 单个实际 target 的一次正数伤害结果；Actor 使用原生 `damageReal`。`skill_slot` 为造成伤害的技能在 source 的 `GetSkills()` 中的下标，没有技能时为 null |
| `unit_created` | `object` | `team_id: u32`, `formation_id: u64`, `unit_type_id: u32`, `position: QVec3` | Unit 生命周期起点 |
| `unit_died` | `object` | `position: QVec3` | Unit 死亡边界 |
| `building_destroyed` | `object` | `position: QVec3` | Building 摧毁边界 |
| `unit_team_changed` | `object` | `previous_team_id: u32`, `new_team_id: u32` | Unit 当前阵营变化 |
| `shield_created` | `object` | `team_id: u32`, `source_kind: ShieldSourceKind`, `position: QVec3` | Shield 加入全量集合并取得身份 |
| `shield_destroyed` | `object` | `position: QVec3`, `reason: ShieldDestroyedReason` | Shield 从全量集合移除，生命周期结束 |
| `terrain_created` | `object`；省略 `source` | `team_id: u32|null`, `terrain_type: TerrainType`, `position: QVec3`, `radius: i64` | Terrain 加入 controller item 集合并取得身份 |
| `terrain_removed` | `object` | `position: QVec3`, `reason: TerrainRemovedReason` | Terrain 退出 controller item 集合，生命周期结束 |
| `terrain_converted` | `object` | `position: QVec3` | 同一 Terrain 发生原生类型/归属转换 |
| `healing` | `target` | `amount: i32` | 一次正数恢复结果 |
| `buff_applied` | `target`；`source` 是施加者（若有），`source_team_id` 是其队伍 | `buff_id: u32`, `duration: i32` | 目标被施加或再次施加一个 buff；`buff_id` 是其数据的 `GetID()`，`duration` 是施加后剩余的 tick 数 |
| `buff_removed` | `target` | `buff_id: u32`, `reason: BuffRemovedReason` | 目标上的一个 buff 被移除 |

`projectile_removed` 的合法原因组合为：原生拦截系统移除使用 `intercepted=true, absorbed_by=null`；战场盾吸收使用 `intercepted=false, absorbed_by=ShieldRef`；其他移除使用两者均为空/false。`absorbed_by` 只能引用 Shield。

`shield_destroyed.reason` 使用 `energy_depleted`、`owner_destroyed`、`round_end`、`scripted` 或 `unknown`。Producer 只在原生销毁入口能确定原因时写具体值，Adapter 怎么读见 8.4。

`terrain_removed.reason` 使用 `time_expired`、`round_expired`、`grid_depleted`、`cleared` 或 `unknown`，同一规则。

`buff_removed.reason` 使用 `expired`、`removed`（技能或科技按数据移除）、`team_changed`、`technology_disabled`、`cleared`（其单位被拆除，单位死亡即是）或 `unknown`。

`terrain_created` 表达 controller 集合成员的生命周期起点，其 JSON object 由 Terrain 身份、队伍、类型、位置和半径完整确定。投射物因果关系通过同 tick、同位置的 `projectile_removed` 与 Terrain 状态变化联合观察。

## 8.4 当前 Adapter 原生事件来源

共享 MCFR model 与事件表实现上表事件。Adapter 当前事件 producer 覆盖下列原生观测：

| 事件 | 当前原生来源 |
| --- | --- |
| `projectile_released` | `ProjectileSystem.Create` 到 `AddProjectile` 的嵌套 trace；同时解析 owner、target、skill slot、weapon index |
| `projectile_removed` | ProjectileSystem 销毁路径 trace，采集末位置与 intercepted；同一伤害链命中 Shield 时补充 `absorbed_by` |
| `damage` | `DamagePerformer.Perform` 提供 provider 归因作用域；每次 `FightController.OnActorHitted(HitDamageInfo)` 提供 Actor target 与 `damageReal`；战场盾伤害 helper 提供逐 Shield 实际结果。provider 给出造成伤害的技能：`SkillDamageProvider` 或 `HitEffectControl` 的 `fightSkill`，或 `FightProjectile` 的 `dataSource`；`GetSkills()` 中没有的技能按其 `ParentSkill` 记。死亡爆炸、击杀爆炸、指挥官技能、空投、地面火与 buff 持续伤害没有技能 |
| `unit_created` | `FightController.CreateMech(team, mech, position, rotation, createType, mechTeam, isRebirth)`，部署、召唤、死亡生成、生产与空投都经过这个入口。只记逻辑 tick 内的调用，部署因此不记；单位死后复活会带 `isRebirth` 再经过一次，也不记。位置取调用返回后单位的位置 |
| `unit_died` | `FightMech.OnDead` trace |
| `building_destroyed` | `FightCrystal.OnDead` trace |
| `healing` | `FightActor.AddLife(value, isShowLifeBar)`（单位、塔与水晶都经它回血）与 `FightConstruction.AddLife`。量是调用后读到的生命减去调用前的：满血截断，调用本身不返回实际加了多少。回血都显示血条；不显示的两处回满——单位死后复活、超级部署落地——不是回血，不记。没有 `source`，调用本身不带 |
| `shield_created` | 相邻采样边界间首次进入 `GetEnergyShields(fightGroup)` 全量集合。同一对边界间加入的多个护盾按身份顺序写，即快照读它们的顺序：逐队读，每队按其集合自身的顺序，不按原生地址 |
| `shield_destroyed` | 相邻采样边界间从全量集合消失。原因在 `GroupAdvancedEnergyShieldManager.Destroy(FightEnergyShield)`（唯一把护盾移出集合的方法）开始时读：能量耗尽为 `energy_depleted`，只有伤害会在销毁前清空能量；有 owner 为 `owner_destroyed`；其余为 `scripted`。回合结束时的销毁在最后一个记录的 tick 之后，不写 `round_end`。同一对边界间离开的多个护盾按身份顺序写 |
| `terrain_created` | 相邻采样边界间首次进入所属 `RangeItemController.GetItems()` 集合 |
| `terrain_removed` | 相邻采样边界间从所属 controller item 集合消失。原因在 `RangeItem.Remove()` 开始时读：处在 `RangeItemController.OnExitFight` 内为 `round_expired`，火焰也是，其控制器不看回合数、随战斗结束移除；否则 `IsTimeOver()` 为 `time_expired`，网格全部清空为 `grid_depleted`。油被点成火、被技能清除为 `unknown`，不写 `cleared` |

`unit_team_changed` 与 `terrain_converted` 具备共享 schema、canonical 表达和 Reader/Writer 支持，供能够直接观测对应演化来源的 Producer 写入。

`unit_created` 与 `healing` 的记录没有夹具覆盖：钉住的对局里没有 tick 内生成的单位，也没有回血；哪些召唤、哪些回血经过这两个入口，在研究对应机制时逐一核对。

buff 经 `BuffManager` 的三个方法进出，Adapter 各挂一个钩子。`buff_applied` 来自 `AddBuff(Buff)`（新 buff 经 `Buff.Init` 填好后加入列表的私有方法）和 `Buff.Reset`（同一 buff 再次施加时走它，不再加第二个）；事件写正在运行的那个 buff 的行号，合并不改变它。`buff_removed` 来自 `RemoveBuff(Buff)`，在它把 buff 还回对象池之前读。原因取它所在的调用：`RemoveBuff(IBuffData)`、`RemoveBuffEffect`、`ClearSelfResourceBuffByDisableTech` 或 `Clear`，否则是 `BuffManager.Update` 发现时间到了。战斗开始前就有的 buff 没有事件。

buff 的作用仍在 Unit 状态轨道上：布尔状态进入 `status_mask`，综合数值修正进入 `modifiers` 的 `buff` 通道。钉住的对局只覆盖塔被摧毁写下的 buff；其他 buff 是否经过这些方法、每个移除原因是否就是 build 的本意，在研究对应机制时逐一核对。

---

# Part IX — 写入、读取与验证

## 9.1 Writer 生命周期

Writer 的公开生命周期为：

```text
create(producer, game_build, context, layout_yaml)
append_tick(S(1), E(1))
...
append_tick(S(n), E(n))
finish()
```

Writer 在目标同目录创建临时成员和 `.zip.part`，完成 Parquet footer 和 ZIP 封装后，以 `McfrReader` 重新打开并复核结构及持久化哈希元数据，最后通过 `persist_noclobber` 发布目标文件。目标路径在创建时保持空闲，发布具有防覆盖语义。

`in_memory(producer, game_build, context, layout_yaml)` 做同样的检查，把每个规范化后的 tick 留在内存里而不写入存储，`finish_in_memory()` 交回这条时间线和它的哈希，不产生文件。读一场战斗的读取器通过 `Recording` 读它，`McfrReader` 和这条时间线都实现它：内嵌 layout、producer、哈希、终止 tick，以及每个 tick 的状态和事件。读取器是模拟器一场战斗的唯一用途时就这样保留它，`verify` 和 `convert --to fight` 读 layout 的战斗即如此。

每次写入前，WorldSnapshot 先按规范顺序 canonicalize，再执行对象 ID、列表顺序、状态 bit 和 modifier 约束校验。一次局部写入失败会使 Writer 进入 poisoned 状态，完整文件只由成功的 `finish()` 发布。

## 9.2 Reader 验证

Reader 在打开容器时验证：

- ZIP 成员集合、STORE method、ZIP64 可读性与成员唯一性；
- `layout.yaml` 的 UTF-8、共享结构、规范化表示以及 round 与 DurableContext 一致性；
- 六个 Parquet schema、required/nullable 结构及 Zstd column compression；
- `producer`、`game_build`、DurableContext canonical JSON、元数据类型和格式标识；
- tick 连续性、状态表排序、事件排序与 ordinal 连续性；
- ObjectRef、enum tag、初始身份顺序、列表顺序、`status_mask` 保留位和 modifier 分量；
- tick hash 列的连续性、宽度和编码，result hash 及其 profile 的编码，以及 result hash 是 tick hash 列的摘要。

Reader 信任 MCFR 自身持久化的 tick hash。`McfrReader::open()` 不会为了校验哈希而逐 tick 重建 `S(t)`/`E(t)`，也不重新计算整条时间线；`first_divergence()` 直接比较持久化的 `tick_hash`。完整读取一个 tick 的 `tick(t)` 会重算它的状态和事件的哈希，存储的 `tick_hash` 对不上就拒绝，所以定位差异后读到的内容是哈希担保过的。

合法录像至少具有一个 `T(1)`，不存在 tick 0 状态或事件，并在 `terminal_tick` 处完整结束。

---

# 附录 A — 公共数据类型与 enum tag

## A.1 Q32.32

空间、旋转以及原生定点 modifier 使用有符号 `i64` raw bits：

```text
real_value = raw / 2^32
```

`QVec3 = { x: i64, y: i64, z: i64 }`。Parquet 使用三个 required `INT64` 子字段；`QPlanar = { x: i64, z: i64 }` 同理，位于地面。position、radius 和 bounds 的量纲为米，velocity 为米/秒，rotation 为度。

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

## B.1 format 0.18.0 身份规则

format `0.18.0` 采用 `team_zx_sequential_v1`。初始 Unit 按 `(team_id, position.z, position.x)` 严格升序排列，再依次分配 `unit_id = 1..N`。同一队伍的初始单位具有唯一 `(z, x)`，因此 Adapter 与 Simulator 可从相同场景构造相同编号。

战斗期间首次出现的 Unit 按首次观察顺序取得当前 Unit namespace 的下一个连续编号。Unit namespace 从 1 开始单调递增；历史引用持续使用对象首次取得的编号。

`formation_id` 由上述初始 Unit 顺序首次遇到的 formation 依次分配。因此初始
`formation_id` 的首现顺序必须严格为 `1..F`；它表达成员按世界 `z/x` 排序后的首现编号，
不等同于布局 formation 声明索引。布局声明顺序保留原生 Unit index，因为 formation
成员散布使用 `match_seed + unit_index`，其中 `unit_index` 是布局的 `index`
而不是声明位置：卖掉的单位会在其中留下空缺。动态 Unit、Projectile 和 Building 在各自 namespace
中按首次观察顺序追加。ID 生命周期覆盖其退出快照后的历史引用。

初始 Building 按 `(team_id, building_type_id, position.x, position.y, position.z)`
严格升序分配 `building_id = 1..N`，位置使用 Q32.32 raw 整数；相同键的多个对象使
采集失败，不能用原生指针或创建计数器作为不稳定的 tie-breaker。一项 construction
可对应多段城墙，每段按自己的位置获得独立 ID。layout 不保存 construction index；
原生 building/construction index 和 layout 声明顺序都不参与 MCFR Building ID 分配。
两种游戏模式采用同一规则。分配后的 ID 及所有 ObjectRef 在整场保持稳定，动态对象
按首次观察时的同一排序追加；死亡或移位不会重新编号。

初始 Shield 在 S(1) 按队伍分组：活跃盾按 `active_order` 升序，同队 inactive 盾随后。首 tick 已移除但事件仍可能引用的盾排在全部 S(1) 对象之后，再按队伍分组，确保初始状态中的 ID 从 1 连续。inactive/已移除组内按 `(source_kind, owner, position.x/y/z, radius, round_policy, energy.maximum, energy.current)` 排序，已移除项使用最后观测状态；无法区分的相同键使采集失败。编号仅规范化一次，并同步转换 E(1) 与缓存引用，不能每 tick 用 active_order 重新编号。此后新盾按首次观察顺序追加，ID 不因失活、重激活或 active_order 变化而改变。初始 Terrain 按 `(terrain_type, native controller item index)` 分配，动态 Terrain 按首次观察顺序追加。Shield 与 Terrain 从各自权威集合移除后，原生指针进入 tombstone 并保持历史 ID 唯一。

状态快照最终统一按对象 ID 排序；`modifiers` 按 `(channel, skill_slot, field, part)`，`skills` 按 `skill_slot`、技能的 `weapons` 按 `weapon_index`，投射物 `spawn_containing_shields` 按 Shield ObjectRef 排序。

## B.2 状态与事件的同帧约定

`E(t)` 表示从 `S(t-1)` 推进到 `S(t)` 期间由 hook 直接观测的事件。`S(t)` 是该次推进结束后的权威完整状态。因而同一 tick 的死亡/摧毁事件可以与对象退出 `S(t)` 同时出现。

# 附录 C — 哈希规范

## C.1 公共编码

Hasher 为 BLAKE3。每个摘要先输入固定前缀和 domain；前缀、domain 以及后续每个输入段都编码为：

```text
LE_u64(byte_length) || bytes
```

固定前缀为 `mechcore.mcfr.canonical\0`。整数均按指明宽度使用小端补码原始位。所有公开摘要均编码为 64 位小写十六进制，Parquet tick 列保存原始 32 bytes。

## C.2 定义

完整状态和事件先编码为 canonical JSON：UTF-8、递归字典序排列 object key、紧凑编码和 schema 定义的数组顺序。哈希覆盖全部 `S(t)`/`E(t)` 字段；它不包含布局、DurableContext 或其他文件元数据。

```text
tick_hash(t) = H_content-tick-0.7.0(LE_u32(t), JSON(S(t)), JSON(E(t)))
result_hash  = H_content-result-0.7.0(
    LE_u32(tick_count),
    tick_hash(1)..tick_hash(n)
)
```

这个定义比格式老：它的 domain 字符串和公式自 format 0.7.0 起没有变过，`hash_profile` 的值 `mcfr-content-0.7.0` 就以这个版本命名。格式变了而 `S(t)`、`E(t)` 的编码不变，所有哈希就都不动；定义本身要变，就是新的 profile 和新的 domain 字符串，不能就地修改。

`mechcore diff` 与 `mechcore verify` 以这个哈希决定 `equal` 和首个分歧。`diff` 还会逐字段说明两份录像在哪里不同，这是哈希做不到的：字段组的定义见 [cli.md](../mechcore/cli.md#diff)。

## C.3 哈希的准入

哈希是模拟器的打分标准，它读哪些量是单独的决定。`crates/mcfr/hashed-content.txt` 列出 `S(t)` 和 `E(t)` 的每个字段、变体、枚举值和 `status_mask` 位，一个测试让类型与它保持一致：改哈希读的内容，就是改这个文件。

**哈希在分歧发生的那个 tick 报出它。** 模拟器把一个机制做错，先错在 build 保存的某个量上，之后才错、或者永远不错在一回合的结算上。哈希读的是战斗继续下去所依据的状态，所以走错的一步在走错的那个 tick 就显现，`first_divergence()` 指向它，而不是后果浮现的那个 tick。

**哈希读的内容只增不减。** 它比过的每一场仗都是按全部内容比的，删掉一个字段、放宽一个字段的含义，会削弱已经做过的每一次比较。三种改动不算删减：新旧编码互相决定的重新编码；修正 producer 读错的字段；在 build 既不更新也不读一个值的期间不写它，而 build 再用它时它持有的值会在那时记下，例如被关掉的技能的状态。

**一个量同时满足四条才能进入。**

1. **它属于战斗。** 它是以下之一：
   - **结算量**：进入一回合的结算，核心伤害、统计、经验、分数；
   - **原因**：指明时间线已记录的某个变化由什么造成，例如一次伤害、一个 buff 或一个新单位背后的技能、投射物或落地；
   - **跨 tick 的状态**：build 把它从一个 tick 带到下一个 tick，之后的 tick 取决于它，例如计时、计数、进度、状态机的状态。

   build 每 tick 重新算出的量，或者只说明一次决策如何做出的量，例如一次搜索比较过的候选、一次求解的各项，不属于战斗，是 [instrument 通道](mcfr.md#instrument-channels)。
2. **是游戏自己的值。** producer 在 build 的某个成员上读到它，和上面每个字段一样有名可指。由其它已记录的值算出的量不存。
3. **一场仗只有一个值，与谁来写无关。** 从回放录和从 layout 录得到同一个值，模拟器打同一场仗也写得出它。
4. **每 tick 的量有上界**，不超过实体数的常数倍。

**晚到的分歧是缺了准入。** 一个机制挪一 tick 或改一个数之后，首个分歧远晚于它起作用的 tick，或者只出现在结算里，说明带着这个错误的状态不在哈希里，补上它的办法是准入它。instrument 通道可以在此之前帮助定位，但补不上这个缺口。所以读 build 跨 tick 保存的成员的通道，要在 [Excluded fields](mcfr.md#excluded-fields) 写明该成员不满足哪一条。

**准入如何合入。** 改 `hashed-content.txt` 的 pull request 为它增删的每一行写明满足上面哪一条，或属于哪种不算删减的改动。它不需要别的签字，在游戏上重录钉子被它移动的每个 fixture，然后和其它 pull request 一样合入。

**准入会动哪些钉子。** 新的事件种类、枚举值或 `status_mask` 位只出现在发生它的仗里，只动这些仗的钉子。状态对象或事件的新字段、新的状态集合每个 tick 都写，没有时写 null 或空，会动全部钉子。已准入字段的新取值，例如 modifier 的 `field` 指向另一个原生字段，不是准入。

# 附录 D — 物理编码约定

- ZIP member 顺序固定为 `ticks`, `units`, `projectiles`, `buildings`, `shields`, `terrains`, `events`。
- ZIP member 使用 STORE 和固定时间戳，Parquet column chunk 使用 Zstd level 6。
- Parquet 全局 dictionary 关闭；team、类型、enum、固定半径/上限等低基数字段按列启用 dictionary。
- 各 Parquet 的 `tick` 列使用 `DELTA_BINARY_PACKED`。
- required list 使用空列表表达当前对象没有对应项；nullable struct 表达 `Option<T>`。
- modifier 数值叶为 nullable，null 的规范值为 0；Buff 和单位动态根 struct 全零时为 null，技能动态列表只保存非零 skill slot。
- 状态哈希使用展开后的零默认值；skill 全零项在 canonicalize 时移除，因此稀疏物理编码可重建相同的规范状态。
- 状态表每个 tick 保存完整当前集合，读取任一 tick 可直接重建 `S(t)`。
- 事件字段集合由事件 type 精确确定；Reader 逐行校验必要引用与字段集合。

# 附录 E — 原生 modifier 映射示例

粘油减速进入 BuffManager 的 `move_speed_rate` 综合值。光子投射产生的承伤变化进入 `amplify_damage_rate`，其 `IsInvincible()` 当前值进入 `status_mask.invincible`。剑齿虎科技副炮等子技能在 `modifiers` 和 `skills` 中使用各自 `skill_slot`；回合 `+15` 射程增益形成的技能级动态变化保留在对应 skill modifier 字段。

这些例子说明三个采集通道的归因边界：BuffManager 综合效果、FightMech 单位级动态修正、FightSkill 技能级动态修正分别持久化，原生字段归属保持可观察。
