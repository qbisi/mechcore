# Unit levels

A deployed unit's level selects
one row of `attributeUpgradeDatas`. Its `lifeRating` and `damageRating`
multiply the description's life and damage before dynamic data changes.
They do not create entries in the unit's or skill's modifier overlay. The
recordings behind this rule are build 1.11.1.3.2259's.

## The ratings

The level is its own multiplier. It is not a correction and not one of the
`DataSet`s an officer, a technology or a buff writes into: it scales the
description's life and damage first, and every overlay applies to the
product.

A level-`n` unit, for `n` from 1 to 9, has `n` times its description's life
and `n` times its description's damage.

The game reads this from a table,
`ConfigDataContainer.m_Structure.attributeUpgradeDatas`, whose nine rows are
looked up by level. In builds 1.11.1.3.2259 and 2.0.0.1.2324 every row's
`lifeRating` and `damageRating` equals its own level (an FPoint on 2.0), so the
simulator multiplies by the level and keeps no table. A layout refuses a level outside
one to nine by name, because there is no row for it. A build whose rows stop
equalling their level is the change that would need the table.

## Where the level enters

`FightMech` holds the description as an `IMechData` and the selected row as
an `IMechLevelData`. `GetBaseLife` calls the
description's `GetBaseLife` and the row's `GetLifeRating`; it converts the
integer to Q32.32, multiplies, then shifts back to an integer.
`GetBaseDamage` similarly calls `GetBaseDamage` and `GetDamageRating`, and
returns the product as Q32.32 to `DamageProperty.RefreshBaseDamage`.
`DamageProperty.CalculateBaseDamage` applies the skill's damage multiplier
and adds its own damage before converting to an integer. `CalculateDamage`
then applies dynamic skill and buff rates. Because every rating is a whole
number, the level's product is exact; only later damage conversions truncate.

The stable source symbols are `FightMech.GetBaseLife`,
`FightMech.GetBaseDamage`, `IMechLevelData.GetLifeRating`,
`IMechLevelData.GetDamageRating`, `DamageProperty.RefreshBaseDamage`,
`DamageProperty.CalculateBaseDamage` and `DamageProperty.CalculateDamage`.
The `FightMech` getters read the two fields and perform the multiply; this is
the fight's formula, independently of the card display's `MechUtility`
getters.

For a Marksman, whose description life and damage are
[`config/units/marksman.yaml`](../../config/units/marksman.yaml)'s, life and
damage are twice those at level two and three times at level three. With Advanced Offensive Tactics,
level two still has twice the description's damage as its base, and the skill
overlay carries only the officer's +0.3, applied to that base and truncated.
Adding the level into that rate is not this rule. The layout and recording procedure are in
[`tests/level/`](../../tests/level/README.md).

No level multiplier is applied by these getters to range, speed or attack
interval. The level-one, level-two and level-three Marksman recordings keep the
description's range, speed and first interval.

## Not covered

This is the selection of starting life and damage for deployed units at
levels one through nine. It does not establish how `ExpSystem` distributes
experience during a fight, when experience changes a level, equipment's
composition, reactor damage, or the level semantics of constructions and
towers. Those have distinct readers or correction sources; these two
getters do not establish their behavior. A level above nine has no row in
this table. New build data or a native counterexample to these getter paths
requires revisiting this rule.
