# What an officer does to a fight

The corrections an officer writes onto the units it targets, how those
corrections are encoded, and which units each one reaches. What an officer
does to a ledger, a discount, an income, a squad it hands out, is
[reinforce_items.md](reinforce_items.md)'s and
[`config/officers.yaml`](../../config/officers.yaml)'s.

[`config/officer_effects.yaml`](../../config/officer_effects.yaml) holds every
officer that carries a correction; `scripts/extract-officer-effects.py` writes
it from `ConfigDataContainer.officerDatas`, leaving out officers limited to
Interstellar Expedition.

**The numbers are checked against the game's own words.** Every percentage an
officer's English description states, as the build localizes it with its
parameters filled, has to be one of the rates the table holds for it: Aerial
Specialist's "ATK by 13% and HP by 13%" is a `damage_rate` and `life_rate` of
`+0.13`. The extraction refuses to write a table that disagrees.

## Where it lands

An officer is a modifier rather than a rule of its own. `Officer.AddData`
passes the officer to `MechDataModifer.TryAddCommonData`, which reaches
`MechDataModifer.AddData` and `SkillDataModifier.AddData`: the two overlays
[`architecture.md`](../spec/simulation/architecture.md) describes, one on the
unit and one on its skills. The officer is the `IDataModifier` that tags each
entry, which is how taking the officer away takes its corrections with it.

The fields are the ones `GameRiver.OfficerData` answers
`ICommonMechDataChangeDataSource` with. `TechnologyData`, `EquipmentData` and
`EnergyTowerSkillData` implement the same interface, so one table shape serves
all four sources.

## How a number is read

| Kind | Encoding | Example |
| --- | --- | --- |
| `*_rate` | `FPoint`, Q32.32 raw | `1288490188` is `+0.3` |
| `*_value` | `FPoint` in the number's own units | `42949672960` is `+10` of range |
| `speed_value`, `extra_life` | a plain integer, by the runtime getter's own return type | `3` |

The raw integer is what the file carries, because it is what the build stores
and what a fixed-point simulation needs. Each line's comment is the same number
read as a decimal, and is a comment because the decimal is the lossy one:
`1288490188 / 2^32` is `0.29999999981`, and the build never computes with
`0.3`.

## How a correction composes

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

truncated toward zero once, at the end. The shape is the build's own, and
[`architecture.md`](../spec/simulation/architecture.md) reads it out of
`DataSet`'s aggregation classes. The fixtures in
[`tests/modifier/`](../../tests/modifier/README.md) each put one clause on one
unit, and their README gives what the game answered.

**Enhancements sum.** Two enhancements on one number sum before they multiply
the description once, and the build sums them where it writes them: a
recording holds one entry of their sum, not two entries. Summing entries in a
simulator mirrors the build rather than inventing an order over them.

**An impairment is not a negative enhancement.** Enhancements sum inside one
bracket; impairments each contribute their own factor. Two of `0.11` leave
`0.89 × 0.89 = 0.7921`, not `1 − 0.22 = 0.78`, and the build stores the
combined reduction `1 − 0.89²`.

**A value is added in the number's own unit**, a range value in metres, and it
sums across sources: a technology's range value and an officer's are stored as
one entry. Which is why this rule is
[`technology_effects.md`](technology_effects.md)'s too.

**A value joins the description before a rate multiplies it.** Where one number
carries both, the value is added first.

**A plain integer is a value too, and it sums.** `speed_value` lives in
`DataSet.intDatas`, whose arithmetic is whichever `DataInt` subclass built the
entry: `DataIntGroup` sums and clamps, `DataIntSingleMax` keeps the largest
entry, `DataIntSingleMin` the smallest. A unit's integers are a
`DataIntGroup` across the whole `Int32` range, so they sum and the clamp never
binds. `+3` of speed is three metres per second, the number the game's own text
shows, and not a proportion of anything.

## Which channel a field lands in

A recording keeps exactly one place for each field, so which channel a
correction lives in is the recording's own shape rather than a choice: MCFR's
skill modifier set holds `damage_rate`, `attack_range_rate` and
`attack_interval_rate`, and its unit modifier set holds `life_rate` and the
move-speed fields. An officer's damage rate lands in the skill channel and not
on the unit; its life rate lands in the unit channel.

## What the simulator refuses

`crates/simulation/src/modifier/officers.rs` turns a row of the table into
corrections on the units it reaches, tagged `Modifier` so removing the officer
removes them: a row whose every field is a rate, a value or a plain integer on a
number the simulator derives (damage, life, attack interval, attack range,
movement speed) and whose `mech_type` it answers. It refuses the side that
holds any other row, by name, for one of these reasons:

- the row corrects a tower, shield, mine, deployment clock, experience or a
  projectile's life, which needs the mechanism that owns that object;
- the row carries a `*_by_kill_count` rate, which needs a mechanism that counts
  a unit's kills;
- the row carries a `splash_range_value`, which needs a splash radius among the
  numbers the simulator derives.

What is left is no longer about how a correction composes: every remaining
refusal is a mechanism this simulator does not have.

A partly applied officer is not offered: a side carrying one this build cannot
compose is refused, because a fight with two thirds of an officer on it is a
fight whose numbers nobody can check.

## Which units a correction reaches

Each row carries the `mech_type` the build stores and, where the build resolves
one, the unit list that goes with it:

| `mech_type` | Units listed | Read as |
| ---: | --- | --- |
| 10 | the units themselves | those units |
| 0 | none | every unit |
| 11 | none | no unit: the correction is on a tower, a shield or a mine |
| 1 | the air units | those units |
| 4 | none | ranged units |

Type 10 is the common case and needs no reading: 改进型铁锤 lists Sledgehammer
and corrects Sledgehammer. Type 0 carries no list and reaches everything, which
is what 先进进攻战术's `+0.3` damage is: the officer a side may hold twice, and
`docs/spec/document/layout.md` keeps `officers` a multiset for exactly that.
Type 11 rows correct `tower_life_rate`, `energy_shield_rate`, `land_mine_rate`
or `extra_life`, none of which is a unit's number at all.

Type 4 is 先进瞄准系统, with `+10` of range and no unit list. The category is
`UnitEffectTargetType.Ranged`, and `UnitUtility.IsEffectTarget`, which
officers, equipment, energy-tower skills and unit reinforcements all ask,
answers it from the unit's main `SkillData.isMeleeAttack`: a unit whose main
skill is not a melee attack is ranged. Melee (3) is the same flag set. Of the
units the simulator describes, only the Rhino and the Crawler are melee, and the
unit table carries the flag as `attack.melee`.

## What the odd fields touch

Four fields correct something that is not a unit's number, and the officer
index's text says what each one is:

| Field | What it corrects |
| --- | --- |
| `tower_life_rate` | the Energy Tower's life; 能量塔过载's `+1` is its "by 100%" |
| `energy_shield_rate` | the Energy Shield device's shield; 先进护盾装置's `+0.4` |
| `land_mine_rate` | the Sentry Missile device's damage; 先进飞弹装置's `+2` |
| `super_deployment_time_rate` | the teleport time a rear deployment takes; 快速传送's `-0.5` |

`exp_rate` is a fifth that corrects no unit's number: a rate on the experience
a unit gains, `+1` or `+0.75`, which the text reads as "increases EXP Growth
Rate by 100%".

`extra_life` is stored like a unit's number and is not one. Its single row is
紧急避险, whose text describes a side's life being reset to 1 rather than a unit
gaining any, so the field's name is not what it does.

## Evidence

### Recorded

- One enhancement multiplies the description once, and two on one number sum
  into one stored entry: `tests/modifier/regressions.mcscript`.
- An officer's damage rate lands in the skill channel, and its life rate in the
  unit channel: `tests/modifier/regressions.mcscript`.
- Two impairments compound into one stored reduction:
  `tests/modifier/regressions.mcscript`.
- A range value is added in metres, beside an impairment on the same unit:
  `tests/modifier/regressions.mcscript`.
- A technology's range value and an officer's sum into one stored entry:
  `tests/modifier/regressions.mcscript`.
- Two speed values sum: `tests/modifier/regressions.mcscript`.
- A `Ranged` row reaches the ranged units of a side and not its melee ones:
  `tests/modifier/regressions.mcscript`.

### Read

- An officer writes through the unit's and the skill's overlays, as their
  modifier: `Officer.AddData`, `MechDataModifer.TryAddCommonData`,
  `MechDataModifer.AddData`, `SkillDataModifier.AddData`.
- A data set keeps its values, its integers and its rates in three lists, each
  with its own arithmetic: `DataSet.floatDatas`, `DataSet.intDatas`,
  `DataSet.floatRateDatas`, `AdditiveDataFloat.Refresh`,
  `MultiplicativeDataFloat.Refresh`.
- An integer entry sums, keeps the largest or keeps the smallest by its class:
  `DataIntGroup.Refresh`, `DataIntSingleMax.Refresh`,
  `DataIntSingleMin.Refresh`.
- A target type is answered from the unit's main skill:
  `UnitUtility.IsEffectTarget`, `UnitEffectTargetType.Ranged`,
  `SkillData.isMeleeAttack`.
- The experience correction is a rate: `OfficerData.expChangeRate`.

### Not established

- **That a value joins before a rate.** It was measured on a Sledgehammer's
  attack interval in another version, with a script that needs the game, and
  no offline test pins it.
- **What a correction does once it lands**, for the odd fields above: no
  mechanism here reads a tower's life, a mine, a shield device or a deployment
  clock yet.
- **Which of a unit's skills a correction reaches beyond its main skill.** The
  writer that selects them, `SkillDataModifier.AddData` taking the source,
  asks its source more than it did; every recorded row reaches the main skill.
- **How an attack interval composes when two channels correct it.** Damage and
  move speed compose across channels as within one, which
  [towers.md](towers.md) records; the interval is not recorded.
