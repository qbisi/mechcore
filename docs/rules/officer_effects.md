# What an officer does to a fight

The corrections an officer writes onto the units it targets, how those
corrections are encoded, and which units each one reaches. What an officer
does to a ledger, a discount, an income, a squad it hands out, is
[reinforce_items.md](reinforce_items.md)'s and
[`config/officers.yaml`](../../config/officers.yaml)'s.

[`config/officer_effects.yaml`](../../config/officer_effects.yaml) holds every
officer a standard 1v1 side can hold that carries a correction:
a card the pool deals, an opening's specialist, a chain blueprint's officer
and a unit round's supply. `scripts/extract/extract-officer-effects.py` writes
it from `ConfigDataContainer.officerDatas`. An officer no standard side can
hold is in no table, and a layout cannot name it.

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
[`tests/modifier/`](../../tests/modifier/) each put one clause on one
unit, and their README gives what the game answered.

**Enhancements sum.** Two enhancements on one number sum before they multiply
the description once, and the build sums them where it writes them: a
recording holds one entry of their sum, not two entries. Summing entries in a
simulator mirrors the build rather than inventing an order over them.

**An impairment is not a negative enhancement.** Enhancements sum inside one
bracket; impairments each contribute their own factor. Cost Control
Specialist's `0.11` and Mass-Produced Rhino's `0.2` on one Rhino leave
`0.89 × 0.8 = 0.712`, not `1 − 0.31 = 0.69`, and the build stores the combined
reduction `1 − 0.89 × 0.8`.

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

## What the simulator applies

`crates/simulation/src/modifier/officers.rs` turns a row of the table into
corrections on the units it reaches, tagged `Modifier` so removing the officer
removes them. Every field a standard officer carries is a rate, a value or a
plain integer on a number the simulator derives (damage, life, attack interval,
attack range, splash radius, movement speed), or one of the side's own numbers
below. A field the build's officers answer but no standard officer carries, a
tower's life, a life per kill, a side's lives, a projectile's numbers, is not
read: a table that came to hold one fails to load rather than drop it.

## A rate per kill

Berserk Rhino's `damage_rate_by_kill_count` of `+0.1` is a rate on damage that
counts once for every kill. It lands beside the damage rate, in the skill's
`DataSet`, and the Rhino carries it from the first tick as
`damage_rate_by_kill_count`. Each kill adds the rate to the damage's
enhancements, beside the skill's damage rate and its buffs':

```text
damage = base × (1 + damage rate + buff rate + kills × per-kill rate) × remaining
```

A kill is counted for every unit that hit the target and is alive when it
dies, the killer or not. Two Rhinos on one Sledgehammer each count it however
it was finished, and a Rhino counts nothing for a target another unit killed
that it never hit. The count starts at zero each fight, and the fight's end
clears it, so the last state of a fight reads each unit's damage without its
kills. The rate's `+0.1` is stored as `429496729`, a hair under a tenth, so
four kills on a Rhino's 3560 make 4983 rather than 4984.

## Which units a correction reaches

Each row carries the `mech_type` the build stores and, where the build resolves
one, the unit list that goes with it:

| `mech_type` | Units listed | Read as |
| ---: | --- | --- |
| 10 | the units themselves | those units |
| 0 | none | every unit |
| 11 | none | no unit: the correction is on a shield, a mine or a travel time |
| 1 | the air units | those units |
| 4 | none | ranged units |

Type 10 is the common case and needs no reading: Improved Sledgehammer lists Sledgehammer
and corrects Sledgehammer. Type 0 carries no list and reaches everything, which
is what Advanced Offensive Tactics' `+0.3` damage is: the officer a side may hold twice, and
`docs/spec/document/layout.md` keeps `officers` a multiset for exactly that.
Type 11 rows correct `energy_shield_rate`, `land_mine_rate` or
`super_deployment_time_rate`, none of which is a unit's number at all.

Type 4 is Advanced Targeting System, with `+10` of range and no unit list. The category is
`UnitEffectTargetType.Ranged`, and `UnitUtility.IsEffectTarget`, which
officers, equipment, energy-tower skills and unit reinforcements all ask,
answers it from the unit's main `SkillData.isMeleeAttack`: a unit whose main
skill is not a melee attack is ranged. Melee (3) is the same flag set. Of the
units the simulator describes, only the Rhino and the Crawler are melee, and the
unit table carries the flag as `attack.melee`.

## What the odd fields touch

Three fields correct something that is not a unit's number, and the officer
index's text says what each one is:

| Field | What it corrects |
| --- | --- |
| `energy_shield_rate` | the Energy Shield device's shield; Advanced Shield Device's `+0.4`, which [contraptions.md](contraptions.md#what-an-officer-adds) states |
| `land_mine_rate` | the Sentry Missile device's damage; Advanced Missile Device's `+2`, likewise |
| `super_deployment_time_rate` | the teleport time a rear deployment takes; Quick Teleport's `-0.5` |

`exp_rate` is a fourth that corrects no unit's number: a rate on the experience
a unit gains, `+1` or `+0.75`, which the text reads as "increases EXP Growth
Rate by 100%". It lands on the unit's card rather than on the unit, and the
card hands it to the unit's formation;
[unit_experience.md](unit_experience.md#an-officers-rate) states what it does
to a gain.

## Evidence

### Recorded

- One enhancement multiplies the description once, and two on one number sum
  into one stored entry: `tests/modifier/`.
- An officer's damage rate lands in the skill channel, and its life rate in the
  unit channel: `tests/modifier/`.
- Two impairments compound into one stored reduction:
  `tests/modifier/`.
- A range value is added in metres, beside an impairment on the same unit:
  `tests/modifier/`.
- A technology's range value and an officer's sum into one stored entry:
  `tests/modifier/`.
- Two speed values sum: `tests/modifier/`.
- A `Ranged` row reaches the ranged units of a side and not its melee ones:
  `tests/modifier/`.
- A kill-count damage rate raises damage once per kill, counts a death for
  every living unit that hit the target, counts nothing for a target the unit
  never hit, and is cleared as the fight ends:
  `tests/modifier/officer-kills-crawlers.yaml`,
  `tests/modifier/officer-kills-assist.yaml`,
  `tests/modifier/officer-kills-others.yaml`.
- An experience rate is on no unit's modifier set:
  `tests/modifier/officer-exp-rate-marksman.yaml`.

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
- A kill-count damage rate is written beside the damage rate, into
  `SkillDataChangeFloatRate.DamageRateByKillCount`, and its enhancement times
  the skill's kills joins the damage's: `SkillDataModifier.AddData`,
  `DamageProperty.CalculateDamage`, `DamageCalculator.killCount`.
- A death counts for every attacker on the target's list that is alive:
  `FightCoreSystem.OnActorHitted`, `FightCoreSystem.AddAttackData`,
  `FightController.OnActorHitted`, `FightMech.AddKillCount`,
  `FightSkill.AddKillCount`, `DamageCalculator.AddKillCount`.
- The count is set back to zero: `DamageCalculator.Clear`.
- A target type is answered from the unit's main skill:
  `UnitUtility.IsEffectTarget`, `UnitEffectTargetType.Ranged`,
  `SkillData.isMeleeAttack`.
- The experience correction is a rate, on the unit's card and not on the unit:
  `OfficerData.expChangeRate`, `OfficerData.get_ExpChangeRate`, and
  `OfficerData.GetExpChangeRate`, which answers zero to
  `MechDataModifer.TryAddCommonData`.

### Not established

- **What clears the count at the fight's end.** `DamageCalculator.Clear` sets
  it to zero, and every recording reads the damage without kills on its last
  tick; which call of the fight's end reaches it is not read, and a fight that
  runs out of time is assumed to clear it the same way.
- **That a value joins before a rate.** It was measured on a Sledgehammer's
  attack interval in another version, with a script that needs the game, and
  no gameless test pins it.
- **Which of a unit's skills a correction reaches beyond its main skill.** The
  writer that selects them, `SkillDataModifier.AddData` taking the source,
  asks its source more than it did; every recorded row reaches the main skill.
- **How an attack interval composes when two channels correct it.** Damage and
  move speed compose across channels as within one, which
  [towers.md](towers.md) records; the interval is not recorded.
