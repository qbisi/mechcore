# Dynamic terrain

How the build creates, shapes and ends the in-fight area effects that
`RangeItemSystem` manages.

Map decoration, deployment footprints and unit movement collision are not area
effects and are not here. Neither are the specific radii, effect clocks and
lifetimes an individual source produces: those are per-source values, each
resting on its own capture, and none of them is established below.

## Objects and controllers

The authoritative object is a `RangeItem`. `RangeItemSystem` provides one
`RangeItemController` per `RangeItemType`, and
`RangeItemController.GetItems()` returns the set that currently exists. That set
is authoritative: an object is in play exactly while it is a member.

`RangeItemType` has six values, which a recording spells `fire`, `oil`, `fog`,
`acid`, `recovery_zone` and `fog_sand`. Four are the deployable battle skills, the incendiary bomb being
`fire`, the sticky oil bomb `oil`, the smoke bomb `fog` and the acid bomb
`acid`. What `recovery_zone` and `fog_sand` do is not established.

## How a terrain is created

A battle skill's ground impact reaches the item set through one entry point:

```text
battle skill ground impact
  -> RangeItemEffectController.PerformEffect(position, index)
  -> RangeItemSystem.AddItem(...)
  -> the matching RangeItemController's item set
```

`PerformEffect` calls `AddItem` directly, which is what fixes the entry point
rather than merely suggesting it.

A unit technology projectile also creates a `RangeItem`. Its path is not closed
on a static call edge, and it is reconstructed from position and timing instead:
a projectile's removal and a terrain's creation coincide in tick and position.
That is a correspondence, not a proven call chain, and it cannot distinguish the
projectile from another cause acting at the same place on the same tick.

## Which units a terrain affects

`RangeItemController.Update()` calls `UpdateAffectedActorChange()` to refresh the
affected set, and that pass contains `FightActor.IsValidTarget` and a
two-dimensional range check. Each controller's own effect is applied through a
virtual dispatch, so which effect runs is not decidable from the static call
graph.

An affected relation may carry a periodic clock, `{elapsed, duration}`, which a
repeating effect needs and a continuous correction such as a sustained slow does
not. In fire and acid controllers as recorded, `elapsed` advances once per
logic advance and wraps to zero after `duration`. These counters therefore use
the recording's `DurableContext.logic_step`, not its separate
`time_units_per_second` scale; [mcfr.md](../spec/mcfr/mcfr.md) defines the file
fields.

## A battle skill's terrains

A terrain battle skill (Incendiary Bomb, Sticky Oil Bomb, Smoke Bomb, Acid
Blast) is a line of its row's `subEffectCount` sub-effects, placed and timed as
a line strike's are ([`battle_skill.md`](battle_skill.md)), and each leaves one
terrain of the row's `subEffectRange` where it lands, of the skill's side, on
the tick it lands. Smoke Bomb's seven fogs of 50 metres land every four ticks
from tick 63.

**Each tick**, after `MineSystem` and before `FightCoreSystem`, each
controller holding terrains updates:

1. Each terrain, last first, ages, and one whose lifetime is up goes,
   `time_expired`. A fire burns its row's `lifeTime`, Incendiary Bomb's 700
   ticks, counted from 1 on the tick it lands; a fog has no lifetime and
   stays the round.
2. It finds who stands in its terrains. Side by side, blue's first, it asks
   the side's unit tree for the units under its own tree's node, and takes
   each terrain against each unit found, in the tree's query order: a ground
   unit, alive and not hidden, whose edge the terrain's range reaches in the
   plane, by `FPoint`'s tolerant comparison, is found in it. Both sides' units
   are, whichever side the terrain is.
3. Every affected unit not found anywhere leaves, last first. Every unit found
   that is not affected enters the first terrain it was found in, and one
   already affected stays in its own, even when that one no longer reaches
   it. A unit that dies leaves as it dies, the rest keeping their order.
4. A controller with a period, a fire's, counts one for every affected unit,
   last first, from zero as it entered; one whose count reaches the period
   counts it back off and takes the effect again.

**A fog** writes its row's `attackRangeChangeRate`, Smoke Bomb's `-0.35`, on
every skill of a unit that enters it whose attack is not a melee one, as the
skill's `attack_range_rate`, and takes it back as the unit leaves. The
controller is the modifier, so a unit carries one fog's rate however many it
stands in. A unit with a melee attack enters and is left alone. The range is
then the description's times the rate in `FPoint`: a Sledgehammer's 95 metres
are 61.75.

**A fire** deals `Config.groundFireDamage`, 54, to a unit as it enters and
again every `fireAttackInterval`, four ticks, while it stays: a hit with no
owner, under the fire's side, scaled by the unit's rate on damage taken and
taken first by its own shield, as any hit is. A kill it makes counts for the
dead unit's enemies.

**An oil** writes its row's buff, Sticky Oil Bomb's `400001`, a slow of 0.55
for 20 ticks, on a unit as it enters, by no object under the oil's side, and
again every 19 ticks it stays: its controller's period is the buff's duration
in ticks less one, never under one (`BuffItemController.Add`). The slow runs
while the unit stands in the oil, and runs out after it leaves.

**As the fight ends**, after the buffs the units still run are cleared, every
unit leaves its terrain, which takes back what it did, and every terrain
counts a round. One that has stood its row's `effectDuration` goes,
`round_expired`, and so does every fire, whose controller ignores rounds; the
rest stand into the next round. A fog stands one round and goes; an oil
stands two and is still there in the last state, with one round left. A
terrain of more than one round reads its rounds left as `remaining_rounds`.

A recording finds a terrain made or gone by comparing one snapshot's terrains
with the last, so `terrain_created` and `terrain_removed` are the last events
of their tick, after every other: the made, then the gone, each in identity
order.

The simulator refuses a terrain in a fight with a battlefield shield, which
turns it to a grid; two kinds of terrain in one fight, whose controllers'
order is not read; and a controller's twentieth terrain, which splits its tree.

## Circles and grids

An ordinary terrain is a `position` and a `radius`, stored as Q32.32 raw
integers, with no grid.

When space must be subtracted from that circle, the object enters grid mode and
carries an origin, a size and one bit mask per row. The native
`GridBlockInt.grids` encodes x as columns with y in the high bits; a recording
stores the transpose, y as rows with x in the low bits. The transposition
happens at the read boundary, so a consumer never sees the native layout.

## Shields subtract from terrain

`RangeItemEffectLayerGrid.GenerateGrid` calls
`AdvancedEnergyShieldSystem.GetActiveEnergyShields`,
`FightEnergyShield.GetFightTransform` and `FightEnergyShield.GetRadius`, then
`GridBlockInt.TryDisableGrid`, `RefreshMask` and `Sync`. Grid generation
therefore reads the live battlefield shields and subtracts the cells that fall
inside one.

A battlefield shield takes part in projectile and area-effect intersection only.
It is not an RVO agent, not a movement blocker, and not a layout deployment
footprint.

## Identity and lifecycle

A terrain's identity follows its native pointer within one recording, and a
pointer that leaves its controller's set is tombstoned so that nothing later
inherits the identity.

A recording confirms a creation or a removal from the difference between two
adjacent membership samples. That method establishes the fact and not the cause,
which is why a removal reason is `unknown`. An instance created and removed
inside a single logic advance is invisible to it entirely.

A terrain changing native type is expressed as a removal and a creation at the
same position, with two separate identities, rather than as a conversion of one.

## Two ways to read a retained terrain

A cross-round terrain, which a layout states as a standing `sticky_oil_bomb`
entry of its `battle_skills`, can be recovered from the replay file alone or
from the game, and the two check each other without depending on each other.

The file path extracts the `BattleRecord` XML from the GRBR's BinaryFormatter
wrapper, reads the named `PlayerRoundRecord`, and decodes `activeState` and the
flat `gridInfo/ByteMask` into zero-based active indices and canonical grids,
keeping the original control points.

The game path enumerates the restored objects from
`RangeItemController.GetItems()` before the fight, groups them by shared
provider, recovers the control points from the surviving endpoints, and exports
the grids by `RangeItem.Index`. After a replay restore the manager no longer
holds the provider's release data, so this path fails closed when an endpoint is
missing. The file path has no such requirement, and only the game path observes
what a specific build actually restored.

`CommanderSkillManager.CalculateAttackPositions` generates the
interior centres from the stored control points, so only the endpoints need
storing. The method is private and absent from the runtime reflection table;
reproducing it requires the same native `FVector3` and `FPoint` operations,
because the fixed-point rounding is part of the result.

Restoring into a game computes those centres, then adds the objects in index
order. A non-empty grid must overwrite both the native `Queue<ByteMask>` and
`GridBlockInt.grids`, with an immediate read back. `Sync` cannot be used to
deserialise a final state: it only subtracts further cells from a grid that
already exists.

## Evidence

### Recorded

- A Smoke Bomb's fogs land along its line every four ticks, take ground units
  of either side whose edge they reach one fog at a time, hold a ranged
  attack's range to 0.65 of it, leave melee units alone, and go as the fight
  ends, taking the rate back: `tests/terrain/fights/smoke.yaml`,
  `tests/terrain/fights/smoke-both-sides.yaml`.
- An oil writes its slow as a unit enters and every 19 ticks it stays, and
  stands into the next round: `tests/terrain/fights/oil.yaml`.
- A fire hits a unit as it enters and every four ticks it stays, the units
  counting last first, burns out after 700 ticks, and its removal is the
  last event of its tick: `tests/terrain/fights/fire.yaml`,
  `tests/terrain/fights/fire-burns-out.yaml`.

### Read

- A controller ages its terrains, then finds who stands in them, then runs its
  periodic effects: `RangeItemController.Update`,
  `RangeItemController.UpdateItemStatus`, `RangeItem.IsTimeOver`.
- Who stands in a terrain is found side by side, the terrain tree's nodes
  against the side's unit tree, and an affected unit keeps its terrain:
  `RangeItemController.UpdateAffectedActorChange`,
  `FightQuadtree.GetInteractableNodes` (a node with items, against the
  generic tree's `IsInteractableRange` and `Query`),
  `FightCalculator.IsInRange2D`, `RangeItemController.OnActorEnter`,
  `RangeItemController.OnActorExit`, `RangeItemController.AddAffectedActor`.
- Every controller takes ground units of either side: `FogController.GetAttackTarget`,
  `GroundFireController.GetAttackTarget`, `BuffItemController.GetAttackTarget`,
  `RangeItemController.GetEffectTargetType`.
- A fog rates every ranged skill's range, the controller its modifier:
  `FogController.PerformItemEffect`, `FogController.OnActorExit`,
  `SkillDataChangeFloatRate.AttackRangeRate`, `FightSkill.IsMeleeAttack`.
- A fire's lifetime is its own count, and its controller hits with the
  setting's damage every interval: `FightGroundFire.Update`,
  `FightGroundFire.IsTimeOver`, `GroundFireController.PerformItemEffect`,
  `Config.groundFireDamage`, `Config.fireAttackInterval`,
  `FightCalculator.PerformHitTargetEffect`.
- An oil's controller writes its buff, its period the buff's duration less
  one tick: `BuffItemController.PerformItemEffect`, `BuffItemController.Add`,
  `BuffSystem.AddBuff`.
- As the fight ends a terrain counts a round and goes when its rounds are
  over, a fire's at once: `RangeItemController.OnExitFight`,
  `RangeItem.AddRound`, `RangeItem.IsRoundOver`,
  `GroundFireController.IsIgnoreRoundDuration`.
- `RangeItemSystem` updates between `MineSystem` and `FightCoreSystem`:
  `FightController.AddModules`, `RangeItemSystem.Update`.

- A terrain is in play exactly while its controller's set holds it:
  `RangeItemController.GetItems`.
- There are six terrain types: `RangeItemType.Fire`, `RangeItemType.Oil`,
  `RangeItemType.Fog`, `RangeItemType.Acid`, `RangeItemType.RecoveryZone`,
  `RangeItemType.FogSand`.
- A battle skill's ground impact adds its terrain directly:
  `RangeItemEffectController.PerformEffect`, `RangeItemSystem.AddItem`.
- The affected set is refreshed each update from target validity and a
  two-dimensional range: `RangeItemController.Update`,
  `RangeItemController.UpdateAffectedActorChange`, `FightActor.IsValidTarget`.
- A grid subtracts the live battlefield shields:
  `RangeItemEffectLayerGrid.GenerateGrid`,
  `AdvancedEnergyShieldSystem.GetActiveEnergyShields`,
  `FightEnergyShield.GetRadius`, `GridBlockInt.TryDisableGrid`,
  `GridBlockInt.RefreshMask`, `GridBlockInt.Sync`.
- A line of centres is generated from its stored endpoints:
  `CommanderSkillManager.CalculateAttackPositions`.

### Not established

- **What acid does to a unit**, and a fire lit from oil.
- **`FogController.SelectBestTarget`**, which picks the stronger of two fogs
  and which no path of a fog's update reaches.
- **Any radius, effect clock or lifetime.** They vary by source, and two sources
  of one type can differ, so no value derives from a type.
- **The native grid's orientation.** That `GridBlockInt.grids` holds x as
  columns with y in the high bits is what the Adapter's read assumes and what
  recorded grids agreed with; the grid methods were not read for it.
- **`recovery_zone` and `fog_sand` behaviour.**
- **A type conversion within one identity**: whether a native producer for one
  exists.
- **A technology projectile's terrain.** Whether the projectile causes the
  terrain it coincides with, as opposed to being a reliable correlate of it.
