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

**How a correction composes with a description is not stated here.** Whether a
rate multiplies or adds, and in what order two of them apply, belongs to the
derived-value layer and is
[architecture.md](../spec/simulation/architecture.md)'s unresolved question.
This index says what the build stores and what it targets, and stops there.

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

- **How a correction composes with the description.** Whether a rate multiplies
  or adds, in what order two of them apply, and whether two officers of one kind
  stack by adding their rates, are one question asked three ways. It belongs to
  the derived-value layer and is the one thing a mechanism cannot be finished
  without.
- **Which units are "ranged"**, which `mech_type` 4 selects and neither the data
  nor the text enumerates.
- **What a correction does once it lands**, for the four fields above: no
  mechanism here reads a tower's life, a mine, a shield device or a deployment
  clock yet.
