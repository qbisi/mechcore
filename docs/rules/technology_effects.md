# What a technology does to a fight

The corrections a technology writes onto the unit that researched it, how they
are encoded, and how they grow with the unit's rank. Which technologies a unit
may research and what each costs is [unit_techs.md](unit_techs.md)'s.

[`config/technology_effects.yaml`](../../config/technology_effects.yaml) holds
every technology a unit may research, with the numbers it writes onto its
unit; `scripts/extract/extract-technology-effects.py` writes it from every list
of `TechnologyGroupData` in `level0`. A row's `kind` names the list it comes
from.

The fields are the ones `GameRiver.TechnologyData` answers
`ICommonMechDataChangeDataSource` with, which is the interface `OfficerData`
answers too. So this table has the same shape as
[officer_effects.md](officer_effects.md)'s, a correction means the same thing in
both, and **the composition rule is the same one**:

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

[officer_effects.md](officer_effects.md#how-a-correction-composes) carries that
rule and the captures that measured it.

## An effect is a list, indexed by rank

`lifeChangeRate` and its neighbours are `List<FPoint>` rather than one value,
because a technology's effect can grow with the unit's rank. Elite Marksman is
the clear case: its `damage_rate` and `attack_range_value` carry nine entries
each, one per rank, which is the game's "increasing range by 5 and ATK by 25%
per Rank increased". A technology whose effect does not grow carries a single
entry.

**A growing list is its first entry times the rank**, with each rank rounded on
its own; the extraction refuses a table where that does not hold.

## How a number is read

| Kind | Encoding | Example |
| --- | --- | --- |
| `*_rate` | `FPoint`, Q32.32 raw | `1073741824` is `+0.25` |
| `*_value` | `FPoint` in the number's own units | `21474836480` is `+5` of range |
| `speed_value`, `min_attack_range_value` | plain integers | `5` |

The raw integer is what the file carries, because it is what the build stores;
each line's comment is the same number read as a decimal, and is a comment
because the decimal is the lossy one.

A technology commonly trades one number for another: Launcher Overload is an
`attack_interval_rate` below zero **and** an `attack_range_value` below zero.

**The numbers are checked against the game's own words.** Every number the
table states for a technology appears in that technology's English description,
as the build localizes it with its parameters filled. The check runs in this
direction because a technology's text also states effects this table does not
carry.

## Plain technologies, and the rest

A row of `technologyDatas` is a plain `TechnologyData`: it does nothing in a
fight but correct its unit's numbers. A row of any other list is a subclass,
`BuffTechnologyData`, `SplashTechnologyData`, `ExtraWeaponTechnologyData` and
the rest, and does something more, although many carry numbers as well: Double
Shot's second shot, High-Explosive Ammo's splash, Energy Absorption's life
steal, Field Maintenance's repair. 64 of the 241 technologies are plain. The
simulator applies a plain technology's numbers, and a lifesteal technology's
with the life steal [combat.md](combat.md#lifesteal) states, and refuses every
other technology by name and kind, since applying a subclass's numbers alone
would fight it as something it is not.

A plain or a lifesteal row may still set a field beyond what this table
carries, and names it in `special`: Siege Mode's `isInverseIsLockTarget`, and Machine
Learning's `expChangeRate`, which speeds up the experience its unit gains. The
simulator refuses those too.

## What this table does not carry

A technology that is not plain does something that is not a correction on its
unit's own numbers. It may instead:

- **summon or fire something**, as Fang Production and Anti-Air Barrage do;
- **change a skill rather than the unit**: Aerial Specialization's "ATK against
  aerial units" is a damage rate against one domain, which is
  `ISkillDataChangeDataSource`'s and lives in the skill the technology points
  at with `targetSkillID`;
- **apply something to the enemy**, as Electromagnetic Explosion disables the
  target's technologies on hit.

Its numbers may also be only half of what it does: Photon Coating's reduction
of damage received is not here even though the technology also carries numbers
that are.

## What the recordings show

One Arclight researching Range Enhancement while its side holds Extended Range
Arclight reaches the description's range plus both corrections, and the
recording stores the technology's and the officer's range as **one**
`attack_range_value`: the build merges two sources exactly as it merges two
officers.

An interval value is an `FPoint` of seconds, and it lands in
`attack_interval_value` exactly as the table states it; the interval is
composed in `FPoint` seconds and only then cut into ticks. A value such as
Mechanical Rage's is a tenth of a second, which no whole number of the
description's time units holds.

The interval technologies put a value and a rate on one number: Mechanical Rage
and Armour Piercing Bullets are the one pair that do, and they are what
measured the order in the composition rule, which
[officer_effects.md](officer_effects.md#how-a-correction-composes) carries.

The simulator refuses a side holding a technology whose correction it does not
derive (a minimum range, a splash radius, a projectile's speed or life) or
whose effect grows with rank, rather than read index zero:
`crates/simulation/src/modifier/technologies.rs` names each refusal.

## Evidence

### Recorded

- A technology's range and an officer's range land in one `attack_range_value`,
  and the fight uses their sum: `tests/modifier/fights/`.
- An interval value lands as the table's `FPoint`, and the Rhino's blows follow
  the interval it composes: `tests/modifier/fights/technology-interval-value.yaml`.

### Read

- A technology answers the same correction interface an officer does, so its
  fields land in the same channels: `TechnologyData.lifeChangeRate`,
  `TechnologyData.damageChangeRate`, `TechnologyData.attackRangeChangeValue`.
- An effect is a list indexed by rank, and a growing list is its first entry
  times the rank; `scripts/extract/extract-technology-effects.py` refuses a table where
  that does not hold, and wrote this version's: `TechnologyData.damageChangeRate`.
- The numbers a technology states appear in its own description, which the
  extraction checks: `TechnologyData.lifeChangeRate`.

### Not established

- **A value applies before a rate, for a technology.** Measured on the
  Sledgehammer's interval technologies, whose fights no test pins because the
  simulator refuses the unit.
- **What the other technologies do**, in the terms a simulator needs. Each
  one owes the mechanism it belongs to: a summon, a skill's own numbers, a
  debuff on the target.
- **Which rank index a fight reads.** The list is indexed by rank and this
  document does not state what a unit's rank is at the moment a technology is
  applied, nor whether raising a rank mid-fight re-reads it. Rank one is the
  only case any recording has covered.
- **`min_attack_range_value`, `splash_range_value`, `projectile_speed_value`
  and `projectile_life_rate`**, which no mechanism in `crates/simulation`
  reads.
