# What a construction is

[简体中文](constructions.zh.md)

This index is pinned to game build 2259. It states what a construction placed
by a layout becomes in a fight, what numbers it carries, whether it obstructs
anything, and which of that has been measured against the game rather than only
read out of it. What a construction *does* on its own — a turret's skill, a
barrier's slow — is not here.

The machine-readable table is
[`config/constructions.yaml`](../../config/constructions.yaml), and
`scripts/extract-constructions.py` writes it from
`ConfigDataContainer.constructionDatas`. The build holds **nine** rows; a
layout can name **four** of them, and a ranked match deals with three.

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

## A Defensive Wall is a target and not an obstacle

A block's box is not a collider. Measured from both sides, with the same unit
and the same geometry: a Crawler comes within **0.567 metres** of a block's
centre when its own side placed the wall, and within **0.711 metres** when the
other side did. A Crawler carries 1.5 metres of inner radius against a block's
4, so an obstacle would have held it at 5.5 and it was inside the block
instead. Units walk over a wall.

What differs between the two sides is targeting, not collision. The side that
did not place it **stops and attacks it**: twenty-four Crawlers take four of
the five blocks down and leave the fifth at 401 of 1112. The side that placed
it walks across and leaves all five at full life. A block is one target with
one life, so a wall is destroyed a block at a time and an attacker that
overkills one block does not carry the excess to the next — a Marksman's 2329
against a block's 1112 is recorded as 1112 of damage.

The wall's own description says it sinks into the ground for a friendly unit.
Nothing a recording holds shows that happening: `available`, `targetable` and
`collision_enabled` read true for every block at every tick of both fights, and
`collision_enabled` is `BuildingData.EnableCollision`, a property of the data
rather than a state. Whatever the sinking is, it is not what lets a unit
through, because the other side's units go through as well.

**The scope is a Crawler.** Whether a unit large enough not to fit between two
blocks is held by them has not been measured, and neither has any construction
other than the wall.

**What makes a unit attack a wall is not established.** A wall is not something
a unit searches for — its row answers `IsEnableSearchTarget` with false, and
Crawlers deployed opposite one lock onto the unit behind it at tick one, not
onto the wall. They attack it later all the same, and a Marksman standing
opposite one destroys a block without ever having searched for it. The build
carries a `WallConstructionTargetChecker` and what it decides has not been
read.

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
`EnergyTower` and one `ResearchCenter`, 3400 of life in a 20 × 20 box.

## Scope

Everything above is build 2259 and the 1v1 board. It covers what stands when a
fight begins, and whether a Crawler-sized unit is held by a Defensive Wall. It
does not cover what makes a unit attack a wall, what a turret's skill is, what a Magnetic Barrier does to what
comes near it, what a unit too large for the gaps between blocks does, what
destroying a construction pays, or what any of it costs —
[`economy.yaml`](../../config/economy.yaml) carries the recovery price and
nothing here does.

The readings were taken with
[`tests/layouts/construction/shape.mcscript`](../../tests/layouts/construction/shape.mcscript),
whose control is a side that places nothing and reads back two towers, and
[`wall.mcscript`](../../tests/layouts/construction/wall.mcscript), which asks
both sides the same question with the same unit.
