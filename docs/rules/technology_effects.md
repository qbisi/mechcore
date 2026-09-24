# What a technology does to a fight

The corrections a technology writes onto the unit that researched it, how they
are encoded, and how they grow with the unit's rank. Which technologies a unit
may research and what each costs is [unit_techs.md](unit_techs.md)'s. The
recordings behind this rule are build 1.11.1.3.2259's.

[`config/technology_effects.yaml`](../../config/technology_effects.yaml) holds
every technology a unit may research that writes onto its unit's numbers;
`scripts/extract-technology-effects.py` writes it from every list of
`TechnologyGroupData` in `level0`.

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

## What this table does not carry

A technology the table does not list does something that is not a correction
on its unit's own numbers. It may instead:

- **summon or fire something**, as Fang Production and Anti-Air Barrage do;
- **change a skill rather than the unit**: Aerial Specialization's "ATK against
  aerial units" is a damage rate against one domain, which is
  `ISkillDataChangeDataSource`'s and lives in the skill the technology points
  at with `targetSkillID`;
- **apply something to the enemy**, as Electromagnetic Explosion disables the
  target's technologies on hit.

A row of this table may also be only half of what its technology does: Photon
Coating's reduction of damage received is not here even though the technology
also carries numbers that are.

## What the recordings show

`tests/modifier/technology.mcscript` is what says the channel reaches the game.
One Arclight researching Range Enhancement while its side holds Extended Range
Arclight reaches the description's range plus both corrections, and the
recording stores the technology's and the officer's range as **one**
`attack_range_value` — the build merges two sources exactly as it merges two
officers.

The interval technologies did one more thing: Mechanical Rage and Armour
Piercing Bullets are the one pair that put a value and a rate on one number,
which is what measured the order in the composition rule.
[officer_effects.md](officer_effects.md#a-value-applies-before-a-rate) carries
the reading.

The simulator refuses a side holding a technology whose correction it does not
derive (a minimum range, a splash radius, a projectile's speed or life) or
whose effect grows with rank, rather than read index zero:
`crates/simulation/src/modifier/technologies.rs` names each refusal.

## What is not established here

- **What the other technologies do**, in the terms a simulator needs. Each
  one owes the mechanism it belongs to: a summon, a skill's own numbers, a
  debuff on the target.
- **Which rank index a fight reads.** The list is indexed by rank and this
  index does not state what a unit's rank is at the moment a technology is
  applied, nor whether raising a rank mid-fight re-reads it. Rank one is the
  only case any capture has covered.
- **`min_attack_range_value`, `splash_range_value`, `projectile_speed_value`
  and `projectile_life_rate`**, which no mechanism in `crates/simulation`
  reads.
