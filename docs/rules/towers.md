# Towers

A side's two towers, the Energy Tower and the Research Center, can be
strengthened before a fight, and losing one in a fight writes a debuff on the
side.

## Strengthening

`tower_strengthen_levels` holds one level per tower, 0 to 4, in the order the
map lists a side's towers (`BuildingManager.buildings`, and
[`docs/spec/document/layout.md`](../spec/document/layout.md)). A level adds
life and chooses the buff the tower's loss writes. Both are
`ConfigDataContainer.towerStrengthenDatas`: each row adds its `life` on top of
the levels below it, and names the `buffDatas` row a loss writes. Level 0 is
the tower's own life and buff, which no strengthen row names.
[`scripts/extract-towers.py`](../../scripts/extract-towers.py) writes them to
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
otherwise. Every live unit of the side takes it. A tower is a
`FightTower : FightCrystal, IBuffTarget` with a `BuffManager` of its own, and
`buffDatas` rows carry `canAffectTower`, so the standing tower may take the
loss as well.

The buff a loss writes is one buff that differs by level only in duration.
Its rows correct three rates: `speedChangeRate` on move speed,
`damageChangeRate` on damage dealt, and `amplifyDamageRate` on damage taken.
All share one `buffDivide`, and `isAdditiveMode` is set. The values are in
`config/towers.yaml`.

The rates land in the buff channel, which a recording keeps as a unit's
`buff_modifiers`. A number corrected in the buff channel and another composes
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

## When

The buff is on the side from the hit that fells the tower. `BuffManager.Update`
runs last in `FightMech.Update`, after the skill and the motion, and a buff ends
on the update its elapsed ticks reach its duration. A side updated before the
side that fells its tower counts from the next tick.

A unit's buffs go on its first update after it dies, not on the tick it dies.
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

## Evidence

### Recorded

- Losing a tower at each strengthen level writes its level's buff on every live
  unit of the side, with the level's added life:
  `tests/tower/regressions.mcscript`.
- The buff's rates act in the buff channel, composing with a unit's own as one
  factor on damage dealt, speed and damage taken:
  `tests/tower/regressions.mcscript`.
- A second loss inside the first's debuff lengthens it by the new row's
  duration and does not stack the rates: `tests/tower/regressions.mcscript`.
- The buff counts from the hit that fells the tower, reaches a side updated
  before the felling side from the next tick, and a projectile takes its
  owner's damage as it lands: `tests/tower/regressions.mcscript`.

### Read

- A tower's loss hands its buff on through one call, to the side's own actors
  or the group's: `FightTeamController.OnTowerDestoryed`,
  `BuildingSystem.OnTowerDestroyed`, `BuildingSystem.SetAddTowerBuffTarget`,
  `TowerDefaultData.addBufToOwner`.
- A second buff of the same divide lengthens the running one: `Buff.Reset`.
- A tower's buffs are kept apart from a unit's: `BuffManager.towerBuffDatas`,
  `BuffManager.GetTowerBuffDamageChangeAddRate`.
- A buff row may reach a tower: `BuffData.canAffectTower`.

### Not established

- **A construction standing when its side loses a tower.** `canAffectConstruction`
  is set on the buff and `can_be_effected_by_tower_buff` on every construction
  but the Defensive Wall's first row, and no recording has one standing through
  a loss; the simulator refuses the fight when it happens.
- **A tower taking a buff.** Read above, not recorded.
- **Whether a tower's separate buff set changes the composition.** Not recorded.
- **`isClearSelfBuffWhenDisableTech`**, set on the buff: nothing this simulator
  places disables a unit's technologies.
- **The officer that lengthens the debuff and the energy tower's skills**, which
  stay refused as `energy_tower_skills` and their officers.
