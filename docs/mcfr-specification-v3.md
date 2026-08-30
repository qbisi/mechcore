# MCFR schema v3 字段规范

本文逐字段描述当前仓库实现的 MCFR schema version 3，以及它在 HDF5 container
version 2 中的物理投影。内容以以下实现为准：

- `crates/mcfr/src/model.rs`：逻辑数据模型；
- `crates/mcfr/src/writer.rs`、`reader.rs`、`canonical.rs`：写入、读取和哈希；
- `crates/mcfr/src/storage.rs`：HDF5 列布局；
- `crates/adapter/src/capture.rs`：原生游戏采集来源；
- `crates/simulation/src/kernel.rs`：当前 simulator 的字段生成方法。

本文描述的是已经实现的 v3，而不是 `plan.md` 中讨论的 v4 方向。正式 `.mcfr`
只包含 `D + S + E`；研究用 `.mcfr-i` instrumentation sidecar 不属于 schema v3
的正式状态或结果哈希，本文仅在末尾说明其边界。

## 1. 记号与范围说明

一份录像表示：

```text
MCFR = D + T(0)..T(n)
T(t) = S(t) + E(t)
E(0) = []
S(t-1) --E(t)--> S(t), t >= 1
```

- `D`：整场单回合战斗不变的 `DurableContext`。
- `S(t)`：第 `t` 个逻辑边界的完整 `WorldSnapshot`。
- `E(t)`：从 `S(t-1)` 推进到 `S(t)` 期间按发生顺序记录的事件。
- `t`：从零开始的数组位置，不作为独立状态列保存。

后续表格中的“范围”分三层理解：

1. **物理范围**：Rust/HDF5 整数类型能够表示的范围；
2. **格式校验**：当前 writer/reader 明确拒绝的值；
3. **producer 范围**：adapter 或 simulator 当前实际会生成的值。

若只写 `i64`、`u32` 等，表示 v3 没有增加更窄的语义约束。常见游戏值不是格式约束。

## 2. 公共数值约定

### 2.1 当前 producer 使用的尺度

| 量 | `D.numeric_convention` 字段 | 当前值 | 含义 |
| --- | --- | ---: | --- |
| 距离 | `distance_units_per_meter` | `1000` | 1000 个 MCFR 距离单位等于 1 米。 |
| 旋转 | `rotation_units_per_degree` | `1000` | 1000 个 MCFR 旋转单位等于 1 度；常用一圈为 `360000`。 |
| 时间 | `time_units_per_second` | `2000` | 2000 个原生时间单位等于 1 秒。 |
| 逻辑步 | `logic_step` | `1/20` 秒 | 每个 tick 为 50 ms，即 100 个时间单位。 |

schema 只要求三个 scale 以及 `logic_step` 分子、分母均为正数，并不强制上述固定值，
也不要求分数已经约分。

### 2.2 原生 Q32.32 转换

adapter 对位置、速度、半径和旋转使用同一个有符号定点换算：

```text
scaled = raw_q32 * scale
value  = round_half_away_from_zero(scaled / 2^32)
```

结果必须能装入 `i64`，否则采集失败。位置和速度使用距离 scale，旋转使用旋转
scale。Status 的四个时间字段不经过该换算，而是保留原生 `i32` 计数器。

### 2.3 公共复合类型

#### `Vec3`

| 字段 | 类型/范围 | 定义 |
| --- | --- | --- |
| `x` | `i64` | Unity 世界横轴。 |
| `y` | `i64` | Unity 世界垂直高度。 |
| `z` | `i64` | Unity 世界战场纵轴。 |

HDF5 中以 `[row, 3]` 的 `i64` dataset 保存，分量顺序固定为 `(x, y, z)`。

#### `Pose`

| 字段 | 类型/范围 | 定义 |
| --- | --- | --- |
| `position` | `Vec3` | 姿态所属 transform 的世界位置。 |
| `rotation` | `i64` | 绕战场垂直轴的原生旋转经 scale 换算后的值；v3 不强制归一到某个一圈区间。 |

#### `Gauge`

| 字段 | 类型/范围 | 定义 |
| --- | --- | --- |
| `current` | `i64` | 当前值。 |
| `maximum` | `i64` | 最大值。 |

v3 不强制 `0 <= current <= maximum`，也不强制 `maximum >= 0`。

## 3. HDF5 根属性与版本字段

根属性在当前实现中均以 HDF5 字符串保存，包括看起来是整数的版本和 tick 数。

| 属性 | 值/类型 | 定义与校验 |
| --- | --- | --- |
| `format` | 字符串，固定 `mechcore.mcfr` | reader 必须精确匹配。 |
| `container_version` | 十进制字符串，固定 `2` | 解析为 `u32` 后必须等于 `2`。 |
| `schema_version` | 十进制字符串，固定 `3` | 必须与 `D.schema_version` 相等。 |
| `tick_count` | `u64` 十进制字符串 | 至少为 1。 |
| `terminal_tick` | `u64` 十进制字符串 | 必须等于 `tick_count - 1`。它是 fighting 到 over 的最终逻辑边界。 |
| `scenario_hash` | 64 个小写十六进制字符 | `D` 与 `S(0)` 的 BLAKE3 摘要。reader 校验编码，不在 `open` 时重算。 |
| `result_hash` | 64 个小写十六进制字符 | scenario 与全部 tick hash 的 BLAKE3 摘要。reader 校验编码，不在 `open` 时重算。 |

`/context/data` 是一维 `u8` dataset，内容是规范 JSON 编码的 `DurableContext`。
对象 key 递归按字典序排列；整数按 JSON 整数编码；enum 使用下文给出的 snake-case
字符串。未知字段会被 serde 拒绝。

## 4. 持久上下文 `D = DurableContext`

| 字段 | 类型与范围 | 定义 | adapter 来源 | simulator 来源 |
| --- | --- | --- | --- | --- |
| `schema_version` | `u32`，必须为 `3` | 逻辑 schema 版本。 | 常量 `MCFR_SCHEMA_VERSION`。 | 同左。 |
| `game_build` | 非空字符串 | 游戏 build 身份。当前校验只要求 trim 后非空。 | `UnityEngine.Application.get_version()`。 | simulation config 的 `game_build`。 |
| `logic_step.numerator` | `u64`，必须大于 0 | 每步秒数的分子。 | 固定为 `1`。 | `gcd(100, 2000)` 约分后为 `1`。 |
| `logic_step.denominator` | `u64`，必须大于 0 | 每步秒数的分母。 | 固定为 `20`。 | 约分后为 `20`。 |
| `numeric_convention.distance_units_per_meter` | `u64`，必须大于 0 | 距离整数尺度。 | 固定为 `1000`。 | 固定为 `1000`。 |
| `numeric_convention.rotation_units_per_degree` | `u64`，必须大于 0 | 旋转整数尺度。 | 固定为 `1000`。 | 固定为 `1000`。 |
| `numeric_convention.time_units_per_second` | `u64`，必须大于 0 | 时间整数尺度。 | 固定为 `2000`。 | 固定为 `2000`。 |
| `combat_round` | `u32`，必须大于 0 | 从 1 开始的当前战斗回合。 | `CurrentMatch.get_RoundCount()`，拒绝非正值。 | 编译后的 layout `round`。 |
| `match_seed` | `i32` 全范围 | Match 随机流的 seed；`0` 在格式层没有特殊含义。 | `CurrentMatch.GetRandom().GetSeed()`，即实际生效 seed。 | CLI/调用者传入的 seed。 |
| `identity_contract` | enum，仅 `team_zx_sequential_v1` | v3 的来源中立身份分配规则。 | 固定枚举值。 | 固定枚举值。 |

`identity_contract` 在规范 JSON 中编码为字符串 `"team_zx_sequential_v1"`。

## 5. Tick、轨道与变长行边界

逻辑 API 的 `TickSlice` 包含：

| 字段 | 类型/范围 | 定义 |
| --- | --- | --- |
| `tick` | `u64`，`0..terminal_tick` | 由读取位置生成，不单独存储。 |
| `state` | `WorldSnapshot` | 当前逻辑边界 `S(t)`。 |
| `events` | `TransitionEvents` | 当前边界 `E(t)`；tick 0 必须为空。 |
| `tick_hash` | 64 位小写十六进制字符串 | `/ticks/hash[t]` 的 32 字节摘要转 hex。 |

每条变长轨道使用一个长度为 `tick_count + 1` 的 offsets dataset：

| HDF5 路径 | 类型 | 选取的行集合 |
| --- | --- | --- |
| `/ticks/unit_offsets` | `u64[]` | `/states/units/*` |
| `/ticks/projectile_offsets` | `u64[]` | `/states/projectiles/*` |
| `/ticks/building_offsets` | `u64[]` | `/states/buildings/*` |
| `/ticks/status_offsets` | `u64[]` | `/states/statuses/*` |
| `/ticks/event_offsets` | `u64[]` | `/events/*` |

所有 offsets 首项必须为 0，必须单调不减，末项必须等于对应列的总行数。tick `t`
读取半开区间 `[offsets[t], offsets[t+1])`。offset dataset chunk 为 1024，使用
shuffle + deflate level 1。

`/ticks/hash` 的类型/shape 为 `u8[tick_count, 32]`，chunk 为 `[1024, 32]`，当前不启用
deflate。

## 6. 身份与对象引用

### 6.1 `ObjectKind`

| 逻辑值 | 规范 JSON | HDF5 `kind` 编码 |
| --- | --- | ---: |
| Unit | `unit` | 1 |
| Projectile | `projectile` | 2 |
| Building | `building` | 3 |
| Status | `status` | 4 |

### 6.2 `ObjectRef`

| 字段 | 类型/范围 | 定义 |
| --- | --- | --- |
| `kind` | `ObjectKind` | 被引用对象的独立 ID namespace。 |
| `id` | `u64` | 该 namespace 内的对象 ID。 |

可选引用在 HDF5 中拆成三列：

```text
<name>_valid : u8   // 0 = None，非 0 = Some
<name>_kind  : u8   // None 时写 0
<name>_id    : u64  // None 时写 0
```

reader 只在 `valid != 0` 时解码 `kind`。当前 writer 不验证引用 ID 非零、不验证引用
对象存在，也不限制某个字段可以引用哪些 kind；adapter producer 会把原生指针解析成
当前已知的 Unit、Building 或 Projectile 引用，并在关键 target 无法解析时失败。

### 6.3 v3 ID 分配和实际校验

Unit、Projectile、Building、Status 各自拥有独立的 `u64` namespace，Formation 另有一套
`u64` namespace。规范分配器从 1 开始逐个递增。

对于 `S(0)`，writer 强制：

- 每个顶层对象 ID 都大于 0，且同 kind 内不重复；
- 每个对象 namespace 必须恰好为 `1..N`，不允许空洞；
- Formation ID 的去重集合必须恰好为 `1..F`；
- Unit 按 `(team_id, position.z, position.x)` 严格升序排列，因此同 team 同 `x/z`
  位置也会被拒绝。

adapter 在初始快照前以 `(team_id, world_z, world_x, native_pointer)` 排序，但会提前拒绝
同 team 的相同 `x/z`，所以 pointer 只参与非初始快照的确定排序，不会成为合法初始身份
的 tie-breaker。simulator 使用 `(team, z, x)` 排序。

对于 `t > 0`，当前 writer 只再次检查顶层对象 ID 大于 0 且同 kind 的当前快照内不重复。
它没有验证跨 tick ID 不变、死后不复用、动态 ID 连续、Formation 连续或引用完整性。
这些是 producer 责任，不是当前 reader/writer 已执行的检查。

## 7. `S(t) = WorldSnapshot`

`WorldSnapshot` 有四个按 ID 规范排序的数组：

| 字段 | 元素类型 | 规范排序 |
| --- | --- | --- |
| `units` | `UnitState` | `unit_id` 升序 |
| `projectiles` | `ProjectileState` | `projectile_id` 升序 |
| `buildings` | `BuildingState` | `building_id` 升序 |
| `statuses` | `StatusState` | `status_id` 升序 |

writer 在写每个 tick 前自动排序。reader 在读取具体 state 时要求物理列重建出的数组已经
是该规范顺序。

所有实体列使用跨 tick 追加的 SoA：标量 shape 为 `[rows]`，`Vec3` shape 为
`[rows,3]`；row chunk 为 4096，使用 shuffle + deflate level 1。

### 7.1 UnitState

| 逻辑字段 | HDF5 列与物理类型 | 定义、范围和当前来源 |
| --- | --- | --- |
| `unit_id` | `unit_id: u64` | v3 Unit namespace 的正 ID。adapter 用原生对象指针作为仅驻内存映射键，按 v3 顺序分配连续 ID；不是 `FightMech.ID` 或 `mechIDIndex`。simulator 用 `IdentityAllocator` 同序分配。 |
| `team_id` | `team_id: u32` | Team 归属。schema 接受整个 `u32`；adapter 将 `FightTeamController.GetTeamIndex()` 的非负 `i32` 转为 `u32`；当前 1v1 producer 通常为 0/1。simulator 来自 placement team。 |
| `formation_id` | `formation_id: u64` | v3 Formation namespace ID。adapter 以 `FightMech.GetMechTeam()` 指针分组；空指针时退化为该 Unit 自己的指针。simulator 以 `(team, formation_index)` 分组。S(0) 去重后必须从 1 连续。 |
| `unit_type_id` | `unit_type_id: u32` | 原生单位类型 ID。adapter 读取 `FightMech.GetMechID()` 并拒绝负数；simulator 来自 unit config。v3 不限制上界。 |
| `domain` | `domain: u8` | `ground=0`、`air=1`，其它值读取失败。adapter 来自 `FightMech.IsFly()`；simulator 来自 unit config。 |
| `position` | `position: i64[rows,3]` | Unit 根 `FightTransform.GetPositionInt3D()`，Q32.32 按距离 scale 舍入。simulator 使用内部 Q32 位置舍入，`y` 由 unit domain 的当前模型高度给出。每分量为 `i64`，v3 不限制地图边界。 |
| `body_rotation` | `body_rotation: i64` | 根 `FightTransform.GetRotationInt()` 按旋转 scale 舍入。simulator 使用当前机体旋转。v3 不强制角度区间。 |
| `aim_pose.position` | `aim_position: i64[rows,3]` | `FightMech.GetMainSkill().GetMainTransform().GetPositionInt3D()`。simulator 当前使用 Unit 位置。 |
| `aim_pose.rotation` | `aim_rotation: i64` | 上述主技能 transform 的 `GetRotationInt()`。simulator 使用当前主武器瞄准旋转。 |
| `velocity` | `velocity: i64[rows,3]` | `MotionController.GetCurrentVelocity()` 的 Q32.32 三分量按距离 scale 舍入。simulator 从内部当前速度舍入，当前平面模型令 `y=0`。schema 不额外限制速度范围。 |
| `motion_state` | `motion_state: u8` | 原生 `MotionController.fsm.GetCurrentState()` 的封闭映射：`idle=0`、`moving=1`、`attacking=2`、`stopped=3`；未知状态类会使 adapter 采集失败。simulator 输出自己的同语义状态。 |
| `mech_lock_target` | `mech_lock_target_{valid:u8,kind:u8,id:u64}` | `FightMech.lockTarget` 的可选对象引用。adapter 直接读字段并解析为当前 Unit/Building/Projectile；非空但无法解析会失败。simulator 输出当前 actor lock target。 |
| `collision_radius` | `collision_radius: i64` | adapter 的 `FightMech.GetRadius()` 按距离 scale 换算；simulator 来自 unit config。格式未强制非负。 |
| `life` | `life: i64` | 当前生命。adapter 将 `GetLife(): i32` 无损拓宽，所以原生 producer 范围为 `i32`；simulator 使用内核生命值。格式本身接受 `i64`。 |
| `max_life` | `max_life: i64` | 最大生命。adapter 将 `GetMaxLife(): i32` 拓宽；simulator 来自 config。格式不校验与 `life` 的关系。 |
| `alive` | `flags` bit 0 | adapter 的 `IsAlive()`；simulator 为 `life > 0`。 |
| `active` | `flags` bit 1 | adapter 的 `get_IsActive()`；当前 simulator 固定为 `true`。 |
| `targetable` | `flags` bit 2 | adapter 调用 `IsValidTarget(ref visibility)` 的 bool 返回值；simulator 当前为 `life > 0`。调用中的辅助 visibility 输出不持久化。 |
| `visibility` | `visibility: u8` | `normal=0`、`disappear=1`、`stealth=2`、`hide=3`；adapter 来自 `GetVisibility()`，其它原生值失败；simulator 当前固定 `normal`。 |
| `personal_shield.active` | `flags` bit 3 | `GetEnergyShieldController().IsActive()`；simulator 当前固定 `false`。 |
| `personal_shield.enabled` | `flags` bit 4 | `GetEnergyShieldController().IsEnable()`；simulator 当前固定 `true`。 |
| `personal_shield.energy` | `shield_energy: i64` | adapter 将 `GetEnergy(): i32` 拓宽；simulator 当前为 0。 |
| `personal_shield.max_energy` | `shield_max_energy: i64` | adapter 将 `GetMaxEnergy(): i32` 拓宽；simulator 当前为 0。 |

Unit `flags` 的 bit 5..7 当前写为 0；reader 忽略这些未定义 bit。格式没有对 life、alive、
active、targetable、visibility 与 shield 字段之间的一致性做玩法校验。

### 7.2 ProjectileState

| 逻辑字段 | HDF5 列与物理类型 | 定义、范围和当前来源 |
| --- | --- | --- |
| `projectile_id` | `projectile_id: u64` | Projectile namespace 正 ID。adapter 在 AddProjectile hook 或首次枚举时连续分配，并以原生 projectile 指针维持存活期映射；simulator 在释放时由 `IdentityAllocator` 分配。 |
| `team_id` | `team_id: u32` | adapter 来自 `ProjectileController.GetTeamController().GetTeamIndex()` 的非负 `i32`；simulator 来自 owner team。 |
| `owner` | `owner_{valid,kind,id}` | 可选 owner。adapter 来自 `FightProjectile.GetOwner()` 并通过当前对象映射解析；simulator 当前总是 Unit owner。格式不限制 kind。 |
| `position` | `position: i64[rows,3]` | `FightProjectile.GetFightTransform().GetPositionInt3D()`；simulator 为当前投射物位置。 |
| `orientation` | `orientation: i64` | 原生 projectile transform 的 `GetRotationInt()`；simulator 当前写 0。 |
| `target` | `target_{valid,kind,id}` | `FightProjectile.GetTarget()` 的可选引用；simulator 当前为 Unit 或 Building。 |
| `cached_target_position` | `cached_target_position: i64[rows,3]` | `FightProjectile.GetTargetInfo().GetPosition()`；这是 projectile 自己缓存的目标点，不从当前 target 快照重新计算。simulator 保存释放时/跟踪时的对应缓存。 |
| `cached_target_radius` | `cached_target_radius: i64` | `FightProjectile.GetTargetInfo().GetRadius()` 按距离 scale 换算；格式不强制非负。 |
| `released` | `released: u8` | bool，adapter 来自 `FightProjectile.IsRelease()`；reader 将任何非零值视为 true。simulator 当前对保留到快照的 projectile 写 false。 |
| `life.current` | `life_current: i64` | adapter 将 `GetLife(): i32` 拓宽；simulator 为当前 projectile life。 |
| `life.maximum` | `life_maximum: i64` | adapter 将 `GetMaxLife(): i32` 拓宽；simulator 当前保存创建时的 projectile life 配置。格式不校验 gauge 关系。 |

Projectile 只在 tick-end 仍存在于 ProjectileSystem 时进入 `S(t)`；同一个 tick 内释放又移除的
瞬时 projectile 仍可只出现在 `E(t)` 中。

### 7.3 BuildingState

| 逻辑字段 | HDF5 列与物理类型 | 定义、范围和当前来源 |
| --- | --- | --- |
| `building_id` | `building_id: u64` | Building namespace 正 ID。adapter 初始按 `(team_id, GetBuildingIndex())` 排序后连续分配；simulator 按 training-ground config 顺序从 1 分配。 |
| `team_id` | `team_id: u32` | 所属 FightTeam 的非负 team index；simulator 来自 building config。 |
| `building_type_id` | `building_type_id: u32` | adapter 的 `GetBuildingType(): i32` 转为非负 `u32`；simulator 来自 config。 |
| `position` | `position: i64[rows,3]` | `GetFightTransform().GetPositionInt3D()`；simulator 使用 config 世界位置且当前 `y=0`。 |
| `rotation` | `rotation: i64` | `GetFightTransform().GetRotationInt()`；simulator 当前为 0。 |
| `bounds_width` | `bounds_width: i64` | `GetBoundsRect().size.x` 按距离 scale 换算；simulator 为配置半径的两倍。格式不强制非负。 |
| `bounds_height` | `bounds_height: i64` | `GetBoundsRect().size.y` 按距离 scale 换算；这里是地面矩形另一边，不是 Unity `y` 高度。simulator 为配置半径的两倍。 |
| `life` | `life: i64` | `GetLife(): i32` 拓宽；simulator 当前生命。 |
| `max_life` | `max_life: i64` | `GetMaxLife(): i32` 拓宽；simulator 为初始配置生命。 |
| `alive` | `flags` bit 0 | adapter 的 `IsAlive()`；simulator 为生命是否大于 0。 |
| `destroyed` | `flags` bit 1 | adapter 的 `IsDestroyed()`；simulator 在生命降到 0 时置 true。 |
| `available` | `flags` bit 2 | adapter 的原生拼写 `IsAvaliable()`；simulator 当前固定 true。 |
| `targetable` | `flags` bit 3 | adapter 的 `IsValidTarget(ref visibility)` 返回值；simulator 为生命是否大于 0。 |
| `collision_enabled` | `flags` bit 4 | `GetBuildingData().get_EnableCollision()`；simulator 来自 config。 |

Building `flags` 的 bit 5..7 当前写为 0，reader 忽略。v3 不校验这些 bool 之间或它们与
life 的一致性。

### 7.4 StatusState

| 逻辑字段 | HDF5 列与物理类型 | 定义、范围和当前来源 |
| --- | --- | --- |
| `status_id` | `status_id: u64` | Status namespace 正 ID。adapter 用原生 Buff 指针在其被枚举期间维持连续身份；消失后移除指针映射。当前 simulator 不生成 Status。 |
| `status_type_id` | `status_type_id: u32` | `Buff.GetBuffID(): i32` 转为非负 `u32`。 |
| `source` | `source_{valid,kind,id}` | `Buff.GetSource()` 的可选对象引用；无法映射时 adapter 保存 None。 |
| `target` | `target_kind: u8`、`target_id: u64` | 必选逻辑引用。当前 adapter 将包含该 Buff 的 `FightMech` 作为 target；如果无法映射，整条 Status 跳过。HDF5 没有 target valid 列。 |
| `additive_stack` | `additive_stack: i32` | `Buff.GetAdditiveStack()` 的原始值，完整 `i32` 范围。 |
| `duration_time` | `duration_time: i32` | 原生 `Buff.durationTime` 字段。保留原始计数器，不重新命名为 elapsed/remaining。 |
| `max_duration_time` | `max_duration_time: i32` | 原生 `Buff.maxDurationTime` 字段。 |
| `step_time` | `step_time: i32` | 原生 `Buff.stepTime` 字段。 |
| `step_time_config` | `step_time_config: i32` | 原生 `Buff.stepTimeConfig` 字段。 |
| `finished` | `flags` bit 0 | `Buff.IsFinish()`。 |
| `frozen` | `flags` bit 1 | `Buff.IsFreeze()`。 |

Status `flags` 的 bit 2..7 当前写为 0。四个时间字段的格式范围均是完整 `i32`，v3
不验证它们之间的大小关系，也不把它们统一解释为秒。

## 8. `E(t) = TransitionEvents`

`TransitionEvents.events` 是有序 `Event[]`。writer 保留数组顺序，不另存 `event_seq`。
adapter 在 `FightController.Update` 内按 hook 记录进入 trace 的先后顺序生成事件；
simulator 按内核操作顺序 push 事件。

### 8.1 Event 公共字段

| 字段 | HDF5 列 | 定义与范围 |
| --- | --- | --- |
| `payload.kind` | `kind: u8` | `projectile_released=0`、`projectile_removed=1`、`damage=2`；其它值读取失败。 |
| `subject` | `subject_{valid:u8,kind:u8,id:u64}` | 事件本体，可选。Projectile 事件通常指向 projectile；Damage 当前为空。 |
| `source` | `source_{valid:u8,kind:u8,id:u64}` | 直接动作来源，可选。 |
| `target` | `target_{valid:u8,kind:u8,id:u64}` | 直接目标，可选。 |

当前格式没有按 event kind 验证 subject/source/target 的存在性或 kind，也不验证引用在
`S(t-1)`、`S(t)` 或历史中存在。

### 8.2 EventPayload

| kind | 专用字段与 HDF5 列 | 定义、范围和 adapter 采集源 | simulator 生成 |
| --- | --- | --- | --- |
| `projectile_released` | 无专用 payload；`position=(0,0,0)`、`intercepted=0`、`amount=0` | Hook `ProjectileSystem.AddProjectile` 完成后读取 projectile、owner、target。`subject=Projectile`，source/target 为能够解析出的可选引用。 | 创建 projectile 时发出；subject 为新 projectile，source 为 owner Unit，target 为 Unit/Building。 |
| `projectile_removed` | `position: i64[3]`；`intercepted: u8`；`amount=0` | Hook `ProjectileSystem.Destroy`，在原方法执行前读取当前位置，并直接记录原生 `intercepted` 参数。subject 为 Projectile，source 为 owner，target 为 projectile target。reader 将非零 intercepted 视为 true。 | 命中/移除时记录最终位置；当前实现写 `intercepted=false`。 |
| `damage` | `amount: i64`；`position=(0,0,0)`；`intercepted=0` | Hook `DamagePerformer.Perform`，只在返回 `result > 0` 时记录，因此 adapter 实际范围为 `1..=i32::MAX`，amount 为返回值拓宽；source 是 provider 可解析引用，target 必须可解析，否则丢弃该事件；subject=None。格式本身允许完整 `i64`。 | 保存实际扣除量而非请求伤害；当前事件 amount 为正，source 可能为空或 Projectile，target 为 Unit/Building。 |

共享 union 列中不属于该 kind 的 cell 固定写零，逻辑上没有意义。v3 没有 Healing、Death、
StatusAdded、StatusRemoved 或单位生成/消失事件。

## 9. HDF5 dataset 完整清单

以下清单用于实现非 Rust reader。所有列在同一 group 内必须具有相同行数。

### 9.1 `/states/units`

```text
unit_id:u64                 team_id:u32
formation_id:u64            unit_type_id:u32
domain:u8                   position:i64[rows,3]
body_rotation:i64           aim_position:i64[rows,3]
aim_rotation:i64            velocity:i64[rows,3]
motion_state:u8             mech_lock_target_valid:u8
mech_lock_target_kind:u8    mech_lock_target_id:u64
collision_radius:i64        life:i64
max_life:i64                flags:u8
visibility:u8               shield_energy:i64
shield_max_energy:i64
```

### 9.2 `/states/projectiles`

```text
projectile_id:u64           team_id:u32
owner_valid:u8              owner_kind:u8
owner_id:u64                position:i64[rows,3]
orientation:i64             target_valid:u8
target_kind:u8              target_id:u64
cached_target_position:i64[rows,3]
cached_target_radius:i64    released:u8
life_current:i64            life_maximum:i64
```

### 9.3 `/states/buildings`

```text
building_id:u64             team_id:u32
building_type_id:u32        position:i64[rows,3]
rotation:i64                bounds_width:i64
bounds_height:i64           life:i64
max_life:i64                flags:u8
```

### 9.4 `/states/statuses`

```text
status_id:u64               status_type_id:u32
source_valid:u8             source_kind:u8
source_id:u64               target_kind:u8
target_id:u64               additive_stack:i32
duration_time:i32           max_duration_time:i32
step_time:i32               step_time_config:i32
flags:u8
```

### 9.5 `/events`

```text
kind:u8                     subject_valid:u8
subject_kind:u8             subject_id:u64
source_valid:u8             source_kind:u8
source_id:u64               target_valid:u8
target_kind:u8              target_id:u64
position:i64[rows,3]        intercepted:u8
amount:i64
```

## 10. 规范排序与哈希

逻辑对象先经 serde 转为 JSON value，再递归按对象 key 排序。数组顺序不被通用 JSON
规范化改变；在此之前 `WorldSnapshot` 的四个对象数组已分别按各自 ID 排序，Event 数组
仍保留发生顺序。

定义：

```text
feed(bytes) = little_endian_u64(len(bytes)) || bytes

H(domain, items...) = BLAKE3(
    "mechcore.mcfr.canonical\0"
    || feed(UTF8(domain))
    || feed(item_0)
    || ...
)
```

则：

```text
scenario_hash = H("scenario-v3", canonical_json(D), canonical_json(S(0)))

tick_hash(t) = H(
    "tick-v3",
    little_endian_u64(t),
    canonical_json(S(t)),
    canonical_json(E(t))
)

result_hash = H(
    "result-v3",
    scenario_hash_raw_32_bytes,
    little_endian_u64(tick_count),
    tick_hash(0)_raw,
    ...,
    tick_hash(n)_raw
)
```

注意 `tick_hash` 彼此独立，不是 hash chain。HDF5 的 chunk、压缩、attribute 顺序和文件
字节不进入正式哈希。

## 11. 当前 reader/writer 实际验证边界

### 11.1 Writer 明确验证

- 目标文件不存在；写入期间使用同目录临时文件，完成时不覆盖发布；
- `DurableContext` 的 schema、非空 build、正 round、正比例；
- 至少有 tick 0，且 `E(0)` 为空；
- 每个快照的顶层对象 ID 非零、同 kind 不重复；
- `S(0)` 的对象/Formation ID 连续以及初始 Unit 顺序；
- canonical serialization 和 HDF5 append 成功；
- partial append 失败后不允许 finish。

### 11.2 Reader `open` 明确验证

- 根 format、container/schema 版本、tick_count/terminal_tick；
- hash 是 64 位规范小写 hex，但不重算其内容；
- `/context/data` 是规范 JSON，且 context 自身合法；
- offsets 的长度、初始零和单调性；
- 每个列的 shape 与 offsets 末项一致；
- `/ticks/hash` shape 为 `[tick_count,32]`；
- `E(0)` 为空。

读取某个 state/event 时还会验证 enum 编码，并要求 state 数组为 ID 规范顺序。

### 11.3 当前未验证

- 保存的 tick/scenario/result hash 是否能由内容重算得到；
- 跨 tick 的对象身份连续性、不可复用性或动态 ID 无空洞；
- ObjectRef 的 ID 是否非零、对象是否存在、kind 是否适合该字段；
- Status target、Event 三个引用的玩法合法性；
- gauge、半径、bounds、时间计数器和 bool 之间的关系；
- `E(t)` 是否足以解释 `S(t-1) -> S(t)`，或事件是否重复/遗漏；
- terminal snapshot 是否确实对应游戏 over；该事实由 producer 生命周期保证。

因此“能被 `McfrReader::open` 打开”“能够播放”“hash 与另一 producer 相同”和“玩法上
完整合法”是不同强度的结论。

## 12. 当前 producer 覆盖差异

| 对象/能力 | adapter | simulator |
| --- | --- | --- |
| Unit | 完整采集 v3 定义字段 | 输出全部字段，但 active/visibility/shield/部分 aim 语义为当前封闭模型值。 |
| Projectile | 完整采集 v3 定义字段 | 输出当前支持武器的 projectile；orientation 固定 0，retained projectile 的 released 为 false。 |
| Building | 完整采集 FightTeam towers | 输出 training-ground config 中的建筑和当前支持的受伤状态。 |
| Status | 枚举 Unit BuffManager 中的 Buff | 当前始终为空。 |
| Projectile events | 原生 AddProjectile/Destroy hook | 当前支持 projectile 路径生成。 |
| Damage event | 原生 DamagePerformer 返回值 | 当前支持的直接、laser、projectile 伤害路径生成。 |

schema 能表达某字段不代表 simulator 已经具备该机制；反过来，simulator 私有的 Q32、RVO、
selector 或随机流状态也不会自动成为 v3 字段。

## 13. Instrumentation sidecar 边界

`.mcfr-i` 使用独立格式 `mechcore.mcfr.instrumentation` 和 container version 1。它按记录
保存 `step`、`channel`、`content_type`、payload bytes 及 payload offsets，并以根属性
保存 `scenario_hash`、`profile`、`producer`、`record_count`。

这些字段不是 schema v3 的 `D/S/E`，不进入 `tick_hash` 或 `result_hash`。sidecar 只能作为
机制研究证据，不能用于补写或推导正式 MCFR 中缺失的字段。
