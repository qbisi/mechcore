# MCFR, format 0.17.0

[简体中文](mcfr.zh.md)

## Scope

This contract defines the MCFR logical model: the container's members, the
schema of each, the identity and ordering rules that make two recordings of one
fight the same recording, and what a reader must validate before trusting one.

```text
format = "0.17.0"
```

The native field mapping is bound to the game version the repository pins in
`GAME_VERSION`. Another version may produce the same format, provided its producer has verified that the native
interfaces it reads mean what this document says they mean.

The embedded `layout.yaml` is [layout.md](../document/layout.md) and is not
restated here. The operation that captures a recording is
[adapter.md](../adapter/adapter.md). What this document owns is what ends up
inside the file.

One hash is specified, over everything the timeline holds. Two recordings are
the same fight when every field of every tick agrees: the hash never lets a
difference go unseen to spare a re-record. What it reads is admitted field by
field, under [Admission to the hash](#admission-to-the-hash).

## Container shape

An `.mcfr` is a STORE-only ZIP64 whose member set is fixed, apart from the
instrument channels a recording asked for:

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
└── instrument/<channel>.parquet   zero or more
```

The eight tables from `units.parquet` to `events.parquet` are present only when
they hold a row: a recording with no projectile has no `projectiles.parquet`,
and a reader reads a missing table as empty.

| Member | Logical content | Time covered | Physical encoding |
| --- | --- | --- | --- |
| `layout.yaml` | the replayable canonical scene layout, first line `kind: layout` | recording | UTF-8 YAML, LF endings |
| `ticks.parquet` | DurableContext, recording metadata, per-tick digests | `T(1)..T(n)` | Parquet + Zstd level 6 |
| `units.parquet` | complete state of every live FightMech | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `projectiles.parquet` | complete state of every projectile in ProjectileSystem | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `buildings.parquet` | each FightTeam's live Crystal and Construction state | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `shields.parquet` | battlefield shields still present in AdvancedEnergyShieldSystem | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `terrains.parquet` | dynamic battlefield terrain in RangeItemSystem, with its unit applications | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `statistics.parquet` | the build's damage and kill counters for the fight so far | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `formations.parquet` | every formation's experience | `S(1)..S(n)` | Parquet + Zstd level 6 |
| `events.parquet` | ordered discrete events between adjacent snapshots | `E(1)..E(n)` | Parquet + Zstd level 6 |
| `instrument/<channel>.parquet` | one [instrument channel](#instrument-channels), outside the hash | the ticks it has rows for | Parquet + Zstd level 6 |

The ZIP layer stores; compression is the Parquet pages' Zstd. The per-tick
tables flush a row group every 1024 logical ticks, with at most 1,000,000 rows
in one row group. State tables sort by `(tick, object_id)` and events by
`(tick, ordinal)`.

The timeline is:

```text
T(t) = { S(t), E(t), tick_hash(t) }, 1 <= t <= n
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
replay and `mechcore verify`. It does not enter the hash.

## Ticks and scene context

`ticks.parquet` is the container index. It carries the format identifier, the
DurableContext, the result hash, and the per-tick hashes from `T(1)`.

```text
tick       : UINT32 required
tick_hash  : FIXED_LEN_BYTE_ARRAY(32) required
```

The legal `tick` sequence is exactly `1..=tick_count`. `tick_hash` covers that
tick's complete `S(t)` and `E(t)`, and `result_hash` covers every `tick_hash` in
order. Both are defined under [The hash](#the-hash).

### File metadata

Parquet key-value metadata keys and values are both UTF-8 strings.

| Key | Data | Meaning |
| --- | --- | --- |
| `format` | exactly `0.17.0` | the logical and physical contract version |
| `producer` | `game` or `simulator` | what wrote the recording: the game, through the adapter, or the simulator |
| `game_build` | non-empty UTF-8 | capture provenance; the adapter reads `UnityEngine.Application.get_version()` |
| `durable_context` | canonical JSON | the context `D` that holds steady for one round |
| `hash_profile` | exactly `mcfr-content-0.7.0` | the hash definition, named for the domain strings it uses |
| `result_hash` | 64 lowercase hex digits | ordered digest of every `tick_hash`; what regression compares |
| `tick_count` | canonical decimal `u32` | logical ticks recorded, counting from `S(1)` |
| `terminal_tick` | canonical decimal `u32` | the confirmed final logical boundary, equal to `tick_count` on a continuous timeline |

### DurableContext

| Field | Type and data | Meaning | Native source |
| --- | --- | --- | --- |
| `logic_step` | `Rational<u32>`, both parts above 0 | seconds per logic advance | the adapter fixes `1/20` |
| `time_units_per_second` | `u32 > 0` | native discrete time density | the build fixes `2000` |
| `combat_round` | `u32 > 0` | the combat round | `CurrentMatch.get_RoundCount()` |

`match_seed` is not written to `ticks.parquet`. The writer still validates the
embedded layout against the caller's seed, and the reader recovers the public
`DurableContext.match_seed` from the canonical `layout.yaml.seed`. `game_build`
describes the capture source as its own file metadata.

`producer` is the one thing a recording says about who made it: `game` for
a recording the adapter captured, `simulator` for one the simulator wrote.
Both write the same timeline for the same fight, so it stays outside the hash,
as `game_build` does, and two recordings of one fight from the two producers
share every hash. A reader uses it to tell evidence of what the game does from
a statement of what the simulator computed.

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
| `turret_rotation` | `INT64 nullable` | turret facing, Q32.32 raw; null for a unit without a body | `FightMech.mechBody`'s FightTransform `GetRotationInt()` |
| `velocity` | `QPlanar required` | current velocity on the ground plane, world `x` and `z` | MotionController native velocity, whose vertical part the build always sets to 0 (the capture refuses anything else) |
| `motion_state` | `UINT8 required` | idle, moving, attacking, stopped, transitioning; [below](#what-a-unit-is-directed-at) | native motion state machine mapping |
| `mech_lock_target` | nullable `ObjectRef` | what the unit's body is directed at; [below](#what-a-unit-is-directed-at) | `FightMech.lockTarget` |
| `collision_radius` | `INT64 required` | collision radius, Q32.32 raw | `FightMech.GetRadius()` |
| `life` | `GaugeI32 required` | current and maximum life | `GetLife()` / `GetMaxLife()` |
| `active` | `BOOLEAN required` | current activation | `get_IsActive()` |
| `targetable` | `BOOLEAN required` | whether it is a legal target now | `IsValidTarget(visibility=0)` |
| `visibility` | `UINT8 required` | native visibility state | `GetVisibility()` |
| `status_mask` | `UINT64 required` | four native booleans | below |
| `modifiers` | required sparse list | every non-zero correction on the unit, its skills and its buffs | below |
| `personal_shield` | required struct | the unit's own energy shield | below |
| `weapon_aims` | required list | per-weapon channel state across main and sub skills | below |
| `derived` | required struct | the numbers the fight reads, after every correction | below |

### `derived`

The modifiers above say what was **written onto** a unit; this says what
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
and the divergence shows up in these fields first, which is what they are
for.

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

`layouts/technology-interval*.yaml` measured the composition
rule's order through this field without ever locating that zero: three fixtures
pin the line and the fourth is read against it.

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
their aggregate effect through the `buff` modifiers instead.

### `modifiers`

```text
channel    : UINT8 required    (0=buff, 1=mech_float, 2=mech_float_rate, 3=mech_int,
                                4=skill_float, 5=skill_float_rate, 6=skill_int)
skill_slot : UINT16 nullable   the skill's index in GetSkills(), on a skill channel only
field      : UTF8 required     the native member in snake_case, as the build spells it
part       : UINT8 required    (0=value, 1=add, 2=reduce)
value      : INT64 required    Q32.32 raw for a float or rate, the integer for an int
```

A unit's modifiers are one sparse list of what the build holds as non-zero,
strictly ascending by `(channel, skill_slot, field, part)`. A field no content
sets costs nothing, and a member the build adds is recorded without a change to
the format.

The six `DataSet` channels are keyed by the build's own enums, and the adapter
reads every member they define, found by enumerating each enum when it starts:

| Channel | Native enum | Read with |
| --- | --- | --- |
| `mech_float` | `MechDataChangeFloat` | `FightMech.GetDataFloat` |
| `mech_float_rate` | `MechDataChangeFloatRate` | `GetDataFloatAddRate` / `GetDataFloatReduceRate` |
| `mech_int` | `MechDataChangeInt` | `FightMech.GetDataInt` |
| `skill_float` | `SkillDataChangeFloat` | `FightSkill.GetData` |
| `skill_float_rate` | `SkillDataChangeFloatRate` | `GetDataFloatAddRate` / `GetDataFloatReduceRate` |
| `skill_int` | `SkillDataChangeInt` | `FightSkill.GetData` |

A member's name is the enum member's in `snake_case`, spelled as the build
spells it: `CBLifeRecoveryRate` is `cb_life_recovery_rate`, and
`DamageChagneRateGround` keeps its typo. The skill channels are read for every
skill `FightMech.GetSkills()` returns.

A rate is two parts. `add` is the sum of the enhancements, and `reduce` is what
the impairments take off: the native reduce getters return the remaining
multiplier, and MCFR stores

```text
reduce = 1.0_q32_32 - native_reduce_factor
```

Both are non-negative. A float or an int is one signed `value`.

The `buff` channel is the aggregate getters on `FightMech.GetBuffManager()`,
over every live buff, kept apart from the `DataSet` channels so which store an
effect came from stays attributable:

| Field | Parts | Native source |
| --- | --- | --- |
| `move_speed_rate` | add, reduce | `GetMoveSpeedChangeAddRate/ReduceRate` |
| `damage_rate` | add, reduce | `GetDamageChangeAddRate/ReduceRate` |
| `attack_interval_rate` | add, reduce | `GetAttackIntervalChangeAddRate/ReduceRate` |
| `extra_attack_interval_rate` | add, reduce | `GetExtraAttackIntervalChangeAddRate/ReduceRate` |
| `amplify_damage_rate` | add, reduce | `GetAmplifyDamageAddRate/ReduceRate` |
| `attack_range_rate` | add, reduce | `GetAttackRangeAddRate/ReduceRate` |
| `extra_attack_range_rate` | add, reduce | `GetExtraAttackRangeAddRate/ReduceRate` |
| `move_speed_value` | value | `GetMoveSpeedChangeValue` |
| `attack_range_add_value`, `attack_range_reduce_value` | value | `GetAttackRangeAddValue`, `GetAttackRangeReduceValue` |
| `extra_attack_range_add_value`, `extra_attack_range_reduce_value` | value | `GetExtraAttackRangeAddValue`, `GetExtraAttackRangeReduceValue` |

The buff's value getters are signed: a recorded round has shown
`GetAttackRangeReduceValue` at -20. `amplify_damage_rate` is the incoming-damage
multiplier applied to this unit. A buff's application, duration and expiry are
observed as changes to `status_mask` and the `buff` channel across consecutive
snapshots, because the format records state rather than buff objects.

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
modifiers leave out.

### What a unit is directed at

Three fields answer three different questions about a unit's intent, and a
reader who takes any one of them for another misreads the fight.

| Field | Answers | Owned by |
| --- | --- | --- |
| `mech_lock_target` | what the unit's **body** is directed at | the mech |
| `weapon_aims[].attack_target` | what each **weapon channel** fires at | the skill that owns the channel |
| `motion_state` | whether the body is travelling, holding to attack, idle or stopped, or between two of those while a move ability runs | the mech's motion state machine |

**`mech_lock_target` is the body's target.** It is what the mech's own search
found, or for a grouped skill the target it most recently allocated to one of
its slots: the unit moves toward it while `moving`, and a unit with a body keeps
its body facing it while `attacking`. A slot is allocated a target and then
fires at whatever stands in the way of it, so an object in the line of fire is
never what the lock reports. It is not a statement about what is being
shot at, and it is null when the mech holds no target.

**`attack_target` is the weapon's target.** It is what the owning skill fires
that channel at, and it may differ from `mech_lock_target`: a skill hands its
weapons an object standing in the line of fire while the mech keeps its lock,
and the channels of one grouped skill may each hold a different target. A unit
without a body has no facing of its own apart from its weapon's, so its
`body_rotation` follows its attack target rather than its lock.

**`turret_rotation` is where a unit with a body aims.** Such a unit carries
its turret, `FightMech.mechBody`, on the chassis: `body_rotation` stays where
the chassis faces while `turret_rotation` turns toward the attack target at
the unit's rotate speed, and the attack angle is measured from it. A Fortress
standing still at 0.68° fires at a Crawler 32° off once its turret has turned
within 20° of it. A unit without a body has no turret, and the field is null.

**`motion_state` follows the attack target, not the lock.** `attacking` means a
weapon's attack target is within reach and the body has stopped for it; `moving`
is the only state in which the body travels, and it travels toward
`mech_lock_target`. A unit can therefore be `attacking` while its lock is out of
reach, whenever something it can shoot stands in reach in front of it.

A capture records all three as the native objects report them and derives none
from another, which is what lets a reader compare them. The hash reads all
three, so a fight whose units move and hit alike but disagree in them is a
different fight.

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
| `target` | nullable `ObjectRef` | the current target | `FightProjectile.GetTarget()` |
| `cached_target_position` | `QVec3 required` | the target position the projectile cached | `GetTargetInfo().GetPosition()` |
| `cached_target_radius` | `INT64 required` | the target radius it cached | `GetTargetInfo().GetRadius()` |
| `life` | `GaugeI32 required` | current and maximum life | `GetLife()` / `GetMaxLife()` |
| `spawn_containing_shields` | required `list<ObjectRef>` | the shields its own hit test considers that already contained it at creation, by Shield ID | `ProjectileController.inEnergyShields` |

`owner` and `target` may reference objects that have already left the current
snapshot. A reference identity stays valid for the whole recording.

A live projectile has no facing and no release flag here. It never rotates
(`GetRotationInt()` is always 0), and `isReleased` is set only by the pool's
reset on the way back, so a live projectile always reads false; the capture
refuses a build where either is otherwise. The fact of firing is the
`projectile_released` event.

`spawn_containing_shields` is the projectile's birth exemption: the shields it
can never hit because it was born inside them. `ProjectileController.Init`
fills it once, after the projectile has its muzzle position, with
`GetInEnergyShields`: the shields of the groups the projectile's
`GetEffectTargetType()` names (its own team for `Self`, the opponents for
`Opponent`, all for `Both`) whose radius contains that position. The hit test,
`IsProjectileHitEnergyShield`, rebuilds the shields containing the projectile
now and takes the first one not in the list, and nothing edits the list until
the projectile returns to the pool: the exemption lasts its whole life, not
until it leaves the shield.

So the list is non-empty only when a projectile is fired from inside a shield
its own hit test looks at, which for a damaging shot means a shooter standing
inside an enemy's dome. It is empty in nearly every row. Every element must be
`ObjectKind::Shield`.

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
data source makes the adapter fail closed.

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

## Statistics

`statistics.parquet` holds `BattleStatisticManager`'s current round at each
snapshot: one row per recorder, strictly ascending by
`(tick, team_id, recorder, recorder_id)`. `FightController.OnActorHitted`
updates it inside the logic tick, so it is fight state, though nothing in the
fight reads it back.

| Column | Parquet type | Meaning |
| --- | --- | --- |
| `tick` | `UINT32` required | the snapshot |
| `team_id` | `UINT32` required | the team whose dictionary holds the entry, or for a unit counted alone the side it serves |
| `recorder` | `UINT8` required | `0=formation` (`MechTeam`), `1=construction` (`FightConstructionCombination`, or a construction outside one), `2=unit` (a unit with no `MechTeam`: a summon, or a mind-controlled unit) |
| `recorder_id` | `UINT64` required | the formation's `formation_id`; a construction's lowest `building_id`; a unit's `unit_id` |
| `damage` | `INT32` required | `DamageMax`: each hit's damage after every mitigation, before it is held to what the target had left |
| `damage_real` | `INT32` required | `DamageReal`: the life, or personal shield energy, the hits took |
| `kills` | `INT32` required | `KillCount`: hits after which the target was no longer alive |
| `damage_taken` | `INT32` required | `DamageTaken`: what the recorder's members were hit for, raised by what increases damage taken and before anything reduces it |

A hit credits the recorder of its `HitDamageInfo.sourceSkillOwner` and charges
the recorder of its target. A unit's recorder is its formation; a
construction's is its construction group; a tower is no recorder, so it earns
nothing and is charged nothing, though a hit on it credits its attacker. A hit
with no skill owner, such as ground fire, buff damage over time, a commander
skill or an air drop, credits nobody. A target already dead is skipped before
anything counts. Every formation and construction group has its row, at zero,
from deployment, and a formation that dies keeps it. A summon has no formation
and is counted alone, from the first hit it counts; so is a mind-controlled
unit, in the round's temporary dictionary, under the side it serves.

The counters are the build's `int`, and each row restarts at zero with each
fight.

## Formations

`formations.parquet` holds every formation's experience at each snapshot, one
row per `MechTeam` the recording has numbered, dead or alive, strictly
ascending by `(tick, formation_id)`.

| Column | Parquet type | Meaning |
| --- | --- | --- |
| `tick` | `UINT32` required | the snapshot |
| `formation_id` | `UINT64` required | the formation, as `units.parquet` numbers it |
| `team_id` | `UINT32` required | its side, its first unit's original team |
| `experience` | `INT64` required | `MechTeam.expFloat`, FPoint raw; -1.0 until the formation first gains any |
| `max_experience` | `INT64` required | `MechTeam.maxExpFloat`, FPoint raw: the full bar gains stop at |

`ExpSystem` hands experience out inside the tick, as a kill happens, so it is
fight state. What a kill hands out and to whom
is [`docs/rules/unit_experience.md`](../../rules/unit_experience.md#what-a-kill-hands-out).
As the fight ends, `BattleSystem.OnFightOver` prunes each gain to a whole
number. A fight that ends before its time runs out is pruned inside its last
logic tick, so its last snapshot holds whole numbers; one that runs out of
time is pruned after its last tick, so its last snapshot holds the fractions
the cut removes.

## Events

`events.parquet` holds one row per event. Within one tick, `ordinal` increases
contiguously from 0. The whole table is strictly ascending by
`(tick, ordinal)`, and `tick` lies in `1..=tick_count`.

| Column | Parquet type | Meaning |
| --- | --- | --- |
| `tick` | `UINT32` required | the advance this event belongs to |
| `ordinal` | `UINT32` required | observation order within the tick |
| `type` | `UINT8` required | the event kind tag, the row number of the table below counting from 0 (`0=projectile_released` … `14=buff_removed`) |
| `object` | `ObjectRef` nullable | the subject |
| `source` | `ObjectRef` nullable | the direct source |
| `source_team_id` | `UINT32` nullable | the team of the source or subject at the event boundary |
| `target` | `ObjectRef` nullable | the direct target |

Every payload field below has a nullable column of its own, of the payload
field's type: `skill_slot` `UINT16`, `weapon_index` `INT32`, `position`
`QVec3`, `intercepted` `BOOLEAN`, `absorbed_by` `ObjectRef`, `amount` `INT32`,
`team_id` `UINT32`, `formation_id` `UINT64`, `unit_type_id` `UINT32`,
`previous_team_id` and `new_team_id` `UINT32`, `source_kind` `UINT8`
(`ShieldSourceKind`), `reason` `UINT8`, `terrain_type` `UINT8` (`TerrainType`),
`radius` `INT64` Q32.32, `buff_id` `UINT32` and `duration` `INT32`. A row sets exactly the fields its type carries and
leaves every other payload column null; a reader refuses a row that sets a
field its type does not carry or lacks one it requires.

| `type` | Required reference | Payload | Meaning |
| --- | --- | --- | --- |
| `projectile_released` | `object` | `skill_slot: u16\|null`, `weapon_index: i32\|null` | a projectile was created and joined ProjectileSystem, with its weapon channel; the two channel fields are both present or both null |
| `projectile_removed` | `object` | `position: QVec3`, `intercepted: bool`, `absorbed_by: ObjectRef\|null` | it left the system; `absorbed_by` names the battlefield shield that took it |
| `damage` | `target`; `object` is the projectile that carried it, if one did | `amount: i32`, `skill_slot: u16\|null` | one positive damage result on one actual target; an Actor uses the native `damageReal`. `skill_slot` is the dealing skill's index in the source's `GetSkills()`, null when no skill deals it |
| `unit_created` | `object` | `team_id: u32`, `formation_id: u64`, `unit_type_id: u32`, `position: QVec3` | a unit's lifecycle start |
| `unit_died` | `object` | `position: QVec3` | a unit's death boundary |
| `building_destroyed` | `object` | `position: QVec3` | a building's destruction boundary |
| `unit_team_changed` | `object` | `previous_team_id: u32`, `new_team_id: u32` | a unit changed side |
| `shield_created` | `object` | `team_id: u32`, `source_kind: ShieldSourceKind`, `position: QVec3` | a shield joined the full collection and took an identity |
| `shield_destroyed` | `object` | `position: QVec3`, `reason: ShieldDestroyedReason` | it left the full collection, ending its lifecycle |
| `terrain_created` | `object`, no `source` | `team_id: u32\|null`, `terrain_type: TerrainType`, `position: QVec3`, `radius: i64` | a terrain joined a controller item set and took an identity |
| `terrain_removed` | `object` | `position: QVec3`, `reason: TerrainRemovedReason` | it left the controller item set, ending its lifecycle |
| `terrain_converted` | `object` | `position: QVec3` | one terrain changed native type or ownership |
| `healing` | `target` | `amount: i32` | one positive recovery result |
| `buff_applied` | `target`; `source` is the actor that put it on, if one did, and `source_team_id` its side's | `buff_id: u32`, `duration: i32` | a buff put on the target, or put on again; `buff_id` is its data's `GetID()`, `duration` the ticks left on it once applied |
| `buff_removed` | `target` | `buff_id: u32`, `reason: BuffRemovedReason` | a buff taken off the target |

`projectile_removed` admits exactly three combinations. Native interception is
`intercepted=true, absorbed_by=null`. Absorption by a battlefield shield is
`intercepted=false, absorbed_by=ShieldRef`. Any other removal has neither.
`absorbed_by` may only reference a Shield.

`shield_destroyed.reason` is one of `0=energy_depleted`, `1=owner_destroyed`,
`2=round_end`, `3=scripted` or `4=unknown`. A producer writes a specific value only
where the native destruction entry point determines the cause.

`terrain_removed.reason` is one of `0=time_expired`, `1=round_expired`,
`2=grid_depleted`, `3=cleared` or `4=unknown`, under the same rule.

`buff_removed.reason` is one of `0=expired`, `1=removed` (a skill or
technology took it off by its data), `2=team_changed`, `3=technology_disabled`,
`4=cleared` (its actor was torn down, as a unit is when it dies) or
`5=unknown`.

A `terrain_created` object is fully determined by the terrain's identity, team,
type, position and radius. Causation from a projectile is observed jointly, from
a `projectile_removed` at the same tick and position together with the terrain
state change.

### Which events a producer can emit

The MCFR model and the event table implement every event above. What a given
producer can actually observe is narrower, and the adapter covers:

| Event | Native source |
| --- | --- |
| `projectile_released` | the nested trace from `ProjectileSystem.Create` to `AddProjectile`, which also resolves owner, target, skill slot and weapon index |
| `projectile_removed` | the ProjectileSystem destruction path, capturing final position and interception; `absorbed_by` is added when the same damage chain hits a shield |
| `damage` | `DamagePerformer.Perform` gives the provider attribution scope, each `FightController.OnActorHitted(HitDamageInfo)` gives the Actor target and `damageReal`, and the battlefield shield damage helper gives the per-shield actual result. The provider names the dealing skill: a `SkillDamageProvider`'s or `HitEffectControl`'s `fightSkill`, or a `FightProjectile`'s `dataSource`; a skill `GetSkills()` lacks answers for its `ParentSkill`. A death explosion, a kill explosion, a commander skill, an air drop, ground fire and buff damage over time deal for no skill |
| `unit_created` | `FightController.CreateMech(team, mech, position, rotation, createType, mechTeam, isRebirth)`, the funnel deployment, summons, spawns on death, production and air drops all pass. Only calls inside the logic tick are recorded, which leaves deployment out, and a unit rising from its death passes it again with `isRebirth` set and is not recorded. The position is the unit's once the call returns |
| `unit_died` | the `FightMech.OnDead` trace |
| `building_destroyed` | the `FightCrystal.OnDead` trace |
| `healing` | `FightActor.AddLife(value, isShowLifeBar)`, which units, towers and crystals heal through, and `FightConstruction.AddLife`. The amount is the life read after the call less the life read before it, since the gauge clamps at full life and the call returns nothing. Every heal shows the life bar; the two refills that do not, a unit rising from its death and one landing from a super deployment, are not recorded. No `source`: the call does not carry one |
| `shield_created` | first entry into the full `GetEnergyShields(fightGroup)` collection between adjacent sampling boundaries. Shields that join between the same two boundaries are written in identity order, which is the order the snapshot reads them in, team by team and each team's collection in its own order, not in the order of their native addresses |
| `shield_destroyed` | disappearance from that collection. The reason is read as `GroupAdvancedEnergyShieldManager.Destroy(FightEnergyShield)`, the one method that takes a shield out of it, begins: spent energy is `energy_depleted`, since only damage empties a shield before destroying it; a shield with an owner is `owner_destroyed`; any other is `scripted`. A shield leaving as the round ends goes after the last recorded tick, so `round_end` is not written. Shields that leave between the same two boundaries are written in identity order |
| `terrain_created` | first entry into the owning `RangeItemController.GetItems()` collection between adjacent boundaries |
| `terrain_removed` | disappearance from that collection. The reason is read as `RangeItem.Remove()` begins: inside `RangeItemController.OnExitFight` it is `round_expired`, fire included, whose controller ends it with the fight whatever its round count; otherwise `IsTimeOver()` is `time_expired`, and a grid whose cells are all cleared is `grid_depleted`. Oil burnt into fire and oil cleared by a skill are `unknown`, and `cleared` is not written |

`unit_team_changed` and `terrain_converted` have a shared schema, a canonical
form and reader and writer support, for a producer able to observe the
evolution directly.

`unit_created` and `healing` are recorded without a fixture that exercises
them: no pinned fight creates a unit inside the tick or heals one, and which
summons and heals reach the two entry points is checked as each mechanism is
researched.

A buff enters and leaves through three `BuffManager` methods, and the adapter
hooks each. `buff_applied` comes from `AddBuff(Buff)`, the private method a new
buff joins its actor's list through once `Buff.Init` has filled it, and from
`Buff.Reset`, which the same buff put on again runs instead of adding a second;
the event names the running buff's row, which a merge does not change.
`buff_removed` comes from `RemoveBuff(Buff)`, read before it returns the buff
to its pool. Its reason is the method it runs under: `RemoveBuff(IBuffData)`,
`RemoveBuffEffect`, `ClearSelfResourceBuffByDisableTech` or `Clear`, or else
`BuffManager.Update` finding the buff's time run out. Buffs a fight starts
with, put on before its first tick, have no event.

What a buff does is still on the unit state track: boolean state in
`status_mask` and aggregate numeric corrections in the `buff` modifiers. The
pinned fights exercise the buff a tower's loss writes, and nothing else; which
other buffs reach these methods, and whether each removal reason is the one
the build means, is checked as each mechanism is researched.

## Instrument channels

A channel is what a study asked to see of how the fight did what the
recording says it did: a skill's state machine, every call of a decision
method. It rides in the recording as `instrument/<channel>.parquet`, and
the hash does not read it, so asking for a channel never changes a pin and a
channel's schema can change without a format version.

- A channel's rows are one Rust type, and its Arrow schema is traced from that
  type (`serde_arrow`, enums without data as strings). The member's first
  column is `tick`, `u32`, the tick the row was observed on; the type's own
  fields follow and never include `tick`.
- A channel a recording asked for is published even if it observed nothing, as
  a member with no rows. A channel it did not ask for is absent, and a reader
  answers `None` for it rather than an empty list.
- Rows are written as the fight is, in the row groups of the state tables, not
  held until the recording finishes.
- A channel's volume per tick is bounded by a constant times the number of
  entities: what it records per entity has an upper bound, and pairs of
  entities are not recorded.

The channels the Adapter records are `target_refs`,
`skill_attackable_checker`, `group_slots`, `target_search`, `target_candidate`,
`rvo_solve`, `rvo_neighbour`, `rvo_vo`, `unit_pose`, `projectile_reach` and
`control_progress`
([adapter.md](../adapter/adapter.md#record_replay_round)).

## Writing, reading and validation

The writer's public lifecycle is:

```text
create(producer, game_build, context, layout_yaml)
append_tick(S(1), E(1))
...
append_tick(S(n), E(n))
finish()
```

`append_instrument(rows)` may follow any `append_tick`, and adds rows of one
channel to the tick last appended.

It creates temporary members and a `.zip.part` beside the target, completes the
Parquet footers and the ZIP envelope, reopens the result with
`McfrReader` to re-check its structure and persisted hash metadata, and
publishes through `persist_noclobber`. The target path is required to be free at
creation, and publication refuses to overwrite.

`in_memory(producer, game_build, context, layout_yaml)` takes the same checks
and keeps each canonicalised tick instead of storing it, and
`finish_in_memory()` hands the timeline back with its hashes and no file. A
reader of one fight reads it through `Recording`, which `McfrReader` and that
timeline both answer: the embedded layout, the producer, the hashes, the
terminal tick, and each tick's state and events. The simulator keeps a fight
this way when its reader is the only use it has, as `verify` and
`convert --to fight` read a layout's fight.

Before each write, the WorldSnapshot is canonicalised into the order under
[Normal form](#normal-form), then checked for object IDs, list order, state bits
and modifier constraints. One failed partial write poisons the writer, and only
a successful `finish()` publishes a file.

### What a reader validates

On opening a container, a reader verifies:

- the ZIP member set, the STORE method, ZIP64 readability and member uniqueness;
  a member outside the fixed set is admitted only as `instrument/<channel>.parquet`
  with a channel name of lowercase letters, digits and underscores;
- `layout.yaml` as UTF-8, its shared structure, its canonical representation,
  and its round's agreement with the DurableContext;
- the six Parquet schemas, their required and nullable structure, and Zstd
  column compression;
- `producer`, `game_build`, the DurableContext canonical JSON, metadata types
  and the format identifier;
- tick contiguity, state table ordering, event ordering and ordinal contiguity;
- ObjectRefs, enum tags, initial identity order, list order, `status_mask`
  reserved bits and modifier components;
- the contiguity, width and encoding of the tick hash column, the encoding of
  the result hash and its profile, and that the result hash is the digest of
  the tick hash column.

A reader trusts the tick hashes the recording persisted.
`McfrReader::open()` does not rebuild `S(t)` and `E(t)` tick by tick to
re-derive them, and does not recompute the timeline. `first_divergence()`
compares the persisted `tick_hash` values directly. Reading one tick whole,
`tick(t)`, rehashes its state and events and refuses a tick whose stored
`tick_hash` does not match them, so a caller that has located a divergence
reads content the hash vouches for.

A legal recording has at least `T(1)`, no tick 0 state or event, and ends
completely at `terminal_tick`.

A channel is decoded when it is read, not when the recording is opened: its
rows are checked against the row type the reader asks for, and each tick
against `1..=tick_count`.

## Common types and enum tags

Space, rotation and native fixed-point modifiers use signed `i64` raw bits:

```text
real_value = raw / 2^32
```

`QVec3 = { x: i64, y: i64, z: i64 }`, three required `INT64` children in
Parquet, and `QPlanar = { x: i64, z: i64 }` the same on the ground plane.
Position, radius and bounds are metres, velocity metres per second, and
rotation degrees.

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
| `MotionState` | `0=idle`, `1=moving`, `2=attacking`, `3=stopped`, `4=transitioning` |
| `Visibility` | `0=normal`, `1=disappear`, `2=stealth`, `3=hide` |
| `ShieldSourceKind` | `0=contraption`, `1=commander_skill`, `2=owner_advanced`, `3=spawned_temporary` |
| `ShieldRoundPolicy` | `0=destroy_at_round_end`, `1=reset_to_max`, `2=retain_state` |
| `TerrainType` | `0=fire`, `1=oil`, `2=fog`, `3=acid`, `4=recovery_zone`, `5=fog_sand` |

## Normal form

Identity is what makes two recordings of one fight the same recording, so
every namespace numbers its objects by a rule that depends on the scene rather
than on the pointer that happened to be observed first.

Format `0.17.0` uses `team_zx_sequential_v1`.

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
`match_seed + unit_index`, where `unit_index` is the layout's `index` and not
the declaration position: a sold unit leaves a gap in it.

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
`modifiers` sort by `(channel, skill_slot, field, part)`, `weapon_aims` by
`(skill_slot, weapon_index)`, and a projectile's `spawn_containing_shields` by
Shield ObjectRef.

**States and events in one frame.** `E(t)` is what hooks observed directly
while advancing from `S(t-1)` to `S(t)`, and `S(t)` is the authoritative
complete state after that advance finishes. A death or destruction event may
therefore share a tick with the object's absence from `S(t)`, and that is not a
contradiction.

## The hash

BLAKE3 throughout. Every digest begins with a fixed prefix and a domain, and the
prefix, the domain and each later input segment are encoded as:

```text
LE_u64(byte_length) || bytes
```

The fixed prefix is `mechcore.mcfr.canonical\0`. Integers are little-endian
two's complement raw bits at the stated width. Every public digest is 64
lowercase hex digits, and the Parquet tick column holds the raw 32 bytes.

State and events are first encoded as canonical JSON: UTF-8, object keys sorted
recursively, compact encoding, and the array order the schema defines. The hash
covers every `S(t)` and `E(t)` field. It carries neither the layout, nor the
DurableContext, nor any other file metadata.

```text
tick_hash(t) = H_content-tick-0.7.0(LE_u32(t), JSON(S(t)), JSON(E(t)))
result_hash  = H_content-result-0.7.0(
    LE_u32(tick_count),
    tick_hash(1)..tick_hash(n)
)
```

The definition is older than the format: its domain strings and formula have
not changed since format 0.7.0, and `hash_profile`, `mcfr-content-0.7.0`, names
it by that version. A format change that leaves `S(t)` and `E(t)` encoding the
same leaves every hash where it was. A change to the definition itself is a new
profile and new domain strings, never an edit in place.

`mechcore diff` and `mechcore verify` decide `equal` and the
first divergence from this hash. `diff` also says where two recordings
differ field by field, which a hash cannot:
[cli.md](../mechcore/cli.md#diff) defines its field groups.

## Admission to the hash

The hash is what a simulator is scored against, so what it reads is a decision
of its own. `crates/mcfr/hashed-content.txt` lists every field, variant, enum
value and `status_mask` bit that `S(t)` and `E(t)` carry, and a test holds the
types to it: a change to what the hash reads is a change to that file.

**What the hash reads only grows.** Every fight it has compared was compared
on all of it, so taking a field out, or loosening what one means, weakens every
comparison already made. Two changes are not removals: re-encoding the same
information, when the old encoding and the new each determine the other, and
correcting a field a producer read wrong.

**A quantity enters when it meets all four conditions.**

1. **It settles the fight or names a cause.** It feeds what a round settles:
   core damage, the statistics, experience, a score. Or it names what caused a
   change the timeline already records, such as the skill, projectile or
   landing behind a damage, a buff or a new unit. A quantity that only helps
   find where two fights part is an [instrument channel](#instrument-channels).
2. **It is the game's own.** A producer reads it at a build member, as every
   field above names one. A value computed from other recorded values is not
   stored.
3. **It is one value per fight, whoever writes it.** A recording made from a
   replay and one made from its layout agree on it, and a simulator of the
   same fight writes it too.
4. **Its volume per tick is bounded** by a constant times the number of
   entities.

**What an admission moves.** A new event kind, enum value or `status_mask` bit
appears only in the fights where it happens, and moves only their pins. A new
field of a state object or of an event, or a new state collection, is written
in every tick, null or empty where nothing has it, and moves every pin. A new
value of a field already admitted, such as a modifier's `field` naming another
native field, is not an admission.

## Physical encoding

- ZIP member order is fixed: `layout`, `ticks`, `units`, `projectiles`,
  `buildings`, `shields`, `terrains`, `events`, then the instrument channels by
  name; a table without rows is left out of the order.
- No Parquet member embeds its Arrow schema: every table's schema is this
  document's. `ticks.parquet`'s metadata is Parquet file key/value metadata.
- Column statistics are kept per column chunk; no page statistics or page
  index is written.
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
  required references and the field set row by row.

### Native modifier mapping, by example

Sticky oil's slow lands in BuffManager's aggregate `move_speed_rate`. The
incoming-damage change from a photon projection lands in `amplify_damage_rate`,
while the present value of `IsInvincible()` lands in `status_mask.invincible`. A
sub-skill such as the Sabertooth technology's secondary cannon uses its own
`skill_slot` in both `modifiers` and `weapon_aims`, and a round's
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
  state, in the Parquet schema, or in the hash. A unit's `body_rotation` and
  a weapon pose's `rotation` are unaffected.
- **`system_order` for shields.** The native full-collection index is used for
  initial ID assignment and the adapter's consistency check and never reaches
  the file, because `shield_id` and `active_order` already carry everything a
  reader needs.
- **The layout in the hash.** `layout.yaml` does not enter the hash. It is the
  scene's input, not its outcome, and a
  recording's identity is what happened rather than what was asked for.
- **Buff objects.** There is no buff track. A buff is observable as the change
  in `status_mask` and the `buff` modifiers between adjacent snapshots, so the
  format stores state rather than the engine's internal buff instances.

## Unresolved

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

**Should the format define events no producer emits?** `unit_team_changed`
and `terrain_converted` have schema, canonical form, and reader and writer
support, and no producer observes them today.
Defining them keeps a later producer from inventing an incompatible shape;
defining them also means a reader cannot tell an event that never happened from
one nobody can see.
