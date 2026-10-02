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
the rows of every `EquipmentGroupData` list a standard match can deal, which
[`extract-equipment-effects.py`](../../scripts/extract/extract-equipment-effects.py)
reads from the build's typed export. A row's `kind` is the list it comes from.
Every class is an `EquipmentData`, and `Equipment.AddData` writes the
correction of a row of any class: Absorption Module, a
`LifestealEquipmentData`, also raises its unit's life by 0.3. What a subclass
does beyond that is its kind's mechanism. `MobilityIntensifyEquipment`, the
Deployment Module's class, overrides nothing of `Equipment`: in a fight it
writes its row's numbers, none, and its effect is the deployment rule
[mobility.md](mobility.md) states. A field is written only when it is set,
and only the fields that say what a row does in a fight: its targeting, its
skill selection, its lifetime and its corrections. What an item costs is
[`config/reinforce_items.yaml`](../../config/reinforce_items.yaml)'s, and its
names are [equipment.md](equipment.md)'s.

## Across rounds

An item a standard 1v1 deals stays on the formation wearing it for every
round the formation survives, and writes the same corrections in each. Its
`Equipment.durability` starts at -1. `Equipment.SetOwner` sets it from the
row's `roundDuration` only when that is positive, and
`UnitManager.OnEnterDeployment` counts it down through
`Equipment.ReduceDurability` as each deployment opens. Nothing in a fight reads
it. Every row with a `roundDuration`, all 1, is an experimental item
limited to scene 1300, the reinforcement label that game rule `999903`
(Experimental Equipment) swaps in; a standard 1v1 names no game rule, and a
replay that does is refused when it is converted.

## What is refused

A layout is refused by name, rather than fought with part of an item, when it
carries:

- an equipment of a class whose mechanism is not here, named by its kind:
  shields and production lines among them. Absorption Module's and Nano
  Repair Kit's classes are read: their life steal and repair are
  [combat.md](combat.md#lifesteal)'s. So is Explosive Ammo's, which adds its
  `splash_range` to the skill's splash as [combat.md](combat.md#damage-and-death)
  states;
- a row that sets `importantUnit` (Dominion Core) or `roundDuration` (Rapid
  Autoloader, rule `999903`'s), whose effects have not been recorded.

## Evidence

### Recorded

- Heavy Armor's life rate lands in the unit's channel, and sums with an
  officer's life rate in one aggregate: `tests/equipment/fights/`.
- Improved Firepower Control System's damage rate lands in the skill's channel,
  and sums with an officer's damage rate in one aggregate:
  `tests/equipment/fights/`.
- Laser Sights' range value lands in the skill's channel, and the fight uses it:
  `tests/equipment/fights/`.
- Two items on one formation each write in their own channel:
  `tests/equipment/fights/`.
- A `Ranged` row reaches the ranged units of a side and not its melee ones, as an
  officer's does: `tests/modifier/fights/`.

### Replayed

- A fitted item stays on its formation into every round the formation
  survives, with durability -1 throughout: `scripts/corpus/verify-matches.py`.

### Read

- An item writes through the writers an officer does:
  `Equipment.AddData`, `MechDataModifer.TryAddCommonData`,
  `SkillDataModifier.AddData`.
- An item's target type is answered as an officer's is:
  `UnitUtility.IsEffectTarget`.
- An item wears out only by a positive `roundDuration`, counted down as a
  deployment opens: `Equipment.SetOwner`, `Equipment.ReduceDurability`,
  `UnitManager.OnEnterDeployment`, `EquipmentData.roundDuration`.
- Rule `999903` deals the items limited to scene 1300:
  `GameRule.replaceReinforceLabel`, `ItemData.limitedScene`.

### Not established

- **Two items correcting the same number.** Whether they sum in one channel, as
  two officers do, is read from the writers, not recorded.
- **An item below some level.** Only a technology overrides
  `IsLocked(CardLevel)`; what an item answers is not read.
- **An item whose durability runs out.** How `OnEnterDeployment` takes it off
  is not read; no standard item reaches it.
- **Equipment on a construction or a tower**, what the other item classes do,
  and the reactor supply an item changes, which the ledger owns.
