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
`acid`. A `fog_sand` is the sand fog Sandstorm's Sandworm leaves as it
surfaces ([underground.md](underground.md#a-sand-fog-as-it-surfaces)). No
source makes a `recovery_zone` in this build: its one producer, the
`CS_Recovery` battle skill, has no row, `CommanderSkillGroupData`'s
`recoveryCommanderSkills` being empty, and no technology clears one.

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

An extra weapon technology's shot creates a `RangeItem` through a second entry
point: `ExtraSkillProvider.PerformHitEffect` calls `RangeItemSystem.AddItem`
with the unit's fire where the hit struck, height included
([extra_weapons.md](extra_weapons.md#a-fire-where-it-lands)). A fire's repeat
test (`GetRepeatItem`) compares the whole point, height too.

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

**A controller finds its units through two quadtrees.** Each controller keeps
its items in a `FightQuadtree<RangeItem>` (`MaxDepth` 7, 20 elements to a node
before it splits), each by its bounds (`RangeItem.GetBounds`): a square about
where it stands as wide as its range, where a unit's bounds are twice its
radius wide. `UpdateAffectedActorChange` takes, side by side, the item tree's
nodes the side's units' tree is interactable with
(`FightQuadtree.GetInteractableNodes`: every node holding items, a node before
its children, whose rect overlaps a node of the units' tree that holds units),
and for each node, each of its items against every unit the units' tree
answers for the node's rect. A unit found in several items enters the first.
Until a controller's twentieth item splits its root, that is every item in the
order it was added against every unit of the side; after, the items go node by
node. A removed item leaves the tree as it leaves the controller
(`RangeItem.Remove` raises `OnDestroyed`). A fire reaching oils asks the
oils' tree too (`RangeItemController.GetItems`, from `CheckInteractableItems`
and `TriggerInteractableItem`): `FightQuadtree.Query` answers the root's oils
always, then every oil of each node the fire's circle overlaps
(`FightRange.Overlaps`: the point of the node's rect nearest the circle's
centre lies strictly within its radius), a node's before its children's, and
the oils it answers that the circle reaches burn in that order. A Fire Badger with Napalm keeps 150
fires burning among its own units and the Rhino charging them, and the game
and the simulator burn each of them alike.

## A battle skill's terrains

A terrain battle skill (Incendiary Bomb, Sticky Oil Bomb, Smoke Bomb, Acid
Blast) is a line of its row's `subEffectCount` sub-effects, placed and timed as
a line strike's are ([`battle_skill.md`](battle_skill.md)), and each leaves one
terrain of the row's `subEffectRange` where it lands, of the skill's side, on
the tick it lands. Smoke Bomb's seven fogs of 50 metres land every four ticks
from tick 63.

**Each tick**, after `MineSystem` and before `FightCoreSystem`, each
controller holding terrains updates, in the order `RangeItemSystem.Init`
makes them: fire, oil, fog, sand fog, acid, recovery zone.

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
   it or is gone. A unit that dies leaves as it dies, the rest keeping their order.
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

**A sand fog** writes its row's `FogAttackRangeChangeRate` on the skill
range of a unit that enters it whose attack is not a melee one, as a fog
does, and its `reduceDamageFromRemote` in every unit that enters it as the
unit's `MechDataChangeFloatRate.ReduceDamageFromRemote`; it takes both back
as the unit leaves, its controller the modifier of both. A hit whose
`IDamageProvider.GetAttackDistanceType` is `remote` is then multiplied by
what the unit's rates leave of one, in `FPoint`, its whole part kept, after
the rate on damage taken and before the unit's damage reduction. A
projectile's hit is remote, and so is a skill's own hit unless the skill
is melee (`SkillDamageProvider`'s `2 - IsMelee`); every other hit, a
fire's, an explosion's, a summon's drop, a battle skill's, is not.
Sandstorm's fog takes 30% off: a Marksman's 2329 at a Sandworm standing in
it is 1630, and a Steel Ball's rising beams lose as much, rounded down.

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

**An acid** writes its buff as an oil does, Acid Blast's `500001`: 1.5 more
damage taken for 20 ticks, again every 19 ticks the unit stays. The buff also
changes the unit's life every step. `Buff.Init` gives a buff of a nonzero
`lifeChangeRate` a controller, `IBEC_ChangeLIfe`, and a step of its
`stepTime` in ticks, ten for this buff. `Buff.Update` counts the step on every
update of the unit's buffs, the update on the tick the buff was written being
the first, and at ten it goes back to zero and the controller acts. Rewriting
the buff (`Buff.Reset`) leaves the count where it was, so the acid steps every
ten ticks from the unit's first writing whatever the oil-like rewrites do. The
controller takes the unit's maximum life times the rate in `FPoint`, its whole
part, a loss rounded away from zero: 1.5% of 3478 is 52.17 and takes 53. The
loss is a hit with no owner, under the side that wrote the buff, that the
unit's rate on damage taken does not touch (`isAmplifyDamageAffected` false),
so the buff's own 1.5 does not raise it; the unit's shield takes it first, as
any hit. A kill it makes counts as a fire's does. A step that kills the unit
ends its buffs' update, and the buffs before it in the list neither age nor
step that tick. With the fight over, from the tick after a side is decided, no
buff steps.

**As the fight ends**, after the buffs the units still run are cleared, every
unit leaves its terrain, which takes back what it did, and every terrain
counts a round. One that has stood its row's `effectDuration` goes,
`round_expired`, and so does every fire, whose controller ignores rounds; the
rest stand into the next round. A fog stands one round and goes; an oil
stands two and is still there in the last state, with one round left. A
terrain of more than one round reads its rounds left as `remaining_rounds`.

**A fire and an oil.** `RangeItemSystem.interactiveInfos` holds one
interaction: a fire takes oil. A fire that lands (`DoAddItem`) takes every
oil whose circle its own reaches (`CheckInteractableItems`), in the oil
controller's order: each goes, and then each turns, in that order, to a fire
where it stood. A centre no further from another than the two radii
together, by `FPoint`'s tolerant comparison, reaches it
(`CircleRange.Overlaps`). The new fire is made with the oil's provider, so it
has the oil's range, 30 metres, and rounds, and burns the oil row's
`fireLifeTime`, 700 ticks (`CS_Oil.GetFireLifeTime`). It is of the side of
the fire that lit it, and it lands as any fire does, so it takes the oils it
reaches in turn: a fire that reaches one oil of a line burns the line. An oil
that lands where a fire already burns (`AddItem`, through the fire
controller's `IsInteractable`) turns at once to such a fire, of its own
side. A fire of a provider that already has one standing at the same place
is not made again; the standing one burns from the start
(`GetRepeatItem`, `FightGroundFire.Reset`). Taking an oil away does not
release the units in it: its controller lets them go on its next update. In
a tick, a side's battle skills land in the order it released them.

**A hit that deals fire burns the oil its splash reaches.** A Vulcan's and a
Fire Badger's skills deal `EDamageType.Fire`, and
`DamagePerformer.PerformHitTargetsEffect` ends such a hit by handing where it
landed and its splash to `RangeItemSystem.TriggerInteractableItem`, which
takes every oil that circle reaches and burns each, for the hitting side, as
a new fire burns them. The oils it reaches are taken together before any
burns, so the first one's fire takes the rest of its line first: a Fire
Badger's shot that lands among the first three oils of a line burns the
first, then the line beyond the third, then the second and the third.

**A battlefield shield keeps the oil within it from a fire hit.** Of the oils
the circle reaches, `TriggerInteractableItem` passes over each that lies
wholly within an active shield of either side with energy left
(`AdvancedEnergyShieldSystem.GetActiveEnergyShields`,
`FightCalculator.IsInEnergyShield`: the oil's distance from the shield's
centre less than the shield's radius less the oil's, by `FPoint.op_LessThan`);
it stays an oil, and only the rest burn. Read from the build; the corpus
rounds that hold it are not pinned.

**Standing oil.** An oil an earlier round left is restored before the fight
from its release's panel skill, which keeps it in its `rangeItems`: the line
between its two control points is expanded as the release expanded it
(`CalculateAttackPositions`), and each point that still stands is added in
order through `AddItem`, one round old. Each side's areas are restored in turn,
blue's first. Each area's skill is a provider of its own. A standing oil is
there in the recording's first snapshot, named in that order with no
`terrain_created`, with one round left; it slows and burns as any oil does,
and goes `round_expired` as the fight ends.

A recording finds a terrain made or gone by comparing one snapshot's terrains
with the last, so `terrain_created` and `terrain_removed` are the last events
of their tick, after every other: the made, then the gone, each in identity
order. It names a terrain the first time a snapshot holds it, controller by
controller in the system's order and item by item. A terrain made and gone
in one tick, an oil that lands in a fire, is never named. A terrain it only
finds gone, an oil that burns, goes `unknown`.

The simulator refuses a circle of 80 metres or more meeting a grid, which
the build compares as a `GridBlockLong`.

## Circles and grids

An ordinary terrain is a `position` and a `radius`, stored as Q32.32 raw
integers, with no grid.

**A grid** (`GridBlockInt`) is a terrain cut into cells of five metres
(`RangeItemEffectLayerGrid.gridSize`). The cells of a circle are a template
the fight keeps by radius (`FightCacheData.GetGridBlockInt`): every cell of a
circle centred at `(r, r)` whose centre the circle contains, by `FPoint`'s
tolerant comparison, its first cell centred at
`RoundToInt((r - r) / 5) × 5 + 2.5` on each axis, across `⌊2r / 5⌋ + 1` cells,
and its size the cells it sets. A grid of a circle where it stands is that
template with its first cell where `CalculateGridDataForCheck` puts it, the
circle's lower corner divided by five, rounded half to even and taken back to
the lattice of cell centres at `5k + 2.5` (`GridBlockInt.Create`). An oil's
grid is twelve cells by twelve, a fire's sixteen and a fog's twenty.

Two grids are compared by shifting one onto the other's cells
(`ConvertToLocalSpace`): row `y` of the native `grids` holds column `x` at bit
`31 - x`, a column shift masked to five bits as C#'s is. A circle meets a grid
when the circle's own grid, made as above, shares a cell with it
(`GridBlockInt.Overlaps`), after a rectangle test of the grid's bounds
(`CalculateBounds`, `FightRange.Overlaps`). A small circle may hold no cell:
a crawler's of 2 metres holds one.

A recording reads the native rows as columns, so its row `i` holds the cells
of column `i`, cell `(i, j)` at bit `j`. Its origin is the first cell's
centre.

## Shields cut terrain

A terrain sub-effect that cannot cross shields (`isCrossAdvancedShield`, false
for every terrain row) is tested as it falls, as a strike's is
([`battle_skill.md`](battle_skill.md)). On the first shield it comes inside it
ends, and `InterruptEffect`'s `PerformHitEffect` leaves no terrain and deals
the shield nothing.

A terrain added with `useGrid` and no cells of its own
(`RangeItemEffectLayerGrid.OnAddRangeItem`) becomes a grid when an active
shield of either side reaches its circle (`CircleRange.Overlaps`), and stays a
circle otherwise. Its grid then loses the cells every active shield's own grid
covers (`GenerateGrid`, `GridBlockInt.TryDisableGrid`), unless it was turned
from another kind (`isConvertFromOtherType`, a fire burnt from oil) or its
centre stands inside the first shield that reaches it
(`FightCalculator.IsInRange3D`); then it is a whole grid. A shield raised later
cuts nothing already standing. A terrain added with cells takes them
(`GenerateGrid` with `detailMasks`, `Sync`): a fire burnt from a cut oil takes
the oil's, and a standing oil a point's recorded cells. A standing oil with
none is added without `useGrid`, a circle whatever shields stand.

A grid's unit must also meet one of its cells with its bounds circle, its
position and radius (`RangeItemEffectLayerGrid.IsInRange`), and a grid reaches
a fire or an oil by its cells (`RangeItemController.GetItems`,
`IsInteractable`).

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
the grids by `RangeItem.Index`. The last point falls a fraction of a millimetre
off the second control point, so its centre is taken to the nearest whole
metre, within 2^-16 metres. After a replay restore the manager no longer
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

- A controller past its twentieth item finds its units node by node: 150
  fires of a Fire Badger's Napalm burn its own units and a Rhino as the game
  does, and 32 of a Stormcaller's Incendiary Bomb a Rhino:
  `tests/terrain/napalm-quadtree.yaml`,
  `tests/fire_intensify/stormcaller-friendly.yaml`.
- A fire among 28 standing oils, their tree split, burns them in the order
  the tree answers its circle, root first and quadrant by quadrant; in the
  order they were added, the simulator parts from the game at t67:
  `tests/terrain/standing-oils-quadtree.yaml`.
- A Fire Badger's shot burns the oil its splash reaches, and the first fire
  the line beyond it: `tests/corpus/201340110-r5.yaml`, tick 140.
- A Smoke Bomb's fogs land along its line every four ticks, take ground units
  of either side whose edge they reach one fog at a time, hold a ranged
  attack's range to 0.65 of it, leave melee units alone, and go as the fight
  ends, taking the rate back: `tests/terrain/smoke.yaml`,
  `tests/terrain/smoke-both-sides.yaml`.
- An oil writes its slow as a unit enters and every 19 ticks it stays, and
  stands into the next round: `tests/terrain/oil.yaml`.
- An acid's buff takes 1.5% of a unit's maximum life every ten ticks from its
  first writing, unraised by the buff's damage taken and unreset by its
  rewriting, and stops once the fight is over: `tests/terrain/acid.yaml`.
- A fire takes the oils it reaches, for its own side, and an oil that lands
  in a fire burns for the oil's side; a burning oil is a fire of 30 metres,
  two rounds and 700 ticks: `tests/terrain/oil-ignited.yaml`,
  `tests/terrain/oil-ignited-by-the-enemy.yaml`.
- A unit held to a fire that burns out while it stands in the next one takes
  the gone fire's hits on, every four ticks: an Arclight held to the fifth of
  an Incendiary Bomb's fires, gone after tick 777, takes 54 on ticks 778 and
  782, `tests/corpus/67156354-r3.yaml`.
- A shield of either side ends the terrain sub-effects that fall inside it and
  cuts the terrains it reaches; a unit stands in a cut terrain where its cells
  meet the terrain's, a burnt cut oil keeps its cells, and a standing oil is
  restored with its recorded cells or whole:
  `tests/terrain/oil-cut-by-a-shield.yaml`,
  `tests/terrain/units-in-cut-terrains.yaml`,
  `tests/terrain/fire-cut-by-an-enemy-shield.yaml`,
  `tests/terrain/cut-oil-ignited.yaml`,
  `tests/terrain/oil-standing-cut.yaml`.
- An oil an earlier round left stands from the first tick, one round old and
  named in point order, side by side, blue's first; it burns when a fire
  reaches it and goes as the fight ends: `tests/terrain/oil-standing.yaml`,
  `tests/terrain/oil-standing-ignited.yaml`,
  `tests/terrain/oil-standing-both-sides.yaml`.
- A fire hits a unit as it enters and every four ticks it stays, the units
  counting last first, burns out after 700 ticks, and its removal is the
  last event of its tick: `tests/terrain/fire.yaml`,
  `tests/terrain/fire-burns-out.yaml`.

### Read

- A hit that deals fire burns the oil its splash reaches:
  `ProjectileSkillData.damageType`, `EDamageType`,
  `IDamageProvider.GetDamageType`, `DamagePerformer.PerformHitTargetsEffect`,
  `RangeItemSystem.TriggerInteractableItem`.

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
- An acid's buff changes the unit's life every step, its own count that a
  rewrite leaves: `Buff.Init`, `Buff.Update`, `Buff.Reset`,
  `BuffManager.Update`, `IBEC_ChangeLIfe.Update`,
  `FightCalculator.PerformHitTargetEffect`.
- A new fire takes the oils it reaches, and a new oil a fire reaches burns:
  `RangeItemSystem.Init` (the controllers and `interactiveInfos`),
  `RangeItemSystem.AddItem`, `RangeItemSystem.DoAddItem`,
  `RangeItemSystem.CheckInteractableItems`, `RangeItemSystem.GetRepeatItem`,
  `RangeItemController.IsInteractable`, `RangeItemController.GetItems`,
  `CircleRange.Overlaps`, `RangeItem.GetRange`, `CS_Oil.GetFireLifeTime`,
  `FightGroundFire.Reset`, `RangeItemSystem.Update`.
- A grid is a template of five-metre cells placed on the lattice, cut by the
  shields' own grids and compared by shifting one onto the other:
  `RangeItemEffectLayerGrid.OnAddRangeItem`, `RangeItemEffectLayerGrid.GenerateGrid`,
  `RangeItemEffectLayerGrid.IsInRange`, `FightCacheData.GetGridBlockInt`,
  `GridBlockInt.CalculateGridDataForCheck`, `GridBlockInt.Create`,
  `GridBlockInt.TryDisableGrid`, `GridBlockInt.ConvertToLocalSpace`,
  `GridBlockInt.Overlaps`, `GridBlockInt.CalculateBounds`,
  `CircleRange.Contains`, `FightRange.Overlaps`,
  `FightActor.GetBoundsCircle`.
- A falling terrain sub-effect ends at a shield, leaving nothing:
  `CommanderSkillSubEffectAgent.OnHitEnergyShield`,
  `CSRC_Common.InterruptSubEffect`,
  `CommanderSkillSubEffectController.InterruptEffect`,
  `RangeItemEffectController.PerformEffect`.
- A fight's controllers start from the items they already hold, each tree
  rebuilt in item order: `RangeItemSystem.OnFightStart`,
  `RangeItemController.OnFightStart`.
- As the fight ends a terrain counts a round and goes when its rounds are
  over, a fire's at once: `RangeItemController.OnExitFight`,
  `RangeItem.AddRound`, `RangeItem.IsRoundOver`,
  `GroundFireController.IsIgnoreRoundDuration`.
- `RangeItemSystem` updates between `MineSystem` and `FightCoreSystem`:
  `FightController.AddModules`, `RangeItemSystem.Update`.

- A terrain is in play exactly while its controller's set holds it:
  `RangeItemController.GetItems`.
- A sand fog's rates and the remote hits they reach:
  `FogSandController.PerformItemEffect`, `FogSandController.OnActorExit`,
  `FogSandController.GetAttackTarget`, `FightCalculator.PerformHitTargetEffect`,
  `HitDamageInfo.damageDistance`, `IDamageProvider.GetAttackDistanceType`
  (`FightProjectile`, `SkillDamageProvider`, `NormalDamageProvider`,
  `SupportUnitDamageProvider`, `DeadExplosiveDamageProvider`,
  `CommanderSkillDamageProvider`, `HitEffectControl`).
- There are six terrain types: `RangeItemType.Fire`, `RangeItemType.Oil`,
  `RangeItemType.Fog`, `RangeItemType.Acid`, `RangeItemType.RecoveryZone`,
  `RangeItemType.FogSand`.
- A recovery zone's one producer is a battle skill this build has no row of:
  `CS_Recovery`, `CommanderSkillGroupData.recoveryCommanderSkills`.
- A battle skill's ground impact adds its terrain directly:
  `RangeItemEffectController.PerformEffect`, `RangeItemSystem.AddItem`.
- The affected set is refreshed each update from target validity and a
  two-dimensional range: `RangeItemController.Update`,
  `RangeItemController.UpdateAffectedActorChange`, `FightActor.IsValidTarget`.
- The item tree and how the pass walks it: `RangeItemController..ctor`
  (`FightQuadtree<RangeItem>` of depth 7 and 20 to a node),
  `RangeItemController.Add`, `RangeItemController.OnFightStart`,
  `RangeItem.GetBounds`, `FightActor.GetBoundsRect`, `RangeItem.Remove`,
  `FightQuadtree.GetInteractableNodes`, `FightQuadtreeNode`1.GetElementCount`,
  `FightQuadtree`1.IsInteractableRange`, `FightQuadtreeNode`1.IsInteractableRange`,
  `RectRange.Overlaps`.
- The oils a fire reaches: `RangeItemSystem.CheckInteractableItems`,
  `RangeItemSystem.TriggerInteractableItem`, `RangeItemController.GetItems`,
  `FightQuadtree`1.Query`, `FightQuadtreeNode`1.Query`,
  `FightRange.Overlaps`, `RangeItem.GetCircleRange`.
- A grid subtracts the live battlefield shields:
  `RangeItemEffectLayerGrid.GenerateGrid`,
  `AdvancedEnergyShieldSystem.GetActiveEnergyShields`,
  `FightEnergyShield.GetRadius`, `GridBlockInt.TryDisableGrid`,
  `GridBlockInt.RefreshMask`, `GridBlockInt.Sync`.
- A line of centres is generated from its stored endpoints:
  `CommanderSkillManager.CalculateAttackPositions`.

### Not established

- **A fire hit beside a battlefield shield**, read from the build and not
  recorded.

- **A buff that heals**, a positive `lifeChangeRate`: the controller's other
  branch, which the simulator refuses.
- **`FogController.SelectBestTarget`**, which picks the stronger of two fogs
  and which no path of a fog's update reaches.
- **Any radius, effect clock or lifetime.** They vary by source, and two sources
  of one type can differ, so no value derives from a type.
- **A whole grid.** A terrain turned from another kind or standing inside the
  shield that reaches it is read as a whole grid, not recorded.
- **`FogSandController.SelectBestTarget`**, as a fog's.
- **A type conversion within one identity**: whether a native producer for one
  exists.
