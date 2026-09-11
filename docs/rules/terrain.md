# Dynamic terrain

How build `1.11.1.3.2259` creates, shapes and ends the in-battle area effects
that `RangeItemSystem` manages.

Map decoration, deployment footprints and unit movement collision are not area
effects and are not here. Neither are the specific radii, effect clocks and
lifetimes an individual source produces: those are per-source values, each
resting on its own capture, and none of them is established below.

## Objects and controllers

The authoritative object is a `RangeItem`. `RangeItemSystem` provides one
`RangeItemController` per `RangeItemType`, and
`RangeItemController.GetItems()` returns the set that currently exists. That set
is authoritative: an object is in play exactly while it is a member.

Six native types exist: `fire`, `oil`, `fog`, `acid`, `recovery_zone` and
`fog_sand`. Four are the deployable battle skills, the incendiary bomb being
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

On build 2259 `PerformEffect` calls `AddItem` directly, which is what fixes the
entry point rather than merely suggesting it.

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
not. In the recorded fire and acid controllers, `elapsed` advances once per
logic advance and wraps to zero after `duration`. These counters therefore use
the recording's `DurableContext.logic_step`, not its separate
`time_units_per_second` scale; [mcfr.md](../spec/mcfr/mcfr.md) defines the file
fields.

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
`GridBlockInt.TryDisableGrid`, `GenerateMask` and `Sync`. Grid generation
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

A cross-round terrain in a layout can be recovered from the replay file alone or
from the game, and the two check each other without depending on each other.

The file path extracts the `BattleRecord` XML from the GRBR's BinaryFormatter
wrapper, reads the named `PlayerRoundRecord`, and decodes `activeState` and the
flat `gridInfo/ByteMask` into zero-based active indices and canonical grids,
keeping the original control points.

The game path enumerates the restored objects from
`RangeItemController.GetItems()` before the battle, groups them by shared
provider, recovers the control points from the surviving endpoints, and exports
the grids by `RangeItem.Index`. After a replay restore the manager no longer
holds the provider's release data, so this path fails closed when an endpoint is
missing. The file path has no such requirement, and only the game path observes
what a specific build actually restored.

Build 2259's `CommanderSkillManager.CalculateAttackPositions` generates the
interior centres from the stored control points, so only the endpoints need
storing. The method is private and absent from the runtime reflection table;
reproducing it requires the same native `FVector3` and `FPoint` operations,
because the fixed-point rounding is part of the result.

Restoring into a game computes those centres, then adds the objects in index
order. A non-empty grid must overwrite both the native `Queue<ByteMask>` and
`GridBlockInt.grids`, with an immediate read back. `Sync` cannot be used to
deserialise a final state: it only subtracts further cells from a grid that
already exists.

## What is not established here

- The effect any type has on a unit, beyond that an affected set exists.
- Any radius, effect clock or lifetime. They vary by source, and two sources of
  one type can differ, so no value derives from a type.
- `recovery_zone` and `fog_sand` behaviour.
- Whether a native producer for a type conversion within one identity exists.
- Whether a technology projectile is the cause of the terrain it coincides with,
  as opposed to a reliable correlate of it.
