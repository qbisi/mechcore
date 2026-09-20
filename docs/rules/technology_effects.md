# What a technology does to a fight

[简体中文](technology_effects.zh.md)

This index is pinned to game build 2259. It states the corrections a
technology writes onto the unit that researched it, how they are encoded, and
how they grow with the unit's rank. Which technologies a unit may research and
what each costs is [`unit_techs.md`](unit_techs.md)'s and
[`config/unit_techs.yaml`](../../config/unit_techs.yaml)'s.

The machine-readable table is
[`config/technology_effects.yaml`](../../config/technology_effects.yaml), and
`scripts/extract-technology-effects.py` writes it from `TechnologyGroupData`
at path id 184 of the build's `level0`. Of the 233 technologies the 32 cards
offer, **137 write onto a unit's numbers**.

The fields are the ones `GameRiver.TechnologyData` answers
`ICommonMechDataChangeDataSource` with, which is the interface
`OfficerData` answers too. So this table has the same shape as
[`officer_effects.md`](officer_effects.md)'s, a correction means the same
thing in both, and **the composition rule is the same one**:

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

[`officer_effects.md`](officer_effects.md#how-a-correction-composes) carries
that rule and the captures that measured it.

## An effect is a list, indexed by rank

`lifeChangeRate` and its neighbours are `List<FPoint>` rather than one value,
because a technology's effect can grow with the unit's rank. Elite Marksman is
the clear case: its `damage_rate` reads `+0.25, +0.5, +0.75 …` and its
`attack_range_value` reads `+5, +10, +15 …`, nine entries each, which is the
game's "increasing range by 5 and ATK by 25% per Rank increased".

Of the 137 rows, 5 grow and 132 carry a single entry. **A growing list is its
first entry times the rank**, with each rank rounded on its own — the
extraction refuses a table where that does not hold, which is also what makes a
misparse loud: bytes read at the wrong offset are not arithmetic.

## How a number is read

| Kind | Encoding | Example |
| --- | --- | --- |
| `*_rate` | `FPoint`, Q32.32 raw | `1073741824` is `+0.25` |
| `*_value` | `FPoint` in the number's own units | `21474836480` is `+5` of range |
| `speed_value`, `min_attack_range_value` | plain integers | `5` |

The raw integer is what the file carries, because it is what the build stores;
each line's comment is the same number read as a decimal, and is a comment
because the decimal is the lossy one.

## What the 137 rows carry

| Field | Rows |
| --- | ---: |
| `attack_range_value` | 57 |
| `life_rate` | 36 |
| `damage_rate` | 36 |
| `attack_interval_rate` | 17 |
| `speed_value` | 14 |
| `attack_interval_value` | 12 |
| `splash_range_value` | 5 |
| `projectile_speed_value` | 3 |
| `min_attack_range_value` | 1 |
| `projectile_life_rate` | 1 |

A row commonly carries two or three of them at once, because a technology
commonly trades one number for another: Launcher Overload is
`attack_interval_rate` of `−0.5` **and** `attack_range_value` of `−20`.

## What this table does not carry

The other 96 technologies do something that is not a correction on their unit's
own numbers, and 96 is not a gap in the parse. A technology may instead:

- **summon or fire something** — Fang Production makes eight Fangs every 36
  seconds, Anti-Air Barrage launches sixteen missiles;
- **change a skill rather than the unit** — Aerial Specialization's "ATK
  against aerial units by 90%" is a damage rate against one domain, which is
  `ISkillDataChangeDataSource`'s and lives in the skill the technology points
  at with `targetSkillID`;
- **apply something to the enemy** — Electromagnetic Explosion disables the
  target's technologies on hit.

A row of this table may also be only half of what its technology does: Photon
Coating's "damage received by 30%" is not here even though the technology also
carries numbers that are.

**The numbers are checked against the game's own words.**
[`unit_techs.md`](unit_techs.md) states each technology's effect in the
localized text, read out of build 2227 by another route entirely. Every number
this table states — 137 rows of them — appears in that text. The check runs in
this direction rather than the officers' because a technology's text states
effects this table does not carry, so requiring the text's numbers to be in the
table would refuse the rows above.

## What this build applies

`crates/simulation/src/technologies.rs` turns a row into corrections on the
unit its row names, tagged `Modifier` like an officer's. Of the 137 rows,
**125** are applied — 43 of them on the eleven units the kernel fights — and
the other 12 refuse the side that holds them:

| Why | Rows |
| --- | ---: |
| it corrects a number this simulator does not derive: a minimum range, a splash radius, a projectile's speed or life | 7 |
| its effect grows with rank, and which entry a rank reads is not established | 5 |

A growing technology is refused rather than read at index zero, even though a
layout's units are rank one: the index a rank reads is the question below, and
guessing it would be a rule nobody measured.

`technology.mcscript` is what says the channel reaches the game. One Arclight
researching Range Enhancement while its side holds Extended Range Arclight
answers 155 metres of the description's 95, and the recording stores the
technology's `+40` and the officer's `+20` as **one** `attack_range_value` of
`+60` — the build merges two sources exactly as it merges two officers.

The interval technologies did one more thing: Mechanical Rage and Armour
Piercing Bullets are the only pair in the build that put a value and a rate on
one number, which is what finally measured the order in the composition rule.
[`officer_effects.md`](officer_effects.md#a-value-applies-before-a-rate)
carries the reading.

## What is not established here

- **What the other 96 technologies do**, in the terms a simulator needs. Each
  one owes the mechanism it belongs to: a summon, a skill's own numbers, a
  debuff on the target.
- **Which rank index a fight reads.** The list is indexed by rank and this
  index does not state what a unit's rank is at the moment a technology is
  applied, nor whether raising a rank mid-fight re-reads it. A layout's units
  are rank one today, which is the only case any capture has covered.
- **`min_attack_range_value`, `splash_range_value`, `projectile_speed_value`
  and `projectile_life_rate`**, which 7 rows carry and no mechanism in
  `crates/simulation` reads. The remaining 130 rows are corrections on numbers
  it derives, 47 of them on the eleven units its kernel fights.
