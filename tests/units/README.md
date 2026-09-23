# Unit standard layouts

`plan.md` defines when a unit is supported without technology: six standard
layouts, each recorded by the game with two seeds and played back by the
simulator tick for tick, physics and content. This directory holds those
layouts, one directory per unit, the script that records them, and the
offline `regressions.mcscript` that pins every recording the simulator
reproduces. CI runs the regressions; the record script needs the game.

| Layout | What it measures |
| --- | --- |
| M1 `m1-mirror` | the unit against itself, 200 m apart: formation, approach, first blow, damage, death |
| M2 `m2-rhino` | a single large ground target |
| M3 `m3-crawler` | a swarm of ground targets |
| M4 `m4-wasp` / `m4-marksman` | the air relation: a ground unit against Wasps, an air unit against a Marksman |
| M5 `m5-rotated` | the formation rotated, its target off to one side |
| M6 `m6-formations` | two formations against a Crawler swarm and a Marksman |

**A standard layout ends before any tower falls.** A tower's loss puts a
debuff on its side, which is a mechanism of its own and not the unit's.
A side that cannot hit the other walks to the enemy towers, so where one
side cannot hit back the other fields three formations: a Rhino or a Crawler
against three Wasp formations, three Wasp formations against a Rhino or a
Crawler. M5's target stands nearer than either enemy tower, and an air unit's
M5 target is a Marksman, which shoots back. With one Wasp formation, or with
the first M5 placement, a tower fell in every one of those fights and the
recording parted from the simulator on that tick.

A unit that is its own opponent in a cell does not record it twice: the Rhino
has no M2, the Crawler no M3.

## The reference units

Rhino, Crawler, Wasp and Marksman are the opponents every other unit's
layouts use, so they are held to the definition first. Ticks per recording:

| Unit | Layout | Seed 4242 | Seed 1787720817 |
| --- | --- | ---: | ---: |
| rhino | `m1-mirror` | 210 | 209 |
| rhino | `m3-crawler` | 276 | 293 |
| rhino | `m4-wasp` | 239 | 258 |
| rhino | `m5-rotated` | 202 | 199 |
| rhino | `m6-formations` | 377 | 386 |
| crawler | `m1-mirror` | 368 | 345 |
| crawler | `m2-rhino` | 313 | 304 |
| crawler | `m4-wasp` | 417 | 404 |
| crawler | `m5-rotated` | 295 | 294 |
| crawler | `m6-formations` | 375 | 400 |
| wasp | `m1-mirror` | 217 | 219 |
| wasp | `m2-rhino` | 244 | 304 |
| wasp | `m3-crawler` | 397 | 388 |
| wasp | `m4-marksman` | 195 | 195 |
| wasp | `m5-rotated` | 189 | 187 |
| wasp | `m6-formations` | 433 | 398 |
| marksman | `m1-mirror` | 85 | 83 |
| marksman | `m2-rhino` | 213 | 212 |
| marksman | `m3-crawler` | 239 | 239 |
| marksman | `m4-wasp` | 195 | 187 |
| marksman | `m5-rotated` | 203 | 201 |
| marksman | `m6-formations` | 243 | 238 |

All 44 play back exactly and are pinned, so all four reference units meet the
definition. The Rhino's M6 with seed 1787720817 was the last: a Crawler pushed
out of reach during its backswing, and back on the next tick, starts its next
blow on the tick it returns, which a skill-state capture of the game read and
the simulator had deferred a tick
([`architecture.md`](../../docs/spec/simulation/architecture.md)).

Without technology a unit has only its main skill: `FightMech`'s constructor
adds one skill, `mechData.GetMainSkillID()`, and extra skills reach a mech
only through a technology's `ExtraSkillSystem`. So the definition's second
condition is met by the main attack alone, and each unit's main skill row
agrees with its `config/units/` file.
