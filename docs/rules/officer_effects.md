# What an officer does to a fight

[简体中文](officer_effects.zh.md)

This index is pinned to game build 2259. It states the corrections an officer
writes onto the units it targets, how those corrections are encoded, and which
units each one reaches. What an officer does to a ledger — a discount, an
income, a squad it hands out — is [`reinforce_items.md`](reinforce_items.md)'s
and [`config/officers.yaml`](../../config/officers.yaml)'s.

The machine-readable table is
[`config/officer_effects.yaml`](../../config/officer_effects.yaml), and
`scripts/extract-officer-effects.py` writes it from the build's own
`ConfigDataContainer`. Of the build's 132 officers, 79 carry a correction.

**The numbers are checked against the game's own words.**
[`officers.md`](officers.md) lists each officer's effect in the localized text,
read out of build 2227 by another route entirely. Of the 79, 73 state a
percentage there, and every one of those percentages is in this table: Aerial
Specialist's "ATK by 13% and HP by 13%" is `damage_rate` and `life_rate` of
`+0.13`, Advanced Offensive Tactics' 30% is `+0.3`, Advanced Missile Device's
200% is `land_mine_rate` of `+2`. The extraction refuses to write a table that
disagrees, so a misparse would have to agree with an independent reading of an
earlier build to pass.

## Where it lands

An officer is a modifier rather than a rule of its own. `Officer.AddData`
passes the officer to `MechDataModifer.TryAddCommonData`, which reaches
`MechDataModifer.AddData` and `SkillDataModifier.AddData` — the two overlays
[`architecture.md`](../spec/simulation/architecture.md) describes, one on the
unit and one on its skills. The officer is the `IDataModifier` that tags each
entry, which is how taking the officer away takes its corrections with it.

The fields are the ones `GameRiver.OfficerData` answers
`ICommonMechDataChangeDataSource` with. `TechnologyData`, `EquipmentData` and
`EnergyTowerSkillData` implement the same interface, so one table shape serves
all four sources; the other three join this file when their objects are parsed.

## How a number is read

| Kind | Encoding | Example |
| --- | --- | --- |
| `*_rate` | `FPoint`, Q32.32 raw | `1288490188` is `+0.3` |
| `*_value` | `FPoint` in the number's own units | `42949672960` is `+10` of range |
| `speed_value`, `extra_life`, `exp_rate` | a plain integer, by the runtime getter's own return type | `3` |

The raw integer is what the file carries, because it is what the build stores
and what a fixed-point simulation needs. Each line's comment is the same number
read as a decimal, and is a comment because the decimal is the lossy one:
`1288490188 / 2^32` is `0.29999999981`, and the build never computes with
`0.3`.

## How a correction composes

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

truncated toward zero once, at the end. The shape is the build's own and
[`architecture.md`](../spec/simulation/architecture.md) reads it out of
`DataSet`'s two aggregation classes; what follows is what each clause answered
when it was measured against the game.

**An impairment is not a negative enhancement.** Enhancements sum inside one
bracket; impairments each contribute their own factor. Two of `0.11` leave
`0.89 × 0.89 = 0.7921`, not `1 − 0.22 = 0.78`.

### Enhancements sum

A rate multiplies the description once, and two enhancements on one number sum
before they do.

`tests/layouts/modifier/composition.mcscript` measured it against the game. One
Marksman shoots one Rhino, twice, and the Rhino outlives the fight in all three
recordings, so the reading is the life it has left of 19297:

| Blue's officers | Rhino's life | Damage a hit |
| --- | ---: | ---: |
| none | 14639 | 2329, the description unchanged |
| 先进进攻战术 | 13243 | 3027 |
| 先进进攻战术 twice | 11845 | 3726 |

3027 is `trunc(2329 × 1.3)` and 3726 is `trunc(2329 × 1.6)`. Compounding the
two would be `trunc(2329 × 1.69)` = 3935, leaving 11427, which is not what the
game played. The truncation is visible in the first number on its own:
`2329 × 1.2999999998` is 3027.6999, and the build keeps 3027.

The recordings say the same thing a second way, from the other side of the
question, which `mechcore fight stats` reads. The `once` recording stores
`damage_rate.add` of `1288490188` in the Marksman's **skill** channel — the officer table's `+0.3` exactly — and the
`twice` recording stores `2576980376`, which is that number doubled. **The
build sums two officers where it writes them**, so a recording holds one entry
of `+0.6` rather than two of `+0.3`, and summing entries in a simulator mirrors
the build rather than inventing an order over them. The unit overlay and the
buff aggregate are neutral in both: this officer's damage does not land on the
unit at all.

Two control recordings of the no-officer layout were taken first and compared
tick for tick, so the differences above are the officer's and not the
pipeline's.

### Impairments compound

`tests/layouts/modifier/impairment.mcscript` asked the same question of the other
sign, where the answer is different. Cost Control Specialist is `−0.11` on
damage and life over every unit, and a side may hold it twice:

| Red's officers | Rhino whole | left after two hits |
| --- | ---: | ---: |
| Cost Control Specialist | 17174 | 12516 |
| twice | **15285** | **10627** |

`19297 × 0.89 × 0.89` is 15285. `19297 × (1 − 0.22)` is 15051, and the game
did not play that fight. One impairment cannot tell the two rules apart — it is
`0.89` either way — which is what made that recording the run's control.

The stored half says it more sharply than the fight does, and needs no fight at
all: the two-officer recording holds a `life_rate` of `reduce` **892923700**,
which is `1 − 0.89²` to the last digit. Summed impairments would have stored
944892804.

Both recordings' hashes and all four numbers were the simulator's own
predictions, written into the script before the game was started, and the
prediction came from `MultiplicativeDataFloat.Refresh` rather than from a
guess.

### A value is added in the number's own unit

`tests/layouts/modifier/value.mcscript` closed the last clause. Extended Range
Arclight is `+20` of range and `−0.2` of damage, so one recording carries a
value and an impairment at once. An Arclight reaches 95 metres; with the
officer it opens fire twenty metres earlier and every tick after that moves.

The game's recording is the simulator's prediction to the tick — same 137
ticks, same physics hash, the Rhino left with 16961 against the control's
16377 — under `range + 20_000` in the quantized metres the description uses.
The recording also holds `attack_range_value` of `85899345920` in the skill
channel, the table's raw `+20` unchanged, beside a `damage_rate` whose
`reduce` half carries the impairment.

So a value is a sum in the number's own unit, which is what
`AdditiveDataFloat` doing nothing but summing meant. What no officer can ask
is what happens when a value and a rate meet **on one number**: no officer
carries both for the same stat, so the order in the formula above is the
build's class structure rather than a measurement, and a technology will be
the one to test it.

### A plain integer is a value too, and it sums

`speed_value` is stored as a plain `int` rather than an `FPoint`, which is the
build saying it lives in the third list, `DataSet.intDatas`. That list's
arithmetic is whichever `DataInt` subclass built the entry: `DataIntGroup`
sums and clamps, `DataIntSingleMax` keeps the largest entry, `DataIntSingleMin`
the smallest. `FightMech`'s constructor builds
`DataIntGroup(0x80000000, 0x7FFFFFFF, 0)`, so a unit's integers sum and the
clamp never binds.

`tests/layouts/modifier/speed.mcscript` put that to the game. Advanced Power
System and Speed Specialist are `+3` of movement each, over every unit, and
they went on the Rhino that walks 105 metres to reach the Marksman — a walk
that is the whole clock of the fight:

| Red's officers | Rhino's speed | Fight ends at |
| --- | ---: | ---: |
| none | 16 | tick 120 |
| one | 19 | tick 104 |
| both | **22** | **tick 92** |

Under `SingleMax` two entries of 3 would answer 3, and the second recording
would have been the first one again. The recording stores `move_speed_value`
of **6**. The life left is 14639 in all three, because the Marksman lands two
shots either way: this measurement is the clock, not the damage.

It also fixes the unit. `+3` is three metres per second, the same number the
game's own text shows, and not a proportion of anything.

### Reading a clause directly

Each of the captures above was designed so that a *fight's outcome* would
separate the candidates: two hits' worth of life for the damage, a shifted
first shot for the range, a tick count for the movement. Since MCFR 0.4.0 a
recording carries each unit's derived numbers as well, so
`mechcore fight stats <recording>` answers both halves at one tick — the `+0.6`
that was written and the 3726 the build computed from it. Every reading above
was re-read that way and agrees.

The next clause needs no such design: put the correction on a unit, record one
tick, read the number.

## Which channel a field lands in

A recording keeps exactly one place for each field, so which channel a
correction lives in is the recording's own shape rather than a choice: MCFR's
skill modifier set holds `damage_rate`, `attack_range_rate` and
`attack_interval_rate`, and its unit modifier set holds `life_rate` and the
move-speed fields.

Both readings are measured. The damage capture above found `+0.3` in the
Marksman's skill channel. A second capture, `tests/layouts/modifier/officer-life-rate.yaml`,
put Advanced Defensive Tactics' `life_rate` on a Rhino: the game stored it in
the **unit** channel, gave the Rhino 25086 of its 19297, and left it 20428 after
two hits — all three numbers predicted by the simulator before the recording
existed, and the whole fight hashes identically.

## What this build applies

`crates/simulation/src/officers.rs` turns a row of the table into corrections
on the units it reaches, tagged `Modifier` so removing the officer removes
them. Of the 79 rows, **61** are applied: the ones whose every field is a rate,
a value or a plain integer on a number the simulator derives — damage, life,
attack interval, attack range, movement speed — and whose `mech_type` is 0, 1
or 10.

The rest refuse the side that holds them, by name:

| Why | Rows | What would close it |
| --- | ---: | --- |
| a tower, shield, mine, deployment clock, experience or a projectile's life | 11 | the mechanism that owns that object |
| `*_by_kill_count` | 3 | a mechanism that counts a unit's kills |
| `splash_range_value` | 3 | a splash radius among the numbers this simulator derives |
| `mech_type` 4 | 1 | knowing which units count as ranged |

What is left is no longer about how a correction composes. All three of
`DataSet`'s lists have been read and measured; every remaining refusal is a
mechanism this simulator does not have, or a targeting category nothing
enumerates.

A partly applied officer is not offered: a side carrying one this build cannot
compose is refused, because a fight with two thirds of an officer on it is a
fight whose numbers nobody can check.

## Which units a correction reaches

Each row carries the `mech_type` the build stores and, where the build resolves
one, the unit list that goes with it:

| `mech_type` | Rows | Units listed | Read as |
| ---: | ---: | --- | --- |
| 10 | 61 | the units themselves | those units |
| 0 | 10 | none | every unit |
| 11 | 6 | none | no unit: the correction is on a tower, a shield or a mine |
| 1 | 1 | the seven air units | those units |
| 4 | 1 | none | ranged units |

Type 10 is the common case and needs no reading: 改进型铁锤 lists Sledgehammer
and corrects Sledgehammer. Type 0 carries no list and reaches everything, which
is what 先进进攻战术's `+0.3` damage is: the officer a side may hold twice, and
`docs/spec/document/layout.md` keeps `officers` a multiset for exactly that.
Type 11 rows correct `tower_life_rate`, `energy_shield_rate`, `land_mine_rate`
or `extra_life`, none of which is a unit's number at all.

Type 4 is one row, 先进瞄准系统, with `+10` of range and no unit list. Nothing
in the build's data says what the category selects; the officer index's text
does, "increases the range of all **ranged** units by 10", so the category is
read from the effect rather than from the data. Which units count as ranged is
not stated by either.

## What the four odd fields touch

Four fields correct something that is not a unit's number, and the officer
index's text says what each one is:

| Field | What it corrects |
| --- | --- |
| `tower_life_rate` | the Energy Tower's life; 能量塔过载's `+1` is its "by 100%" |
| `energy_shield_rate` | the Energy Shield device's shield; 先进护盾装置's `+0.4` |
| `land_mine_rate` | the Sentry Missile device's damage; 先进飞弹装置's `+2` |
| `super_deployment_time_rate` | the teleport time a rear deployment takes; 快速传送's `-0.5` |

`exp_rate` is the fifth of that kind and the one that is not a rate at all: its
runtime getter returns an `int`, the five rows hold `100` or `75`, and the text
reads "increases EXP Growth Rate by 100%", so the integer is a percentage.

`extra_life` is stored like a unit's number and is not one. Its single row is
紧急避险, whose text describes a side's life being reset to 1 rather than a unit
gaining any, so the field's name is not what it does.

## What is not established here

- **How a `*_value` correction composes**, which 44 of the table's 79 rows
  wait on, and what order two channels apply in when both correct one number.
  The captures above settled the rate in each channel and reached neither of
  these.
- **Which units are "ranged"**, which `mech_type` 4 selects and neither the data
  nor the text enumerates.
- **What a correction does once it lands**, for the four fields above: no
  mechanism here reads a tower's life, a mine, a shield device or a deployment
  clock yet.
