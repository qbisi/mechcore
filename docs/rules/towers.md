# Towers

A side's two towers, the Energy Tower and the Research Center, can be
strengthened before a fight, a huge unit of the side walks around them, and
losing one in a fight writes a debuff on the side.

## Strengthening

`tower_strengthen_levels` holds one level per tower, 0 to 4, in the order the
map lists a side's towers (`BuildingManager.buildings`, and
[`docs/spec/document/layout.md`](../spec/document/layout.md)). A level adds
life and chooses the buff the tower's loss writes. Both are
`ConfigDataContainer.towerStrengthenDatas`: each row adds its `life` on top of
the levels below it, and names the `buffDatas` row a loss writes. Level 0 is
the tower's own life and buff, which no strengthen row names.
[`scripts/extract/extract-towers.py`](../../scripts/extract/extract-towers.py) writes them to
[`config/towers.yaml`](../../config/towers.yaml).

`towerDefaultDatas` gives a tower's own `life`, `protectionRange` and
`rvoRadius` per scene (`limitedScene`), and `addBufToOwner`, which the next
section reads.

## What a loss writes

The hit that fells a tower runs its `OnDead`, and
`FightTeamController.OnTowerDestoryed` hands the tower's buff to
`BuildingSystem.OnTowerDestroyed`, which makes one
`BuffSystem.AddBuff(buff, targets, team)` call. `targets` is the side's own
actor list when `isAddTowerBuffToOwnerTeam` is set (`SetAddTowerBuffTarget`,
which `towerDefaultDatas.addBufToOwner` feeds), and the group's actor list
otherwise. Every live unit of the side takes it, in the order the units
joined the side: the deployed ones, then a unit a beam turned onto it. A tower is a
`FightTower : FightCrystal, IBuffTarget` with a `BuffManager` of its own, and
`buffDatas` rows carry `canAffectTower`, so the standing tower may take the
loss as well.

The buff a loss writes is one buff that differs by level only in duration.
Its rows correct three rates: `speedChangeRate` on move speed,
`damageChangeRate` on damage dealt, and `amplifyDamageRate` on damage taken.
All share one `buffDivide`, and `isAdditiveMode` is set. The values are in
`config/towers.yaml`.

The rates land in the buff channel, which a recording keeps as the `buff`
channel of a unit's `modifiers`. A number corrected in the buff channel and another composes
as it does within one channel: `DamageProperty.CalculateDamage` adds the buff's
enhancement to the skill's and multiplies the two reduce rates, as one factor,
before the damage; `MoveSpeedProperty.Refresh` does the same over the unit's
`DataSet` and the buffs', in Q32.32 metres a second. `PerformHitTargetEffect`
scales each hit a unit takes by the rate on damage taken. `BuffManager` keeps
a tower's buffs in a separate `towerBuffDatas` set, read through
`GetTowerBuffDamageChangeAddRate` and `GetTowerBuffDamageChangeReduceRate`;
whether that changes the composition is not recorded.

A second loss while the buff runs does not add a second buff. `BuffManager`
finds the running one in the same `buffDivide`, and `Buff.Reset` lengthens it
by the new row's duration, because the row is additive. The rates never stack.

**Another buff runs beside the loss's, and their speeds compose.** A buff of
another divide, a Sticky Oil Bomb's slow, is a second buff on the unit, and
its rate on move speed composes with the loss's as two impairments do, each
multiplying what the other leaves: the oil's -0.55 on a Fang running the
loss's -0.8 leaves it 0.09 of its speed. A unit that dies
running both has them cleared last first, the oil's before the loss's. How
another buff's damage rates compose with the loss's, which `BuffManager` keeps
apart, is not recorded, and a skill's buff that corrects one of the loss's
numbers other than speed is refused.

## A construction takes the loss too

The side's actor list holds its constructions as well as its units, so a
construction standing when its side loses a tower takes the buff after every
unit of the side, when both the buff row's `canAffectConstruction` and the
construction row's `canBeEffectedByTowerBuff` are set. `config/constructions.yaml`
says which rows set the second; `config/towers.yaml` says the buff sets the
first. A layout's Defensive Wall does not set it, and both turrets do.

On a turret the buff does what it does on a unit. Its shots deal the damage
the rates leave, a tenth of the row's; every hit it takes is scaled by the
rate on damage taken; and `BuffManager.Update`, last in
`FightConstruction.Update` as in `FightMech.Update`, ends it when its time is
up. A construction that falls under the buff has it written as cleared just
before its `building_destroyed`, where a unit's follows its `unit_died`.

## When

The buff reaches the side after every unit and every projectile has updated
on the tick the tower falls. The hit that empties the tower only queues it with
`DeadEffectSystem`; that module updates after `FightCoreSystem` and
`ProjectileSystem`, and it is its update that calls the tower's `OnDead`, which
writes the buff. `BuffManager.Update` runs last in `FightMech.Update`, after the
skill and the motion, and a buff ends on the update its elapsed ticks reach its
duration, so either side counts from the next tick, whichever side felled the
tower and however. For the rest of the tick the tower falls, the losing side's
hits and the projectiles that land are without the buff.

From the tick after a side is decided the fight is over, and
`FightMech.Update` and `FightConstruction.Update` return before
`BuffManager.Update`: a unit's or a construction's buffs neither run on nor run
out. A buff still running when the fight ends is taken off every
survivor and written as cleared, after the towers the end tore down: unit by
unit, and then construction by construction, a construction whose side has no
unit left among them.

A unit's buffs go on its first update after it dies, not on the tick it dies.
The recording still writes each buff it had as cleared right after its death,
whatever killed it: a hit, a laser or a projectile.
A projectile takes its owner's damage as the owner has it when the projectile
lands, so a shot fired under the debuff lands for the full damage once the
debuff has ended, or once its dead owner's buffs are gone.

A tower falls as a unit dies: an attacker whose lock it was goes on to
`FightSkill.CheckAttackable` and may switch target, because
`SkillAttackState.CheckAttackable` rejects a dead target outright only of the
`FightConstruction` class. A block of a wall is a construction, and is held as
before.

`RVOControllerFixed` keeps the speed `Active` read when the unit took the field
as `_maxSpeed`, and `StopMove` hands the agent that. A unit stopped to attack
under the debuff is pushed by its neighbours as at full speed; one that moves
gets the debuffed speed through `Move`.

## A huge unit walks around its own side's towers

A huge unit that does not fly turns aside where one of its own side's towers
stands between it and what it walks to. Every move that hands its agent a
point asks first: on the move a tower within three of the unit's radii, edge
to edge, stands in the strip from the unit to its target as wide as the larger
of the two radii, the unit is sent two of its radii from where it stands,
square to the tower on the target's side, and turned towards the tower by up
to 60 degrees as the tower's edge is further than those two radii. The first
such tower in the side's list is the one turned around.

The point found is meant to hold for ten moves while the target stays within
twice its radius of where it stood, but the record of where it stood keeps the
target's `x` and nought for its depth, so a target away from the map's middle
line is never inside it and the point is found anew on every move. A unit with
no tower of its side within three of its radii on a move stops asking for the
rest of the fight. A unit with no lock goes straight to its point.

## Evidence

### Recorded

- Losing a tower at each strengthen level writes its level's buff on every live
  unit of the side, with the level's added life:
  `tests/tower/fights/`.
- The buff's rates act in the buff channel, composing with a unit's own as one
  factor on damage dealt, speed and damage taken:
  `tests/tower/fights/`.
- A second loss inside the first's debuff lengthens it by the new row's
  duration and does not stack the rates: `tests/tower/fights/`.
- A turret standing through the loss takes the buff after the side's units,
  fires at a tenth of its damage, takes hits raised by the rate on damage
  taken, and loses the buff when it expires or, cleared, just before it falls:
  `tests/tower/fights/turret-falls-under-the-loss.yaml`,
  `tests/tower/fights/turret-outlasts-the-loss.yaml`.
- A unit that dies under the buff has it written as cleared after its death,
  a projectile's kill as any other: `tests/tower/fights/`.
- The buff counts from the tick after the tower falls on the side updated
  before the felling side, `tests/tower/fights/`, and on the side updated
  after it, where a Steel Ball of the side updated first fells a Research
  Center with its beam: `tests/corpus/fights/201373545-r1.yaml`. A projectile
  takes its owner's damage as it lands: `tests/tower/fights/`.
- A buff still running when the fight ends is written as cleared on every
  survivor, after the one tower the end tore down:
  `tests/corpus/fights/67160345-r1.yaml`. On a construction it is written
  after the units', `tests/corpus/fights/201377411-r2.yaml`, and also when the
  construction's side has lost its last unit:
  `tests/corpus/fights/134266831-r1.yaml`,
  `tests/tower/fights/turret-buff-cleared-as-fight-ends.yaml`.
- A Sticky Oil Bomb's slow runs beside the loss's buff on blue's Fangs, the
  two speeds composing, and a Fang dying under both has the oil's cleared
  first: `tests/tower/fights/loss-beside-oil.yaml`.
- The losing side's projectiles that land after the fall, on the tick the tower
  falls, land for their full damage: a Fire Badger's shot fells a tower and two
  Mustang shots of the losing side land after it, undebuffed:
  `tests/corpus/fights/67160729-r1.yaml`.

### Replayed

- A tower's loss writes its buff on two blue units turned to red after every
  red unit deployed: replay 2324_20260925--134259672 round 5, tick 377,
  fought by the game with `scripts/corpus/match-replays.py`.

### Read

- A tower's loss hands its buff on through one call, to the side's own actors
  or the group's: `FightTeamController.OnTowerDestoryed`,
  `BuildingSystem.OnTowerDestroyed`, `BuildingSystem.SetAddTowerBuffTarget`,
  `TowerDefaultData.addBufToOwner`.
- A second buff of the same divide lengthens the running one: `Buff.Reset`.
- A tower's loss waits for `DeadEffectSystem`: `FightActor.ReduceLife` hands
  an emptied actor to `DeadEffectSystem.OnActorDead`, which adds it to
  `DeadEffectSystem.deadActors`; `DeadEffectSystem.Update` calls each one's
  `OnDead`, and `FightTower.OnDead` goes through `FightCrystal.OnDead` to the
  side's `FightTeamController.OnTowerDestoryed`.
- `FightController.AddModules` adds `FightCoreSystem`, then `ProjectileSystem`,
  then `DeadEffectSystem`, and `FightingState.Update` updates the modules in
  that order.
- A tower's buffs are kept apart from a unit's: `BuffManager.towerBuffDatas`,
  `BuffManager.GetTowerBuffDamageChangeAddRate`.
- A buff row may reach a tower: `BuffData.canAffectTower`.
- A buff row may reach a construction, and a construction row may let it:
  `BuffData.canAffectConstruction`, `ConstructionData.CanBeEffectedByTowerBuff`.
- A construction runs its buffs last in its update: `FightConstruction.Update`,
  `BuffManager.Update`.
- Each holder, unit or construction, has its own `BuffManager`, whose
  `Clear` removes every buff it holds, the last first: `BuffManager.Clear`,
  `BuffManager.RemoveBuff`.
- A unit or a construction runs no buff once the fight is over:
  `FightMech.Update` and `FightConstruction.Update`, which return before
  `BuffManager.Update` when `isFighting` is false.

- A huge unit that does not fly has a `SimplePathFindingController`, every
  other a `PathFindingController` that passes the point on:
  `MotionController..ctor`, `ISkillOwner.IsEnableAvoidanceAssist`,
  `MechData.IsEnableAvoidanceAssist`, `MechData.mechType`, `MechData.isFly`.
- `MotionController.Move` passes the point it hands the agent through
  `PathFindingController.CalculateNextPoint`, with
  `IMoveBehaviour.IsStaticTarget`: a command's point stands still
  (`MoveAttackCommand`), a lock moves (`AutoMoveBehaviour`).
- The point: `SimplePathFindingController.CalculateNextPoint`,
  `SimplePathFindingController.IsAvoidancePointAvaliable`,
  `SimplePathFindingController.FindAvoidanceActor`,
  `SimplePathFindingController.GetNeighbours`,
  `SimplePathFindingController.CalculateAvoidancePoint`, `LineRange.Contains`,
  `FPlane.GetSide`, `FQuaternion.AngleAxis`. The record's centre goes through
  `FVector3.op_Implicit`, which keeps `x` and `y`, and
  `FVector2.ToVector3XZ`. No tower near sets
  `SimplePathFindingController.checkCount` to -1, which only
  `SimplePathFindingController.EnableAvoidanceCheck` resets.

### Not established

- **What calls `SimplePathFindingController.EnableAvoidanceCheck`**, which
  would let a unit that stopped asking ask again; the call graph shows no
  caller.
- **`LineRange.Contains` at its edge.** The build measures the angles and the
  sine in its fixed point; the simulator asks the exact products, which may
  answer differently for a tower on the strip's edge.
- **What clears the buffs as the fight ends.** `BuffManager.Clear` is reached
  through `FightBehaviour.Clear`, whose caller the call graph does not show,
  so that units clear before constructions is recorded, not read, and two
  constructions' order is not recorded.
- **A Defensive Wall block, or a construction other than the turrets, under
  the buff.** The Defensive Wall a layout places does not take it, and no
  recording has another construction that does.
- **A tower taking a buff.** Read above, not recorded.
- **The losing side's blows and beams on the tick its tower falls.** Read
  above: they land without the buff, as its projectiles are recorded to. No
  recording has a unit of the losing side strike directly after the fall
  within that tick.
- **Whether a tower's separate buff set changes the composition** of damage
  rates. Speeds compose as any two buffs' do; damage is not recorded.
- **`isClearSelfBuffWhenDisableTech`**, set on the buff: nothing this simulator
  places disables a unit's technologies.
- **The officer that lengthens the debuff**, which stays refused.
