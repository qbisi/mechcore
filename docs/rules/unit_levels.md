# Unit levels

A deployed unit's level selects one row of `attributeUpgradeDatas`. Its
`lifeRating` and `damageRating` multiply the description's life and damage
before dynamic data changes. They do not create entries in the unit's or
skill's modifier overlay.

## The ratings

The level is its own multiplier. It is not a correction and not one of the
`DataSet`s an officer, a technology or a buff writes into: it scales the
description's life and damage first, and every overlay applies to the
product.

A level-`n` unit, for `n` from 1 to 9, has `n` times its description's life
and `n` times its description's damage.

The game reads this from a table, `ConfigDataContainer.attributeUpgradeDatas`,
whose nine rows are looked up by level. Every row's `lifeRating` and
`damageRating` equals its own level, so the simulator multiplies by the level
and keeps no table. A layout refuses a level outside one to nine by name,
because there is no row for it. A build whose rows stop equalling their level
is the change that would need the table.

## Where the level enters

`FightMech` holds the description as an `IMechData` and the selected row as
an `IMechLevelData`. `GetBaseLife` calls the description's `GetBaseLife` and
the row's `GetLifeRating`, multiplies in Q32.32, then shifts back to an
integer. `GetBaseDamage` similarly calls `GetBaseDamage` and
`GetDamageRating`, and returns the product as Q32.32 to
`DamageProperty.RefreshBaseDamage`. `DamageProperty.CalculateBaseDamage`
applies the skill's damage multiplier and adds its own damage before converting
to an integer. `CalculateDamage` then applies dynamic skill and buff rates.
Because every rating is a whole number, the level's product is exact; only
later damage conversions truncate. The `FightMech` getters are the fight's
formula, independently of the card display's `MechUtility` getters.

For a Marksman, whose description life and damage are
[`config/units/marksman.yaml`](../../config/units/marksman.yaml)'s, life and
damage are twice those at level two and three times at level three. With
Advanced Offensive Tactics, level two still has twice the description's damage
as its base, and the skill overlay carries only the officer's rate, applied to
that base and truncated. Adding the level into that rate is not this rule.

No level multiplier is applied by these getters to range, speed or attack
interval.

## Evidence

### Recorded

- A level-two and a level-three Marksman have twice and three times the
  description's life and damage: `tests/level/regressions.mcscript`.
- At level two, an officer's damage rate applies to the level's base, not to
  the description's: `tests/level/regressions.mcscript`.
- A level-one, level-two and level-three Marksman keep the description's range,
  speed and first interval: `tests/level/regressions.mcscript`.

### Read

- A unit's base life and damage are the description's times the level row's
  rating, before any `DataSet`: `FightMech.GetBaseLife`,
  `FightMech.GetBaseDamage`.
- The rating is the level row's: `AttributeUpgradeData.lifeRating`,
  `AttributeUpgradeData.damageRating`. That every row equals its level is the
  table's, which `scripts/decomp-diff.py --config` compares between versions.
- Damage reaches the skill as the base times the skill's multiplier, and then
  the dynamic rates: `DamageProperty.RefreshBaseDamage`,
  `DamageProperty.CalculateBaseDamage`, `DamageProperty.CalculateDamage`.

### Not established

- **How experience changes a level during a fight**, and how `ExpSystem`
  distributes it: [unit_experience.md](unit_experience.md).
- **Levels of constructions and towers.** These getters are a mech's.
- **A level above nine.** There is no row for it.
- **The skill's `AttackValue`.** `DamageProperty.CalculateBaseDamage` also reads
  the skill's `SkillDataChangeInt.AttackValue`, a flat addition to the base. No
  table read here names a source that writes it, and no recording has one.
