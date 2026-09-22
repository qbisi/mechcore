# Unit levels

This rule is pinned to build 1.11.1.3.2259. A deployed unit's level selects
one row of `attributeUpgradeDatas`. Its `lifeRating` and `damageRating`
multiply the description's life and damage before dynamic data changes.
They do not create entries in the unit's or skill's modifier overlay.

## The ratings

The level is its own multiplier. It is not a correction and not one of the
`DataSet`s an officer, a technology or a buff writes into: it scales the
description's life and damage first, and every overlay applies to the
product.

`ConfigDataContainer.m_Structure.attributeUpgradeDatas` holds nine rows, and
each rates life and damage at exactly its own level. Both columns store
`FPoint` Q32.32 raw integers:

| Level | Life rating | Damage rating | Raw value in either column |
| ---: | ---: | ---: | ---: |
| 1 | 1 | 1 | 4294967296 |
| 2 | 2 | 2 | 8589934592 |
| 3 | 3 | 3 | 12884901888 |
| 4 | 4 | 4 | 17179869184 |
| 5 | 5 | 5 | 21474836480 |
| 6 | 6 | 6 | 25769803776 |
| 7 | 7 | 7 | 30064771072 |
| 8 | 8 | 8 | 34359738368 |
| 9 | 9 | 9 | 38654705664 |

So the simulator multiplies by the level, and keeps no table of it. A build
whose rows stop being their level is the change that would need one. A layout
refuses a level outside one to nine by name, because there is no row for it.

## Where the level enters

`FightMech` holds the description as `IMechData` at offset `0xB0` and the
selected row as `IMechLevelData` at `0xB8`. `GetBaseLife` calls the
description's `GetBaseLife` and the row's `GetLifeRating`; it converts the
integer to Q32.32, multiplies, then shifts back to an integer.
`GetBaseDamage` similarly calls `GetBaseDamage` and `GetDamageRating`, and
returns the product as Q32.32 to `DamageProperty.RefreshBaseDamage`.
`DamageProperty.CalculateBaseDamage` applies the skill's damage multiplier
and adds its own damage before converting to an integer. `CalculateDamage`
then applies dynamic skill and buff rates. The nine integer ratings make the
unit's level multiplication exact; later damage conversions truncate.

The stable source symbols are `FightMech.GetBaseLife`,
`FightMech.GetBaseDamage`, `IMechLevelData.GetLifeRating`,
`IMechLevelData.GetDamageRating`, `DamageProperty.RefreshBaseDamage`,
`DamageProperty.CalculateBaseDamage` and `DamageProperty.CalculateDamage`.
The `FightMech` getters' interface slots are respectively 6/1 for life and
7/0 for damage (description/level data). Their ISIL reads the fields and
performs the multiply; this is the fight's formula, independently of the
card display's `MechUtility` getters.

For a Marksman, description life 1622 and damage 2329 become 3244/4658 at
level two and 4866/6987 at level three. With Advanced Offensive Tactics,
level two still has base damage 4658 and the skill overlay has only +0.3:
its damage is 6055 after truncation. Adding the level into that rate would
give 5356 and is not this rule. The layout and recording procedure are in
[`tests/level/`](../../tests/level/README.md).

No level multiplier is applied by these getters to range, speed or attack
interval. The level-one, level-two and level-three Marksman observations
retain 140 m, 8 m/s and the same first interval.

## Not covered

This is the selection of starting life and damage for deployed units at
levels one through nine. It does not establish how `ExpSystem` distributes
experience during a fight, when experience changes a level, equipment's
composition, reactor damage, or the level semantics of constructions and
towers. Those have distinct readers or correction sources; these two
getters do not establish their behavior. A level above nine has no row in
this table. New build data or a native counterexample to these getter paths
requires revisiting this rule.
