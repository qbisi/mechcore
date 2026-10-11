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

A row naming several types reaches a unit only if every one does:
`IsEffectTarget` walks them and stops at the first that does not. Barrier's
`[7, 2]` is `Huge` and `Ground`: a unit whose `UnitType` is huge, the unit
description's `size`, and that does not fly.

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

## Buff items

**A buff item whose trigger is the fight's start adds its buff to the unit
wearing it on the fight's first tick, once.** Photon Coating is one: each
unit of the formation adds `buffDatas` row 4000 to itself, recorded as written
by itself, for the row's duration, before any battle skill lands that tick.
The buff writes its `amplifyDamageRate` on the damage the unit takes, as a
tower's loss writes its own, and makes the unit invincible while it runs.

**An invincible unit takes no debuff.** A buff the build marks `debuff`, an
Electromagnetic Impact's among them, does not reach a unit a running buff
makes invincible: nothing is written and nothing is recorded, so neither its
slow nor its disabling of technologies applies. A tower's loss writes no
debuff and still reaches it. `debuff` is
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml)'s,
[`config/towers.yaml`](../../config/towers.yaml)'s and
[`config/contraptions.yaml`](../../config/contraptions.yaml)'s, and the item's
buff is [`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)'s.

Its buff may also raise the unit's damage, its speed and its maximum life,
and stack a step at a time, the item may keep it on the units around its
unit, and a hit may add it, Charged Ammo's a buff that disables technology,
as a buff technology's does
([technology_effects.md](technology_effects.md#buff-technologies)); an item's
is not switched off with its unit's technologies, unless one of them is a
buff source, whose switching stops every controller of the unit
([technology_effects.md](technology_effects.md#switched-off)). A buff
item with any other trigger, target or chance, or whose buff sets a field
beyond these, is refused by name.

**A formation travelling in starts its fight-start buffs as it lands.**
`BuffCycleController.OnEnterFight` sets each controller's time to zero but
starts none on a unit still travelling (`SuperDeploymentSystem.IsTravelling`).
As the unit lands, `FightEffectSystem.ActiveEffect` has
`BuffCycleController.Active` start each whose listener is the fight's start
from its delay (`running`, `BuffCycleState.Delaying`), as a unit made or
summoned in the fight starts them as it joins. A unit that dies has its
controllers taken off (`BuffCycleController.Deactive`), and one that rises
again has them back, their time at zero, and started from their delay again,
a once-only buff added again; a range cycle keeps the frame its
`RangeUnitCycle` stood at. Read from the build; no recording holds it.

**An anti-interference item makes its unit ignore its buff group, from the
fight's start.** Anti-Interference Module's group holds the rows of every
tower's loss and of every electromagnetic buff, the Electromagnetic Impact's
among them; [`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)
lists them as the row's `ignored_buffs`. A buff its unit ignores is not added
and not recorded, whoever writes it. The item is a permanent effect, active
already during deployment, which changes nothing in the fight: the ignored
buffs are in force from its start.

## Production lines

**A production line makes its unit for as long as the fight lasts.** As the
fight starts it hands its wearer's side a creator, which joins the side's
production lines rather than its battle skills' creators. It updates on every
tick from the first, where a summon's creator does and before its side's, and
makes `create_count_per_time` of its unit on its first update, the fight's
first tick, and again every `create_duration`; the row's `start_time` delays
nothing. It stops for good after `max_batch` batches, and makes none while
`max_alive` of its makes stand, counting those still appearing. It never
finishes and never holds the fight: a fight ends while a line could still
make.

**Where a make stands.** The makes of a batch take the row's `positions` in
turn, each an offset `x` to the right and `z` ahead of the wearer, turned by
the wearer's facing about the vertical as `FQuaternion.AngleAxis` turns it,
and face as the wearer faces. A line of one position that may keep two or
more alive first scatters each make by a draw of up to a metre each way from
its side's stream, a hundredth of a metre a step, x then z; Best Partner's
does ([technology_effects.md](technology_effects.md#production-lines)). A line
of no positions makes where its wearer stands, each make scattered within the
wearer's radius, the whole metres of it, by two draws a hundredth of a metre a
step, x then z (`SpecialSupportUnitData.GetRandomRange`).

**A make that takes no time to appear joins at once.** One of `appearType` 0
or 1 joins its side as it is made (`SummonSystem.AddMech`) rather than a
second on, drawing its skills' first intervals from its side's stream before
the next make's scatter; it is made after every unit has updated, so its
clock holds at its interval until its first update. Made on the fight's first
tick, it is in the recording's first snapshot, an initial unit numbered by
side, `z` and `x` among the layout's
([mcfr.md](../spec/mcfr/mcfr.md#normal-form)). Each is then a summon at level 1, carrying what
its side's officers and technologies write onto its type, and appears for a
second as [battle_skill.md](battle_skill.md#a-summon) states. The rows are
[`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)'s
`production`.

A row with a `unit_level`, a `max_create_count`, a `unit_life_rate`, no
`positions` or an `appear_type` other than 5 is refused by name, and so is a
make due after its wearer has fallen.

**A unit runs every line it is handed, each its own creator.** Its
equipment's, its technologies' and its extra weapons' lines each reach it as
a source, and `SupportUnitProvider.AddEffect` has `SupportUnitSystem.
AddSkillOwner` make a `SupportUnitCreator` of each. Each runs as a line alone
does, and a buff that disables technology switches only those its
technologies hand it (`SupportUnitSystem.Disable` finds a creator by its
owner and its source).

**Lines due on one tick make by the unit type they make, the highest first.**
As the fight starts, `TeamSupportUnitManager.OnFightStart` sorts the side's
lines by the unit type each makes (`SupportUnitData.GetUnitID`), then by the
technology or item that hands it (`GetID`), then by where its owner stands
(`FightUtility.PositionComparer`), and `UpdateCreators` walks the list from
its end. A War Factory with Phoenix, Steel Ball and Sledgehammer Production
makes its Phoenix, then its Sledgehammers, then its Steel Balls, whatever
order its technologies were researched in; a Vulcan with a Tank Production
Line and Best Partner makes its Sledgehammers before its Marksman. A line
that joins later, a make's own, is appended after them.

## An important unit

**A side does not outlive its last important unit.** Dominion Core's row sets
`important_unit`, and every unit wearing it is one: each Crawler of a
formation that wears it. The row writes its life and damage rates as any
ordinary row does, once. When the tick's deaths are taken, after every unit
has updated and every shot landed, the death of an important unit that
leaves its side none standing destroys every other unit of the side still
standing. Each loses its whole life, which no shield takes and no damage event
records, and dies after every other death of the tick, in its side's order,
credited to no one. An important unit that dies while another of its side
stands destroys nothing.

A side whose last important unit dies while a summon of it is still
appearing, or a unit of it still travelling, is refused by name.

## What is refused

A layout is refused by name, rather than fought with part of an item, when it
carries:

- an equipment of a class whose mechanism is not here, named by its kind.
  Absorption Module's and Nano Repair Kit's classes are read: their life steal and repair are
  [combat.md](combat.md#lifesteal)'s, Portable Shield's is
  [combat.md](combat.md#personal-shield)'s and Barrier's
  [contraptions.md](contraptions.md#a-shield)'s. So is Explosive Ammo's, which adds its
  `splash_range` to the skill's splash as [combat.md](combat.md#damage-and-death)
  states, Photon Coating's and Anti-Interference Module's, which
  [Buff items](#buff-items) states, the production lines', which
  [Production lines](#production-lines) states, and Dominion Core's, which
  [An important unit](#an-important-unit) states;
- a row that sets `roundDuration` (Rapid Autoloader, rule `999903`'s), whose
  effect has not been recorded.

## Evidence

### Recorded

- Heavy Armor's life rate lands in the unit's channel, and sums with an
  officer's life rate in one aggregate: `tests/equipment/`.
- Improved Firepower Control System's damage rate lands in the skill's channel,
  and sums with an officer's damage rate in one aggregate:
  `tests/equipment/`.
- Laser Sights' range value lands in the skill's channel, and the fight uses it:
  `tests/equipment/`.
- Two items on one formation each write in their own channel:
  `tests/equipment/`.
- A `Ranged` row reaches the ranged units of a side and not its melee ones, as an
  officer's does: `tests/modifier/`.
- Photon Coating's buff is added on tick 1 to each unit wearing it, written by
  itself, cuts the damage it takes, and keeps an Electromagnetic Impact's
  debuff off it: `tests/equipment_buff/photon-coating.yaml`,
  `tests/equipment_buff/photon-coating-crawlers.yaml` and
  `tests/equipment_buff/photon-coating-emp.yaml`.
- Anti-Interference Module keeps an Electromagnetic Impact's buff and a
  tower's loss off its unit: `tests/equipment_buff/anti-interference-emp.yaml`
  and `tests/equipment_buff/anti-interference-tower.yaml`.
- A production line makes its first batch on tick 1 and the next a
  `create_duration` later, each make where its offset turned by the wearer's
  facing puts it, and the fight ends while the line could still make:
  `tests/production/`.
- Lines due on one tick make by the unit type they make, the highest first,
  whatever order the technologies are listed in:
  `tests/production/lines-make-by-unit-type.yaml`.
- A side's last important unit dying destroys the rest of the side on that
  tick, after it and credited to no one, through a full shield and whether a
  shot or a direct hit killed it; one dying while another stands destroys
  nothing: `tests/important_unit/`.

### Replayed

- A fitted item stays on its formation into every round the formation
  survives, with durability -1 throughout: `scripts/corpus/verify-matches.py`.

### Read

- An item writes through the writers an officer does:
  `Equipment.AddData`, `MechDataModifer.TryAddCommonData`,
  `SkillDataModifier.AddData`.
- An item's target type is answered as an officer's is:
  `UnitUtility.IsEffectTarget`.
- A line of no positions makes at its parent's transform, scattered within
  its radius: `SpecialSupportUnitData.GetPosition`,
  `SpecialSupportUnitData.GetRandomRange`, `IBuffTarget.GetRadius`. A make of
  no delay joins at once: `SummonSystem.DoCreateMech`, `SummonSystem.AddMech`.
- A line of one position whose `mechMaxCount` is at least 2 adds
  `NextInRange(100)` hundredths of a metre to x and then to z before its
  offset: `SummonSystem.CreateMech`, `GRRandom.NextInRange`.
- An item wears out only by a positive `roundDuration`, counted down as a
  deployment opens: `Equipment.SetOwner`, `Equipment.ReduceDurability`,
  `UnitManager.OnEnterDeployment`, `EquipmentData.roundDuration`.
- A buff item's fight-start trigger: `BuffEffectProvider.RegisterEffectEvent`,
  `BuffCycleController.OnEnterFight`, which starts no controller on a
  travelling unit, `BuffCycleController.Active` and `Deactive`,
  `BuffCycleController.Update`, which triggers once with no
  delay and no interval, and `BuffSystem.AddBuffByCheck`;
  `FightController.AddModules` adds `BuffSystem` before
  `CommanderSkillSystem`.
- An invincible unit takes no debuff: `BuffManager.AddBuff` returns at once
  for an `IBuffData.IsDebuff` buff while `BuffManager.IsInvincible`.
- An anti-interference item's group is ignored from the fight's start:
  `IgnoreBuffEffectSystem.Active` holds it while the fight has not begun,
  `IgnoreBuffEffectSystem.OnEnterFight` and
  `IgnoreBuffEffectSystem.ApplyIgnoreBuff` add every buff of
  `IIgnoreBuffDataSouce.GetIgnoredBuffs` to `BuffManager.AddIgnoredBuff`, for
  good since `IgnoreBuffEquipment.GetDuration` is zero, and
  `BuffSystem.DoAddBuff` adds no buff its target `IsIgnoredBuff`; a permanent
  effect is activated during deployment by `EffectProvider.ActiveCheck`.
- A side's lines are sorted as the fight starts and walked from the end:
  `TeamSupportUnitManager.OnFightStart`, its comparison
  `<OnFightStart>b__10_0`, `SupportUnitData.GetUnitID`, `SupportUnitData.GetID`,
  `FightUtility.PositionComparer`, `TeamSupportUnitManager.UpdateCreators`.
- A production line's creator: `TeamSupportUnitManager.AddCreator` adds it to
  `creators`, where a battle skill's goes to `temporaryCreator` by
  `TeamSupportUnitManager.AddTemporaryCreator`; `TeamSupportUnitManager.Update`
  runs `creators` before `temporaryCreator`, each latest first, and
  `TeamSupportUnitManager.IsStepFinish` reads `temporaryCreator` alone.
  `SupportUnitCreator.Update` counts its batches against
  `SupportUnitCreator.IsBatchMax` and its living makes against
  `SupportUnitCreator.IsMaxLimit`, and `SupportUnitCreator.CreateMech` places
  a make by its owner's rotation.
- An important unit: `MechDataModifer.TryAddCommonData` marks it by
  `FightMech.SetImportantUnitData` and goes on to write the row,
  `FightMech.IsImportant`; `DeadEffectSystem.Update` takes `deadActors` by
  index as the list grows and, for a dead unit that is no summon, calls
  `DeadEffectSystem.TryProcessDeadImportantUnit`, which removes it from its
  side's and calls `DeadEffectSystem.CheckTeamImportantUnit`; that destroys
  every unit of the side's `activeMeches` with life left by
  `FightActor.DestroySelf`, a suicide's `FightActor.ReduceLife` of its whole
  life, unless an important unit of the side remains.
- Rule `999903` deals the items limited to scene 1300:
  `GameRule.replaceReinforceLabel`, `ItemData.limitedScene`.

### Not established

- **Two items correcting the same number.** Whether they sum in one channel, as
  two officers do, is read from the writers, not recorded.
- **An item below some level.** Only a technology overrides
  `IsLocked(CardLevel)`; what an item answers is not read.
- **An item whose durability runs out.** How `OnEnterDeployment` takes it off
  is not read; no standard item reaches it.
- **Anti-Interference Module against a Hacker's control beam**
  (`IgnoreBuffEquipment.IgnoreControllerBeam`): the simulator fights no
  Hacker.
- **Equipment on a construction or a tower**, what the other item classes do,
  and the reactor supply an item changes, which the ledger owns.
