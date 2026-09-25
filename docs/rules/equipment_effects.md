# Equipment corrections

An ordinary `EquipmentData` row corrects the unit wearing it through the same
writers as an officer, in the same channels, and its rates sum with an
officer's there. It does not change the unit's description.

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
type on the same side carries none of it. A formation wearing two
([equipment.md](equipment.md)) goes through the same writers twice.

`tests/equipment/` holds a level-one Marksman, whose description
[`config/units/marksman.yaml`](../../config/units/marksman.yaml) holds, with
each of three items, alone and beside the officer that corrects the same
number: Heavy Armor's life rate, Improved Firepower Control System's damage
rate, and Laser Sights' range value. Beside the officer, the recording reads
one aggregate holding both rates, and the fight is the one a single summed rate
gives, not a second multiplicative bracket. Laser Sights at a distance beyond
the unequipped range opens fire without walking where the unequipped Marksman
walks first, so the written range acts, not only reads.

## Which units an equipment reaches

A row's `mech_type` is a `UnitEffectTargetType`, answered by the
`UnitUtility.IsEffectTarget` an officer's row is answered by (see
[officer corrections](officer_effects.md#which-units-a-correction-reaches)).
Most rows are `All`. Laser Sights is `Ranged`: every unit whose main skill is
not a melee attack. On a Rhino or a Crawler it writes nothing.

A skill correction reaches the main skill through `mainSkillEffect`, which
every row sets. `extraSkillEffect` selects a unit's other skills, which this
simulator does not give a unit.

## The table

[`config/equipment_effects.yaml`](../../config/equipment_effects.yaml) holds
the rows of `EquipmentGroupData.equipmentDatas` a standard match can deal,
which [`extract-equipment-effects.py`](../../scripts/extract-equipment-effects.py)
reads from the build's typed export. A field is written only when it is set,
and only the fields that say what a row does in a fight: its targeting, its
skill selection, its lifetime and its corrections. What an item costs is
[`config/reinforce_items.yaml`](../../config/reinforce_items.yaml)'s, and its
names are [equipment.md](equipment.md)'s.

## What is refused

A layout is refused by name, rather than fought with part of an item, when it
carries:

- an equipment that is not a row of the table: the catalogue's other items,
  shields, repair kits and production lines among them, are not ordinary
  `EquipmentData`, and no mechanism here reads what they do;
- an equipment in a round after the first, since what `Equipment.durability`
  does across rounds has not been recorded;
- a row that sets `importantUnit` (Dominion Core) or `roundDuration` (Rapid
  Autoloader), whose effects have not been recorded.

## Evidence

### Recorded

- Heavy Armor's life rate lands in the unit's channel, and sums with an
  officer's life rate in one aggregate: `tests/equipment/regressions.mcscript`.
- Improved Firepower Control System's damage rate lands in the skill's channel,
  and sums with an officer's damage rate in one aggregate:
  `tests/equipment/regressions.mcscript`.
- Laser Sights' range value lands in the skill's channel, and the fight uses it:
  `tests/equipment/regressions.mcscript`.
- Two items on one formation each write in their own channel:
  `tests/equipment/regressions.mcscript`.
- A `Ranged` row reaches the ranged units of a side and not its melee ones, as an
  officer's does: `tests/modifier/regressions.mcscript`.

### Read

- An item writes through the writers an officer does:
  `Equipment.AddData`, `MechDataModifer.TryAddCommonData`,
  `SkillDataModifier.AddData`.
- An item's target type is answered as an officer's is:
  `UnitUtility.IsEffectTarget`.

### Not established

- **Two items correcting the same number.** Whether they sum in one channel, as
  two officers do, is read from the writers, not recorded.
- **An item below some level.** Only a technology overrides
  `IsLocked(CardLevel)`; what an item answers is not read.
- **Equipment on a construction or a tower**, what the other item classes do,
  and the reactor supply an item changes, which the ledger owns.
