# What a technology does to a fight

The corrections a technology writes onto the unit that researched it, how they
are encoded, and how they grow with the unit's rank. Which technologies a unit
may research and what each costs is [unit_techs.md](unit_techs.md)'s.

[`config/technology_effects.yaml`](../../config/technology_effects.yaml) holds
every technology a unit may research, with the numbers it writes onto its
unit; `scripts/extract/extract-technology-effects.py` writes it from every list
of `TechnologyGroupData` in `level0`. A row's `kind` names the list it comes
from.

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

## Plain technologies, and the rest

A row of `technologyDatas` is a plain `TechnologyData`: it does nothing in a
fight but correct its unit's numbers. A row of any other list is a subclass,
`BuffTechnologyData`, `SplashTechnologyData`, `ExtraWeaponTechnologyData` and
the rest, and does something more, although many carry numbers as well: Double
Shot's second shot, High-Explosive Ammo's splash, Energy Absorption's life
steal, Field Maintenance's repair. 64 of the 241 technologies are plain. The
simulator applies a plain technology's numbers, a lifesteal technology's
with the life steal [combat.md](combat.md#lifesteal) states, and a repair
technology's with [its repair](combat.md#repair), a sweep technology's with
the strip it changes ([sweep.md](sweep.md#technology)), an armour
technology's with the reduction of each hit [combat.md](combat.md#armour)
states, which its row's `reduce_damage_value` gives by unit level, a
search-target technology's with its numbers against aircraft and its search
by distance, a damage-intensify technology's with its damage against one
domain, a secondary-damage technology's with the second damage it deals
around its unit's hits, an air-attack technology's with its skills turned onto or off
aircraft ([combat.md](combat.md#aerial-and-ground-targets)), Secondary Armament,
Anti-Air Missile, Incendiary Bomb, Scorching Charge, Homing Missile, Sticky
Oil Bomb, Whirlwind and Energy Diffraction with
the skill each adds, and Energy Diffraction's `all_weapon_reduce_damage_rate`
on the damage of its unit's skills
([extra_weapons.md](extra_weapons.md)), and refuses
every other technology by name and kind, since applying a subclass's numbers
alone would fight it as something it is not.

A plain, a lifesteal or a repair row may still set a field beyond what this
table carries, and names it in `special`: Siege Mode's `isInverseIsLockTarget`, and Machine
Learning's `expChangeRate`, which speeds up the experience its unit gains. The
simulator refuses those too.

## Buff technologies

**A buff technology whose trigger is the fight's start adds its buff to its
unit on the fight's first tick, once,** as a buff item does
([equipment_effects.md](equipment_effects.md#buff-items)): both are the same
buff source. Combat Evolvement is one, on the Rhino.

**A buff source that keeps its buff on the units around its unit adds it
again on every tick.** Under the update model `Each`, the source's controller
leaves its delay on its first update, selects nothing on the next seven, and
from its eighth, the fight's ninth tick, takes on every update the units in
reach: every live unit of either side, not underground, of a domain the
source's fly type takes, within the source's range of its unit (to the unit's
edge where the source says so), and of a target type it names: its unit, its
side's others, its side, or the enemy. Each of them takes the buff again, the
ones it held before first and new ones after. The source's interval and delay
never reach the cycle, so the buff is added on every tick, and its duration is
how long it outlasts its unit leaving reach. The cycle stops while its unit is
dead or its technologies are disabled. Mobile Power Station keeps 30% more
damage on its Vortex and its side's ground units within 100 m; Degeneration
Beam keeps 40% less speed, 20% less damage and 30% more damage taken on the
enemies of either domain within 120 m of the Wraith.

**A buff takes back only what it wrote.** When a buff ends, its entries go and
every other buff's stay, whichever source wrote them: a Rhino with Combat
Evolvement, Mobile Power Station and Degeneration Beam keeps two when the
third runs out.

**A buff that stacks counts its stack every step.** A buff marked additive in
effect sets its stack each `stepTime`, below its bound if it has one, and
each rate or value it writes is the buff's times the stack: none before the
first stack. Stacking on time, it counts up by one a step: Combat
Evolvement's buff steps every second, and the Rhino deals 4.5% more damage a
stack. Stacking on distance, the stack is the whole number of the condition's
lengths its unit has moved while its technologies were not disabled, at most
the bound, and it may rise by more than one in a step. The distance is
counted where the unit's movement is handed on before each solve, every
fourth tick: the straight line from where the last such count found it, none
on the first. Kinetic Charge's buff steps every half second with a metre of
range for every 7 metres rolled, to 100.

**A buff's maximum life rate goes into the unit's own life rate, and the life
follows it.** The buff writes its rate once as it starts; a stacking buff takes
it out at each step and puts it back times the stack, from the second stack
on twice the first. Each change refreshes the unit's life: a unit at its whole
life keeps its whole life, and any other keeps its share, the quotient rounded
and its product with the new maximum truncated, at least 1 while it lives. A
Rhino with Combat Evolvement has 2.5% more life a stack, and a hit Rhino's
life passes through its maximum without the buff on each step. When the buff
ends, the fight's end among, the rate goes and the life is refreshed again.

Any other trigger or chance, a source that adds its buff once to other units
or cycles under the update model `All`, one that reaches crystals, measures
from its unit's edge or keeps to a distance type, a buff that stacks on
another condition or lowers what it stacks, and a buff field beyond these is
refused by name.

## What this table does not carry

A technology that is not plain does something that is not a correction on its
unit's own numbers. It may instead:

- **summon or fire something**, as Fang Production and Anti-Air Barrage do;
- **apply something to the enemy**, as Electromagnetic Explosion disables the
  target's technologies on hit.

Its numbers may also be only half of what it does: Photon Coating's reduction
of damage received is not here even though the technology also carries numbers
that are.

## Switched off

A buff that disables technology switches a unit's technologies off while it
runs: an Electromagnetic Impact's, Electromagnetic Barrage's. It is a common
buff effect over a count of such buffs (`CBEC_DisableTechnology`), so the
first to reach a unit switches them off as it is written and the last to
leave switches them on again, as its time runs out or as the fight is left
and its buffs are cleared. The unit's `status_mask` reads
`technology_disabled` the while, whether it carries a technology or not.

Switching off is each of the unit's effect providers' `DisableEffect` on each
of its sources that `CanDisable` (`FightEffectMananger.DisableEffect`), and
only a technology does: `Technology.CanDisable` answers `ignoreElectricEffect`
false, which every row but a few buff, Dual Wield and melee-mode rows does;
an officer, an equipment, an Energy Tower skill and a battle skill answer
false. So the officers' and the items' corrections stay.

- **A plain technology's numbers leave its unit.** The provider removes its
  source's data (`IEffectProviderDataSource.RemoveData`) and writes it again
  as it is switched on (`AddData`); an armour's provider its reduction, a
  damage intensify's its damage. A Marksman with Assault Mode reads none of
  its corrections from the tick the buff is written: its range is 140 metres
  again, and its maximum life 1622 for 9732. Switched on, the corrections are
  back in their first order.
- **Its life follows its maximum** as any change of the maximum moves it
  (`FightMech.RefreshLifeData`): a unit at its whole life keeps its whole life,
  any other its share.
- **Its current interval is made again without its stagger** as its numbers
  change (`FightSkill.RefreshAttackInterval`), both ways: a Rhino with
  Mechanical Rage waits 18 ticks between blows for 12 while it is off, and a
  Marksman's drawn 69 becomes its plain 62. A unit whose technologies wrote
  nothing keeps the interval it drew.
- **A lifesteal's and a second damage's** providers take their hit effect
  away, which the fight asks of the unit at each hit.
- **A search technology's reach and preference leave its unit.** Its
  provider takes away the metres it added to the main skill's range against
  a domain and to what its search counts off a candidate of that domain, and
  turns the skill's selector back to `Normal`
  (`SearchTargetSpecificProvider.DoDisable`, `FightSkill.ChangeSearchTargetType`);
  the base takes its damage rates away. Switched on, it writes them again and
  turns the selector to `DistanceIntensify`, which takes the offsets as they
  then stand. A `Normal` selector is the `DistanceIntensify` one with no
  offsets. Phantom Rays with Ground Targeting drop the Arclight they locked,
  133 metres off, as their range falls from 125 metres to 65.
- **An extra weapon's skills are disabled** (`ExtraSkillProvider.DisableSkill`,
  `FightSkill.Disable`), and enabled as it is switched on. A disabled skill
  that is idle searches for no lock or attack target and starts no attack
  (`SkillIdleState.Update` asks `isEnable`), keeping what it named; one
  attacking fails its check between blows (`SkillAttackState.CheckAttackable`),
  so the blow under way runs out, a burst's every projectile, and its attack
  ends. Enabled again, it starts as any idle skill does: a Centurion's Homing
  Missile, due while disabled, fires two ticks after its buff runs out.
- **A buff its unit added itself is cleared, if its row says so**
  (`isClearSelfBuffWhenDisableTech`): `FightMech.DisableTechnology` raises
  `BuffManager.ClearSelfResourceBuffByDisableTech` after the effects are off.
  One that stacks keeps its stack and writes its rates at none
  (`ResetAdditiveEffectStackByDisableTech`), and adds no stack at its steps
  while the technologies are off (`AddAdditiveStack`). Its life rate stays
  until its next step, which takes it out and refreshes the life once more at
  the same maximum, taking the share a second time
  (`IBEC_ChangeMaxLife.DoAdditiveEffect`). Switched on, nothing is written
  back: the next step puts the old life rate back, takes it out, adds a stack
  to the one kept and writes the rates and life rate at it. A Rhino with
  Combat Evolvement at two stacks loses its 9% damage as the Electromagnetic
  Impact lands and its 5% life at its next second; the buff run out, its next
  second writes three stacks. A buff source that adds its buff once onto its
  unit is the only kind read: one that cycles over the units around stops
  (`BuffEffectProvider.DoDisableCycle`), and a cleared buff that does not
  stack is removed, neither measured.

What switching off does to any other technology, an extra weapon's
production line, explosion, preemptive skill or group, a repair, a shield, a
cycling buff source, a sweep's change, is read below and
refused: a buff that disables technology reaching a unit that carries one is
refused by name.

## What the recordings show

One Arclight researching Range Enhancement while its side holds Extended Range
Arclight reaches the description's range plus both corrections, and the
recording stores the technology's and the officer's range as **one**
`attack_range_value`: the build merges two sources exactly as it merges two
officers.

An interval value is an `FPoint` of seconds, and it lands in
`attack_interval_value` exactly as the table states it; the interval is
composed in `FPoint` seconds and only then cut into ticks. A value such as
Mechanical Rage's is a tenth of a second, which no whole number of the
description's time units holds.

The interval technologies put a value and a rate on one number: Mechanical Rage
and Armour Piercing Bullets are the one pair that do, and they are what
measured the order in the composition rule, which
[officer_effects.md](officer_effects.md#how-a-correction-composes) carries.

The simulator refuses a side holding a technology whose correction it does not
derive (a minimum range, a projectile's life) or
whose effect grows with rank, rather than read index zero:
`crates/simulation/src/modifier/technologies.rs` names each refusal.

## Evidence

### Recorded

- A technology's range and an officer's range land in one `attack_range_value`,
  and the fight uses their sum: `tests/modifier/fights/`.
- An interval value lands as the table's `FPoint`, and the Rhino's blows follow
  the interval it composes: `tests/modifier/fights/technology-interval-value.yaml`.

- A buff technology's buff stacks every second and raises its unit's damage
  and maximum life: `tests/technology_buff/fights/combat-evolvement.yaml`, and
  on hit Rhinos `tests/corpus/fights/134260717-r2.yaml` and
  `tests/corpus/fights/134260717-r3.yaml`.
- A source of the update model `Each` adds its buff to the units in reach on
  every tick from the ninth, its side's ground units or the enemies of either
  domain, and stops with its unit's death:
  `tests/technology_buff/fights/mobile-power-station.yaml` and
  `tests/technology_buff/fights/degeneration-beam.yaml`.
- Three buffs run on one Rhino, and one ending leaves the others' rates:
  `tests/technology_buff/fights/three-buffs.yaml`.
- A buff stacking on distance adds a metre of range for every 7 metres its
  Steel Ball has rolled, two in a step where it rolled that far, and holds
  while it stands: `tests/technology_buff/fights/kinetic-charge.yaml`,
  `tests/technology_buff/fights/kinetic-charge-stops.yaml`, and to 80 metres
  `tests/corpus/fights/268477093-r4.yaml`.
- A disabling buff takes a plain technology's numbers off its unit as it is
  written, life and current interval with them, and they come back as it runs
  out or as the fight is left:
  `tests/technology_disable/fights/barrage-assault-mode.yaml`,
  `tests/technology_disable/fights/impact-to-fight-end.yaml`,
  `tests/technology_disable/fights/impact-expires.yaml`. An armour's reduction
  leaves with it, a Wasp's 142 on an armoured Rhino becoming 202:
  `tests/technology_disable/fights/impact-armor.yaml`. A unit with no
  technology keeps its drawn interval:
  `tests/technology_disable/fights/impact-on-both-sides.yaml`.
- A disabled extra skill lets the burst under way run out, starts no attack
  while disabled, and fires again as it is enabled:
  `tests/technology_disable/fights/barrage-homing-missile.yaml`,
  `tests/technology_disable/fights/barrage-homing-missile-held.yaml`.
- A search technology's ranges and offsets leave with it:
  `tests/technology_disable/fights/impact-ground-targeting.yaml`.
- A stacking buff its unit added writes no stack while the technologies are
  off, takes its life rate out at its next step with a second share of the
  life, and stacks on from the stack it kept:
  `tests/technology_disable/fights/impact-combat-evolvement.yaml`,
  `tests/technology_disable/fights/impact-combat-evolvement-expires.yaml`.

### Read

- A technology answers the same correction interface an officer does, so its
  fields land in the same channels: `TechnologyData.lifeChangeRate`,
  `TechnologyData.damageChangeRate`, `TechnologyData.attackRangeChangeValue`.
- An effect is a list indexed by rank, and a growing list is its first entry
  times the rank; `scripts/extract/extract-technology-effects.py` refuses a table where
  that does not hold, and wrote this version's: `TechnologyData.damageChangeRate`.
- The numbers a technology states appear in its own description, which the
  extraction checks: `TechnologyData.lifeChangeRate`.

- A buff technology is the buff source a buff item is:
  `BuffTech` and `BuffEquipment` both answer `IEffectBuffDataSource`, which
  `BuffEffectProvider` reads. `Buff.Update` updates its controllers each
  `stepTime`; `IBEC_AdditiveEffectBuff.Update` raises its stack through
  `BuffAdditiveStackConditionTimeController.TryAddStack` below
  `IBEC_AdditiveEffectBuff.maxAdditiveStack`; `IBEC_ChangeMaxLife.Enter`
  adds `IBuffData.GetMaxLifeChangeRate` through `MechDataModifer.AddData`,
  and `IBEC_ChangeMaxLife.DoAdditiveEffect` calls `MechDataModifer.RemoveData`
  and then `MechDataModifer.AddData` with `Buff.GetAdditiveStack`;
  `FightMech.AddData` and `FightMech.RemoveData` call
  `FightMech.RefreshLifeData`, which keeps a full `FightActor.lifeGauge` full
  and any other at its share of the new maximum.
- A stack on distance: `IBEC_AdditiveEffectBuff.Update` returns at
  `IBEC_AdditiveEffectBuff.maxAdditiveStack`, and otherwise asks
  `BuffAdditiveStackConditionDistanceController.TryAddStack`, which sets the
  stack to the integer part of `FightMech.GetTotalMoveDistanceWithoutDisableTech`
  over `IBuffData.GetBuffEffectAdditiveConditionParam`, at most the bound, and
  answers whether it moved; then `Buff.RefreshEffect` writes it.
  `MotionController.Move` returns unless `RVOSimulatorFixed.counter` is 3,
  and otherwise adds the magnitude from `MotionController.prevPosition`,
  unless that is zero, to `MotionController.totalMoveDistance` and, while
  `ISkillOwner.IsTechnologyDisabled` is false, to
  `MotionController.totalMoveDistanceWithoutDisableTech`, and sets
  `MotionController.prevPosition` to `FightTransform._position2D`.
- A source's controller runs a range cycle under the update model `Each`:
  `BuffCycleController.useUpdateFinder` is set by its constructor, which hands
  `BuffCycleController.rangeUnitCycle` the owner, the fight and the source but
  never `RangeUnitCycle.delayTimeConfig` or
  `RangeUnitCycle.intervalTimeConfig`. `BuffCycleController.Update` runs it
  while `BuffCycleController.isAvailable`, which
  `BuffCycleController.DisableEffect` clears. `RangeUnitCycle.Update` leaves
  `BuffCycleState.Delaying` once `RangeUnitCycle.curTime` reaches the delay and
  returns; then each update calls `RangeUnitCycle.UpdateSelector`, which
  returns until `RangeUnitCycle.currentFrame` reaches
  `RangeUnitCycle.selectRangeInterval` and never sets it back, and counts up
  `RangeUnitCycle.times`, invoking `RangeUnitCycle.OnActorTrigger` for each of
  `RangeUnitCycle.fightMeches` past the interval.
  `RangeUnitCycle.UpdateSelector` asks
  `RangeTargetCalculator.CalculateRangeActors` with
  `IEffectBuffDataSource.GetMax`, `IEffectBuffDataSource.GetAttackTargetFlyType`
  and `IEffectBuffDataSource.IsDistanceCalculateTargetRadius`, keeps what
  `BuffCycleController.AvailableCheck` passes, drops from its list the units
  no longer found and appends the new. `BuffCycleController.BuffEffectCallBack`
  adds the buff through `BuffCycleController.TriggerBuffOrBuffRangeItemFromSelector`
  and `BuffSystem.AddBuffByCheck`.

- Switching off: `CBEC_DisableTechnology.Enter`, `CBEC_DisableTechnology.Exit`,
  `FightEffectSystem.DisableEffect`, `FightEffectMananger.DisableEffect`,
  `EffectProvider.EnableCheck`, `EffectProvider.DisableEffect`,
  `EffectProvider.EnableEffect`, `Technology.CanDisable`,
  `ArmorStrengthenEffectProvider.DisableEffect`,
  `LifeStealEffectProvider.DoDisableEffect`,
  `SecondaryDamageIntensifyEffectProvider.DisableEffect`,
  `SearchTargetSpecificProvider.DisableEffect`,
  `SearchTargetSpecificProvider.DoDisable`,
  `SearchTargetSpecificProvider.DoEnable`,
  `SearchTargetController.SetTargetSelector`,
  `BuffEffectProvider.DisableEffect`, `BuffEffectProvider.DoDisableCycle`,
  `BuffManager.ClearSelfResourceBuffByDisableTech`,
  `Buff.ResetAdditiveEffectStackByDisableTech`,
  `IBEC_AdditiveEffectBuff.AddAdditiveStack`,
  `IBEC_AdditiveEffectBuff.RefreshAdditiveEffect`,
  `IBEC_ChangeMaxLife.DoAdditiveEffect`,
  `FightMech.RefreshLifeData`, `FightSkill.RefreshAttackInterval`,
  `ExtraSkillProvider.DisableSkill`, `ExtraSkillProvider.EnableSkill`,
  `FightSkill.Disable`, `FightSkill.Enable`, `SkillIdleState.Update`,
  `SkillAttackState.CheckAttackable`.

### Not established

- **Which units `FriendUnits` names in a fight of two teams a side.**
  `BuffCycleController.AvailableCheck` compares the units' groups and then
  their teams, and with one team a side the simulator reads it as the side;
  Mobile Power Station names it beside `OtherSelfUnits` and `MechUnit`, whose
  union is the side either way.
- **A hurt unit's life as a disable moves its maximum.** The share rule is
  the one a buff's maximum life measured; no disable has been recorded on a
  hurt unit whose technology moves its maximum.
- **What switching off does beyond numbers**: an extra weapon's production
  line (`SupportUnitCreator`), explosion, preemptive skill or group, a repair
  (`AutoRecoverySystem.DisableMech`), a unit's own shield and a Barrier's
  (`EnergyShieldProvider.DisableEffect`,
  `AdvancedEnergyShieldSystem.DeactiveEnergyShield`), a buff source that
  cycles over the units around its unit, a cleared buff that does not
  stack, a sweep's change, an air attack's switch. Refused.
- **How the life share rounds.** The quotient rounded and the product
  truncated is what the recordings fit; the arithmetic of `FPoint` division
  and multiplication was not read.

- **A value applies before a rate, for a technology.** Measured on the
  Sledgehammer's interval technologies, whose fights no test pins because the
  simulator refuses the unit.
- **What the other technologies do**, in the terms a simulator needs. Each
  one owes the mechanism it belongs to: a summon, a skill's own numbers, a
  debuff on the target.
- **Which rank index a fight reads.** The list is indexed by rank and this
  document does not state what a unit's rank is at the moment a technology is
  applied, nor whether raising a rank mid-fight re-reads it. Rank one is the
  only case any recording has covered.
- **`min_attack_range_value` and `projectile_life_rate`**, which no
  mechanism in `crates/simulation` reads.
