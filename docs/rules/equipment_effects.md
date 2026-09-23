# Equipment corrections

This rule is pinned to build **1.11.1.3.2259**. An ordinary `EquipmentData`
row corrects the unit wearing it through the same writers as an officer, in
the same channels, and its rates sum with an officer's there. It does not
change the unit's description.

## Where an equipment writes

`Equipment.AddData(ISkillOwner)` calls `MechDataModifer.TryAddCommonData`,
and the overload taking a `FightSkill` calls `SkillDataModifier.AddData`: the
writers an officer's correction goes through. `EquipmentData` implements
`ICommonMechDataChangeDataSource`, `ISkillDataChangeDataSource` and
`IUnitDataChangeDataSource`, as `OfficerData` does, so each field is the
field of [officer corrections](officer_effects.md) with the same name and
lands where that one does: a life rate in the unit's `DataSet`, a damage rate
and a range value in the skill's. A rate enhances or impairs, a value joins
the description, and the whole resolves as
`(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)`.

An equipment is worn by one formation. Another formation of the same unit
type on the same side carries none of it.

`tests/equipment/` recorded a level-one Marksman with each of three items,
alone and beside the officer that corrects the same number:

| Fixture | Reading | Channel aggregate |
| --- | ---: | --- |
| Heavy Armor | life 2838 of 1622 | unit `life_rate` `3221225472` (+0.75) |
| Heavy Armor, Advanced Defensive Tactics | life 3325 | unit `life_rate` `4509715660` (+1.05) |
| Improved Firepower Control System | damage 3842 of 2329 | skill `damage_rate` `2791728742` (+0.65) |
| the same, Advanced Offensive Tactics | damage 4541 | skill `damage_rate` `4080218930` (+0.95) |
| Laser Sights | range 160 of 140 | skill `attack_range_value` `85899345920` (+20) |

A second multiplicative bracket would have given 3690 and 4995. The two
officer fixtures read one aggregate holding both rates, so the equipment is
not kept apart from the officer either. Laser Sights at 170 m apart opens fire
without walking where the unequipped Marksman walks first, so the written
range acts, not only reads.

## Which units an equipment reaches

A row's `mech_type` is a `UnitEffectTargetType`, answered by the
`UnitUtility.IsEffectTarget` an officer's row is answered by (see
[officer corrections](officer_effects.md#which-units-a-correction-reaches)).
Ten rows are `All`. Laser Sights is `Ranged`: every unit whose main skill is
not a melee attack, which `tests/modifier/targeting.mcscript` recorded on four
ranged units across the projectile and laser paths. On a Rhino or a Crawler
it writes nothing.

A skill correction reaches the main skill through `mainSkillEffect`, which
every row sets. `extraSkillEffect` selects a unit's other skills, which this
simulator does not give a unit.

## The table

[`config/equipment_effects.yaml`](../../config/equipment_effects.yaml) holds
the eleven rows of `EquipmentGroupData.equipmentDatas`, path 188 of `level0`,
which [`extract-equipment-effects.py`](../../scripts/extract-equipment-effects.py)
reads in the serialized field order of `GRObject`, `ConfigData`, `ItemData`,
`ReinforceItemData` and `EquipmentData`, checking the three recorded fields
before it writes. A field is written only when it is set, and only the fields
that say what a row does in a fight: its targeting, its skill selection, its
lifetime and its corrections. What an item costs is
[`config/reinforce_items.yaml`](../../config/reinforce_items.yaml)'s.

| ID | Item | What it writes |
| --- | --- | --- |
| 13030001 | Laser Sights | range +20 m; Ranged; main skill only |
| 13030002 | Heavy Armor | life +0.75 |
| 13030003 | Improved Firepower Control System | damage +0.65 |
| 13030004 | Enhancement Module | life +0.25, damage +0.25 |
| 13030005 | Haste Module | damage +0.35, speed +5 m/s |
| 13030006 | Super Heavy Armor | life +1.5 |
| 13030007 | Amplifying Core | life +0.5, damage +0.5 |
| 13030008 | 高级火控系统 | damage +2 |
| 13030009 | Small Amplifying Core | life +0.22, damage +0.22 |
| 13030010 | Dominion Core | life +1, damage +0.5; `importantUnit` |
| 13030011 | 快速装填机 | interval −0.5; `roundDuration` 1 |

## What is refused

A layout is refused by name, rather than fought with part of an item, when it
carries:

- an equipment that is not one of these eleven rows: the catalogue's other
  ten items, shields, repair kits and production lines among them, are not
  ordinary `EquipmentData`, and no mechanism here reads what they do;
- an equipment in a round after the first, since what `Equipment.durability`
  does across rounds has not been recorded;
- Dominion Core's `importantUnit` and 快速装填机's `roundDuration`, whose
  effects have not been recorded.

A unit's level is not a boundary: `Equipment`'s `IsLocked(CardLevel)`
returns false for every level, and the level scales the description before
any correction reaches it.

## Not covered

Equipment on a construction or a tower; what the other classes do; the
reactor supply an equipment changes, which the ledger owns. A new build, or a
recording that contradicts these channels, reopens this rule.
