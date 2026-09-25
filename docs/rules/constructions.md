# What a construction is

What a construction placed by a layout becomes in a fight, what numbers it
carries, whether it obstructs anything, and what a unit does about one in its
way. What a construction *does* on its own is not here: a turret's skill is
[`turrets.md`](turrets.md)'s, and a barrier's slow is nobody's yet.

The machine-readable table is
[`config/constructions.yaml`](../../config/constructions.yaml), and
`scripts/extract-constructions.py` writes it from
`ConfigDataContainer.constructionDatas`. A layout can name four of its rows'
constructions: the Defensive Wall, the Anti-Armor Turret, the Rapid-Fire
Turret and the Magnetic Barrier.

## A construction is several objects

One `constructions` entry is one placement and `count` objects.
`FightConstructionSystem.Create` answers an
`IReadOnlyList<FightConstruction>`, `GroupConstructionManager.CreateConstruction`
builds each of them through `FightController.CreateFightConstruction`, and a
recording holds each as its own building row. So every number in the table is
**one object's**: a Defensive Wall is five blocks, each with the row's life,
killed one at a time, and a turret is one object. An object's box is twice the
row's `radius`, the only length in the table a recording reports back.

Each object also carries its own **durability**, a count of how many times it
may be destroyed before it is gone for good, and a `combination` that ties the
objects of one placement together. The four a layout names all carry
`has_durability: false`.

## Where the objects stand

A Defensive Wall's five blocks stand in one row on the placement's own z,
12 metres apart, the middle one on the placement itself: 48 metres of the 60
the footprint reserves. A turret stands exactly where it was placed.

This is a measurement of a wall, not a formula. The two fields that look like
they should give the spacing do not: `block_width` and `space` span more than
the footprint. The build computes the offsets in
`GroupConstructionManager.CreateConstruction`, whose disassembly divides twice
and multiplies a row index by ten, the ten metres of the deployment grid, but
the interface slots it dispatches through cannot be named from the dump.

## A Defensive Wall is an obstacle only to the other side

**To the side that placed it, a wall is not there to avoid.** A unit of that
side walks through a block. The block is not gone from its avoidance, though:
it still takes one of the twenty neighbours a unit considers, crowding out a
unit it would otherwise have avoided, and only then is dropped. The wall's own
description says it sinks into the ground for a friendly unit.

**To the other side, each block is an immovable obstacle.** It stands on the
collider layer of the wall's `pathfinding_collider_priority` with its own box
for a radius, and a unit of the other team avoids it the way it avoids an
opponent. Opponent avoidance looks a hundredth of a second ahead, so it holds a
unit off a block and does not hold back a crowd pressing into one.

What differs between the two sides is targeting, not collision. The side that
did not place it **stops and attacks it**; the side that placed it walks
across. A block is one target with one life, so a wall is destroyed a block at
a time, and an attacker that overkills one block does not carry the excess to
the next: the damage a block takes is capped at the life it had.

The sinking is not in the building's data: `available`, `targetable` and
`collision_enabled` read true for every block at every tick, and
`collision_enabled` is `BuildingData.EnableCollision`, a property of the data
rather than a state. It is in who avoids the block.

**A block that falls ends the attack on it, and the lock with it.** This is
the build's attack state, not something about walls. `SkillAttackState` checks
its attack target between blows and while the next one winds up, and a dead
attack target of the construction class (`FightConstruction`) fails that
check outright, where a dead unit, or a dead tower, goes on to the checker and
may be switched from. A failed check finishes the attack: `FightSkill.StopAttack`
clears the lock, the weapons keep naming what they fired at, the skill cools
for its cooling time, and it then enters idle with its targets cleared and
searches again. A unit therefore shows no lock for its cooling time plus one
idle tick. A group of weapons drops every slot with the lock.

**A unit turns past a felled block to its lock.** What a unit's body turns to
is `MotionController.CalculateTargetDirection`: the lock, replaced by the
attack target only while `FightSkill.TryGetValidAttackTarget` finds it alive.
So a unit still swinging at a block that has fallen turns through that swing
towards the unit behind it, not towards the block. The same holds for a blow
that fells the block: `MotionAttackState` releases it and then turns
(`AttackRotate`). A unit with a body turns its weapons, not its root.

Only a unit whose skill is attacking has an attack to finish. A unit closing on
a block by its motion, its skill still idle, goes straight on to what is
behind the block the tick after another unit fells it: the idle skill asks
what to fire at every tick, and the answer has changed.

**A wall is never a target a unit looks for.** A construction is never a
unit's lock target, and is its attack target whenever one is in its way; a
tower is locked from the first tick, so the exclusion belongs to the wall and
not to buildings, which is what the wall's own `enable_search_target: false`
says. A unit attacks a wall without ever searching for one, and the next
section says how.

## A wall is attacked because it is in the way

A unit keeps its target and shoots what stands between them: its **lock
target** stays the unit behind the wall while its **attack target** becomes a
block, and different members of one formation pick different blocks from the
same lock.

```text
among the enemy's constructions,
  keep the ones within attack range of the attacker, edge to edge
      (centre distance <= range + attacker radius + block radius),
  keep the ones within 11.5 metres of the line from the attacker to its target,
  take the nearest of those to the attacker.
```

**It is asked wherever the skill asks what to fire at**, which is one method,
`FightSkill.SearchAttackTarget`: it takes the lock and hands the weapons a
shield, else a wall in the way, else the lock itself.
`WallConstructionTargetChecker` has no other caller. The skill asks it every
tick it is idle with a lock, not when the mech's lock is searched on its own
timer, so a unit engages a wall the tick the wall comes into reach; and inside
`SkillAttackableChecker.Check`, which the skill runs every tick an attack is
prepared and, while attacking, between blows and while a blow winds up. What
the check then does with the answer is the same for a block as for a unit:
what the weapons fire at must be in the attack area, or the attack ends.

So a block coming into the line ends an attack being prepared, and a nearer
block entering the line ends an attack between blows; the unit reads idle with
no lock for a tick and takes the block after. A unit that can switch targets
quickly and has the new block in its attack area goes on without a pause, onto
a block the tick one enters its line and off it the tick it leaves. A lock that
dies behind a block is searched for again at once, the block kept if it stands
in the way of the new one too.

It is **the nearest wall the line reaches**, not the nearest wall and not the
wall nearest the line. **The width does not belong to the attacker**: units of
different collision radius take and pass up blocks at the same distance from
the line. **The reach does belong to the attacker, edge to edge**: its range
plus its radius plus the block's. The distance is the build's fast fixed-point
magnitude, which reads a little short of the true one.

**A blow and a beam take a wall the way a shot does**, each on the block in its
own line. A block falls after every hit its tick resolves, a blow's as a
shot's; a beam's falls straight after its damage.

**Every weapon of a group takes a wall, the lock follows none of them, and an
air unit's shot is taken too.** Once a unit with several weapon slots is
attacking, every slot points at the block in its way while its lock stays on
the unit behind the wall. The core takes the block first and the others follow
when the group allocates its children, the same delay the group shows against
units.

**A shot at a block splashes the next one.** A splash that reaches a
neighbouring block's edge damages it too: a shot at a building takes every
other enemy building whose edge its splash reaches.

**A splash does not care what it was aimed at.** A shot at a block takes the
units standing on it, and a shot at a unit takes a block behind it, even one
beyond the shooter's reach and so out of its line of fire, for the shot's full
damage. A splash takes every enemy unit and building within reach, in the
order the target trees hold them. The deaths and falls it causes come at the
end of the tick, in the order they were struck, and a unit that dies leaves the
tree at once.

**A block falls after the shot that felled it is recorded.** The three events
one hit produces arrive in the order `damage`, `projectile_removed`,
`building_destroyed`, and the destruction comes after *every* projectile the
tick resolves, not after its own: a tick that lands two shots reads
`damage`, `removed`, `removed`, `destroyed`.

## What the footprint is and what it is not

A row's `grid_column_count` and `grid_row_count` are cells of the ten-metre
deployment grid, and ten times them is the footprint
[`layout.md`](../spec/document/layout.md) checks a placement against. The
extraction refuses any row the layout catalog names whose grid disagrees with
the footprint the catalog pins, and the catalog's footprints were read off
placements the game accepted, so the two are independent.

The footprint is **not** the objects. A turret stands in a box larger than the
footprint it reserves, and a wall's five blocks span less than its footprint.
What a deployment may overlap and what a shot may hit are different questions
with different boxes.

## What a recording says and what it does not

A building row carries `GameRiver.BuildingType`: `Normal`, `EnergyTower`,
`ResearchCenter` or `Special`. Every construction is `Special`, so **a
recording does not say which construction a row came from**. Two things follow.

- A row is named by matching it back to the layout the recording embeds, which
  is what `fight buildings` does and why it refuses a row that matches no
  single placement.
- A wall block and a turret are told apart by their life and their box, not by
  a type. Nothing in a recording distinguishes two of the build's rows where
  they share both.

The map's own buildings are the exception and are named: each side gets one
`EnergyTower` and one `ResearchCenter`, whose life and box
[`config/towers.yaml`](../../config/towers.yaml) states.

## Evidence

### Recorded

- A Defensive Wall aside from a fight is five blocks at their spacing and
  changes nothing else: `tests/construction/regressions.mcscript`.
- A unit shoots the nearest of the blocks its line reaches, within the width
  and its reach: `tests/construction/regressions.mcscript`.
- A tick that lands two shots reports the fallen block after both:
  `tests/construction/regressions.mcscript`.
- A unit with four weapon slots takes the block in its way with every slot,
  splashes the next block, and drops its slots with the lock when a block falls:
  `tests/construction/regressions.mcscript`.
- A blow takes the block in its line, and the unit reads idle for a tick before
  the next: `tests/construction/regressions.mcscript`.
- A beam takes the block in its line, a block holds off a unit of the other
  side, and a unit whose beam fells a block turns onto its lock that tick:
  `tests/construction/regressions.mcscript`.
- The side that placed a wall walks through it, and the blocks still take
  places among its neighbours: `tests/construction/regressions.mcscript`.
- The other side stops at a wall and takes it down, changing blocks between
  blows and going on when a block it had not struck falls:
  `tests/construction/regressions.mcscript`.
- A splash takes the units on a block it hits and a block behind a unit it
  hits, in target-tree order, and a unit re-locks when its splash kills its
  lock: `tests/construction/regressions.mcscript`.

### Read

- A placement creates one object per count, each a building of its own:
  `FightConstructionSystem.Create`,
  `GroupConstructionManager.CreateConstruction`,
  `FightController.CreateFightConstruction`.
- A dead construction fails the attack check outright, and a failed check
  clears the lock: `SkillAttackState.CheckAttackable`,
  `SkillAttackableChecker.Check`, `FightSkill.StopAttack`.
- A unit turns to its lock unless its attack target is alive, and a blow that
  fells its target releases it before turning:
  `MotionController.CalculateTargetDirection`,
  `FightSkill.TryGetValidAttackTarget`, `MotionAttackState.AttackRotate`.
- The weapons are handed a shield, else a wall in the way, else the lock:
  `FightSkill.SearchAttackTarget`,
  `WallConstructionTargetChecker.CheckWallConstruction`,
  `WallConstructionTargetChecker.GetNearestWall`.
- A wall is not searched for: `ConstructionData.enableSearchTarget`.
- Collision is a property of a building's data:
  `BuildingData.EnableCollision`.
- Every construction is a special building: `BuildingType.Special`.

### Not established

- **The block spacing as a rule.** It is one wall's, measured; the method that
  computes it dispatches through slots the dump does not name.
- **The line's width exactly.** The recordings bracket it between 11.447 and
  11.507 metres, and 11.5 is written. One recorded decision between two blocks
  a centimetre apart in distance went against the centre distance, so the sort
  key is not quite a 2D centre distance.
- **What a release needs beyond the opening's own constructions.** A release
  away from them has been refused, so every construction placed so far is one
  an opening dealt. No Magnetic Barrier, the only row with two rows of objects,
  has been placed.
- **A unit too large for the gaps between blocks**, and every construction
  other than the wall moving against a unit.
- **That a turret can be locked.** Its row answers
  `enable_search_target: true`; no recording has put a unit where it could
  target an enemy turret.
- **Whether a unit's weapons follow its lock** when it turns past a felled
  block.
- **What a Magnetic Barrier does**, what destroying a construction pays, and
  what any of it costs; [`economy.yaml`](../../config/economy.yaml) carries the
  recovery price and nothing here does.
