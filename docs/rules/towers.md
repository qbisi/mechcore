# Towers

This rule is pinned to build **1.11.1.3.2259**. A side's two towers, the
Energy Tower and the Research Center, can be strengthened before a fight, and
losing one in a fight writes a debuff on everything the side still has
standing.

## Strengthening

`tower_strengthen_levels` holds one level per tower, 0 to 4, in the order the
map lists a side's towers (`BuildingManager.buildings`, and
[`docs/spec/document/layout.md`](../spec/document/layout.md)). A level adds
life and chooses the buff the tower's loss writes. Both come from
[`config/towers.yaml`](../../config/towers.yaml), which
[`scripts/extract-towers.py`](../../scripts/extract-towers.py) reads out of
`ConfigDataContainer`: `towerStrengthenDatas` rows 1 to 4 each add their
`life` on top of the levels below, and name the `buffDatas` row a loss writes.

| Level | Life | Buff | Lasts |
| ---: | ---: | ---: | ---: |
| 0 | 3400 | 1 | 9 s, 180 ticks |
| 1 | 23400 | 2 | 7 s, 140 ticks |
| 2 | 59400 | 3 | 5 s, 100 ticks |
| 3 | 131400 | 4 | 3 s, 60 ticks |
| 4 | 235400 | 5 | 1 s, 20 ticks |

The map's own 3400 is in `config/training_ground.yaml`. Level 0's buff is
id 1, which no strengthen row names: the recordings read its 180 ticks.

## What a loss writes

`FightTeamController.OnTowerDestoryed` runs in the hit that fells a tower
(`FightCrystal.OnDead`), and hands the tower's buff to
`BuffSystem.TryAddSpecialBuffForTeam`, which gives it to every live object of
the side (`FightTeam.activeActors`). Buff rows 1 to 5, all named 能量塔摧毁,
are one buff that differs only in duration:

| Field | Raw | Reads |
| --- | ---: | --- |
| `speedChangeRate` | −3435973836 | move speed ×0.2 |
| `damageChangeRate` | −3865470566 | damage ×0.1 |
| `amplifyDamageRate` | 2147483648 | damage taken ×1.5 |
| `buffDivide` | 1 | one group for all five rows |
| `isAdditiveMode` | true | a second loss lengthens it |

The rates land in the buff channel, which a recording keeps as the unit's
`buff_modifiers`. A number corrected in the buff channel and another composes
as it does within one channel: `DamageProperty.CalculateDamage` adds the
buff's enhancement to the skill's and multiplies the two reduce rates, as one
factor, before the damage; `MoveSpeedProperty.Refresh` does the same over the
unit's `DataSet` and the buffs', in Q32.32 metres a second. A Marksman under a
lost tower shoots 232 for 2329. `PerformHitTargetEffect` scales each hit a unit
takes by the rate on damage taken: a Crawler's 79 lands as 118.

A second loss while the buff runs does not add a second buff. `BuffManager`
finds the running one in the same `buffDivide`, and `Buff.Reset` lengthens it
by the new row's duration, because the row is additive. So a level-4 tower lost
inside a level-0 tower's 180 ticks leaves the debuff on for 200 from the first
loss; the rates never stack.

## When

The buff is on the side from the hit that fells the tower. `BuffManager.Update`
runs last in `FightMech.Update`, after the skill and the motion, and a buff ends
on the update its elapsed ticks reach its duration. A side updated before the
side that fells its tower counts from the next tick: blue's units, when red's
Steel Balls take blue's tower on tick T, carry a level-0 loss through tick
T + 179 and lose it on T + 180.

A unit's buffs go on its first update after it dies, not on the tick it dies.
A projectile takes its owner's damage as the owner has it when the projectile
lands, so a shot fired under the debuff lands for the full damage once the
debuff has ended, or once its dead owner's buffs are gone, and for the
debuffed damage while it runs.

A tower is an actor of its own, `FightCrystal`, and falls as a unit dies: the
Steel Ball whose beam fells it reads idle on that tick, and an attacker whose
lock it was goes on to `FightSkill.CheckAttackable` and may switch target,
because `SkillAttackState.CheckAttackable` rejects a dead target only of the
`FightConstruction` class. A block of a wall is a construction, and is held as
before.

`RVOControllerFixed` keeps the speed `Active` read when the unit took the field
as `_maxSpeed`, and `StopMove` hands the agent that. A unit stopped to attack
under the debuff is pushed by its neighbours as at full speed; one that moves
gets the debuffed speed through `Move`.

## Evidence

[`tests/tower/`](../../tests/tower/README.md) holds seven fights the game
recorded: a tower lost at each strengthen level, and a second tower lost inside
the first loss's debuff at levels 0-0 and 0-4. The simulator plays all seven
back tick for tick, physics and content.

## Not covered

- **A construction standing when its side loses a tower.** `canAffectConstruction`
  is set on the buff and `can_be_effected_by_tower_buff` on every construction
  but the Defensive Wall's row 1, and no recording has one standing through a
  loss; the simulator refuses the fight when it happens.
- **`isClearSelfBuffWhenDisableTech`**, set on the buff: nothing this simulator
  places disables a unit's technologies.
- **The officer that lengthens the debuff and the energy tower's skills**, which
  stay refused as `energy_tower_skills` and their officers.
