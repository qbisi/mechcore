# What a construction is

What a construction placed by a layout becomes in a fight, what numbers it carries, whether it obstructs
anything, and which of that has been measured against the game rather than only
read out of it. The recordings behind it are build 1.11.1.3.2259's. What a
construction *does* on its own is not here: a turret's
skill is [`turrets.md`](turrets.md)'s, and a barrier's slow is nobody's yet.

The machine-readable table is
[`config/constructions.yaml`](../../config/constructions.yaml), and
`scripts/extract-constructions.py` writes it from
`ConfigDataContainer.constructionDatas`. A layout can name four of its rows'
constructions, and a ranked match deals with three.

## A construction is several objects

One `constructions` entry is one placement and `count` objects.
`FightConstructionSystem.Create` answers an
`IReadOnlyList<FightConstruction>`, `GroupConstructionManager.CreateConstruction`
builds each of them through `FightController.CreateFightConstruction`, and a
recording holds each as its own building row. So every number in the table is
**one object's**:

| Construction | `count` | Life each | Box | Damage |
| --- | ---: | ---: | ---: | ---: |
| Defensive Wall | 5 | 1112 | 8 × 8 | — |
| Anti-Armor Turret | 1 | 5028 | 24 × 24 | 2748 |
| Rapid-Fire Turret | 1 | 3650 | 24 × 24 | 82 |
| Magnetic Barrier | 10 | 300 | 8 × 8 | — |

A wall therefore holds 5560 of life spread over five targets that are killed
one at a time, not 1112 and not 5560 on one object. The box is twice the row's
`radius`, and the row's `radius` is the only length in the table a recording
reports back.

Each object also carries its own **durability**, a count of how many times it
may be destroyed before it is gone for good, and a `combination` that ties the
objects of one placement together. The four a layout names all carry
`has_durability: false`; the rows that carry it are the build's own.

## Where the objects stand

Measured, for the Defensive Wall, at `(-140, -55)`:

```text
-164   -152   -140   -128   -116      one row, on the placement's own z
```

Five blocks 12 metres apart, the middle one on the placement itself, spanning
48 metres of the 60 the footprint reserves. A turret is one object and stands
exactly where it was placed.

**This is one reading and it does not generalise yet.** The two fields that
look like they should give the spacing do not: `block_width: 15` and
`space: 2` span `5 × 15 + 4 × 2 = 83` metres across a footprint 60 wide. The
build computes the offsets in `GroupConstructionManager.CreateConstruction`,
whose disassembly divides twice and multiplies a row index by ten — the ten
metres of the deployment grid — but the index carries no method bodies and the
interface slots it dispatches through cannot be named from it.

The one construction that would separate a rule from a fitted number is the
Magnetic Barrier, the only row whose `real_row_count` is 2, and **nothing has
placed one**. Its release is refused: the action is created and performed and
`ConstructionManager` then holds no element at that index. That is not a fact
about the barrier — the same refusal answers a Defensive Wall released at
`(-140, -105)` or `(-260, -295)`, and a Defensive Wall at `(-140, -55)` under a
seed whose opening does not already stand there. **What a release needs beyond
the opening's own constructions is not established.** Until it is, every
construction this build can put on a board is one its opening dealt, and the
spacing is a measurement of a wall rather than a formula.

## A Defensive Wall is an obstacle only to the other side

**To the side that placed it, a wall is not there to avoid.** A Crawler of
that side comes within **0.567 metres** of a block's centre, against the 5.5 an
obstacle would have held it at, and walks on. The block is not gone from its
avoidance, though: it still takes one of the twenty neighbours a unit
considers, crowding out a unit it would otherwise have avoided, and only then
is dropped. The wall's own description says it sinks
into the ground for a friendly unit, and that is what the recording shows.

**To the other side, each block is an immovable obstacle.** It stands on the
collider layer of the wall's `pathfinding_collider_priority`, 5, with its own
4-metre box for a radius, and a unit of the other team avoids it the way it
avoids an opponent. A Steel Ball of `wall-laser.yaml` overlapping block 3 is
pushed off it on exactly the ticks and at exactly the velocities that makes
it, and treating the block as no obstacle, or as one to both sides, or with
the 7-metre `path_radius`, each fails somewhere in the six wall fights.
Opponent avoidance looks 0.01 seconds ahead, so it holds a unit off a block
and does not hold back a crowd: a Crawler of the other side still reached
**0.711 metres** of a block's centre with 23 others behind it, and the
simulator puts the same Crawler at the same 0.711 metres on the same tick.

What differs between the two sides is targeting, not collision. The side that
did not place it **stops and attacks it**: twenty-four Crawlers take four of
the five blocks down and leave the fifth at 401 of 1112. The side that placed
it walks across and leaves all five at full life. A block is one target with
one life, so a wall is destroyed a block at a time and an attacker that
overkills one block does not carry the excess to the next — a Marksman's 2329
against a block's 1112 is recorded as 1112 of damage.

The sinking is not in the building's data: `available`, `targetable` and
`collision_enabled` read true for every block at every tick, and
`collision_enabled` is `BuildingData.EnableCollision`, a property of the data
rather than a state. It is in who avoids the block.

**The scope is two units.** A Crawler of the side that placed the wall and a
Steel Ball of the other side are what was measured moving against a block;
a unit too large for the gaps between blocks, and every construction other
than the wall, have not been.

**A block that falls ends the attack on it, and the lock with it.** This is
the build's attack state, not something about walls. `SkillAttackState` checks
its attack target between blows and while the next one winds up, and a dead
attack target of the construction class (`FightConstruction`) fails that
check outright, where a dead unit, or a dead tower, goes on to the checker and
may be switched from. A failed check finishes
the attack: `FightSkill.StopAttack` clears the lock, the weapons keep naming
what they fired at, the skill cools for its cooling time, and it then enters
idle with its targets cleared and searches again. A capture of the skill's own
state reads each step: the Marksman of `wall-line-of-fire.yaml` is attacking
while its shot flies and cooling, lockless, on the fallen block the tick after
it lands; the Rhino of `wall-rhino.yaml` finishes its swing on the fallen
block, reads idle with nothing named for a tick, and attacks block 4 the next.
How long the unit shows no lock is its cooling time plus that idle tick: none
for the Rhino, the Crawlers and the Steel Balls, whose cooling is 0, and four
ticks for a Marksman's 0.2 seconds — the four `wall-line-width.yaml` read. A
group of weapons drops every slot with the lock (below).

**A unit turns past a felled block to its lock.** What a unit's body turns to
is `MotionController.CalculateTargetDirection`: the lock, replaced by the
attack target only while `FightSkill.TryGetValidAttackTarget` finds it alive.
So a Crawler or a Rhino still swinging at a block that has fallen turns
through that swing towards the unit behind it, not towards the block: the
Crawlers of `wall-block.yaml` and the Rhino of `wall-rhino.yaml` on build 2.0.
Build 2259 had no such check and went on facing the block. The same holds for
a blow that fells the block: `MotionAttackState` releases it and then turns
(`AttackRotate`), so the Steel Ball of `wall-laser.yaml` turns onto the
Marksman on the tick its beam fells block 4. A unit with a body turns its
weapons, not its root, and whether they follow the lock is not recorded.

Only a unit whose skill is attacking has an attack to finish. Two Crawlers of
`wall-block.yaml` closing on block 4, attacking by their motion but with their
skill still idle, go straight on to the Marksman behind the wall the tick after
another Crawler fells it: the idle skill asks what to fire at every tick, and
the answer has changed.

**A wall is never a target a unit looks for.** Across the ten recordings under
`tests/construction/`, 931 ticks of which have a wall standing, a
construction is a unit's lock target **zero** times and its attack target 1482
times. A tower is a lock target 6013 times, from the first tick, so the
exclusion belongs to the wall and not to buildings — which is what the wall's
own `enable_search_target: false` says. A unit attacks a wall without ever
searching for one, and the next section says how.

What is **not** measured is the other side of that field. A turret's row
answers `enable_search_target: true`, and no recording has put a unit where it
could target an enemy turret, so that a turret *can* be locked is read from the
table rather than seen.

## A wall is attacked because it is in the way

A unit keeps its target and shoots what stands between them. A recording shows
both at once: the **lock target** stays the unit behind the wall while the
**attack target** becomes a block, and different members of one formation pick
different blocks from the same lock.

The rule, measured:

```text
among the enemy's constructions,
  keep the ones within attack range of the attacker, edge to edge
      (centre distance <= range + attacker radius + block radius),
  keep the ones within 11.5 metres of the line from the attacker to its target,
  take the nearest of those to the attacker.
```

**It is asked wherever the skill asks what to fire at**, which is one method,
`FightSkill.SearchAttackTarget`: it takes the lock and hands the weapons a
shield, else a wall in the way, else the lock itself. `WallConstructionTargetChecker`
has no other caller. The skill asks it every tick it is idle with a lock —
not when the mech's lock is searched, which runs on its own ten-tick timer, so
a Wraith closing on a wall engages it the tick the wall comes into reach — and
inside `SkillAttackableChecker.Check`, which the skill runs every tick an attack
is prepared and, while attacking, between blows and while a blow winds up.
What the check then does with the answer is the same for a block as for a
unit: what the weapons fire at must be in the attack area, or the attack ends.

So a block coming into the line ends an attack being prepared: a Steel Ball of
`wall-laser.yaml`, preparing a beam on the Marksman behind the wall, reads idle
with no lock and no weapon target the tick block 3 comes within the line, and
the tick after it is on block 3 with its lock back on the Marksman. A nearer
block ends an attack between blows the same way: a Crawler of
`wall-block.yaml`, pushed along the wall while it strikes block 4, finds block
3 nearer in its line the tick after its swing ends — another in the wait before
its next blow — reads idle with no lock for a tick, and strikes block 3 after.
A unit that can switch targets quickly and has the new block in its attack
area goes on without a pause: the Arclight of `wall-splash-line.yaml` turns
from a block to the Crawler behind it the tick the block leaves its line, and
onto a block the tick one enters it. And a lock that dies behind a block is
searched for again at once, the block kept if it stands in the way of the new
one too: the Arclight of `wall-splash.yaml`, shooting block 5 for a Crawler its
own splash kills, is on another Crawler and still on block 5 the tick after.

It is **the nearest wall the line reaches**, not the nearest wall and not the
wall nearest the line. One Marksman settles that: with the nearest block 75
metres away and 39 off the line, and a block dead on the line 92 metres away
and 0.12 off, it destroyed neither. It destroyed the block 86 metres away and
9.9 off — the nearest of the ones the line reaches.

**The width does not belong to the attacker.** A Marksman carries 8 metres of
collision radius, a Fang 2 and a Farseer 11, so `attacker + block` would give
them 12, 6 and 15 metres. It gives them the same number instead: the Fang's
members take blocks 10.0 and 10.7 metres off the line, which 6 would have
excluded, and the Farseer takes nothing while a block sits 13.13 off, which 15
would have included.

**The reach does belong to the attacker, edge to edge.** A Crawler reaches 6
metres and stops considering a wall beyond 12, its range plus its radius of 2
plus the block's 4; a Wraith reaches 60 and attacks a block 73.8 metres off,
inside its 60 + 11 + 4. A constant allowance fits the Crawler and not the
Wraith, and an earlier version of this document said "range + 7" because only
the Crawler had been asked. The distance is the build's fast fixed-point
magnitude, which reads a little short: a Crawler whose centre is 12.00023
metres from a block's engages it.

The width is a bracket rather than a reading, from 94 decisions across six unit
types — Crawler, Marksman, Fang, Farseer, Wraith and Fortress — taken at the
first tick of each fight and before the first block of the Crawler fight fell:

| | Bracket | Written as |
| --- | --- | --- |
| width | `[11.447, 11.507)` metres | 11.5 |
| reach | `range + attacker radius + block radius + [0, 1.5]` | edge to edge |

Both explain 93 of the 94; the odd one out is a tie, two blocks 10.94 and 10.95
metres away, so it says the sort key is not quite a 2D centre distance rather
than anything about the width. Those decisions bracketed the width at
`[10.8, 11.8]`; one Steel Ball closing on block 3 a few centimetres a tick
narrowed it, passing the block by at 11.507 metres for nine ticks and taking it
on the tick after 11.447. An earlier version of this document wrote 11 because a
wall's `path_radius` is 7 and its `radius` 4, a reading of the table that the
Steel Ball ruled out.

**A blow and a beam take a wall the way a shot does.** A Rhino charging a
Marksman behind the wall takes the block in its line, one hit each, the damage
event capped at the life the block had — 1112 against its 3560 — and its 6
metres of splash reach no neighbour, whose edge is 8 away. Four Steel Balls
take three blocks with the beam's ramp, 2, 3, 8, 17 and on, each on the block
in its own line, the last hit capped at what is left. A block falls after
every hit its tick resolves, a blow's as a shot's: the Crawlers of
`wall-block.yaml` read three more blows between the one that fells block 5
and `building_destroyed`. A beam's falls straight after its damage.

**Every weapon of a group takes a wall, the lock follows none of them, and an
air unit's shot is taken too.** A Wraith flies and carries four weapon slots.
Once it is attacking, all four point at the block in its way while its lock
stays on the Marksman behind the wall — so whatever a group's weapons tell the
mech's lock, a wall in the way is not part of it. The core takes the block
first and the other three follow eight ticks later, when the group allocates
its children, which is the same delay the group shows against units. A Fortress
does the same with the block in its line.

**A shot at a block splashes the next one.** The Wraith's 8 metres of splash
reach the neighbouring block, whose edge is exactly 8 metres from the one it
aims at: one projectile removed at block 4 reads `damage` 381 on block 4 and
381 on block 5. An earlier version of this document took that for two slots
firing at two blocks; every slot was on block 4. The simulator reproduces it —
`wall-weapon-group.yaml` over all 242 ticks — by giving a shot at a building
every other enemy building whose edge its splash reaches.

**A splash does not care what it was aimed at.** An Arclight shooting a block
in the way of Crawlers takes the Crawlers standing on it, and a shot at a
Crawler takes a block of its wall behind it, beyond the Arclight's reach and
so out of its line of fire, for the shot's full damage — block 5 reads 747 at
tick 77 of `wall-splash-behind.yaml`. A splash takes every enemy unit and
building within reach, in the order the target trees hold them, walls
included. The deaths and falls it causes come at the end of the tick, in the
order they were struck, and a unit that dies leaves the tree at once. The
simulator reproduces all three `wall-splash*.yaml` fights over every tick, in
physics and in content.

**A block falls after the shot that felled it is recorded.** The three events
one hit produces arrive in the order `damage`, `projectile_removed`,
`building_destroyed`, and the destruction comes after *every* projectile the
tick resolves, not after its own: a tick that lands two shots reads
`damage`, `removed`, `removed`, `destroyed`.

## What the footprint is and what it is not

A row's `grid_column_count` and `grid_row_count` are cells of the ten-metre
deployment grid, and ten times them is the footprint
[`layout.md`](../spec/document/layout.md) checks a placement against: 6 × 1 is
the wall's 60 × 10, 2 × 2 is a turret's 20 × 20. The extraction refuses any row
the layout catalog names whose grid disagrees with the footprint the catalog
pins, and the catalog's footprints were read off placements the game accepted,
so the two are independent.

The footprint is **not** the objects. A turret reserves 20 × 20 and stands in a
24 × 24 box; a wall reserves 60 × 10 and its five 8 × 8 blocks span 48. What a
deployment may overlap and what a shot may hit are different questions with
different boxes.

## What a recording says and what it does not

A building row carries `GameRiver.BuildingType` — `Normal`, `EnergyTower`,
`ResearchCenter`, `Special` — and every construction is `Special`, so **a
recording does not say which construction a row came from**. Two things follow.

- A row is named by matching it back to the layout the recording embeds, which
  is what `fight buildings` does and why it refuses a row that matches no
  single placement.
- A wall block and a turret are told apart by their life and their box, not by
  a type. Nothing in a recording distinguishes the build's nine rows from each
  other where two of them share both.

The map's own buildings are the exception and are named: each side gets one
`EnergyTower` and one `ResearchCenter`, whose life and box
[`config/towers.yaml`](../../config/towers.yaml) states.

## Scope

Everything above is recorded on build 2259 and the 1v1 board. It covers what stands when a
fight begins, whether a Crawler-sized unit is held by a Defensive Wall, and
which wall a unit shoots when one is in its way. What a turret's skill does is
[`turrets.md`](turrets.md)'s. It does not cover what a Magnetic Barrier does to what
comes near it, what a unit too large for the gaps between blocks does, what
destroying a construction pays, or what any of it costs —
[`economy.yaml`](../../config/economy.yaml) carries the recovery price and
nothing here does.

The readings were taken with
[`tests/construction/shape.mcscript`](../../tests/construction/shape.mcscript),
whose control is a side that places nothing and reads back two towers, and
[`wall.mcscript`](../../tests/construction/wall.mcscript), which asks
both sides the same question with the same unit, and
[`line-of-fire.mcscript`](../../tests/construction/line-of-fire.mcscript),
whose four layouts separate the nearest wall from the wall in the way, bracket
how wide the way is, and ask a flying unit with four weapon slots the same
question.
