# MCFR v6, format 0.6.0

[简体中文](mcfr.zh.md)

## Scope

This contract defines the MCFR v6 logical model: the container's members, the
schema of each, the identity and ordering rules that make two recordings of one
battle the same recording, and what a reader must validate before trusting one.

```text
format = "0.6.0"
```

The native field mapping is bound to game build `1.11.1.3.2259`. Another build
may produce the same format, provided its producer has verified that the native
interfaces it reads mean what this document says they mean.

The embedded `layout.yaml` is [layout.md](../document/layout.md) and is not
restated here. The operation that captures a recording is
[adapter.md](../adapter/adapter.md). What this document owns is what ends up
inside the file.

Two hashes are specified rather than one, and the separation is the point. A
stable physics projection decides regression identity, so adding an observation
field does not invalidate every existing recording. A full content hash covers
everything else, so the added field is still not invisible.

## Container shape

An `.mcfr` is a STORE-only ZIP64 whose member set is fixed:

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

| Member | Logical content | Time covered | Physical encoding |
| --- | --- | --- | --- |
| `layout.yaml` | the replayable canonical scene layout, first line `kind: layout` | recording | UTF-8 YAML, LF endings |
| `ticks.parquet` | DurableContext, recording metadata, per-tick digests | `T(1)..T(n)` | Parquet + Zstd level 6 |
| `units.parquet` | complete state of every live FightMech | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `projectiles.parquet` | complete state of every projectile in ProjectileSystem | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `buildings.parquet` | each FightTeam's live Crystal and Construction state | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `shields.parquet` | battlefield shields still present in AdvancedEnergyShieldSystem | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `terrains.parquet` | dynamic battlefield terrain in RangeItemSystem, with its unit applications | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `events.jsonl` | ordered discrete events between adjacent snapshots | `E(1)..E(n)` | UTF-8 JSON Lines, LF endings |

The ZIP layer stores; compression is the Parquet pages' Zstd. The six Parquet
members flush a row group every 128 logical ticks, with at most 1,000,000 rows
in one row group. State tables sort by `(tick, object_id)` and events by
`(tick, ordinal)`.

The timeline is:

```text
T(t) = { S(t), E(t), physics_tick_hash(t), content_tick_hash(t) }, 1 <= t <= n
S(1) = the state after the first native logic update completes
```

State, events and per-tick digests all begin at tick 1. The post-deployment
`S(0)` is never written. `tick_count` is at least 1, and `terminal_tick` equals
`tick_count`.

### The embedded layout

The adapter reads the game objects during `StartCapture`'s deployment phase and
caches the layout before combat sampling begins. Both sides come from
`PlayerManager.GetPlayerControllers()`. Units read the `CardElement` type,
native index, level, MapElement position and facing, equipment, and
`SuperDeploymentSystem.IsTravellingUnit()` out of `UnitManager.GetUnits()`;
constructions come from `ConstructionManager.GetConstructionElements()`. Officer
technologies come from `OfficerManager` and unit technologies from the
`TechnologyManager` of the full unit catalogue. The Research Center, Energy
Tower, contraptions and battle skills each read their manager's current
deployment state, release record and landing positions.

An unknown type, a duplicate identity, a broken upgrade chain or an incomplete
release record makes the capture fail closed.

The layout is canonical YAML: `seed` is explicit, a field whose native value is
the format default is omitted, and `formations` keeps declaration order by
native Unit index. The four opening defensive buildings are recorded in manager
order under a sibling `constructions`. This member exists for self-contained
replay and `mechcore fight verify`. It does not enter `physics_*_hash` or
`content_*_hash`.

## Ticks and scene context

`ticks.parquet` is the container index. It carries the format identifier, the
DurableContext, the overall digests, and the per-tick hashes from `T(1)`.

```text
tick               : UINT32 required
physics_tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
content_tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
```

The legal `tick` sequence is exactly `1..=tick_count`. `physics_tick_hash`
covers that tick's stable combat physics projection and `content_tick_hash`
covers the complete `S(t)` and `E(t)`; both are defined under
[Layered hashes](#layered-hashes).

### File metadata

Parquet key-value metadata keys and values are both UTF-8 strings.

| Key | Data | Meaning |
| --- | --- | --- |
| `format` | exactly `0.6.0` | the logical and physical contract version |
| `game_build` | non-empty UTF-8 | capture provenance; the adapter reads `UnityEngine.Application.get_version()` |
| `durable_context` | canonical JSON | the context `D` that holds steady for one round |
| `physics_hash_profile` | exactly `battle-physics-v1` | the stable physics projection version |
| `physics_result_hash` | 64 lowercase hex digits | ordered digest of every `physics_tick_hash`; what regression compares |
| `content_hash_profile` | exactly `mcfr-content-0.6.0` | the full content digest version |
| `content_result_hash` | 64 lowercase hex digits | ordered digest of every `content_tick_hash`; an in-format diagnostic |
| `tick_count` | canonical decimal `u32` | logical ticks recorded, counting from `S(1)` |
| `terminal_tick` | canonical decimal `u32` | the confirmed final logical boundary, equal to `tick_count` on a continuous timeline |

### DurableContext

| Field | Type and data | Meaning | Native source |
| --- | --- | --- | --- |
| `logic_step` | `Rational<u32>`, both parts above 0 | seconds per logic advance | the adapter fixes `1/20` |
| `time_units_per_second` | `u32 > 0` | native discrete time density | build 2259 fixes `2000` |
| `combat_round` | `u32 > 0` | the combat round | `CurrentMatch.get_RoundCount()` |

`match_seed` is not written to `ticks.parquet`. The writer still validates the
embedded layout against the caller's seed, and the reader recovers the public
`DurableContext.match_seed` from the canonical `layout.yaml.seed`. `game_build`
describes the capture source as its own file metadata.

## Units

Each row is one `LiveUnitState`. The adapter walks `FightTeam.GetMeches()` per
team and takes the objects where `FightMech.IsAlive()` holds. Every tick is a
complete snapshot; once a unit leaves the live set, the discrete fact of its
death survives as a `unit_died` event.

| Field | Parquet type | Meaning | Native source |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | the logical moment this state belongs to | the adapter's logic frame counter |
| `unit_id` | `UINT64 required` | stable ID inside the Unit namespace | the identity rule under [Normal form](#normal-form) |
| `team_id` | `UINT32 required` | the team it is on now | `FightTeam` controller index |
| `original_team_id` | `UINT32 required` | the team it was on when first seen | the first sampled `team_id` |
| `formation_id` | `UINT64 required` | formation identity | `FightMech.GetMechTeam()` pointer mapping |
| `unit_type_id` | `UINT32 required` | native unit type | `FightMech.GetMechID()` |
| `domain` | `UINT8 required` | ground or air | `FightMech.IsFly()` |
| `position` | `QVec3 required` | world coordinates | FightTransform `GetPositionInt3D()` |
| `body_rotation` | `INT64 required` | body facing, Q32.32 raw | FightTransform `GetRotationInt()` |
| `velocity` | `QVec3 required` | current velocity | MotionController native velocity |
| `motion_state` | `UINT8 required` | idle, moving, attacking, stopped | native motion state machine mapping |
| `mech_lock_target` | nullable `ObjectRef` | what the FightMech has locked | `FightMech.lockTarget` |
| `collision_radius` | `INT64 required` | collision radius, Q32.32 raw | `FightMech.GetRadius()` |
| `life` | `GaugeI32 required` | current and maximum life | `GetLife()` / `GetMaxLife()` |
| `active` | `BOOLEAN required` | current activation | `get_IsActive()` |
| `targetable` | `BOOLEAN required` | whether it is a legal target now | `IsValidTarget(visibility=0)` |
| `visibility` | `UINT8 required` | native visibility state | `GetVisibility()` |
| `status_mask` | `UINT64 required` | four native booleans | below |
| `buff_modifiers` | nullable struct | non-zero aggregate numeric corrections from BuffManager | below |
| `unit_dynamic_modifiers` | nullable struct | non-zero unit-level dynamic corrections on FightMech | below |
| `skill_dynamic_modifiers` | required sparse list | per-skill non-zero dynamic corrections | below |
| `personal_shield` | required struct | the unit's own energy shield | below |
| `weapon_aims` | required list | per-weapon channel state across main and sub skills | below |
| `derived` | required struct | the numbers the fight reads, after every correction | below |

### `derived`

The modifier structs above say what was **written onto** a unit; this says what
the build then **computed** from them. The two together are what a capture
measuring a composition rule reads, and carrying both means the reading is one
tick of one recording rather than a fight arranged so that its outcome
distinguishes the candidates.

| Field | Type | Native source |
| --- | --- | --- |
| `move_speed` | `INT64 required`, Q32.32 raw | `FightMech.GetMoveSpeed()` |
| `attack_range` | `INT64 required`, Q32.32 raw | `FightSkill.GetAttackRange()` of skill slot 0 |
| `attack_damage` | `INT32 required` | `FightSkill.GetNormalDamage(0)` of skill slot 0 |

Slot 0 is the skill the simulator models. A unit whose skills differ per slot
is outside the closure today; when one enters it, this grows a per-slot list
and the divergence shows up in the content layer first, which is what that
layer is for.

An attack interval is the build's own integer rather than the `FPoint` seconds
its property answers, because `RefreshAttackInterval` divides the property by
the step and truncates, and an integer is the one form both sides can hold
without deciding whose rounding is authoritative. It counts logic ticks: one
second is twenty.

**It is the interval of the cycle in progress, not the description's.** Every
cycle draws a stagger from the team's random stream, so the number changes as
a fight runs and two units of one kind read different numbers at one tick:
three Marksmen read 55, 65 and 56 at tick one against a description of 62, and
62, 70 and 52 once their first shot has gone.
[`combat.md`](../../rules/combat.md) measures the draw.

Both backends answer it, and they agree. The simulator remembers the interval
each cycle was scheduled with, which is the same quantity the build keeps, so a
native recording and a simulated one of the same fight carry the same sequence
— which makes the field a check on the stagger rather than a difference to
explain.

`tests/layouts/modifier/interval-order.mcscript` measured the composition
rule's order through this field without ever locating that zero: three fixtures
pin the line and the fourth is read against it.

These are content-layer fields. The physics layer does not hash them, so
adding them left every recorded `physics_result_hash` unchanged.

### `status_mask`

The mask holds four native bits as of the sampling boundary. A legal mask is
`0x0..=0xF`.

| Bit | Name | Native source | Meaning |
| ---: | --- | --- | --- |
| 0 | `invincible` | `BuffManager.IsInvincible()` | the native predicate's value at the sampling boundary |
| 1 | `frozen` | `BuffManager.IsFreeze()` | the native predicate's value at the sampling boundary |
| 2 | `technology_disabled` | `FightMech.IsTechnologyDisabled()` | unit technology effects are off |
| 3 | `recovery_disabled` | `FightMech.IsRecoverDisabled()` | unit recovery is off |

These four record the present value of native boolean state. `invincible` and
`frozen` keep the native predicates' names, and the names are the safest thing
to call them: what each does in combat is read from build-bound native branches
and controlled observation rather than from the word. Numeric buffs express
their aggregate effect through `buff_modifiers` instead.

### `buff_modifiers`

The buff channels come from the aggregate getters on
`FightMech.GetBuffManager()`. Each Rate field holds Q32.32 raw as
`{ add: i64, reduce: i64 }` and each Value field holds `{ add: i32, reduce: i32 }`.
The neutral value is 0 throughout, and both components are non-negative.

The adapter reads the complete getter set every frame. In Parquet each modifier
field and each numeric leaf is nullable, and null reads as 0; when everything is
0, the root struct is null. An absent field means the capture succeeded and
found no correction. It does not mean the capture was unavailable.

The native reduce getters return the remaining multiplier, and MCFR stores the
reduction instead:

```text
reduce = 1.0_q32_32 - native_reduce_factor
```

| Field | Type | Native source |
| --- | --- | --- |
| `move_speed_rate` | Rate | `GetMoveSpeedChangeAddRate/ReduceRate` |
| `move_speed_value` | Value | `GetMoveSpeedChangeValue`, split by sign |
| `damage_rate` | Rate | `GetDamageChangeAddRate/ReduceRate` |
| `attack_interval_rate` | Rate | `GetAttackIntervalChangeAddRate/ReduceRate` |
| `extra_attack_interval_rate` | Rate | `GetExtraAttackIntervalChangeAddRate/ReduceRate` |
| `amplify_damage_rate` | Rate | `GetAmplifyDamageAddRate/ReduceRate` |
| `attack_range_value` | Value | `GetAttackRangeAddValue/ReduceValue` |
| `extra_attack_range_value` | Value | `GetExtraAttackRangeAddValue/ReduceValue` |
| `attack_range_rate` | Rate | `GetAttackRangeAddRate/ReduceRate` |
| `extra_attack_range_rate` | Rate | `GetExtraAttackRangeAddRate/ReduceRate` |

`amplify_damage_rate` is the incoming-damage multiplier applied to this unit. A
buff's application, duration and expiry are observed as changes to
`status_mask` and `buff_modifiers` across consecutive snapshots, because the
format records state rather than buff objects.

### `unit_dynamic_modifiers`

The unit channel reads the complete `MechDataChange*` set FightMech supports.
`i64` fields hold native fixed-point raw, Rate fields use `{add, reduce}`, and
`i32` fields hold native integers.

Modifier fields and numeric leaves are nullable and null reads as 0; when
everything is 0 the root struct is null. The adapter still calls the complete
enum and getter set, and the MCFR write layer encodes only the non-zero results.

| Group | Fields |
| --- | --- |
| `MechDataChangeFloat` / `i64` | `gf_range_value`, `gf_life_time_value`, `mech_group_distance` |
| `MechDataChangeFloatRate` / Rate | `life_rate`, `life_rate_by_kill_count`, `reduce_damage_from_remote`, `move_ability_exit_time_change_rate`, `move_speed_change_rate`, `amplify_damage_rate` |
| `MechDataChangeInt` / `i32` | `move_speed_value`, `reduce_damage_value`, `child_inherit_technology_effect` |

This channel stays independent of the BuffManager aggregate, so the two remain
separately attributable.

### `skill_dynamic_modifiers`

```text
skill_slot : UINT16 required
modifiers  : SkillDynamicModifierSet required
```

The adapter walks `FightMech.GetSkills()`, orders by skill slot, and reads the
complete `SkillDataChange*` set each skill supports.

All 26 modifier fields of `SkillDynamicModifierSet` and their numeric leaves are
nullable, null reading as 0. The list keeps only skills with at least one
non-zero field, and is empty when nothing is modified. An omitted slot means
that slot currently carries no dynamic correction. It does not mean the unit
lacks the skill.

| Group | Fields |
| --- | --- |
| `SkillDataChangeFloat` / `i64` | `min_attack_range_value`, `attack_range_value`, `attack_air_range_add_value`, `attack_ground_range_add_value`, `attack_interval_value`, `damage_change_rate_ground`, `damage_change_rate_air`, `splash_range_value`, `cb_life_recovery_rate`, `projectile_speed_value`, `attack_point_change_value`, `projectile_duration_value`, `projectile_random_range`, `additional_damage_by_target_life` |
| `SkillDataChangeFloatRate` / Rate | `damage_rate`, `damage_rate_by_kill_count`, `attack_range_rate`, `attack_interval_rate`, `damage_reduce_rate_base`, `projectile_life_rate` |
| `SkillDataChangeInt` / `i32` | `projectile_count_value`, `air_attack_value`, `ground_attack_value`, `attack_range_value_air`, `attack_range_value_ground`, `is_lock_target` |

This is where a technology, an equipment or a round bonus lands when it changes
native skill data, which is what makes those attributable after the fact.

### `personal_shield`

```text
personal_shield = {
  active  : BOOLEAN required,
  enabled : BOOLEAN required,
  energy  : GaugeI32 required
}
```

The adapter reads `IsActive()`, `IsEnable()`, `GetEnergy()` and
`GetMaxEnergy()` through `GetEnergyShieldController()`. A unit's own shield gets
no Shield ID and never appears in `shields.parquet`.

### `weapon_aims`

```text
skill_slot   : UINT16 required
weapon_index : INT32 required
attack_target: ObjectRef nullable
position     : QVec3 nullable
rotation     : INT64 nullable
```

The adapter walks `FightMech.GetSkills()` across main and sub skills, then each
skill's `GetWeapons()`. `skill_slot` is the skill channel; `weapon_index` comes
from `WeaponData.get_Index()` and numbers the native weapon channel inside that
skill. `attack_target` comes from the skill's `GetAttackTarget()`, and the pose
from the weapon's `GetFightTransform()`. A weapon with no FightTransform has
null position and null rotation together, never one of the two.

The list is strictly ascending by `(skill_slot, weapon_index)`. It enumerates
weapon channels independently, so it may carry a skill slot that the sparse
`skill_dynamic_modifiers` omits.

## Projectiles

Each row is one `ProjectileState`. The adapter locates `ProjectileSystem` in the
current Fight's module set, walks its `projectileControllers`, and takes each
projectile through the controller's `GetFightProjectile()`. Every tick stores
the complete set the system enumerates.

| Field | Parquet type | Meaning | Native source |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | the logical moment | the adapter's logic frame counter |
| `projectile_id` | `UINT64 required` | stable ID inside the Projectile namespace | pointer mapped to a source-neutral ID |
| `team_id` | `UINT32 required` | the owning team | controller `GetTeamController().GetTeamIndex()` |
| `owner` | nullable `ObjectRef` | what fired it | `FightProjectile.GetOwner()` |
| `position` | `QVec3 required` | world coordinates | FightTransform `GetPositionInt3D()` |
| `orientation` | `INT64 required` | facing, Q32.32 raw | FightTransform `GetRotationInt()` |
| `target` | nullable `ObjectRef` | the current target | `FightProjectile.GetTarget()` |
| `cached_target_position` | `QVec3 required` | the target position the projectile cached | `GetTargetInfo().GetPosition()` |
| `cached_target_radius` | `INT64 required` | the target radius it cached | `GetTargetInfo().GetRadius()` |
| `released` | `BOOLEAN required` | the native pool release flag | `FightProjectile.IsRelease()`, returning `isReleased` |
| `life` | `GaugeI32 required` | current and maximum life | `GetLife()` / `GetMaxLife()` |
| `spawn_containing_shields` | required `list<ObjectRef>` | the live battlefield shields already containing it at creation, by Shield ID | `ProjectileController.inEnergyShields` |

`owner` and `target` may reference objects that have already left the current
snapshot. A reference identity stays valid for the whole recording.

`released` belongs to the object pool's lifecycle, not to firing.
`FightProjectile.Init()` clears `isReleased`, and the controller's pool reset
path sets it after calling `FightProjectile.ResetData()`. A live projectile in
this table is therefore normally false, and the fact of firing is the
`projectile_released` event instead.

`spawn_containing_shields` is the native intersection algorithm's birth
exemption set: a projectile fired from inside a shield skips that shield until
it leaves and crosses again. An empty list means it was born inside no live
battlefield shield. Every element must be `ObjectKind::Shield`.

## Buildings

Each row is one live `BuildingState`. For each FightTeam the adapter merges
three native collections and deduplicates by object pointer:

```text
FightTeam.GetTowers()
FightTeam.buildings
FightTeam.constructions
```

A member enters the snapshot through `IsAlive()`. The enumeration covers tower
FightCrystals such as the Research Center, team buildings and constructions, and
also objects the team holds that evolve as FightCrystals, the Rapid-Fire Turret
among them. A dead object leaves the state set, and `building_destroyed`
preserves the fact.

| Field | Parquet type | Meaning | Native source |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | the logical moment | the adapter's logic frame counter |
| `building_id` | `UINT64 required` | stable ID inside the Building namespace | assigned in the capture order under [Normal form](#normal-form), dynamic objects appended |
| `team_id` | `UINT32 required` | the owning team | the FightTeam controller index |
| `building_type_id` | `UINT32 required` | native building type | `FightCrystal.GetBuildingType()` |
| `position` | `QVec3 required` | world coordinates | FightTransform `GetPositionInt3D()` |
| `bounds_width` | `INT64 required` | bounds width, Q32.32 raw | `GetBoundsRect().size.x` |
| `bounds_height` | `INT64 required` | bounds height, Q32.32 raw | `GetBoundsRect().size.y` |
| `life` | `GaugeI32 required` | current and maximum life | `GetLife()` / `GetMaxLife()` |
| `available` | `BOOLEAN required` | native availability | `IsAvaliable()` |
| `targetable` | `BOOLEAN required` | whether it is a legal target now | `IsValidTarget(visibility=0)` |
| `collision_enabled` | `BOOLEAN required` | collision enabled in building data | `GetBuildingData().get_EnableCollision()` |

This collection is also the definition of what a building is: an object inside a
team's evolution lists, currently alive, and assignable a stable Building ID.

## Shields

Each row is one `FightEnergyShield` still present in the full
`AdvancedEnergyShieldSystem` collection. The adapter takes the native
`IFightGroup` from `FightTeam.GetFightGroup()` and reads
`GetEnergyShields(fightGroup)`. An inactive shield that can be reactivated
later, or refilled across a round, stays in the state track.

This track is for battlefield shields, which are what projectile spheres
intersect. A unit's own `EnergyShieldController` lives in
`LiveUnitState.personal_shield`. Battlefield shields take no part in RVO, unit
movement blocking, or layout deployment footprints.

| Field | Parquet type | Meaning | Native source |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | the logical moment | the adapter's logic frame counter |
| `shield_id` | `UINT64 required` | stable ID inside the Shield namespace | assigned once at S(1), see [Normal form](#normal-form) |
| `team_id` | `UINT32 required` | the owning team | `GetTeamController().GetTeamIndex()` |
| `source_kind` | `UINT8 required` | what produced it | the runtime type of `get_EnergyShieldData()` |
| `owner` | nullable `ObjectRef` | the bound FightActor; null for a fixed deployed shield | `GetOwner()` |
| `position` | `QVec3 required` | the sphere's centre | FightTransform `GetPositionInt3D()` |
| `radius` | `INT64 required` | the intersection sphere radius, Q32.32 raw | `GetRadius()` |
| `energy` | `GaugeI32 required` | current energy and the effective maximum | `GetEnergy()` / `GetMaxEnergy()` |
| `round_policy` | `UINT8 required` | what happens to it at a round's end | `IsShortLifeTime()` / `IsResetNextRound()`, normalised by native branch priority |
| `active` | `BOOLEAN required` | whether it intercepts projectiles now | `get_IsActive()` |
| `active_order` | nullable `UINT32` | zero-based order within its FightGroup's active set | the actual index in `GetActiveEnergyShields(fightGroup)` |

The four `source_kind` values are `EnergyShieldContraption`, `CS_EnergyShield`,
`AdvancedEnergyShieldController` and `SpawnAdvancedShieldController`. An unknown
data source makes the build 2259 adapter fail closed.

`round_policy` is decided in order: a short-lived object is
`destroy_at_round_end`; of the rest, a reset-next-round object is
`reset_to_max`; everything else is `retain_state`. `reset_to_max` uses the
effective maximum the data source reports at the round's end, so a Shield
Specialist refreshing an existing ordinary shield's maximum energy shows up
directly as a change in the gauge.

### Active order

The native system maintains the full collection and the active collection
separately, and `ActiveEnergyShield()` appends a reactivated object to the
active collection, including one reactivated across a round. The two orders can
therefore differ, and `active_order` cannot in general be derived from
`shield_id` and `active`.

A legal state satisfies all of:

- `active_order` is null exactly when `active` is false, and present otherwise;
- the non-null `active_order` values inside one FightGroup cover `0..k-1`
  contiguously;
- `radius` is positive;
- the snapshot is strictly ascending by `shield_id`;
- `owner` may reference an object that has left the state set but still holds a
  recording-stable identity.

`system_order` is used only for initial ID assignment and the adapter's own
consistency check. It is not a field of the file.

## Terrains

Each row is one dynamic terrain in some `RangeItemController.GetItems()`
collection of `RangeItemSystem`. The adapter enumerates controllers by native
`RangeItemType` and assigns a Terrain ID to each member. A controller not yet
instantiated and an item collection returning null both mean that type's row set
is currently empty. When an object leaves the collection, its last state and its
identity remain available to events.

`TerrainType` covers the six native types: `fire`, `oil`, `fog`, `acid`,
`recovery_zone`, `fog_sand`. Four match deployable battle skills: the incendiary
bomb is `fire`, the sticky oil bomb `oil`, the smoke bomb `fog`, and the acid
bomb `acid`.

| Field | Parquet type | Meaning | Native source |
| --- | --- | --- | --- |
| `tick` | `UINT32 required` | the logical moment | the adapter's logic frame counter |
| `terrain_id` | `UINT64 required` | stable ID inside the Terrain namespace | `RangeItem` pointer, in first-observed order |
| `team_id` | nullable `UINT32` | the team that created or holds it; null when it has no owner | `RangeItem.GetTeamController()` |
| `terrain_type` | `UINT8 required` | one of the six native `RangeItemType` | its controller cross-checked against `RangeItem.GetRangeItemType()` |
| `position` | `QVec3 required` | the centre, or a segment node | `RangeItem.GetPosition()` |
| `radius` | `INT64 required` | effect radius, Q32.32 raw | `RangeItem.GetRange()` |
| `grid` | nullable struct | a grid terrain's origin, size and per-row bit mask | `IsGridMode()` / `GetGridBlock()` |
| `remaining_rounds` | nullable `UINT32` | cross-round uses left at this boundary | `GetDuration() - get_Round()`, written only for cross-round terrain |
| `logic_lifetime` | nullable struct | lifetime in logic frames, `{elapsed, limit}` | `RangeItem.time/lifeTime`; `FightGroundFire` uses its own same-named fields |
| `applications` | required list | the units it currently affects, with their periodic clocks | the controller's `affectedUnits` and `affectedUnitTimes` |

```text
grid = {
  origin_x : INT64 required,
  origin_y : INT64 required,
  size_x   : UINT32 required,
  size_y   : UINT32 required,
  rows     : list<UINT32> required
}
```

`origin_x` and `origin_y` are Q32.32 raw. `rows` has `size_y` entries, and each
row's low `size_x` bits mark the live cells. The native `GridBlockInt` caps both
dimensions at 32.

```text
applications[] = {
  unit_id        : UINT64 required,
  periodic_clock : struct<elapsed: INT32, duration: INT32> nullable
}
```

The list is strictly ascending by `unit_id`, and each `unit_id` must reference a
unit alive on the same tick. `periodic_clock` is written when the controller
supplies a positive `effectTimeDuration`; a continuous effect with no periodic
trigger, a sustained slow or range reduction, uses null. Both clock integers
count logic advances and convert to seconds through `DurableContext.logic_step`;
they do not use the `time_units_per_second` scale.

`remaining_rounds` is a sparse derived field. Among the four deployable
terrains, sticky oil is the one that survives a round in current 1v1 native
scenes. The field stores the subtraction's result, and a reader interprets
cross-round remainder from that single value. Ordinary fire, acid and fog rows
use null.

### Terrain identity and lifecycle

Initial terrains take IDs by `(terrain_type, controller item index)`, and
objects appearing during combat are appended in first-observed order. A native
pointer that leaves the collection is tombstoned, and no later terrain in the
same recording may inherit its ID.

A row existing on a tick is exactly the claim that the terrain belongs to its
controller's authoritative item set. `terrain_created` and `terrain_removed`
record membership changes between adjacent boundaries. A membership diff catches
every instance that spans at least one snapshot boundary; a producer with native
lifecycle hooks may additionally record an instance created and removed inside
one logic advance, which a diff cannot see.

## Events

`events.jsonl` is UTF-8 with LF endings and one JSON object per line. An empty
event stream is a zero-byte file. Each line is written in the writer's fixed
field order with compact JSON.

Within one tick, `ordinal` increases contiguously from 0. The whole file is
strictly ascending by `(tick, ordinal)`, and `tick` lies in `1..=tick_count`.

| Field | JSON type | Meaning |
| --- | --- | --- |
| `tick` | number (`u32`) | the advance this event belongs to |
| `ordinal` | number (`u32`) | observation order within the tick |
| `type` | string | the event kind tag |
| `object` | `ObjectRef` or null | the subject |
| `source` | `ObjectRef` or null | the direct source; an event type that records this reference always carries the field |
| `source_team_id` | number (`u32`) or null | the team of the source or subject at the event boundary |
| `target` | `ObjectRef` or null | the direct target |

In JSON, `ObjectRef.id`, `formation_id` and Q32.32 raw are canonical decimal
strings, so every language reads them exactly. `ObjectRef.kind` is one of
`unit`, `projectile`, `building`, `shield`, `terrain`.

| `type` | Required reference | Payload | Meaning |
| --- | --- | --- | --- |
| `projectile_released` | `object` | `skill_slot: u16\|null`, `weapon_index: i32\|null` | a projectile was created and joined ProjectileSystem, with its weapon channel; the two channel fields are both present or both null |
| `projectile_removed` | `object` | `position_q32_32: QVec3`, `intercepted: bool`, `absorbed_by: ObjectRef\|null` | it left the system; `absorbed_by` names the battlefield shield that took it |
| `damage` | `target` | `amount: i32` | one positive damage result on one actual target; an Actor uses the native `damageReal` |
| `unit_created` | `object` | `team_id: u32`, `formation_id: u64`, `unit_type_id: u32`, `position_q32_32: QVec3` | a unit's lifecycle start |
| `unit_died` | `object` | `position_q32_32: QVec3` | a unit's death boundary |
| `building_destroyed` | `object` | `position_q32_32: QVec3` | a building's destruction boundary |
| `unit_team_changed` | `object` | `previous_team_id: u32`, `new_team_id: u32` | a unit changed side |
| `shield_created` | `object` | `team_id: u32`, `source_kind: string`, `position_q32_32: QVec3` | a shield joined the full collection and took an identity |
| `shield_destroyed` | `object` | `position_q32_32: QVec3`, `reason: string` | it left the full collection, ending its lifecycle |
| `terrain_created` | `object`, no `source` | `team_id: u32\|null`, `terrain_type: string`, `position_q32_32: QVec3`, `radius_q32_32: i64` | a terrain joined a controller item set and took an identity |
| `terrain_removed` | `object` | `position_q32_32: QVec3`, `reason: string` | it left the controller item set, ending its lifecycle |
| `terrain_converted` | `object` | `position_q32_32: QVec3` | one terrain changed native type or ownership |
| `healing` | `target` | `amount: i32` | one positive recovery result |

`projectile_removed` admits exactly three combinations. Native interception is
`intercepted=true, absorbed_by=null`. Absorption by a battlefield shield is
`intercepted=false, absorbed_by=ShieldRef`. Any other removal has neither.
`absorbed_by` may only reference a Shield.

`shield_destroyed.reason` is one of `energy_depleted`, `owner_destroyed`,
`round_end`, `scripted` or `unknown`. A producer writes a specific value only
where the native destruction entry point determines the cause.

`terrain_removed.reason` is one of `time_expired`, `round_expired`,
`grid_depleted`, `cleared` or `unknown`, under the same rule.

A `terrain_created` object is fully determined by the terrain's identity, team,
type, position and radius. Causation from a projectile is observed jointly, from
a `projectile_removed` at the same tick and position together with the terrain
state change.

### Which events a producer can emit

The MCFR model and the JSONL codec implement every event above. What a given
producer can actually observe is narrower, and the build 2259 adapter covers:

| Event | Native source |
| --- | --- |
| `projectile_released` | the nested trace from `ProjectileSystem.Create` to `AddProjectile`, which also resolves owner, target, skill slot and weapon index |
| `projectile_removed` | the ProjectileSystem destruction path, capturing final position and interception; `absorbed_by` is added when the same damage chain hits a shield |
| `damage` | `DamagePerformer.Perform` gives the provider attribution scope, each `FightController.OnActorHitted(HitDamageInfo)` gives the Actor target and `damageReal`, and the battlefield shield damage helper gives the per-shield actual result |
| `unit_died` | the `FightMech.OnDead` trace |
| `building_destroyed` | the `FightCrystal.OnDead` trace |
| `shield_created` | first entry into the full `GetEnergyShields(fightGroup)` collection between adjacent sampling boundaries |
| `shield_destroyed` | disappearance from that collection, so the reason is `unknown` |
| `terrain_created` | first entry into the owning `RangeItemController.GetItems()` collection between adjacent boundaries |
| `terrain_removed` | disappearance from that collection, so the reason is `unknown` |

`unit_created`, `unit_team_changed`, `terrain_converted` and `healing` have a
shared schema, a canonical form and reader and writer support, for a producer
able to observe the evolution directly.

Buffs are not events. Their observation lives on the unit state track: boolean
state in `status_mask`, aggregate numeric corrections in `buff_modifiers`, and a
buff's evolution is the change in those fields between adjacent snapshots.

## Writing, reading and validation

The writer's public lifecycle is:

```text
create(game_build, context, layout_yaml)
append_tick(S(1), E(1))
...
append_tick(S(n), E(n))
finish()
```

It creates temporary members and a `.zip.part` beside the target, completes the
Parquet footers, the JSONL and the ZIP envelope, reopens the result with
`McfrReader` to re-check its structure and persisted hash metadata, and
publishes through `persist_noclobber`. The target path is required to be free at
creation, and publication refuses to overwrite.

Before each write, the WorldSnapshot is canonicalised into the order under
[Normal form](#normal-form), then checked for object IDs, list order, state bits
and modifier constraints. One failed partial write poisons the writer, and only
a successful `finish()` publishes a file.

### What a reader validates

On opening a container, a reader verifies:

- the ZIP member set, the STORE method, ZIP64 readability and member uniqueness;
- `layout.yaml` as UTF-8, its shared structure, its canonical representation,
  and its round's agreement with the DurableContext;
- the six Parquet schemas, their required and nullable structure, and Zstd
  column compression;
- `game_build`, the DurableContext canonical JSON, metadata types and the format
  identifier;
- tick contiguity, state table ordering, event ordering and ordinal contiguity;
- ObjectRefs, enum tags, initial identity order, list order, `status_mask`
  reserved bits and modifier components;
- the contiguity, width and encoding of both tick hash columns, and the encoding
  of both result hashes and their profiles.

A reader trusts the tick and result hashes the recording persisted.
`McfrReader::open()` does not rebuild `S(t)` and `E(t)` tick by tick to
re-derive them, and does not recompute the timeline. `first_divergence()`
compares the persisted `physics_tick_hash` values directly, and a caller that
has located a divergence can then read that tick's full content and
`content_tick_hash`.

A legal recording has at least `T(1)`, no tick 0 state or event, and ends
completely at `terminal_tick`.

## Common types and enum tags

Space, rotation and native fixed-point modifiers use signed `i64` raw bits:

```text
real_value = raw / 2^32
```

`QVec3 = { x: i64, y: i64, z: i64 }`, three required `INT64` children in
Parquet and decimal strings in JSONL. Position, radius and bounds are metres,
velocity metres per second, and rotation and orientation degrees.

```text
GaugeI32 = { current: i32, maximum: i32 }
Rational = { numerator: u32, denominator: u32 }
ObjectRef = { kind: ObjectKind, id: u64 }
```

A gauge keeps the native signed values. Both parts of a rational are above 0.
Each ObjectKind has its own ID namespace, numbered from 1, stable for the
recording's lifetime.

| Type | Parquet tags |
| --- | --- |
| `ObjectKind` | `0=unit`, `1=projectile`, `2=building`, `3=shield`, `4=terrain` |
| `Domain` | `0=ground`, `1=air` |
| `MotionState` | `0=idle`, `1=moving`, `2=attacking`, `3=stopped` |
| `Visibility` | `0=normal`, `1=disappear`, `2=stealth`, `3=hide` |
| `ShieldSourceKind` | `0=contraption`, `1=commander_skill`, `2=owner_advanced`, `3=spawned_temporary` |
| `ShieldRoundPolicy` | `0=destroy_at_round_end`, `1=reset_to_max`, `2=retain_state` |
| `TerrainType` | `0=fire`, `1=oil`, `2=fog`, `3=acid`, `4=recovery_zone`, `5=fog_sand` |

## Normal form

Identity is what makes two recordings of one battle the same recording, so
every namespace numbers its objects by a rule that depends on the scene rather
than on the pointer that happened to be observed first.

Format `0.6.0` uses `team_zx_sequential_v1`.

**Units.** Initial units sort strictly ascending by `(team_id, position.z,
position.x)` and take `unit_id = 1..N` in that order. Initial units on one team
have unique `(z, x)`, which is what lets an adapter and a simulator number the
same scene identically. A unit first appearing during combat takes the next
number in the namespace, which increases monotonically from 1, and a historical
reference keeps whatever number the object first received.

**Formations.** `formation_id` is assigned to formations in the order the unit
ordering above first meets them, so initial formation IDs are exactly `1..F` by
first appearance. This numbers formations by their members' world `z` and `x`
ordering, and is not the layout's formation declaration index. The layout keeps
declaration order by native Unit index, because formation member scatter uses
`match_seed + unit_index`.

**Buildings.** Initial buildings sort strictly ascending by `(team_id,
building_type_id, position.x, position.y, position.z)` using Q32.32 raw
integers, and take `building_id = 1..N`. Two objects with the same key fail the
capture; a native pointer or a creation counter is not an acceptable
tie-breaker, because neither is stable. One construction may be several wall
segments, and each segment takes its own ID from its own position. The layout
stores no construction index, and neither the native building or construction
index nor the layout declaration order takes part in this assignment. Both game
modes use the same rule.

**Shields.** Initial shields are grouped by team at S(1): active shields in
ascending `active_order`, then that team's inactive shields. Shields already
removed on the first tick, which events may still reference, sort after every
S(1) object and are then grouped by team, so that the IDs present in the initial
state run contiguously from 1. Inside the inactive and removed group the sort key
is `(source_kind, owner, position.x/y/z, radius, round_policy, energy.maximum,
energy.current)`, a removed item using its last observed state; an
indistinguishable key fails the capture. This numbering happens exactly once,
converting `E(1)` and cached references with it. Shields are never renumbered
per tick from `active_order`, and an ID never changes because a shield
deactivated, reactivated, or moved within the active set. Later shields are
appended in first-observed order.

**Terrains.** Initial terrains take `(terrain_type, native controller item
index)`, and later ones are appended in first-observed order.

Once a shield or terrain leaves its authoritative collection, its native pointer
is tombstoned so that the historical ID stays unique.

**Ordering inside a snapshot.** State snapshots sort by object ID.
`skill_dynamic_modifiers` sorts by `skill_slot`, `weapon_aims` by
`(skill_slot, weapon_index)`, and a projectile's `spawn_containing_shields` by
Shield ObjectRef.

**States and events in one frame.** `E(t)` is what hooks observed directly
while advancing from `S(t-1)` to `S(t)`, and `S(t)` is the authoritative
complete state after that advance finishes. A death or destruction event may
therefore share a tick with the object's absence from `S(t)`, and that is not a
contradiction.

## Layered hashes

BLAKE3 throughout. Every digest begins with a fixed prefix and a domain, and the
prefix, the domain and each later input segment are encoded as:

```text
LE_u64(byte_length) || bytes
```

The fixed prefix is `mechcore.mcfr.canonical\0`. Integers are little-endian
two's complement raw bits at the stated width, booleans a single `0` or `1`
byte, an `Option<T>` a presence byte followed by `T` when present, and a list's
length an `LE_u64`. Every public digest is 64 lowercase hex digits, and the
Parquet tick columns hold the raw 32 bytes.

### The stable physics layer, `battle-physics-v1`

`physics_tick_hash` digests a version-frozen combat physics projection rather
than the whole MCFR schema. Adding an observation field does not change the
projection. If a projection field, unit, precision, order or encoding must
change, that is a new profile: `battle-physics-v1` is never edited in place.

The WorldSnapshot is canonicalised before hashing. Object lists stay in stable
ID order, events in native `ordinal` order, weapon poses in `(skill_slot,
weapon_index)` order. Q32.32 keeps its `i64` raw bits. Angles are reduced
modulo `360 << 32` with a Euclidean remainder before hashing, so two angles a
whole number of turns apart are equal. `logic_step` is reduced first.

Each tick is three independent lane digests:

| Lane | Fields |
| --- | --- |
| `battle-physics-kinematics-v1` | Unit: `unit_id, position, body_rotation, velocity`, plus `skill_slot, weapon_index, pose.position, pose.rotation` for channels that have a pose. Projectile: `projectile_id, position, orientation`. Building: `building_id, position`. Shield: `shield_id, position, radius`. Terrain: `terrain_id, position, radius, grid(origin_x, origin_y, size_x, size_y, rows)` |
| `battle-physics-vitals-v1` | Unit: `unit_id, unit_type_id, team_id, domain, collision_radius, life, personal_shield.active/energy`. Projectile: `projectile_id, team_id, owner, released, life`. Building: `building_id, building_type_id, team_id, bounds_width, bounds_height, life, available, targetable, collision_enabled`. Shield: `shield_id, team_id, owner, radius, energy, active`. Terrain: `terrain_id, team_id, terrain_type, radius` |
| `battle-physics-interactions-v1` | each event contributes `ordinal, subject, source, source_team_id, target` first, then its type and physical payload: the release channel; a removal's position, interception and absorbing shield; a damage amount; a unit creation's team, type and position; a unit death position; a building destruction position; a team change; a shield creation's team and position; a shield destruction position; a terrain creation's team, type, position and radius; a terrain removal or conversion position; a healing amount |

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

A physics regression therefore still pins logical time, Q32.32 position,
rotation and velocity, life and shields, and the interactions including damage,
while a new purely diagnostic field never forces a re-record.

### The full content layer, `mcfr-content-0.6.0`

State and events are first encoded as canonical JSON: UTF-8, object keys sorted
recursively, compact encoding, and the array order the schema defines. It covers
every `S(t)` and `E(t)` field of format 0.6.0 and diagnoses capture
completeness within one format. It carries neither the layout, nor the
DurableContext, nor any other file metadata.

```text
content_tick_hash(t) = H_content-tick-0.6.0(LE_u32(t), JSON(S(t)), JSON(E(t)))
content_result_hash  = H_content-result-0.6.0(
    LE_u32(tick_count),
    content_tick_hash(1)..content_tick_hash(n)
)
```

`mechcore fight compare` and `mechcore fight verify` decide `equal` and the first
divergence from the physics layer, and return `content_equal` separately. That
is how the format gains observation without losing regression identity, and
without hiding a genuine content difference inside one format.

## Physical encoding

- ZIP member order is fixed: `ticks`, `units`, `projectiles`, `buildings`,
  `shields`, `terrains`, `events`.
- ZIP members use STORE with a fixed timestamp; Parquet column chunks use Zstd
  level 6.
- The Parquet global dictionary is off. Low-cardinality columns enable a
  dictionary per column: team, type, enum, fixed radius and maximum.
- Every `tick` column uses `DELTA_BINARY_PACKED`.
- A required list expresses "this object currently has none" as an empty list; a
  nullable struct expresses `Option<T>`.
- Modifier numeric leaves are nullable with a canonical null of 0. The buff and
  unit dynamic root structs are null when wholly zero, and the skill dynamic
  list keeps only non-zero slots.
- State hashing uses the expanded zero defaults, and canonicalisation drops an
  all-zero skill entry, so the sparse physical encoding rebuilds exactly the
  same canonical state.
- Each state table stores the complete current set every tick, so reading any
  one tick rebuilds `S(t)` without replaying the ones before it.
- An event's field set is determined exactly by its type, and a reader checks
  canonical types, required references and field set line by line.

### Native modifier mapping, by example

Sticky oil's slow lands in BuffManager's aggregate `move_speed_rate`. The
incoming-damage change from a photon projection lands in `amplify_damage_rate`,
while the present value of `IsInvincible()` lands in `status_mask.invincible`. A
sub-skill such as the Sabertooth technology's secondary cannon uses its own
`skill_slot` in both `skill_dynamic_modifiers` and `weapon_aims`, and a round's
`+15` range bonus stays in that skill's modifier field.

The point of the examples is the attribution boundary. A BuffManager aggregate,
a FightMech unit-level dynamic correction and a FightSkill skill-level dynamic
correction persist separately, so which native field produced an effect stays
observable.

## Excluded fields

The following are deliberately absent, and each absence is a decision rather
than an omission.

- **`S(0)`.** The post-deployment state before the first logic update is not
  written. A recording starts at tick 1, so there is one convention for where
  time begins rather than two.
- **`match_seed` in `ticks.parquet`.** The seed is already in the embedded
  layout, and storing it twice invites two answers. A reader recovers the public
  `DurableContext.match_seed` from the canonical `layout.yaml.seed`.
- **A building `rotation`.** Buildings carry no rotation field in the JSON
  state, in the Parquet schema, or in either hash. A unit's `body_rotation` and
  a weapon pose's `rotation` are unaffected.
- **`system_order` for shields.** The native full-collection index is used for
  initial ID assignment and the adapter's consistency check and never reaches
  the file, because `shield_id` and `active_order` already carry everything a
  reader needs.
- **The layout in either hash.** `layout.yaml` enters neither `physics_*_hash`
  nor `content_*_hash`. It is the scene's input, not its outcome, and a
  recording's identity is what happened rather than what was asked for.
- **Buff objects.** There is no buff track. A buff is observable as the change
  in `status_mask` and `buff_modifiers` between adjacent snapshots, so the
  format stores state rather than the engine's internal buff instances.

The physics layer additionally excludes a long list of fields from
`physics_tick_hash` while still storing them: a unit's `original_team_id`,
`formation_id`, `motion_state`, `mech_lock_target`, `active`, `targetable`,
`visibility` and `status_mask`, every modifier, the personal shield's `enabled`,
a weapon's `attack_target` and every channel without a pose; a projectile's
`target`, cached target position and radius, and spawn-containing shields; a
shield's `source_kind`, `round_policy` and `active_order`; a terrain's remaining
rounds, logic lifetime and per-unit application clocks; an event's
`formation_id`, shield source kind, and shield or terrain removal reason. These
are excluded from regression identity, not from the file, and
`content_*_hash` still detects every one of them.

## Unresolved

**Should a removal reason exist when nothing can produce one?**
`shield_destroyed.reason` and `terrain_removed.reason` each define a five-value
vocabulary, and a producer that determines the cause only from a collection
membership diff can write exactly one of those values. Either the format expects
producers with native lifecycle hooks, in which case the vocabulary is right and
the diff is a degraded mode, or it does not, in which case the field is a
boolean wearing five names.

**Should `status_mask` bits be named after predicates or after effects?** Bits 0
and 1 carry the native predicate names `invincible` and `frozen`, and what each
actually does in combat is still being narrowed. A name is a claim readers will
act on. Keeping the predicate name is honest about provenance and misleading
about meaning; renaming to the observed effect is the reverse, and cannot be
undone cheaply once recordings exist.

**Should a derived value be stored at all?** `remaining_rounds` holds
`GetDuration() - get_Round()` rather than the two operands. It is the only
derived field in the format, it is sparse, and a reader cannot recover the
inputs from it. Storing both operands instead would be uniform with the rest of
the format at the cost of a column.

**Should the format define events no producer emits?** `unit_created`,
`unit_team_changed`, `terrain_converted` and `healing` have schema, canonical
form, and reader and writer support, and no producer observes them today.
Defining them keeps a later producer from inventing an incompatible shape;
defining them also means a reader cannot tell an event that never happened from
one nobody can see.
