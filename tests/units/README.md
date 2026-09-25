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
the same six layouts against the reference units: 84 recordings, of which
76 play back exactly and are pinned. Arclight, Mustang and Phoenix meet the
definition. A cell that parts gives the
recording's ticks and the first tick the simulator differs on.

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
| steel_ball | `m6-formations` | 530, parts at 1 | 507 |
| wraith | `m1-mirror` | 430 | 429 |
| wraith | `m2-rhino` | 287, parts at 225 | 286, parts at 248 |
| wraith | `m3-crawler` | 247, parts at 156 | 201, parts at 136 |
| wraith | `m4-marksman` | 265 | 264 |
| wraith | `m5-rotated` | 252 | 247 |
| wraith | `m6-formations` | refused | 330, parts at 124 |
| stormcaller | `m1-mirror` | 120 | 248 |
| stormcaller | `m2-rhino` | 315 | 291 |
| stormcaller | `m3-crawler` | 352 | 301 |
| stormcaller | `m4-wasp` | 213 | 211 |
| stormcaller | `m5-rotated` | 273 | 272 |
| stormcaller | `m6-formations` | 462, parts at 1 | 449 |
| phoenix | `m1-mirror` | 77 | 76 |
| phoenix | `m2-rhino` | 139 | 147 |
| phoenix | `m3-crawler` | 463 | 462 |
| phoenix | `m4-marksman` | 100 | 96 |
| phoenix | `m5-rotated` | 105 | 103 |
| phoenix | `m6-formations` | 301 | 296 |

Each fight that parts does so on one of two mechanisms, none of them the
unit's own damage or motion:

- **Facing a target dead ahead.** A Stormcaller or Steel Ball whose
  presearched Crawler stands 6 Q32 units (1.4 nm) to its left faces +0.245°
  in the game, as it would with no offset, and −0.245° in the simulator,
  until it first moves. `AcosFastest(1)` is not zero, so the offset's sign
  picks the side. Steel Ball's and Stormcaller's M6 with seed 4242. The
  build's `FightUtility.ConvertToAngle` takes `360 - angle` for a direction
  whose x is negative, as the simulator does, so the game's direction at
  presearch has an x of zero or more where the simulator's is −6: the
  positions it is taken between are what differ, and they are not read.
- **The Wraith's grouped search.** Each of the Wraith's weapons searches
  for its own target, and the simulator answers a different one: M2, M3 and
  M6 part on a weapon's target or a released projectile. M6 with seed 4242
  redistributes a live lock by attack count, which the simulator refuses.

## The eighteen released together

Every other unit but the three that cost 800 (War Factory, Abyss, Mountain)
was released to the simulator at once, and recorded in its six layouts with
both seeds before any of it was fixed: 216 recordings. Sandworm is not among
them, as its configuration cannot state a unit that burrows. Five units are
refused by name, for a main skill the kernel has no way to fire: Hacker's
control beam, Raiden's and Vortex's grouped fusillade, and the Farseer's and
Overlord's projectiles, which climb before they fly.

Of the other 156 fights, 146 play back exactly and are pinned:

| Unit | Pinned of 12 |
| --- | ---: |
| centurion | 12 |
| sabertooth | 12 |
| scorpion | 12 |
| tarantula | 12 |
| void_eye | 12 |
| vulcan | 12 |
| fire_badger | 11 |
| fortress | 11 |
| melting_point | 11 |
| sledgehammer | 11 |
| hound | 10 |
| phantom_ray | 10 |
| typhoon | 10 |

Five mechanisms the recordings exposed were fixed on the way, each named in
[`combat.md`](../../docs/rules/combat.md): a weapon is named by its index, a
projectile that follows its target keeps its offset, a single weapon lands its
offsets last drawn first, a projectile in simulated motion that lands on a dead
unit does nothing, and a beam with a splash strikes everything in it.

The ten that part:

| Unit | Layout | Seed | Parts at | On |
| --- | --- | ---: | ---: | --- |
| hound | `m3-crawler` | 4242 | 9 | facing a target dead ahead |
| hound | `m6-formations` | 4242 | 9 | facing a target dead ahead |
| fire_badger | `m6-formations` | 1787720817 | 9 | facing a target dead ahead |
| phantom_ray | `m3-crawler` | 1787720817 | 1 | facing a target dead ahead |
| phantom_ray | `m3-crawler` | 4242 | 176, content only | a weapon's attack target, not read |
| sledgehammer | `m3-crawler` | 1787720817 | 177 | an attack begun from idle after a kill fires a tick early |
| typhoon | `m6-formations` | 4242 | 111 | an attack begun from idle after a kill fires a tick early |
| typhoon | `m3-crawler` | 1787720817 | 148 | a released projectile, not read |
| fortress | `m3-crawler` | 1787720817 | 382 | a released projectile, not read |
| melting_point | `m6-formations` | 1787720817 | 184 | a lock target at 177, not read |

A Sledgehammer or Typhoon whose target is killed goes idle for a tick, locks
another, and in the game fires three ticks after the lock, where the simulator
fires two.
