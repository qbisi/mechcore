# Unit standard layouts

[`plan/units.md`](../../plan/units.md) defines when a unit is supported
without technology: six standard layouts, each recorded by the game with two
seeds and played back by the simulator tick for tick, in every field. This directory holds those
layouts, one directory per unit, the script that records them, and the
gameless `regressions.mcscript` that pins every recording the simulator
reproduces. CI runs the regressions; the record script needs the game. It
fights each layout without a scene, as a
[layout replay](../../docs/spec/document/layout-replay.md), and every pin
hashes the same recorded that way as in the Training Ground.

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
side cannot hit back the other fields enough formations to win first: a
Rhino or a Crawler against three Wasp formations, three Wasp formations
against a Rhino or a Crawler, and five Wraith formations against a Rhino,
where three lost a tower. M5's target stands nearer than either enemy tower, and an air unit's
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

## The other seven

Arclight, Fang, Mustang, Steel Ball, Wraith, Stormcaller and Phoenix have
the same six layouts against the reference units: 84 recordings, all of
which play back exactly and are pinned, so every one of them meets the
definition. Each cell gives the recording's ticks.

| Unit | Layout | Seed 4242 | Seed 1787720817 |
| --- | --- | ---: | ---: |
| arclight | `m1-mirror` | 389 | 386 |
| arclight | `m2-rhino` | 216 | 214 |
| arclight | `m3-crawler` | 199 | 185 |
| arclight | `m4-wasp` | 187 | 186 |
| arclight | `m5-rotated` | 204 | 202 |
| arclight | `m6-formations` | 373 | 377 |
| fang | `m1-mirror` | 340 | 399 |
| fang | `m2-rhino` | 358 | 352 |
| fang | `m3-crawler` | 353 | 318 |
| fang | `m4-wasp` | 251 | 247 |
| fang | `m5-rotated` | 365 | 328 |
| fang | `m6-formations` | 552 | 585 |
| mustang | `m1-mirror` | 238 | 280 |
| mustang | `m2-rhino` | 337 | 282 |
| mustang | `m3-crawler` | 210 | 203 |
| mustang | `m4-wasp` | 183 | 181 |
| mustang | `m5-rotated` | 378 | 439 |
| mustang | `m6-formations` | 255 | 260 |
| steel_ball | `m1-mirror` | 314 | 318 |
| steel_ball | `m2-rhino` | 180 | 182 |
| steel_ball | `m3-crawler` | 454 | 445 |
| steel_ball | `m4-wasp` | 355 | 338 |
| steel_ball | `m5-rotated` | 173 | 171 |
| steel_ball | `m6-formations` | 530 | 507 |
| wraith | `m1-mirror` | 430 | 429 |
| wraith | `m2-rhino` | 287 | 286 |
| wraith | `m3-crawler` | 247 | 201 |
| wraith | `m4-marksman` | 265 | 264 |
| wraith | `m5-rotated` | 252 | 247 |
| wraith | `m6-formations` | 312 | 330 |
| stormcaller | `m1-mirror` | 120 | 248 |
| stormcaller | `m2-rhino` | 315 | 291 |
| stormcaller | `m3-crawler` | 352 | 301 |
| stormcaller | `m4-wasp` | 213 | 211 |
| stormcaller | `m5-rotated` | 273 | 272 |
| stormcaller | `m6-formations` | 462 | 449 |
| phoenix | `m1-mirror` | 77 | 76 |
| phoenix | `m2-rhino` | 139 | 147 |
| phoenix | `m3-crawler` | 463 | 462 |
| phoenix | `m4-marksman` | 100 | 96 |
| phoenix | `m5-rotated` | 105 | 103 |
| phoenix | `m6-formations` | 301 | 296 |

The Wraith's M3 with seed 1787720817 was the last to play back. Its
Wraith drifted 100° off its facing at tick 168 and the game still published
its full 10 m/s: the Wraith is one of the units `MechData.PreProcess` marks
free-moving, which never slow for their facing. Behind it, at tick 178, its
core took a Crawler three of its siblings held, and the sibling the
simulator moved off it kept it in the game.

## The eighteen released together

Every other unit but the three that cost 800 (War Factory, Abyss, Mountain)
was released to the simulator at once, and recorded in its six layouts with
both seeds before any of it was fixed: 216 recordings. Sandworm is not among
them, as its configuration cannot state a unit that burrows. One unit is
still refused by name, for a main skill the kernel has no way to fire:
Hacker's control beam.

All 204 of the other fights play back exactly and are pinned:

| Unit | Pinned of 12 |
| --- | ---: |
| centurion | 12 |
| farseer | 12 |
| fire_badger | 12 |
| fortress | 12 |
| hound | 12 |
| melting_point | 12 |
| overlord | 12 |
| phantom_ray | 12 |
| raiden | 12 |
| sabertooth | 12 |
| scorpion | 12 |
| sledgehammer | 12 |
| tarantula | 12 |
| typhoon | 12 |
| void_eye | 12 |
| vortex | 12 |
| vulcan | 12 |

Each mechanism the recordings exposed is named in
[`combat.md`](../../docs/rules/combat.md). Two of them were read off
[`skill-state.mcscript`](skill-state.mcscript), which records five of the
fights with each skill's state beside the turret's rotation, which the MCFR
carries as `turret_rotation`: the attack angle is measured from it, and it
showed the game turning exactly as the simulator did and parting only on the
state change. The fifth, the Phantom Ray's M3 with seed 4242, showed three
Phantom Rays still cooling, and still naming the dead last Crawler, for the
five ticks the fight runs on after it is won.

The Raiden's twelve were read against [`../raiden/`](../raiden/README.md),
which records them again with each weapon slot's lock and state.

The Overlord's M3 fields five formations in a row and its M6 two side by side:
with three in a column, or two one behind the other, the Crawlers felled a
tower before the Overlords could stop them.
