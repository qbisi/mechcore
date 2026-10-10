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

**A fight reads the entry for the unit's level**, and the last entry for a
level beyond the list. A rank is the unit's level: `TechnologyData`'s getters
take the unit and read entry `GetLevel()`, a `CardLevel` that counts from zero,
so a level-3 Marksman's Elite Marksman writes +15 metres and +0.51 of damage.
A unit's level does not change within a fight (`UnitSystem.ChangeLevel` runs
between rounds), so the entry is read once.

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
steal, Field Maintenance's repair. 66 of the 241 technologies are plain. The
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
([extra_weapons.md](extra_weapons.md)), a splash technology's with its
`splash_range` added to its unit's skills' splash as a splash value is
([splash](#splash-technologies)), a multi-attack technology's with the
projectiles it adds its unit's bursts ([below](#multi-attack-technologies)),
a stealth technology's with the stealth it puts its unit in once it is hurt
([below](#stealth-technologies)), a dead-line technology's with the life
under which its unit's hits destroy what they strike
([below](#dead-line-technologies)), a damage-share technology's with the
group its units share every hit in ([below](#damage-share-technologies)), a
barrier technology's with the battlefield shield its unit carries
([contraptions.md](contraptions.md#a-shield)), a
reactive armor technology's with the rate it puts on its unit's first hits
([below](#reactive-armor-technologies)), a fire technology's with the fire
each hit of its unit's main skill leaves ([below](#fire-technologies)), a
missile interception technology's
with the interceptors it makes its unit
([below](#missile-interception)), a production technology's with the line it
runs ([below](#production-lines)), a move-ability attack technology's with
its unit's shorter surfacing and the stronger first attack after it
([underground.md](underground.md#a-stronger-surfacing)), a move-ability
terrain technology's with the sand fog its unit leaves as it surfaces
([underground.md](underground.md#a-sand-fog-as-it-surfaces)), and refuses
every other technology by name and kind, since applying a subclass's numbers
alone would fight it as something it is not.

High-Speed Engine (`mobilityIntensifyTechnologies`) is plain in a fight:
`MobilityIntensifyTech` overrides nothing of `Technology`, so its 5 lands in
its unit's move speed as a plain technology's speed value does, and is
switched off with it. What else it does, freeing its formation during
deployment, happens before the fight.

A plain, a lifesteal or a repair row may still set a field beyond what this
table carries, and names it in `special`; the simulator refuses those too.

## Turning the lock over

A row that sets `isInverseIsLockTarget`, Siege Mode's and Saturation
Bombardment's, carries `inverse_lock_target`. `SkillDataModifier.AddData`
writes it into each skill it reaches as `SkillDataChangeInt.IsLockTarget`:
-1 where the unit's main skill's row locks its target and 1 where it does
not. `FightSkill.IsLockTarget` is the row's flag plus that, equal to one, so
one such technology turns a locking skill free and a free one locking. A
skill that locks nothing fires shells that fly to where it aimed rather than
after its target: a Scorpion's under Siege Mode. A row's
`min_attack_range_value`, whole metres, lands in the skill's
`MinAttackRangeValue`, which `FightSkill.GetMinAttackRange` adds to its row's:
Siege Mode's 75 keeps a Scorpion from firing at a Rhino within 75 metres. Machine Learning's `exp_rate` is no
correction but a rate on what its unit gains, which
[unit_experience.md](unit_experience.md#a-technologys-rate) carries.

## Buff technologies

**A buff technology whose trigger is the fight's start adds its buff to its
unit on the fight's first tick, once,** as a buff item does
([equipment_effects.md](equipment_effects.md#buff-items)): both are the same
buff source. Combat Evolvement is one, on the Rhino. Photon Coating is
another: 30 seconds in which its unit takes 30% less damage and no debuff
reaches it. The effect a source names is only what the client shows.

**Under the update model `All`, a buff source triggers after its delay and,
with an interval, again every interval.** Its controller counts its updates
from the fight's first tick: when the count reaches the delay in whole ticks
it triggers, and with an interval it triggers again each time the count,
less what it last spent, reaches the interval in whole ticks. A source whose
targets are its unit alone gives the buff to its unit; any other gives it,
once a trigger, to every live unit in reach of a target type it names, as a
range cycle finds them. Like a range cycle, it stands still while its unit is
dead or its technologies are disabled. Photon Emission covers its side's
other units within 100 m with 20 seconds of invincibility and 30% less
damage taken, 0.6 seconds into the fight, once: the Overlord's of either
domain, the Farseer's only on the ground. Photon Loop covers the Mountain for
25 seconds every 30.

**A buff source that keeps its buff on the units around its unit adds it
again on every tick.** Under the update model `Each`, the source's controller
leaves its delay on its first update, selects nothing on the next seven, and
from its eighth, the fight's ninth tick, takes on every update the units in
reach: every live unit of either side, not underground, of a domain the
source's fly type takes, within the source's range of its unit (to the unit's
edge where the source says so), and of a target type it names: its unit, its
side's others, or the enemy; the type of its group's other teams names none,
a side being one team. A source of the melee distance type passes only a
unit whose main skill is melee, and one of the remote distance type only a
unit whose main skill is not. Each of them takes the buff again, the
ones it held before first and new ones after. The source's interval and delay
never reach the cycle, so the buff is added on every tick, and its duration is
how long it outlasts its unit leaving reach. The cycle stops while its unit is
dead or its technologies are disabled. Mobile Power Station keeps 30% more
damage on its Vortex and its side's ground units within 100 m; Degeneration
Beam keeps 40% less speed, 20% less damage and 30% more damage taken on the
enemies of either domain within 120 m of the Wraith.

**A buff source triggered by a hit adds its buff to what the hit struck.**
Its controller is a hit effect of each of its unit's skills its corrections
reach, and after a hit it adds the buff, under the unit's side and as written
by it, to every live unit the hit struck, in the order it struck them: no
construction, and none at all while the unit's technologies are disabled.
A buff that disables technology reaches even a unit the hit killed, and is
cleared with the rest of its buffs on the same tick.
Neither the source's targets nor its domains nor its distance type are read
on a hit. Suppression Shots cuts a struck unit's range by 30% for 3.5
seconds: the range's add rates sum with its skill's and its reduce rates
multiply with them, the range times one plus the adds and that times the
reduces, which a melee reach does not take. Electromagnetic Shot
switches a struck unit's technologies off for some seconds and takes 40% off
its speed; an item's buff of the kind, Charged Ammo's, is the same source.

**A hit source that names a range item leaves an acid where the hit lands.**
After its buff, the source leaves an acid of the range item's radius and
life, under its unit's side, at the point the hit lands, when what the skill
fires at (its attack target, or else its lock) is on the ground. The acid
keeps the range item's buff on the units standing in it, as a battle skill's
acid does ([terrain.md](terrain.md)). Acid Attack's hit adds buff 2019 to what
it struck and leaves 15 m of acid for 18 seconds with the same buff; Advanced
Acid Ammo, an item, leaves one for 10 seconds. A range item of another kind,
or one whose buff writes what a terrain's buff does not, is refused.

**A buff that burns takes a share of its unit's maximum life every step, and
one that disables recovery stops its repair and lifesteal.** Each `stepTime`,
the buff takes the whole part of the unit's maximum life times its life
change rate, as a hit of no object under the side that added it, which the
rate on damage taken does not touch. While a buff that disables recovery
runs, the unit's repair and lifesteal add nothing, though their clocks run on.
Ignite, a Vulcan's hits, takes 3% of the struck unit's maximum life every half
second for two seconds and disables its recovery; every hit starts it over. A
buff that heals is refused.

**A buff source that is not certain draws for each unit it reaches.** The
source's chance is truncated to whole thousandths, so the 0.35 a table writes
is 349. A buff that reaches a unit (not ignored, and no debuff on a unit made
invincible) is added outright at a chance of 1000, never at 0, and otherwise
when a draw below 1000 from the unit's own side's stream falls under the
chance. Every other buff source is certain, and draws nothing. The Wasp's,
Fang's and Fire Badger's Ignites add the Vulcan's buff at 35%, 14% and 70%.

**A buff source triggered by its unit's losing life adds its buff to the unit
itself, each time a hit takes life from it.** The buff is added as the life
goes, before the unit's death is handled, so a unit the hit killed takes it
only when it summons or disables technology; a hit a shield holds takes no
life and adds nothing, and nothing is added while the unit's technologies are
disabled. The source's cycle is not read. Counter-Fire is one: a Fire Badger
hit reaches 70 m further for 20 seconds. A source that reaches the unit's attacker instead is refused.

**A buff source triggered by its unit's being hit adds its buff to the unit
that hit it.** Each hit a live unit lands on it, once the hit has taken what
it took (from its shield or its life), adds the buff to that unit, as written
by the unit hit and under its side, unless the unit hit's technologies are
disabled. A buff that disables technology waits on the attacker and is added
as the attacker's buffs have updated, on its own update; any other is added
at once. Electromagnetic Armor is one: a Void Eye hit switches off its
attacker's technologies for 3 seconds and takes 40% off its speed.

**A buff that summons makes its unit summon as it dies.** A buff with a
summon reaches even a unit the hit that adds it killed, and a unit that dies
running it summons, as the tick's dead effects come after every unit and
projectile has updated, units of
the type the row names, or below 1 the type of the unit that added the buff:
`max(1, (dead radius / summon radius) ^ 1.5)` of them, the whole part, in
`FPoint`'s power. They stand where it died, scattered by two draws each of
the adding unit's side's stream within its radius, join that side at once at
level 1, each in a formation of its own and with what its side's technologies
give the type, and appear in a recording right after the death, numbered by
side and then where they stand. A summon has no speed until a move before a
solve hands it one. Neither one a buff summoned nor a unit of another domain
summons. When a summoning buff is added again, the unit adding it becomes its
source if its level is the higher. Replicate is one: a Marksman killed by
Crawlers leaves 7, and a Rhino 12.

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

**A stack that resets on a hit goes back to none each time its unit's main
skill hits.** After the hit's effects, its stack and the stack its rates are
written at are none, and it builds again from its next step, which runs on.
Chamber Compression's buff stacks every 0.1 seconds, 6.5% more damage a
stack and with no bound, and a Hound's shot that lands sets it back to none.
A reset on another condition, or on a buff that holds a life rate, is
refused.

**A buff's maximum life rate goes into the unit's own life rate, and the life
follows it.** The buff writes its rate once as it starts; a stacking buff takes
it out at each step and puts it back times the stack, from the second stack
on twice the first. Each change refreshes the unit's life: a unit at its whole
life keeps its whole life, and any other keeps its share, the quotient rounded
and its product with the new maximum truncated, at least 1 while it lives. A
Rhino with Combat Evolvement has 2.5% more life a stack, and a hit Rhino's
life passes through its maximum without the buff on each step. When the buff
ends, the fight's end among, the rate goes and the life is refreshed again.

Any other trigger or update model, a source that steals life beside a hit
buff, one that reaches crystals, measures
from its unit's edge, a buff that stacks on
another condition or lowers what it stacks, and a buff field beyond these is
refused by name.

## Splash technologies

High-Explosive Ammo is a plain technology and a splash: `SplashTech.AddData`
writes its row's numbers, then adds its `range`, read at the unit's level as
its numbers are, to the skill's `SplashRangeValue`, as a splash item does.
The skill's splash is its row's plus that range: a Stormcaller's 5.5 metres
reads 10.5 with its 5, a Wasp's none reads 7, and a War Factory's four
skills and a Wraith's four slots each read theirs.

## Multi-attack technologies

A row of `multiAttackTechnologies` is a `MultiAttackTech`, Doubleshot, Burst
Mode and Saturation Bombardment. `MultiAttackTech.AddData` writes its row's
numbers, then adds, at the unit's level as its numbers are, its
`projectile_count_value` to the skill's `ProjectileCountValue`, its
`projectile_duration_value` to its `ProjectileDurationValue` and its
`projectile_random_range_value` to its `ProjectileRandomRange`, through
`SkillDataModifier.AddData` as a splash value goes. A burst fires the row's
count with the value added, the row's time between two projectiles with the
value added, and lands each within the row's radius with the value added
about its target: a Marksman's Doubleshot fires two projectiles an attack
0.2 seconds apart, and a Farseer's Burst Mode twelve 0.1 seconds apart,
each within 18 metres.

`ProjectileMultiAttackPerformer.OnStartFirstPerform` spaces a burst by
`PROJECTILE_INTERVAL`, 0.2 seconds, when the time between two is zero or
less, which a Sabertooth's row's is. Its projectiles take turns between two
weapons only when the performer makes a
`MultiAttackTargetPositionController`, for a burst that lands about its
target, and the skill has two: a Sabertooth's and a Fortress's Doubleshot
fire both from weapon 0.

Saturation Bombardment also [turns its unit's lock over](#turning-the-lock-over),
and its four projectiles leave from each of a Mountain's standalone weapons,
each weapon's burst its own ([standalone_weapons.md](standalone_weapons.md#a-batch-of-main-skills)).

## Stealth technologies

A row of `stealthTechData` is a `StealthTech`, the Vortex's Emergency Armor:
its `stealth` carries the share of its unit's maximum life and the seconds
it answers `IStealthTechDataSource` with, a half and 4.
`StealthTechEffectProvider.DoActive` hands the unit to `StealthTechSystem`
(`AddMech`), which listens to the unit's `OnLifeChange`; a unit travelling in
is handed over as it arrives, and then goes into stealth at once if its life
is already low enough.

**It goes into stealth once.** As the fight starts every unit the system
holds is pending (`OnEnterFight`). When a hit takes life from a pending unit
that is alive (`FightActor.ReduceLife` invoking `OnLifeChange`), and its life
over its maximum is no more than the share by `FPoint.op_LessThanOrEqual`
(`CheckActiveStealth`), its visibility becomes `Stealth` unless it is hidden
further, and it is triggered (`ActiveStealth`): no longer pending, and never
again in this fight. A repair or a lifesteal invokes no `OnLifeChange`.

**It is shown once its time is past the duration.** `StealthTechSystem`
updates after `DeadEffectSystem` and `FightConstructionSystem`, among the
last modules, and counts each triggered unit's time a tick at a time from
the tick it went into stealth; the update that finds it past the duration
(`FPoint.op_GreaterThan`) sets its visibility back to `Normal` (`EndStealth`),
as does every update after. A tick's 0.05 seconds fall a few raw short, so a
Vortex is shown 81 ticks after it went into stealth. Leaving the fight shows
every triggered unit (`OnExitFight`).

**In stealth it takes no damage and no search finds it.**

- `FightCalculator.PerformHitTargetEffect` sets a hit on a unit in stealth to
  nothing after its damage reduction, so no shield takes it, though the hit
  counts as taken; `FightActor.ReduceLife` takes nothing from it but a
  suicide.
- It is not visible (`FightActor.IsVisible`), so a skill's range check finds
  it out of range as it does a unit underground, and an attacker that moves
  underground, which reaches a hidden one, does not reach one in stealth
  (`SkillAttackRangeChecker.IsActorInAttackRange`). The Marksmen shooting a
  Vortex hold no lock from the tick after it goes into stealth, and so does
  each grouped slot of a Wraith firing at one: every skill of a group asks
  the same range check of its own target.
- `AttackTargetFilter.Check`, which every selector's search asks of each
  candidate, passes over a unit in stealth, where it takes one underground:
  those Marksmen then lock red's towers.
- What asks `IsValidTarget(Stealth)` still strikes it: a projectile already
  on its way lands on it (`FightProjectile.Update`), a splash takes it
  (`RangeTargetCalculator.CalculateRangeActors`), and so does a sweep
  (`DamageEffect.PerformInRange`).

## Dead-line technologies

A row of `deadLineTechDatas` is a `DeadLineTech`, the Mustang's Culling
Rounds: beside its numbers, a 35% cut in damage, it carries the life by its
unit's level, `dead_line_value` (320 at level one, 200 more each level), and
whether a unit's own shield keeps it off, `dead_line_ignores_shield` (it
does not). `DeadLineEffectProvider` hands its unit's main skill a pre-hit
effect (`FightSkill.AddPreHitEffectProvider`), and
`DamagePerformer.PerformHitTargetEffect` asks it of each target the skill's
hit strikes, the skill's own or its projectile's (`FightProjectile` hands
its pre-hit on to its skill), before anything else.

**A unit at or under the line is destroyed whole**
(`DeadLineEffectProvider.PerformPreHitEffect`): a live unit, not a building,
whose life is no more than the line at the level of the skill's owner
(`GetDeadLineValue`, by `CardLevel` counting from zero), unless its own
shield has energy left and the row does not ignore shields, or the owner's
technologies are disabled. `FightActor.ReduceLife` takes its whole life as a
suicide's, which no damage reduction lessens and neither a shield nor stealth
stops, and the hit deals it nothing more. `OnActorHitted` credits the owner
with the life as damage and charges the target the line as taken: a Crawler
culled at 250 is charged 320. The hit names no damage provider, so its
`damage` names neither a projectile nor a skill.

## Damage-share technologies

A row of `damageShareTechnologies` is a `DamageShareTech`: Damage Sharing,
the Sledgehammer's and the Steel Ball's, whose numbers give their units 120%
more life and whose groups share damage (`group_purpose` 0, `DamageShare`),
and Grid Integration, the Vortex's, whose groups raise their members' damage
(`group_purpose` 1, `DamageChange`). Its `share_distance` is what it answers
`IMechGroupSource.GetShareDistance` with, 25 metres and Grid Integration's
35, by its unit's level. `MechGrounpEffectProvider.DoActive` writes it on the unit as
its `MechDataChangeFloat.MechGroupDistance` and hands the unit to its side's
`TeamMechGroupManager` (`AddMech`); a unit travelling in is handed over as it
arrives, and a unit that dies leaves as `DeadEffectSystem` deactivates its
effects (`DoDeactive`, `RemoveMech`). A unit a Hacker turns leaves its old
side's manager for its new side's (`MechGrounpSystem.ChangeMechGroup`),
grouping there with the units of its type it reaches; one that dies turned
leaves before it goes back, and joins nothing.

**Units of one type within the distance form a group.** Two units link when
the edges of their bodies (`FightActor.Distance2D`) are within the distance
by `FPoint.op_LessThanOrEqual`; the distance is one static the last unit
taken wrote (`PrepareAvaliableMechs`). As the fight starts, each type's units
are one group (`RebuildGroup`), put in order squad by squad
(`MechGroupInternal.Refresh`: by `ActorComparer`, each squad chained onto
the last by the two ends nearest each other, `LinkMeches`). Every update,
before `RangeItemSystem` (`Update` falls through to `DoRefresh`), each unit
in turn splits its group into the parts still linked, a part of one unit
leaving and the group keeping the first part (`TrySplitGroup`), joins the
groups it reaches and takes in the units it reaches that have none
(`UpdateGroupInfo`). What a group does is its source's purpose
(`IMechGroupSource.GetGroupPurpose`).

**A hit on a member of a group that shares damage is shared.** The group
writes nothing on its members. `FightCalculator.PerformHitTargetEffect`
takes a first hit's damage through the target's rates, reduction and
stealth, and then, the target in such a group, hands it to
`CalculateGroupDamage`:
the whole part of the damage over the group's count, dead members counted,
is dealt to each member alive in the group's order as a hit of its own,
which no rate, reduction or further share touches, and which each member's
own shield takes first. The remainder, and a dead member's part, are lost.
Fifteen Sledgehammers share a Marksman's 2329 as 155 each; each member takes
its part as taken damage, and the target nothing more.

**A group that changes damage raises its members' main skills.** It shares
no hit. Each time its members change (`MechGroupInternal.Init`, `Add`,
`Remove`, `LinkGroup`, `SetGroupElement`, `Refresh`), `RefreshMechData`
writes on every member's main skill a rate on its damage
(`SkillDataChangeFloatRate.DamageRate`): the source's rate
(`group_damage_rate`, `GetFloatRateValue`, Grid Integration's 35%) times the
group's count less one, the count, dead members in it, no more than the
source's most (`group_max_count`, `GetMaxCount`, four) when that is above
zero, by `FPoint` product. A unit that leaves its group, or joins another,
takes its group's rate away first (`FightMech.SetGroup`,
`RemoveMechData`); a group left with fewer than two members takes it from
every one. Five Vortexes in one group each deal 3218, 1570 with 105% more;
two pairs each deal 2119, and a Vortex alone 1570. A row that names its
extra skills (`extra_skill_effect`) is refused.

## Reactive armor technologies

A row of `reactiveArmorTechDatas` is a `ReactiveArmorTech`: Reactive Armor,
the Typhoon's and the Centurion's. It answers `IReactiveArmorTechDataSource`
with its `reactive_armor_rate`, -0.8, and its `reactive_armor_count`, 5; its
`GetDamageReduceValue` reads a `damageReduceValues` list no row sets.
`ReactiveArmorTechEffectProvider.DoActive` lists the unit in its side's
`ReactiveArmorSystem` with its count, and leaves a unit listed already as
it is. Before the fight that is all, and the system's `OnEnterFight` writes
the rate on every unit listed as its
`MechDataChangeFloatRate.AmplifyDamageRate` (`AddEffect`), its count whole.
During the fight (`FightController.IsFighting`), `DoActive` writes it at
once on a unit not in force: a unit travelling in, whose effects
`FightEffectSystem.ActiveEffect` activates as it arrives, has the rate from
then, and a hit before that takes its whole damage and counts nothing. A
summon or a make has it as it joins, with its whole count; no line, skill or
buff of this build makes a Typhoon or a Centurion, so that is read and not
recorded.

**The rate is on the damage the unit takes.** `PerformHitTargetEffect`
multiplies a hit on a unit by one plus its buffs' and its own
`AmplifyDamageRate` increases, then by its buffs' decreases and its own: a
Marksman's 2329 takes 465 off an armored Typhoon, a Wasp's 202 takes 40.

**It lasts for as many hits as its count.** `FightController.OnActorHitted`
hands every hit to `OnReactiveArmorOwnerDamaged`, which takes one off the
count of a unit in force for a hit that took life (`damageReal` above zero)
and takes the rate away (`RemoveEffect`) once the count is none. The hit
that does has had the rate, and the next one, in the same tick or later,
takes its whole damage.

## Siege-mode technologies

A row of `siegeModeTechDatas` is a `SiegeModeTech`, Field Entrenchment, the
Sabertooth's and the Typhoon's: its `siege_mode` carries what it answers
`ISiegeModeEffectDataSource` with, the rates and values its unit's main skill
and life take while it is dug in (the Sabertooth's 50% more life, 20% off its
interval and 20 metres of range, the Typhoon's 70% more life and 20 metres),
the seconds with no enemy in range after which it leaves, 7, and the seconds
it stands after, 0.3. Its rate on its unit's move speed no fight code reads.
`SiegeModeEffectProvider.DoActive` hands the unit to `SiegeModeEffectSystem`
(`AddSiegeModeOwner`), which registers with its main skill's blows
(`RegisterMainSkillPerformAttack`); a unit it already holds is passed over.

**It digs in as the fight starts, after the presearch.** Each skill has
drawn its first interval as its unit was deployed, from the interval the
trench has not shortened, and has its first lock from the presearch, which
`FightPrepareState` runs (`PresearchTargetController.Start`) before
`FightingState.Enter` enters the modules into the fight: the presearch scores
with the range the unit was deployed with, not the trench's.
`OnEnterFight` then digs each held unit in (`AddEffect`): the rates and
values go on its main skill (`FightSkill.AddData`) and the rate on its life
(`FightMech.AddData`), the life refreshed to the new maximum as a buff's
maximum life refreshes it, and its motion changes to `MotionStopState`
(`ChangeToStopState`). The stop locks its agent where it stands, keeping its
collider priority, at full priority (`RVOControllerFixed.Lock(true, false)`):
it never moves, and its `Update` turns it to a target in range
(`AttackUpdate`), whose skill starts and fires as it would from the attack
state. Its skill letting a target go does not move it out of the stop. A
unit travelling in digs in as it arrives (`DoActive` during the fight). A
Sabertooth dug in has 21213 life, its 14142 with half more, and 115 metres
of range; a Typhoon 16199, its 9529 with 70% more, and 120.

**It leaves once no enemy has stood in range for its duration.**
`SiegeModeEffectSystem` updates after `FightConstructionSystem`, before
`StealthTechSystem`, last unit first. A unit whose time has reached the
duration (`FPoint.op_GreaterThanOrEqual`) leaves its trench; any other's
time is set back to none when the units in its main skill's range of it
(`RangeTargetCalculator.CalculateRangeTargets`: no building, the unit's own
radius, only the fully visible, the main skill's targets, and then no
`FightConstruction`) are any, and counts the tick otherwise. Each blow of
its main skill sets the time back too (`AttackingController.Update`,
`FightMech.PerformMainSkillAttack`, `ResetTimer`), so a unit firing at a
wall or a tower stays dug in. A tick's 0.05 seconds fall a few raw short, so
a unit leaves 141 updates after the last that found an enemy. Leaving
(`RemoveEffect`) takes the rates and values away, the life refreshed to its
share of the old maximum, and, the unit alive and the fight going on, idles
its motion once the delay is over (`GRTimerManager`, six ticks), the unit
standing locked until then; it does not dig in again in that fight.

**A disable, a death and the fight's end take the trench away.** A
disabling buff makes the unit leave on the system's next update, the tick it
lands (`EndSiegeMode`), and switched on gives nothing back. A unit that dies
leaves as `DeadEffectSystem` deactivates its effects (`DoDeactive`,
`RemoveSiegeModeOwner`). Leaving the fight takes every trench away
(`OnExitFight`), its life refreshed and its motion idle on the last tick. A
unit a Hacker turns stays dug in, counting the enemies of its new side.

## Fire technologies

A row of `fireIntensifyTechnologies` is a `FireIntensifyTech`: Incendiary
Bomb, the Stormcaller's, and Napalm, the Fire Badger's. Its numbers are a
plain technology's (-20 metres of range, -0.3 of life), and it answers
`IFireIntensify` with its `fire_range` and `fire_life_time` at its unit's
level (`FireIntensifyTechnologyData.GetRange`, `GetLIfeTime`): 5.5 metres
for 15 seconds, and 12 metres for 8. `FireIntensifyEffectProvider.AddEffect`
writes them onto the unit's `DataSet` as its fire's range and life time
(`MechDataModifer.AddData`), as a burning extra weapon writes its own
([extra_weapons.md](extra_weapons.md#a-fire-where-it-lands)); a unit with
both, whose numbers would sum, is refused.

**Each hit of the main skill leaves the unit's fire.** The provider is a hit
effect of the main skill (`RegisterMechEventInternal`, on
`SkillDataModifier.AvaliableCheck`, which no row's `extraSkillEffect`
passes for an extra skill). Its `PerformHitEffect` leaves
`GroundFireController.GetFireMech`, the fire of the unit's numbers, under
the unit's side through `RangeItemSystem.AddItem`, at the point the hit
landed: every shell of a Stormcaller's volley leaves one, the shells that
strike nothing too. It leaves none for a second damage's hit, and none
while the unit's technologies are off. Where the skill's lock stands on
another side than the first unit the hit struck, or it holds none, the
provider takes another point: on the shield the skill fires at
(`GetTargetEnergyShield`, `FightUtility.GetAttackPositionOnEnergyShield`
toward the lock, or toward the hit's point with no lock), and with no shield
the lock's own position (`FightTransform.position3D`) for a skill that locks
its target (`IsLockTarget`) and the hit's point for any other. Read from the
build; no recording holds it. The fire burns as any fire does
([terrain.md](terrain.md)).

## Wreckage-recovery technologies

A row of `wreckageRecoveryTechnologies` is a `WreckageRecoveryTech`,
Wreckage Recycling, the Rhino's and the Abyss's: beside its numbers, 60% and
35% more damage, its `wreckage` carries what it answers `IWreckageRecovery`
with, the seconds a strike counts, 2, and the whole metres, by the dying
unit's level, within which its unit takes life, 15 and 250.
`WreckageRecoveryEffectProvider.DoActive` hands the unit to its side's
`TeamWreckageRecoveryManager` (`Add`), which hands the skills a technology's
numbers reach a hit effect (`SkillManager.AddHitEffect`), as a buff source's
hit effect is handed; a unit travelling in is handed over as it arrives, and
one that dies is let go (`DoDeactive`).

**A hit records what it struck.** Each unit a hit of one of those skills
strikes is recorded against the holder, its count back at none if it
already is (`PerformHitEffect`); a building is not, and nothing is while the
holder's technologies are disabled. `WreckageRecoverySystem` updates after
`DeadEffectSystem` and counts every record's updates, last first, dropping
it once they reach the time over a tick, by `FPoint` quotient's whole part:
40.

**A recorded unit's death heals.** As it dies (`FightMech.OnDead` raising
`OnMechDead`), each holder in order, the fight's own by where they stand,
the second coordinate and then the first (`OnFightStart`,
`FightUtility.SkillOwnerComparer`), and then those arriving, that is alive and short
of its maximum life drops its last record of it; one that had one and stands
within the distance of it (`FightActor.Distance2D`, edge to edge,
`FPoint.op_LessThanOrEqual`) is among those healed
(`PerformTargetDeadEffect`). Each takes the whole part of the dead unit's
maximum life over their count, at least 1 (`PerformRecoveryEffect`,
`FightMech.RecoveryLife`), held to what it lacks, recorded after the death.
A Rhino that fells a Marksman takes its 1622; two Rhinos that fell a Vulcan
take half its 30279 each, held to what each lacks. A holder disabled still
heals for what it struck before.

## Rebirth technologies

A row of `rebirthEffectTechologyDatas` is a `RebirthTech`, an `IDeadEffect`
and an `IRebirthData`: Field Reassembly, the Typhoon's, and Quantum
Reassembly, the Phoenix's. Its `rebirth` carries the whole seconds its unit
waits, 5 and 12, the times a fight brings it back, 1, the unit it rises as,
its own, and whether it follows an ally while it waits, which Quantum
Reassembly does and Field Reassembly does not. `DeadEffectProvider.DoActive`
hands its unit to `DeadEffectSystem`.

**A unit that dies starts a task where it fell.** `DeadEffectSystem.Update`,
after `FightCoreSystem` and `ProjectileSystem`, hands each controller in turn
the units with its dead effect that died this tick (`PerformDeadEffect`) and
then updates it, all before any dead unit's `OnDead`. The rebirth
controller sets a unit's count to its source's the first time it dies
(`CostRebirthCount`, `GetRebirthCount`); while any is left it spends one and
starts the unit's task (`GetRebirthTask`, `RebirthTask.StartTask`). A unit
that does not follow an ally is marked rising (`FightMech.isRebirthing`).

**A rising unit holds its side.** A side whose only units are rising has not
lost: `FightCoreSystem.TryDstroyTower` and `IsStepFinish` count a unit alive
or rising, so the towers stand, the fight goes on, and an enemy that has
killed the last unit standing reads on at its cycle, as against an enemy
still in the fight.

**It stands again after its seconds.** Each update the task counts one, the
update it started on first (`RebirthTask.Update`); the tasks are taken by
index and one that ends is taken out under it, so the task after it is not
counted on that update. Once it has counted the seconds over a tick, 100 for
Field Reassembly and 240 for Quantum Reassembly, the unit stands where it
fell, or where its pilot flies, facing as the Phoenix it rises behind
(`RebirthTask.RebirthMech`): its whole life back, with no heal
(`FightMech.ForceRecoveryLife`), among its side's active units again
(`FightTeam.ActiveMech`), so that it updates after every other unit of its
side, its technologies' effects active again
(`FightEffectSystem.ActiveEffect`), and every skill's interval drawn again
and its attack time at it, so its first blow is due (`FightMech.EnterFight`).
Its avoidance agent is made again (`MotionController.EnterFight`,
`RVOControllerFixed.Active`), as a landing unit's is: the first tree built
after reads it at zero, and its first solve takes no speed unless entering a
state hands it one ([rvo.md](../spec/simulation/rvo.md#the-first-tree)). It is counted
reborn (`FightMech.AddRebirthCount`), which cuts its score
([`reactor_damage.md`](reactor_damage.md)). A unit that dies again with no
rebirth left is gone. A fight that ends while a unit rises drops its task
(`DeadRebirthController.OnFightExit`), and the unit scores nothing.

**A pilot follows the nearest of its kind.** Quantum Reassembly's unit is not
marked rising: its pilot (`RebirthSurvival`, starting where it fell) follows
the Phoenix of its side nearest the one it followed, or where it fell
(`RebirthTask.TryGetNearestTeamMech`). Each update a pilot whose Phoenix has
died turns to the nearest other, and one with none left fails, its unit gone
(`UpdateReadyRebirth`); a side whose last units are pilots has lost. The last
2.5 seconds it rises (`UpdateMoveState`) and no longer moves.

**Points behind the Phoenix.** After the tasks, each Phoenix followed moves
its pilots (`FollowPointManager.Update`), in the order each was first
followed. It has fifty points, rows of five 17 metres apart, the first row 10
metres behind it and each next 10 metres further, sorted by their distance
from it, the nearest first (`CreateNearestPoints`, `List.Sort`, which is not
stable); each stands behind the Phoenix as it faces (`FollowPoint.UpdatePoint`),
and its pilots, in the order they began following, take them in turn
(`Follow`).

**How a pilot flies** (`RebirthSurvival.DoMoveSurvival`). The update a pilot
turns to a Phoenix, its landing is drawn from its own side's stream, the
point moved by 5 metres times -1 or 0 on each axis (`GRRandom.Next(-1, 1)`,
whose upper bound it never draws): one 50 metres or more from its point
waits 0.75 seconds where it is and then stands there (`DoTransfer`), and
one nearer flies at 40 metres a second until within half a metre of its
point and its swing's offset, then follows by 0.04 of the way each update
(`FVector3.MoveTowards`, `FVector3.Lerp`). Each update it moves, its swing
turns by 0.04 radians, and each time it comes round, the first time at once,
its offset is drawn again, 5 metres times its cosine times -1 or 0 on each
axis (`MakeRandomOffest`).

## Loose-formation technologies

A row of `rVORadiusChangeTechnologyTechDatas` is an
`RVORadiusChangeTechnology`, Loose Formation, the Crawler's: beside its
numbers, 40% less life, it answers `IRVORadiusChangeSource` with a move
radius of 3.4 (`GetMoveRadius`) and a threshold of 25 metres
(`GetNearTargetThreshold`). `RVORadiusChangeProvider.DoActive`
(`MotionController.ActiveRVOChangeRadius`) hands the source to its unit's
motion and the move radius to its RVO controller, and the motion switches.

**Its agents keep a team radius from each other.** As the unit enters the
fight, `RVOControllerFixed.Active` puts its agent in the team of its unit
type (`FightMech.GetMechID`, `RVOAgentFixed.EnableTeamRadius`), its radius
being above zero, with that radius (`SetTeamRadius`). An agent finds a
neighbour of its own group and its own team, the team above zero, with the
two team radii added, whatever their sizes, in place of the inner or outer
radii (`RVOAgentFixed.GenerateNeighbourAgentVOs`). So the Crawlers of one
side that hold it keep the move radius, 3.4 each, from each other, and their
own radii from every other agent.

**Only an agent activated after the technology joins the team.**
`RVOControllerFixed.Active` reads the radius as it activates the agent. A
unit in the fight as it starts has its effects active before
`MotionController.EnterFight`, and a unit made or summoned has them before
its agent is made (`SummonSystem.DoCreateMech` runs `FightEffectSystem.
AddEffect` before `MotionController.ActiveMoveFunction`): both join. A unit
landing from a flank has its agent made first
(`SuperDeploymentController.FinishTranvel` runs `ActiveMoveFunction` before
`ExitTravel`'s `ActiveEffect`), finds the radius at zero and joins no team:
its Crawlers keep their own radii from each other, while still switching
the radius their agents would keep.

**Near its lock it closes up.** Each update, after its state machine and
before it moves the body, a motion that switches asks whether its unit's
lock (`FightMech.lockTarget`) stands within the threshold, edge to edge
(`MotionController.TryUpdateRVOChange`, `IsTargetInRVONearRange`,
`FightActor.Distance2D`, `FPoint.op_LessThanOrEqual`); with no lock it is
not. On a change it hands its agent its own inner radius near, 1.5 for a
Crawler, and the move radius far. `MotionController.EnterFight` asks once
as the unit enters the fight.

## Fire-extinguisher technologies

A row of `clearRangeItemTechDatas` is a `ClearRangeItemTech`, Fire
Extinguisher, the Hound's: it answers `IClearRangeItem` with a radius of 40
whole metres (`GetRadius`) and the kinds fire, acid and fog, in that order
(`GetRangeItemTypes`). `ClearRangeItemEffectProvider.AddEffect` puts its
unit in its side's `TeamClearRangeItemManager`, active; `DoDeactive`, as
the unit dies, makes it inactive. A unit made or summoned with it, a Hound
a Centurion makes with Summon Hounds among them, joins the manager as it
joins the fight; a unit travelling in clears nothing until it arrives.

**Every second update it clears about each active unit.** Each side's
manager counts its updates (`m_deltaTime`) and, on reaching
`m_updateInterval`, 2, counts back to none and clears
(`TeamClearRangeItemManager.Update`), after `WreckageRecoverySystem` and
before `SiegeModeEffectSystem` (`FightController.AddModules`). For each
active unit and each of its kinds, the kind's controller is asked for the
terrain in a circle about the unit (`RemoveRangeItemGrids`,
`RangeItemController.Query`): its radius the source's 40 and the whole part
of the unit's radius (`FightTransform.radius`), 44 for a Hound. Each answered
terrain whose own circle the circle reaches (`RangeItem.GetCircleRange`,
`CircleRange.Overlaps`) loses its cells under the circle's
(`RangeItemController.RemoveGrids`, `GridBlockInt.TryDisableGrid`), a
terrain of either side alike. A circle becomes a grid of its whole circle
first (`RangeItemEffectLayerGrid.ConvertToGrid`), and every active shield of
every group, a carried barrier among them, takes its cells from it as from a
new terrain's, unless it was turned from another kind, a fire burnt from oil
(`GenerateGrid`, `AdvancedEnergyShieldSystem.GetActiveEnergyShields`,
`m_isConvertFromOtherType`). A grid left with no cell goes. Which unit
clears first changes nothing that is left.

## Repair technologies

A row of `recoveryTechDatas` is a `RecoveryTech`, Maintenance Array, the
Typhoon's: it answers `IRecoveryTechEffectDataSource` with 500 life at the
first level, 500 more each level up to 4500 at the ninth (`GetLife`, the last
entry past the list), no rate of maximum life (`GetMaxLifeRate`), an interval
of 3 seconds (`GetRecoveryInterval`) and a range of 100 metres (`GetRange`),
repairing no enemy, units only and aerial units too (`CanRecoverEnemy`,
`IsOnlyRecoverMech`, `CanRecoverAir`).

**It repairs every 61 ticks about its unit.**
`RecoveryEffectProvider.DoActive` adds the unit to `RecoveryEffectSystem`
(`AddRecoveryOwner`), and `OnEnterFight` sets each clock at zero, enabled; a
unit added during the fight, arriving from its travel, rising again or made,
starts at zero, enabled. `DoDeactive`, as the unit dies, takes it out. Each
update while fighting (`RecoveryEffectSystem.Update`, one module after
`StealthTechSystem`), each clock, in its dictionary's order, adds
`FightUtility.DeltaTime`; on reaching the interval
(`FPoint.op_GreaterThanOrEqual`), which 60 updates fall short of, it goes
back to zero and repairs (`RecoveryEffectSystem.RecoveryDataInfo.Update`).
`DoRecover` takes every
unit `RangeTargetCalculator.CalculateRangeActors` finds within the range of
the unit's position, its own radius counted and no building. Of those it
keeps the unit's side's unless it repairs enemies, and aerial units only if
it repairs those. Each gains the life at the unit's level plus the rate of
its own maximum life, the whole part
(`FightActor.RecoveryLife`). The unit repairs itself. Its repairs follow the
tick's deaths. A row that repairs targets beside units is refused.

## Kill-explosion technologies

A row of `killExplosionTechDatas` is a `KillExplosionTech`, Wreckage
Detonation, the Typhoon's: it answers `IKillExplosionDataSource` with 115
damage at every level (`GetExplosionDamage`, the last entry past the list), a
range of 12 metres (`GetExplosionRange`), striking allies too (`CanHitAlly`),
raised by buffs (`CanBeAffectedByBuff`), and no explosion set off by an
explosion (`CanExplosionTriggerExplosion`). Its row is the main skill's and
no extra skill's (`mainSkillEffect`, `extraSkillEffect`).

**A hit's kills explode, up to the first unit that lives.**
`KillExplosionEffectProvider.EnableEffect` hands the skills that take the
main skill's corrections a hit effect (`SkillManager.AddHitEffect`,
`SkillDataModifier.AvaliableCheck`). After a hit of one of them
(`PerformHitEffect`), it walks the units the hit struck in the order it
struck them, passing over what is not a unit (`FightMech`). Each that is
dead, its death dealt by the skill's unit (`FightActor.deadSourceSkillOwner`,
which `ReduceLife` sets as the life runs out), explodes; the first that is
alive, or dead by another's hand, ends the walk, and those after it do not
explode however they died. A rocket that strikes two Crawlers that live and
then kills two sets nothing off. An explosion can kill a unit later in the
walk, which then explodes in its turn.

**An explosion strikes its domain about where the unit fell.** Each is
`DamagePerformer.Perform` of a `KillExplosionDamageProvider` aimed at the
dead unit. It is owned by the skill's unit and of that unit's side as it now
stands (`GetTeamController`). It deals the damage at that unit's level,
raised by its tower buffs (`GetDamage`), to everything of either side
(`GetEffectTargetType`) and of the dead unit's domain (`GetTargetType`,
`IsFly`) within the range of where it fell, each one's own radius counted
(`CalculateDamagePosition`, `GetSplashRange`). Its damage names the unit and
no skill, and its deaths follow the hit's. A Wasp's explosion leaves a
Crawler beneath it whole. Where a row chains, the units an explosion struck
are walked as the hit's are, each dead of that explosion
(`FightActor.deadSourceDamageProvider`,
`KillExplosionEffectProvider.KillExplosionDamageProvider.DispatchHitDamageEvent`); none does. A row of
extra skills, or of a range of a fraction of a space unit, is refused.

## Burrowing technologies

A row of `burrowTechnologies` is a `BurrowTech`, an `IBurrow`: Subterranean
Blitz, the Crawler's. Beside its numbers, 3 more speed, its row carries the
rate on the damage its unit takes while burrowed, -0.4, and the metres
within which an enemy brings it up, 50, each by the unit's level
(`TechnologyData.GetLevelValue`). It does not take its unit underground
(`IsEnterUnderGround` is false on every row): a row that did is refused.
`BurrowEffectProvider.AddEffect` hands the unit to its side's
`TeamBurrowManager` as it is deployed (`BurrowSystem.AddMech`), its status
`Deactive`, and `OnFightStart` sorts each side's units by where they stand
(`FightUtility.SkillOwnerComparer`), which held them in unit order in every
recording. It is `Normal` once `FightEffectSystem.ActiveEffect` activates
its effects (`DoActive`, `BurrowSystem.Active`): as the fight starts, or as
it lands from a flank, in its place. A unit made or summoned in the fight,
by a production line or as another dies, is held after the rest as it
joins, renumbered as the recording numbers it. Its death brings it up and
leaves it `Deactive` in its place (`DoDeactive`, `BurrowSystem.Deactive`)
until it rises again.

**A unit burrows while no enemy is near.** `BurrowSystem` updates after
`WreckageRecoverySystem`. Each held unit comes up when its main skill's
search measured no enemy nearest, when that enemy stands within the
distance, or when its attack target, else its lock, does
(`TeamBurrowManager.Update`, `GetDistanceToTarget`,
`GetDistanceToAttackTarget`, `FPoint.op_LessThan`); any other burrows. The
update measures only a unit whose main skill holds an attack target
(`FightSkill.attackTarget`); one with none is left as it stands: a unit
landed this tick, which no update since has searched one for, and one whose
attack an enemy's death ended. The four Crawlers whose attack a Marksman's
death ended stay up, and Crawlers that land on tick 161 burrow from 162. A
`Deactive` unit is passed over. Both
distances are `FVector3.Distance` less the two radii, not held at zero. The
nearest enemy is `SkillSearchTargetController.nearestActor`, which each of
the controller's selectors writes as it searches, `TrySelect`, `Select` and
`PerformSearch` alike (`ScoreRatingTargetSelector.Selector.Calculate`): the
first candidate strictly nearer than any before it, edge to edge, under the
skill's minimum range or not, and none when it had no candidate. It is kept
until the next search, a dead enemy's place still measured.

**Burrowing is the technology as its own buff.** `TryBurrowDown` marks the
unit `Underground` and adds `BurrowTech`, which is its own `IBuffData`, with
no source (`BuffSystem.DoAddBuff`): it never fails (`GetProbablity` is
1000), lasts `FightUtility.MaxTime`, 2^30 ticks in `Buff.Init`, and writes
its rate as `AmplifyDamageRate` (`IBuffDataFloatRate.GetData`), on every
hit the unit takes. The rate joins its unit's other buffs' in
`BuffManager.buffDatas`, which `GetAmplifyDamageAddRate` and
`GetAmplifyDamageReduceRate` read composed, as any two buffs' are: a
burrowed Crawler in an acid takes both. Read from the build; no recording
holds the two together. Its data is the technology, whose id the buff
and its events name. `TryBurrowUp` marks it `Normal` and removes the buff
(`BuffSystem.RemoveBuff`), `removed`. What the manager writes follows the
tick's deaths. A unit that dies burrowed has its buff cleared after its
death, and `DoDeactive`'s `TryBurrowUp` finds nothing to remove; nor does
`OnFightEnd`'s after the fight's clearing, which clears each side's units in
the order they joined it (`FightTeam.activeActors`), not by id.

## Acid technologies

A row of `deadAcidRangeItemTechnologyDatas` is a `DeadAcidRangeItemTech`, an
`IDeadAcidRangeItem` and an `IRangeItemProvider`: Acidic Explosion, the
Crawler's. It answers a range of 9 whole metres (`GetRangeItemRange`, its
`subEffectRange`), one round (`GetRoundDuration`), no life time
(`GetLifeTime`) and the buff its `buffID` names (`PreProcess`), 500001, the
battle skill acid's: 1.5 on the damage taken and -0.015 of the maximum
life every half second. `DeadEffectProvider` hands its unit to
`DeadEffectSystem`'s acid controller.

**A unit that dies leaves an acid where it fell.** `DeadEffectSystem` makes
its controllers in the order buff, acid, explosive, summon, rebirth
(`DeadEffectSystem.Init`), and as it updates each controller performs the
dead effects of the units that died this tick, in the order they died,
before any dead unit's `OnDead`. The acid controller adds the technology's
acid where the unit fell, for the side it stands on as it dies
(`DeadAcidRangeItemController.PerformDeadEffect`,
`RangeItemSystem.AddItem`), as a battle skill's acid is added: its
`BuffItemController` keeps the buff on the enemies standing in it. Its
creation follows the unit's death among the tick's events. A unit travelling
in is held only once it lands, and a unit whose technologies are off is not
held (below), so neither leaves one; made and summoned units are held as
they join.

## Missile Interception

Missile Interception makes its unit an interceptor: Mustang, Sabertooth,
Farseer and War Factory each research one. Its row's `intercept` holds what
an interceptor building's row holds, and the unit's interceptors take enemy
projectiles out of the air as a building's do
([contraptions.md](contraptions.md#an-interceptor)), with three differences.

- **They stand where the unit stands.** The reach is measured from the unit
  as it moves, and they intercept for the side the unit stands on.
- **A unit has `weapon_count` of them**, each locking and attacking on its
  own: the War Factory's four take two rockets on one tick.
- **A preemptive one locks its unit's main skill.** The Mustang's interceptor
  locks the main skill as it locks a projectile (`SkillLockState`), so the
  skill lets its target go and the unit stands idle from its next update. It
  lets the skill idle again as it goes idle itself, after its attack and
  reset, and the skill searches anew. The other three units' interceptors
  leave the main skill be.

The unit's interceptors are added to its side's as its effects activate when
the fight starts. A unit that travels in has them added as it arrives, and one turned to the other side has them moved to the end of that
side's. A dead unit's interceptors are taken from its side's: they update no
more and no projectile joins them, while a lock one held still counts against
what the others lock.

A buff that disables the unit's technologies disables each of its
interceptors (`InterceptMissileEffectProvider.DisableEffect`,
`InterceptEffectBase.DoDisable`): it lets its target go and returns to idle,
a preemptive one handing its unit's main skill back, and does not update
until the technologies come back, when each is enabled and returns to idle
again (`DoEnable`). Read from the build; no recording holds it.

## Production lines

A row of `supportUnitTechnologies` is a `SupportUnitTech`, which answers
`ISupportEffectDataSource` as a production item's `SupportUnitEquipment` does:
`SupportUnitEffectProvider` hands its unit's side a creator as the fight
starts, and the line runs as
[equipment_effects.md](equipment_effects.md#production-lines) states. Its row's
`production` holds what the line makes and where. Best Partner, Shooting Squad
and Summon Hounds make their units at the unit's own level
(`unit_level` 3, `DynamicMechLevel.Parent`), each appearing by a transition
(`appear_type` 5) for `APPEAR_DURATION`'s second, once: their next batch is
10^6 seconds away.

Fang Production, Crawler Production and Mothership make theirs at the
unit's first level about where it stands, appearing with an effect
(`appear_type` 1) at once, each batch every 32 to 36 seconds. Dark
Companion makes one at the Abyss's level, 40 metres ahead of it, appearing at
once with no effect (`appear_type` 0), once; the two types differ only in the
duration of the effect `SupportUnitCreator.CreateMech` hands the client.

The War Factory's three lines make theirs at its level, each taking the row's
`productTime`, a second, to appear (`SupportUnitData.GetProductMoveTime`).
Phoenix Production's (`appear_type` 6) appears as a transition does, at the
War Factory itself, a Phoenix every 17.2 seconds; a level-4 War Factory's
Phoenix is level 4 (`tests/production/phoenix-production-level-4.yaml`),
and so is its Sledgehammer
(`tests/production/sledgehammer-production-level-4.yaml`). Steel Ball Production's and
Sledgehammer Production's (`appear_type` 7) come out of it, every 9.7 and 6.6
seconds (`SummonSystem.CreateMechDelaySetPos`). Each is made where the War
Factory stands, with no agent, and draws two hundredths of a metre of its
side's stream, x then z, a draw of up to a metre each way. As it joins
(`SummonSystem.AddMechDelay`) it gets its agent
(`MotionController.ActiveMoveFunction`) and stands at its offset, the row's
whole metres with its draws added, turned by the War Factory's facing from
where the War Factory then stands, facing as it faces
(`FightMech.UpdatePositionAndRotation`).

Electromagnetic Twin makes a Vortex Mirage at the unit's first level, 25
metres behind it, by a transition, once. Its `position_space` 0
(`SupportUnitPositionSpace.None`) turns the offset by the unit's root, as 1
does: `SpecialSupportUnitData.GetRotation` turns by the body for 2 alone. The
row corrects its make (`SupportUnitCreator.CreateMech`): its
`unit_life_rate` joins the make's `MechDataChangeFloatRate.LifeRate`, and its
`unit_damage_rate` its main skill's `SkillDataChangeFloatRate.DamageRate`,
both written by the line, so no technology switch takes them away. The
Mirage's life rate of -1 leaves it 1 point of life, the least
`FightMech.CalculateMaxLife` gives any unit, and its damage rate of -0.7
leaves it 30% of a Vortex's damage.

A row whose makes appear any other way, take a level of their own, have their
attack range corrected by the row, are capped in all, come in its
`intensifyMode` or without their side's technologies is refused by name; no
row of this version does.

A buff that disables the unit's technologies disables the line
(`SupportUnitProvider.DisableEffect`, `SupportUnitSystem.Disable`, which
clears the unit's `SupportUnitCreator.isEnable` for the row): its update
still counts its life and its interval, but makes nothing, and its support
skill finds no batch due (`PreCalculate`). Switched on again
(`SupportUnitSystem.Enable`), a line whose interval ran out meanwhile makes
its batch on its next update. A unit's line from its equipment is not this
provider's and runs on. An extra weapon's line, a Tarantula's Spider Mine, is
its support skill's: disabling the skill (`FightSupportSkill.Disable`, through
`ExtraSkillProvider.DisableSkill`) clears its creator's `isEnable` as well,
and enabling it sets it again, so the line counts on and makes nothing
meanwhile, as above. Read from the build; no recording holds it.

## Summons where a unit dies

A row of `deadSummonTechnologies` is a `DeadSummonTech`, an `IDeadSummon`.
As its unit dies, `DeadSummonController.PerformDeadEffect` has
`SummonSystem.CreateMech` make, for the unit's side, the row's `unit_count`
entry for the unit's level of its `unit_id`. Each is a unit at the first level
for `unit_level` 0 and at the unit's own for 3 (`DynamicMechLevel.Parent`),
where the unit stood and facing as it faced. Each is moved
within the whole metres of the unit's radius by two draws of its side's
stream, x then z, as a buff's dead summon is, and joins at once. Unlike a
buff's, it is made whatever the unit's domain and whether or not a buff
summoned the unit, and the recording writes its `unit_created` before its
unit's `unit_died` rather than after. Mechanical Division leaves five Crawlers
where a Steel Ball dies:
`tests/dead_summon/mechanical-division.yaml`. The Sandworm's leaves
four Larvas at its own level:
`tests/dead_summon/sandworm-mechanical-division.yaml`.

## Flying and landing technologies

A row of `flyTechDatas` is a `FlyTech`, an `IFlyTechDataSource`: Aerial
Mode, the Void Eye's, which takes a ground unit into the air, and Land
Cruiser, the Wraith's, which brings an air unit to the ground. Beside its
numbers (the Void Eye's 3 more speed and 15 metres less range, the Wraith's
50 metres more range and 0.6 second more interval), its row carries only
`landingDuration`, 1 second on both. `FlyTechData.PreProcess` works out the
rest from the unit whose card holds the technology: the unit takes the
other domain than its type's (`GetUnitStateType`), and `canAttackAir` is set
for a ground type and cleared for a flying one, whatever the row says.

**Its unit stands in the other domain.** `FlyTechEffectProvider.DoActive`
sets the unit's `FightMech.isFly` to the opposite of its type's
(`DoEnableOutOfFight`, `FightMech.SetTechFly`) as its effects are activated:
as the fight starts, or as it lands, joins or rises again; a unit still
travelling keeps its type's domain until it lands. `SetTechFly` moves the
unit's agent to the layer of its new domain at its height
(`RVOControllerFixed.RefreshMainLayer`), and the unit stands at the height of
its new domain from its next placing (`FightMech.SetPosition`). Everything
that asks whether a unit flies asks `FightMech.IsFly`: its own search and
the searches that find it, its attack range and damage against it, what a
splash, a projectile or a terrain reaches of it, its agent, and the domain
the recording holds of it each tick. As it dies, `DoDeactive` gives it its
type's domain back.

**Its skills turn onto or off aircraft.** `SkillDataModifier.AddData` asks
`FlyTechData.GetIsInverseAirAttack`, whether `canAttackAir` differs from
whether the unit's main skill attacks aircraft, and where it does turns
`SkillDataChangeInt.AirAttackValue` of each skill the row reaches by that
skill's own row, -1 for one that attacks aircraft and +1 for one that does
not. The Void Eye in the air attacks aircraft as well as the ground; the
Wraith on the ground and the extra skills its row reaches attack the ground
alone. The turn is among its numbers, not switched off with its
technologies.

**Switched off, it takes its type's domain back a second later.**
`DisableEffect`, on a unit in the technology's domain, starts a
`GRTimerManager` timer of the row's `landingDuration`, its last one stopped
(`DoDisableInFight`, `TryStopByObject`), whose callback lands or flies the
unit to its type's domain (`LandingCallBack`, `FlyUpCallBack`). `EnableEffect`,
on a unit in its type's domain, gives it the technology's at once and stops
the timer (`DoEnableInFight`); switched on before the second is out, the unit
never left the technology's domain.

A unit whose side's officers name units by whether they fly is refused:
`FightEffectMananger.RefreshOtherEffect` adds or removes such an officer's
effect as the unit's domain turns, which is not read. Read from the build;
no recording holds either technology.

## Ignoring a buff effect

A row of `ignoreBuffEffectTechnologyDatas` is an `IgnoreBuffEffectTech`, an
`IIgnoreBuffDataSouce`: Power Armor, the Rhino's, whose numbers give it 25%
more life. Its row names one kind of buff effect, a `BuffEffectType`, which
its unit then ignores (`useIgnoredBuffEffectType`, `buffEffectType`):
`SpeedChangeRate`, the rate a buff writes on its speed. It ignores no buff
group, keeps no control beam off and lasts as long as the fight
(`duration` none).

**No buff slows its unit, and none speeds it.** As the unit's effects are
activated, `IgnoreBuffEffectSystem.ApplyIgnoreBuff` raises its
`BuffManager.stateDatas` of that kind (`AddIgnoredBuffEffectData`); a unit
activated before the fight begins is held and raised as it begins
(`Active`, `OnEnterFight`). While it is above zero, a buff writes nothing at
that index of `BuffDataFloatRate`, `MoveSpeedChangeRate`, as it enters
(`Buff.AddEffect`), as it is added again (`Buff.Reset`) or as its stack moves
(`Buff.RefreshEffect`): the buff runs and is recorded, its other numbers
written, and the unit keeps its speed. The rate a buff wrote stays until the
buff ends, and one it left unwritten is written when the buff is added again
once the unit no longer ignores it. A buff that stacks a speed rate on such a
unit is refused, which is not measured.

**Switched off, it ignores nothing.** `Disable` lowers the count again, and
`Enable` raises it (`RemoveIgnoreBuff`, `ApplyIgnoreBuff`). A buff that
disables technology switches the unit's technologies off before it writes
its own numbers (`BuffManager.AddBuff` raises the `DisableTechnology` count
and adds its `CBEC_DisableTechnology` before `Buff.AddEffect`), so an
Electromagnetic Shot slows a Rhino with Power Armor from its first hit.

## Searching for the most life

A row of `searchTargetModifyTechnologies` is a `SearchTargetModifyTech`, an
`ISkillSearchTargetProviderDataSource`: Fortified Target Lock, the Steel
Ball's, whose row names the `SkillSearchTargetType` its unit's main skill
searches by, `CurrentLifeHighestFirst`.

**Its unit's main skill locks the enemy of the most life in its reach.**
`SkillSearchTargetProvider`, a `SingleEffectProvider`, turns the skill's
selector to a `LifePriorityTargetSelector` as the unit's effects are
activated (`DoActive`, `FightSkill.ChangeSearchTargetType`,
`SearchTargetController.Change`). Of the enemies its filters pass, it keeps
those whose edge stands beyond the skill's minimum range and within its
range (`FightTransform.Distance2D`, the distance less both radii), and of
them the ones of the most life, every one tied at it
(`SelectBySearchTargetType`), and hands them to the plain selector it holds,
which scores them as any search does; when none stands in reach, it hands it
every enemy it passed. Its selector is no `ScoreRatingTargetSelector`, so its
search is never prepared at the tick's start
(`MainSkillSearchTargetController.PrepareSearch`): it searches with `Select`
where everything stands whenever it searches.

Switched off, the selector turns back to `Normal`, and on again to the
technology's (`DoDisable`, `DoEnable`). A row of another type, or one that
reaches the extra skills, is refused.

## Dead-explosion technologies

A row of `deadExplosiveTechnologyDatas` is a `DeadExplosiveTech`, an
`IDeadExplosive` and an `IDeadEffectTech`: Final Blitz, the Rhino's. Its
`DeadEffectProvider` hands its unit's death to `DeadExplosiveController`,
the controller an explosion skill's death goes to, with the technology's
numbers in the skill's place.

**Its unit's death strikes everything about it with its maximum life.** As
`DeadEffectSystem` updates, `DeadExplosiveController.PerformDeadEffect`
strikes, from where the unit fell, every unit and building within the row's
`range` (48 metres) beyond the unit's radius
(`DeadExplosiveDamageProvider.GetSplashRange`) that the unit's own skill
attacks (`GetTargetType`, `FightMech.GetAttackTargetType`), with what its
`explosiveDamageCondition` names times its `damageMultiplier`: Final Blitz's
condition 1 is the unit's maximum life as it died (`lifeGauge`'s maximum),
times 1. `enableFriendlyFire` makes it strike its own side too. It leaves no
terrain (`HasDeadRangeItem`). A unit its blast kills that explodes in turn
explodes on the same update.

**Switched off, its unit's death strikes nothing**, as a technology's dead
effect does not go off while its unit's technologies are disabled
(`IDeadEffect.IsTechnologyEffect`); an invincible unit's goes off.

## Additional-damage technologies

A row of `additionalDamageTechDatas` is an `AdditionalDamageTech`, an
`IAdditionalDamage`: Ionization, the Raiden's, which takes 0.7 off its
damage and makes each hit take half its target's life besides
(`additionalDamageByTargetLife`). Its provider, a `SingleEffectProvider`, is
a hit effect of the main skill and, its row setting `extraSkillEffect`, of
every extra skill.

**Each hit takes a share of what its target has left.** After a first hit,
not a second damage's, `AdditionalDamageProvider.PerformHitEffect` hands each
target the hit struck that still lives a hit of the rate times its life
left, the `FPoint` product's whole part, through `FightActor.OnHitted`, from
no object and under its unit's side. `OnHitted` neither raises it by the
target's rate on damage taken nor takes its reduction off: a unit's own
shield takes it first and the rest comes off its life. It raises no hit
event, so the recording holds no damage for it and the statistics do not
count it. Among a skill's hit effects it runs where its provider stands,
after a buff technology's of a lower id, which is every buff technology a
Raiden holds. A source that `CanDisable` takes nothing while its unit's
technologies are disabled (`DoDisableEffect` takes the hit effect off the
skills).

## Chain technologies

A row of `iterationHitDamageTechDatas` is an `IterationHitTech`, an
`IIterationHit`: Chain, the Raiden's, which takes 20 metres off its range
and makes each hit jump on, once, 0.2 seconds later, to an enemy within 60
metres of where it landed, preferring those within 25, for a quarter of the
skill's damage. Its provider, a `SingleEffectProvider`, is a hit effect of
the main skill, each of the Raiden's three weapons its own skill.

**A hit records what it struck and starts a jump.** After a first hit, not
a second damage's, `IterationHitEffectProvider.PerformHitEffect` records
every unit it struck with its skill in the unit's
`FightMechIgnoreTargetManager` (`AddIgnoreBySkillHit`), and unless a chain of
that skill is under way (`IterationEffectSystem.IsIteration`) starts one
from where the hit landed (`HitEffectControl.Init`, `Perform`): a
`GRTimer` of the delay, which strikes nothing yet. A source that
`CanDisable` does nothing while its unit's technologies are disabled.

**The jump strikes an enemy drawn at random.** As the timer fires
(`OnTimerOver`, `GetTarget`), the enemy units fully visible whose edge
stands within the range of the point, in the order each side's objects are
searched, are taken; of those within the preferred range that no record of
the unit passes over one is drawn from the side's stream
(`RandomElementSync`), or failing any, of those within the range. The one
drawn takes the skill's damage now times the rate to the jump's number,
the `FPoint` product's whole part, as the unit's hit under its skill
(`DamagePerformer.Perform`, `HitEffectControl.GetDamage`), which the
skill's hit effects take as a first hit; the chain under way, the jump's own
hit starts none. The chain jumps again from the one struck until it has
jumped its count; with none drawn, or its count done, it ends
(`OnIterationEnd`): the skill stops attacking in the records, and once no
skill of the unit is, every record goes (`ClearTargetRecord`). A jump at a
unit a battlefield shield covers that does not cover the Raiden is refused,
which is not measured.

## A second damage that writes a buff

Electromagnetic Cloud, the Vortex's, is a `SecondaryDamageIntensifyTech`
whose second damage deals nothing within 10 metres of where the hit landed
and names a buff, `hitEMPBuffID` 1031, an electromagnetic buff that disables
technology. Its `SecondaryDamageIntensifyEffectProvider.DoActive`, as the
row names a buff and `canDisableTech` is set, hands the main skill a
`BuffCycleController` of the technology (`TeamBuffCycleManager.
CreateBuffController`, `AddHitEffectProvider`): a buff source on a hit
(`GetBuffTechListener`), on the opponent's units, certain, with no delay,
interval or reach.

**Its buff lands on what the hit struck and on what the second damage
reaches.** `BuffCycleController.PerformHitEffect` writes on a first hit, and
on a second damage's hit only for a source that is itself an
`ISecondaryDamageIntensifyEffectDataSource`, which this one is. Each time,
`BuffSystem.AddBuff` adds the buff to every unit in the hit's list, in its
order, under the Vortex's side. The second damage of nothing still lists
every enemy unit in its range, less what the hit struck, and hands them to
the skill's hit effects (`ApplySecondaryDamageToActors`,
`DispatchSecondaryDamageEvent`). A source that `CanDisable` writes nothing
while the Vortex's technologies are off, and no second damage is dealt then.

## Cloak technologies

A row of `moveAbilityDynamicTechDatas` is a `MoveAbilityDynamicTech`, an
`IMoveAbilityDynamicSource`: Stealth Cloak, the Phantom Ray's, whose
numbers give it a fifth more life and damage, and whose row gives the
seconds before it cloaks (`delay`, 2.5) and that it stays seen once it shows
itself (`delayExit`, 1), counted in updates (50 and 20).

**Its unit cloaks while no enemy is within its reach.** Its
`MoveAbilityDynamicProvider` hands the unit a `CloakController` of its
side's `TeamCloakManager` (`CloakSystem.Active`). The controller of a unit
deployed on the field exists as the fight starts, and
`TeamCloakManager.OnEnterFight` leaves it counting with its count full
(`ResetData`). `CloakSystem` is the first module to update, each side's units
in the order `OnFightStart` sorts them. A unit is ready while the nearest
unit its main skill's search measured stands beyond its range, edge to edge
(`IsNothingAround`; `IsMainSearcherSkillIdle` reads `FightSkillBase.IsIdle`,
which nothing sets). Counting, a ready unit counts an update and, once the
count reaches 50, cloaks (`ActorVisibility.Disappear`); one not ready counts
from none. Cloaked, a unit not ready starts to show; showing, it counts 20
updates whatever happens and is then seen again, and starts counting anew
once ready. Each blow of any of its skills starts a cloaked unit showing and
sends a counting one back to none (`FightMech.OnMechSkillPerformAttack`).
A cloaked unit is searched and locked still, and no skill reaches it but one
moving underground.

**Switched off, it shows and holds.** Its technologies disabled start a
cloaked unit showing and a counting one from none, and the controller does
nothing more until they come back, in `None`; its death shows it at once
(`Disable`, `Enable`, `Deactive`).

## Shield-spawning technologies

A row of `spawnAdvancedShieldTechDatas` is a `SpawnAdvancedShieldTech`, an
`ISpawnAdvancedShieldDataSource`: Accumulator Shield, the Vortex's. Its
provider, a `SingleEffectProvider`, makes the unit a
`SpawnAdvancedShieldController`, a hit effect of the main skill alone
(`SkillManager.AddHitEffect`).

**Its unit's hits spawn shields where it stands.** Each first hit of the
main skill, not a second damage's, counts once, whatever it struck, while
the unit has spawned fewer than `maxTriggerTimes` (100) and its source is
not switched off. Once the count reaches `attackCount` (5) plus
`attackCountIncrement` (5) for each shield already spawned, it starts over
and `AdvancedEnergyShieldSystem.Create` stands a shield where the unit
stands: of its side, with no owner, active and full, of `shieldRadius` (40
metres) and `shieldValue` energy (2000 a level). Shields come at the 5th,
15th, 30th hit and so on, and each stands until a hit empties it; it is
short-lived (`IsShortLifeTime`), so the round's end destroys it and the
recording says so of it from the start. The counts outlast the unit's
technologies switched off (`Disable` takes the hit effect away and keeps
them), and the shields stand whatever befalls the unit.

## Melee-mode technologies

A row of `meleeModeTechData` is a `MeleeModeTech`, an
`IMeleeModeEffectDataSource`: Melee Mode, the Centurion's. Its
`MeleeModeEffectSystem.AddOwner` adds the unit its melee skill, 32002, as a
permanent preemptive skill locked from the start, and makes its main skill
spend rounds (`IsLoadingTypeValue`, `LoadingCapacityValue`): `ammoCount`, 20,
which no reload refills, and `extraAmmoCount` more beside an extra skill the
melee skill names incompatible, Dual Wield's side arm, which then shares
them: `AmmoSkillPool.CollectPoolSkills` pools the main skill and every extra
skill of a loading type (`isLoadingType`), the side arm's, whose magazine is
never reloaded. Its source answers
`CanDisable` false (`ignoreElectricEffect`): nothing switches it off.

**The last round brings the melee skill.** Each projectile of a pooled skill
takes a round (`ReduceLoadingRemainCount`). Once none is left, the melee
skill's condition holds (`AmmoEmptyController`), checked as a pooled skill's
blow ends (`SkillAttackController.ChangeToIdle`,
`SkillManager.TryTriggerAmmoEmptyPreemptiveCheck`), before its weapon turns,
and after the unit's skills have updated: the main skill locks for good, the
body turns to where the unit points (`IFightMechBody.UpdateRotation`) and the
melee skill disables it (`MeleeModeTech.DisableBody`,
`MechDataChangeInt.DisableBody`), so that `FightMech.IsHaveBody` answers false
and the unit turns its root to what it attacks (`MotionAttackState.AttackRotate`)
and its skills measure their angles from the root
(`FightSkill.GetMainTransform`), the incompatible extra skills lock, the unit takes its whole life
back whatever holds its recovery off (`OnMeleeSkillActive`,
`FightMech.ForceRecoveryLife`, which shows no life bar), and the melee skill waits out its
transition, the condition's 1.5 seconds, locked, the motion stopped
(`StartActiveTransition`). Then it idles and takes the motion
(`FinishActiveTransition`), and the melee mode writes its numbers
(`OnMeleeModeTransitionComplete`): 3 on the unit's life, which keeps its
share, 18 on its speed, and 3 on its main skill's damage, which reaches the
melee skill, whose rate is 1. As the fight ends they leave it again
(`RemoveMeleeModeDataModifier`).

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
runs: an Electromagnetic Impact's, Electromagnetic Barrage's, Electromagnetic
Shot's. It is a common buff effect over a count of such buffs
(`CBEC_DisableTechnology`), so the first to reach a unit switches them off as
it is written and the last to leave switches them on again, as its time runs
out, as its unit's buffs go at its first update after it died, or as the fight
is left and its buffs are cleared. A shot the unit left in the air as it died
lands with the technologies on again. A summon's technologies are those its
side's give a unit of its type, as a deployed one's; a dead summon's go with
it, and nothing comes back. `FightMech.IsTechnologyDisabled` answers true the
while, whether the unit carries a technology or not.

Switching off is each of the unit's effect providers' `DisableEffect` on each
of its sources that `CanDisable` (`FightEffectMananger.DisableEffect`), and
only a technology does: `Technology.CanDisable` answers `ignoreElectricEffect`
false, which every row but a few buff, Dual Wield and melee-mode rows does;
an officer, an equipment, an Energy Tower skill and a battle skill answer
false. So the officers' and the items' corrections stay, and so does a
technology that sets `ignoreElectricEffect`, its numbers, its providers and
its extra skills: `ExtraSkillProvider.DisableSkill` disables only the skills
of the source it is handed (`FightSkill.IsBelongExtraSkill`). A Centurion
with Dual Wield and Homing Missile struck by Electromagnetic Barrage keeps
its second gun firing and its range 25 metres short, and loses the missiles.

Every technology is an `IDataModifier`, whose numbers its provider takes
away and writes again, and a class that answers an interface beside it is
that interface's provider's source too: a `LifestealTech` the
`LifeStealEffectProvider`'s, a `StealthTech` the
`StealthTechEffectProvider`'s. A plain, mobility, damage intensify, splash or
multi-attack technology answers none, so switching it off takes its numbers
alone, a splash's `splash_range` and a multi-attack's projectile values among
them. The simulator switches every provider of the unit in one place, as
`FightEffectMananger.DisableEffect` does, and refuses by name a provider
whose own `DisableEffect` it does not mirror.

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
  Marksman's drawn 69 becomes its plain 62. Each technology's provider
  takes its data off the main skill and writes it back
  (`SkillDataModifier.RemoveData`, `AddData`), which ends by refreshing the
  skill's data (`FightSkill.RefreshDatas`) whether it wrote numbers or none,
  every unit technology of this build reaching the main skill
  (`mainSkillEffect`): a Fortress whose only technology is Barrier has its
  drawn 35 made its plain 36. A unit with no technology keeps the interval it
  drew.
- **A lifesteal's and a second damage's** providers take their hit effect
  away, which the fight asks of the unit at each hit.
- **A Barrier's shield leaves its side's shields and comes back as it was.**
  `AdvancedEnergyShieldProvider.DisableEffect` acts only on a shield that is
  available (`EnergyShieldBehaviour.IsAvaliable`: active, enabled and holding
  energy): its controller records its energy and is disabled
  (`AdvancedEnergyShieldController.Disable`), and the shield is deactivated
  (`AdvancedEnergyShieldSystem.DeactiveEnergyShield`), holding nothing and
  shielding nothing. `EnableEffect` acts only on a disabled controller: the
  shield gets its recorded energy back (`Enable`) and, its owner alive, is
  activated without being refilled (`ActiveEnergyShield` with no reset),
  joining its side's active shields after every other. A Fortress's shield
  at 60614 goes on tick 164 and is back at 60614 on tick 301.
- **A reactive armor's rate leaves its unit, its count kept.**
  `ReactiveArmorSystem.DisableReactiveArmor` takes the rate of a unit in
  force away, and `EnableReactiveArmor` writes it again on one with a count
  left. A hit while it is off takes its whole damage and counts nothing: a
  Typhoon's armor off takes a Void Eye's 1144 whole, and still has five hits
  of 40 once it is on.
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
  Each skill of a grouped row is a skill of the unit's extra skills
  (`ISkillOwner.GetExtralSkills`) and is disabled on its own, and so is each
  of a row's skills that joined the main skill's group, which
  `FightSkillFactory.PrepareGroupedSkill` only adds to that group: an idle
  one starts no attack, one attacking fails its check, and the main row's
  skills go on. A control beam's skill has no `Disable` of its own: leaving
  its attack stops its `ControllEffect`, which takes the skill off its
  target's entry, as any end of its attack does. Read from the build; no
  recording holds it.
- **A permanent preemptive explosion neither activates nor explodes.** Its
  condition holds nothing while the unit's technologies are disabled, its
  skill being a technology's
  (`PermanentPreemptiveActiveConditionLifeController.CheckCanActive`), and a
  unit that dies so, not invincible, sets off no dead effect that is a
  technology's (`DeadEffectController.IsAvaliable`), as its explosion is.
  Fire Badgers with Scorching Charge lose their 80% more life with the
  plain numbers, fall to 14% of what is left without charging, and die
  neither exploding nor burning; switched on again, they charge below half
  and explode as before. An active one is invincible, so no debuff reaches
  it.
- **A dead-line technology culls nothing.** `DeadLineEffectProvider` takes
  its pre-hit effect off its unit's skills, and `PerformPreHitEffect` does
  nothing while the technologies are off. A level-three Mustang squad under
  an Electromagnetic Impact shoots Marksmen down through its line of 720
  with whole shots of 108, its 35% cut gone with the numbers; a Mustang
  killed meanwhile has them back, and its shot still in the air culls.
- **A stealth technology's unit counts as triggered.**
  `StealthTechSystem.DisableStealthTech` adds it to the triggered units and
  shows it (`EndStealth`): a Vortex in stealth is shown as the
  Electromagnetic Impact lands, and one disabled before it was hurt does not
  go into stealth as its life falls. Switched on, `EnableStealthTech` takes a
  unit still pending out of the triggered ones again, and it goes into
  stealth at once if its life is low enough; this is read and not recorded.
- **A grouping technology's unit leaves its groups.**
  `MechGrounpEffectProvider.DisableEffect` takes it out of its side's
  manager (`TeamMechGroupManager.RemoveMech`): it leaves its group, taking
  away the rate a group that changes damage wrote, is no longer a candidate,
  and no longer hears its side change; its `MechGroupDistance` stays.
  Switched on, `EnableEffect` hands it back (`AddMech`), and it links with
  what it reaches. Two of five Vortexes with Grid Integration under an
  Electromagnetic Impact deal 1570 while the three left keep 2668, 70% more;
  the Impact's 25 seconds over, the two link again as a pair at 2119.
- **A siege-mode technology's unit leaves its trench.**
  `SiegeModeEffectProvider.DisableEffect` sets its time to its duration
  (`SiegeModeEffectSystem.EndSiegeMode`), so the system's next update, on
  the tick the buff lands, takes it out; switched on, nothing is given back
  (`EnableEffect` is `SingleEffectProvider`'s alone). A Sabertooth under an
  Electromagnetic Impact goes from 21213 to 14142 at whole life on t57, and
  idles 0.3 seconds later.
- **A wreckage-recovery technology records no one.**
  `WreckageRecoveryEffectProvider.DisableEffect` takes its hit effect off its
  unit's skills (`SkillManager.RemoveHitEffect`), and `PerformHitEffect`
  records nothing while the technologies are off; what it struck before
  still heals it as it dies, and switched on, `EnableEffect` hands the effect
  back. A Rhino that fells Marksmen under an Electromagnetic Impact heals
  nothing for them.
- **A loose-formation technology's agents close up.**
  `RVORadiusChangeProvider.DisableEffect`
  (`MotionController.DisableRVOChangeRadius`) hands the agent its own inner
  radius as its team radius and stops the motion switching; switched on,
  `EnableEffect` (`EnableRVOChangeRadius`) lets it switch from its next
  update. Crawlers under an Electromagnetic Impact keep 1.5 from each other
  for the rest of the fight.
- **A fire-extinguisher technology's unit clears nothing.**
  `ClearRangeItemEffectProvider.DisableEffect`
  (`TeamClearRangeItemManager.DisableMech`) makes it inactive; switched on,
  `EnableEffect` (`EnableMech`) makes it active for the manager's next
  clearing. Hounds under an Electromagnetic Impact leave every fire whole.
- **A repair technology's clock stands at zero.**
  `RecoveryEffectProvider.DisableEffect` (`DisableRecovery`) disables it, and
  each update sets it back to zero; switched on, `EnableEffect`
  (`EnableRecovery`) counts it from there. Typhoons under an Electromagnetic
  Impact repair nothing.
- **A dead effect is taken away.** `DeadEffectProvider.DisableEffect`
  takes the unit's technology dead effects off their `DeadEffectSystem`
  controller and `EnableEffect` hands them back, so a unit that dies with
  its technologies off leaves no acid, summons nothing and does not rise
  (`DeadEffectController.IsAvaliable`). Crawlers under an Electromagnetic
  Impact die leaving no acid.
- **A kill-explosion technology sets nothing off.**
  `KillExplosionEffectProvider.DisableEffect` takes its hit effect off its
  unit's skills (`SkillManager.RemoveHitEffect`), and `PerformHitEffect`
  returns while the technologies are off; switched on, `EnableEffect` hands
  the effect back. Typhoons under an Electromagnetic Impact kill Crawlers
  that leave the one beside them whole.
- **A burrowing technology's unit comes up and stays up.**
  `BurrowEffectProvider.DisableEffect` brings it up (`TryBurrowUp`, its buff
  `removed` before the disabling buff is applied) and leaves it `Deactive`,
  which the manager's update passes over (`BurrowSystem.Deactive`), until
  `EnableEffect` makes it `Normal` again (`BurrowSystem.Active`). Crawlers
  under an Electromagnetic Impact come up on tick 57 and none burrows again.
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
  second writes three stacks. A cleared buff that does not stack
  (`IsAdditiveEffect` false) is removed (`BuffManager.RemoveBuff`), as a buff
  that runs out is: what it wrote taken away, the life refreshed
  (`Buff.Exit`), and a `buff_removed` recorded; read from the build, no
  recording holds one.

  Every buff controller of the unit stops too, whichever source it is of,
  once one of its technologies that switches is a buff source
  (`BuffEffectProvider.DisableEffect` runs `DoDisableCycle` over every source
  the provider holds): its `isAvailable` cleared and its listener taken off
  (`BuffCycleController.RemoveListener`), so `Update` passes over it and
  being hit or losing life adds nothing. Switched on, `DoEnableCycle` puts the
  listener back, sets `isAvailable` and `timeSum` to zero: a cycle under
  `All` counts its delay or interval again from nothing, and a range cycle's
  `RangeUnitCycle` keeps where it stood. A hit's buff is the skill's hit
  effect and not a listener: `BuffCycleController.PerformHitEffect` adds
  nothing while the technologies are off for a source that `CanDisable`.
  Read from the build; no recording holds a cycling buff switched off.

An air attack technology's switch stays while its unit's technologies are
off: `AirAttackEffectProvider.DisableEffect` and `EnableEffect` are
`NormalEffectProvider`'s, which do nothing, and its provider turned the skills
as it activated (`SwitchMechAirAttackEnabled`), not among the numbers its
technologies write. Read from the build; no recording holds it.

A surfacing line stops too: `MoveAbilitySummonProvider.DisableEffect` stops
hearing the unit's ability change and takes the line's action off its move
ability (`MoveAbilitySummonSystem`), so a surfacing that begins while the
technologies are off makes nothing, and `EnableEffect` puts both back. Read
from the build; no recording holds it.

Every provider's `DisableEffect` is mirrored. What an extra weapon's other
explosion or preemptive skill does switched off is not measured: a buff that
disables technology reaching a unit whose technologies hold one is refused by
the technology's id.

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

A projectile's life is the one a skill's projectiles leave with, which an
interceptor's hits take off: Heavy Missile's `projectile_life_rate` lands in
the skill's `ProjectileLifeRate`, and the life is the row's times one plus its
enhancements and then times their remainder, cut to a whole number and at
least 1. Heavy Missile's +2 triples a Stormcaller rocket's 42000.

The simulator refuses a side holding a technology whose correction it does not
derive (a minimum range):
`crates/simulation/src/modifier/technologies.rs` names each refusal.

## Evidence

### Recorded

- A technology's range and an officer's range land in one `attack_range_value`,
  and the fight uses their sum: `tests/modifier/`.
- A growing technology writes the entry for its unit's level: Elite Marksman
  gives a level-3 Marksman 155 metres of range and a level-1 one 145, and
  reading the first entry on both parts from the game at t1:
  `tests/modifier/technology-elite-marksman.yaml`.
- A production technology runs its line as a production item does, its makes
  at its unit's level, and a line of one position scatters its make:
  `tests/production/best-partner.yaml`,
  `tests/production/shooting-squad.yaml`,
  `tests/production/summon-hounds.yaml`; one of no positions makes
  about its unit, its makes joining at once:
  `tests/production/fang-production.yaml`,
  `tests/production/crawler-production.yaml`,
  `tests/production/mothership.yaml`.
- One inversion turns a locking skill free, and a minimum range value keeps
  its unit from firing within it: Siege Mode's Scorpion fires two shells that
  lock nothing at a charging Rhino, and none once it is within 75 metres;
  fought with the lock kept, the simulator parts from the game on the first
  shell's aim: `tests/modifier/technology-siege-mode.yaml`.
- A damage-share technology links its units into groups that share each
  hit as the whole part of it over their count, in the group's order:
  fifteen Sledgehammers take a Marksman's 2329 as 155 each, ten a beam's
  tick as its tenth, eight Steel Balls a Rhino's 3560 as 445, and two
  squads 140 metres apart share apart:
  `tests/damage_share/sledgehammers.yaml`,
  `tests/damage_share/beams.yaml`,
  `tests/damage_share/steel-balls.yaml`,
  `tests/damage_share/apart.yaml`. A tank a Hacker turns leaves its
  group and links with the next one turned on its new side:
  `tests/damage_share/hackers.yaml`.
- Grid Integration's group of five Vortexes raises each one's damage by
  105%, the count held to four; as they fall and part, two pairs take 35%
  and a Vortex alone nothing:
  `tests/damage_share/vortexes.yaml`. Two of them an Electromagnetic
  Impact disables leave the group and link again as it runs out:
  `tests/damage_share/vortexes-disabled.yaml`.
- Field Entrenchment digs its unit in from the first tick, a Sabertooth at
  21213 life and 115 metres, two Typhoons at 16199 and 120, and takes the
  trench away 141 updates after the last enemy left its range, the motion
  stopped 0.3 seconds more: `tests/siege_mode/sabertooth.yaml`,
  `tests/siege_mode/typhoon.yaml`. The Sabertooth's blows at a wall
  keep it dug in: `tests/siege_mode/wall.yaml`. An Electromagnetic
  Impact takes the trench away as it lands:
  `tests/siege_mode/disabled.yaml`. The fight's end takes it away on
  its last tick: `tests/siege_mode/ends.yaml`. A unit turned by a
  Hacker stays dug in on its new side, one travelling in digs in as it
  arrives, and one dies in its trench:
  `tests/siege_mode/turned.yaml`,
  `tests/siege_mode/travel.yaml`,
  `tests/siege_mode/dies.yaml`. A Typhoon presearches with its 100
  metres and locks a Phantom Ray out of either reach over a Void Eye that
  its trench's 120 would reach: `tests/corpus/67263060-r5.yaml`.
- Wreckage Recycling heals its unit by the maximum life of each enemy it
  struck as that enemy dies, held to what it lacks, and nothing at its whole
  life: `tests/wreckage/rhino.yaml`,
  `tests/wreckage/abyss.yaml`, `tests/wreckage/shared.yaml`.
  Two Rhinos that fell a Vulcan both heal:
  `tests/wreckage/split.yaml`. Disabled, it heals for no one it
  strikes: `tests/wreckage/disabled.yaml`.
- Field Reassembly brings each Typhoon back where it fell 100 updates after
  it dies, its whole life and its attack time at its interval, once: the
  second waits a tick longer as the first rises before it,
  `tests/rebirth/field-reassembly-anti-air.yaml`. While both wait
  blue stands and the Fortress reads on at its cycle, and two reborn
  Typhoons standing at the end score 31 each,
  `tests/rebirth/field-reassembly-stands.yaml`. A fight that ends
  while one waits drops it, and it scores nothing,
  `tests/rebirth/field-reassembly-rising-at-the-end.yaml`.
- A Phoenix that rises again is a new avoidance agent: the solve after it
  rises reads it at zero in the tree, so Stormcallers across the field lose
  their neighbours for that solve, and it takes no speed on its first:
  `tests/corpus/201475625-r9.yaml`, ticks 848, 852 and 928.
- Quantum Reassembly's pilot flies at speed to the nearest point behind its
  partner, turns to the far pair as the partner falls, waits 15 ticks and
  lands on a point drawn from blue's stream, follows by its lerp and its
  swing's offset, and rises 240 ticks after it fell facing as the Phoenix it
  follows, its interval drawn again; pilots with no Phoenix left fail:
  `tests/rebirth/quantum-reassembly.yaml`. Seven pilots behind one
  Phoenix take its second row, and a Phoenix that rises updates after its
  side's others, in the order they rose:
  `tests/rebirth/quantum-reassembly-rows.yaml`.
- Loose Formation keeps its Crawlers 3.4 apart each as they walk, and 1.5
  once their lock is within 25: `tests/loose_formation/crawler-rhino.yaml`.
  Disabled, they keep 1.5: `tests/loose_formation/crawler-impact.yaml`.
  Landing from a flank, they join no team:
  `tests/loose_formation/crawler-travelling.yaml`. Made by a
  production line or summoned as a Steel Ball dies, they join one:
  `tests/loose_formation/melting_point-production.yaml`,
  `tests/loose_formation/steel_ball-death-summon.yaml`.
- Fire Extinguisher clears the fires within 44 metres of each Hound every
  second tick, a fire turning into a grid as it is first cleared:
  `tests/fire_extinguisher/fire.yaml`. It clears acid and fog alike,
  and an acid left with no cell goes:
  `tests/fire_extinguisher/acid-smoke.yaml`. Disabled, it clears
  nothing: `tests/fire_extinguisher/fire-impact.yaml`. The Hounds a
  Centurion makes clear as deployed ones do:
  `tests/fire_extinguisher/made.yaml`. Travelling in, they clear from
  their arrival: `tests/fire_extinguisher/travelling.yaml`. A fire a
  barrier reaches after it lands loses the barrier's cells as it is first
  cleared: `tests/fire_extinguisher/shield-cut.yaml`.
- Maintenance Array repairs every 61 ticks from the fight's start, its
  Typhoons handing each hurt unit about them their level's life:
  `tests/maintenance_array/typhoons.yaml`. Aerial units are repaired
  too: `tests/maintenance_array/air.yaml`. A Typhoon reborn counts
  from its rise: `tests/maintenance_array/rebirth.yaml`, and one
  travelling in from its arrival:
  `tests/maintenance_array/travelling.yaml`. Disabled, they repair
  nothing: `tests/maintenance_array/impact.yaml`.
- Wreckage Detonation explodes each unit a Typhoon's rocket kills, up to
  the first it struck that lives, striking both sides about it:
  `tests/wreckage_detonation/crawlers.yaml`. A Wasp's explosion
  strikes aerial units alone: `tests/wreckage_detonation/air.yaml`.
  Disabled, it sets nothing off:
  `tests/wreckage_detonation/impact.yaml`.
- Power Armor keeps a Rhino at its speed under Degeneration Beam's debuff,
  which slows the Wasp beside it, through the whole fight:
  `tests/ignore_buff/power-armor-degeneration-beam.yaml`, the Rhino
  slowed in `tests/technology_buff/degeneration-beam.yaml`. An
  Electromagnetic Shot slows it from its first hit, which switches Power
  Armor off: `tests/ignore_buff/power-armor-electromagnetic-shot.yaml`.
- Fortified Target Lock has two Steel Balls lock a Rhino in their reach
  over the Crawlers standing nearer, once the Crawlers they locked die:
  `tests/search/fortified-target-lock.yaml`.
- Final Blitz strikes the Rhino's own Crawlers and every enemy within 48
  metres of its edge with its 19297 life as it dies:
  `tests/dead_explosion/final-blitz.yaml`. A Rhino that dies with its
  technologies disabled strikes nothing:
  `tests/dead_explosion/final-blitz-disabled.yaml`.
- Ionization takes half the life left of a Fortress a Raiden hits, and
  half of a Fang's through its emptied shield, with no damage recorded:
  `tests/additional_damage/ionization.yaml`.
- Chain jumps each of a Raiden's weapons' hits on to another Mustang 0.2
  seconds later, killing it: `tests/chain/raiden-mustangs.yaml`.
- Electromagnetic Cloud writes buff 1031 on the Rhino a Vortex strikes and
  the two Crawlers within 10 metres of it, switching Power Armor off:
  `tests/secondary_damage/electromagnetic-cloud.yaml`.
- Stealth Cloak cloaks three Phantom Rays from the first tick and shows each
  20 updates after an enemy comes within its reach:
  `tests/stealth/stealth-cloak.yaml`,
  `tests/stealth/stealth-cloak-electromagnetic-shot.yaml`.
- Accumulator Shield spawns a 40-metre shield of 2000 where a Vortex
  stands on its fifth hit, which the Crawlers empty:
  `tests/shield/accumulator-shield.yaml`.
- Melee Mode takes a Centurion's melee skill up after its twentieth round,
  stopped for 1.5 seconds, then at 28 speed and four times its life, which
  the fight's end takes away: `tests/extra_weapon/melee-mode.yaml`.
- With Dual Wield the main gun and the side arm spend forty rounds between
  them, the side arm's last bringing the melee skill as its blow ends; the
  melee skill then strikes, the Centurion turning its root to each Crawler
  and choosing the next by the root's facing, its body held:
  `tests/extra_weapon/melee-mode-dual-wield.yaml`.
- Subterranean Blitz burrows its Crawlers from the first tick and brings
  each up as its enemy comes within 50: `tests/burrow/crawler-rhino.yaml`.
  Burrowed, they take a Marksman's shot less 0.4; one that dies burrowed,
  and one burrowed as the fight ends, has its buff cleared and nothing
  removed; after the Marksman's death, those whose search found a tower
  burrow and those left with no target stay up:
  `tests/burrow/crawler-marksman.yaml`. Disabled, they come up and
  stay up: `tests/burrow/crawler-impact.yaml`. Landing from a flank,
  they burrow from the tick after: `tests/burrow/crawler-travelling.yaml`.
  Made by a production line or summoned as a Steel Ball dies, they are held
  as they join, and the fight's end clears their buffs in that order:
  `tests/burrow/melting_point-production.yaml`,
  `tests/burrow/steel_ball-death-summon.yaml`.
- Acidic Explosion leaves an acid where each Crawler dies:
  `tests/dead_acid/crawler-rhino.yaml`; none from a Crawler whose
  technologies an Electromagnetic Impact switched off:
  `tests/dead_acid/crawler-impact.yaml`; none from one killed while
  it travels in, and one from a Crawler killed after it lands:
  `tests/dead_acid/crawler-travelling.yaml`; one from each made or
  summoned Crawler: `tests/dead_acid/melting_point-production.yaml`,
  `tests/dead_acid/steel_ball-death-summon.yaml`.
- A dead-line technology destroys a unit its unit's shots strike at or
  under the line at its level, before the shot's damage: Culling Rounds
  culls Crawlers at 250 under a level-one Mustang's 320, and Marksmen at 712
  under a level-three one's 720:
  `tests/dead_line/culling-rounds-crawlers.yaml`,
  `tests/dead_line/culling-rounds-level-three.yaml`. Disabled, it
  culls nothing and its cut leaves with its numbers, and a shot its unit
  left in the air as it died culls again:
  `tests/dead_line/culling-rounds-disabled.yaml`.
- A stealth technology puts its unit in stealth as a hit leaves it at no
  more than half its life, and shows it 81 ticks on: no search finds it, a
  shot already on its way lands on it and takes nothing, and the units that
  shot it lock the towers:
  `tests/stealth/emergency-armor.yaml`. Disabled before it is hurt,
  it does not go into stealth:
  `tests/stealth/disabled-before.yaml`; disabled in stealth, it is
  shown at once: `tests/stealth/disabled-during.yaml`.
- A Wraith's grouped slot firing at a Vortex that goes into stealth ends its
  attack the tick after: `tests/corpus/67257112-r11.yaml`, tick 433.
- A multi-attack technology adds to its unit's bursts: Doubleshot fires two
  projectiles an attack, a Sabertooth's 0.2 seconds apart where its row's
  interval is zero and both from weapon 0, and Burst Mode twelve from a
  Farseer and ten from a Phantom Ray, 0.1 seconds apart, and Saturation
  Bombardment four from each of a Mountain's weapons, 0.65 seconds apart:
  `tests/multi_attack/`.
- A projectile life rate multiplies the life a skill's projectiles leave with:
  Heavy Missile's rockets leave with 126000, and fought without the rate the
  simulator parts from the game at t20:
  `tests/interceptor/stormcallers-heavy-missile.yaml`.
- An interval value lands as the table's `FPoint`, and the Rhino's blows follow
  the interval it composes: `tests/modifier/technology-interval-value.yaml`.

- A buff technology's buff stacks every second and raises its unit's damage
  and maximum life: `tests/technology_buff/combat-evolvement.yaml`, and
  on hit Rhinos `tests/corpus/134260717-r2.yaml` and
  `tests/corpus/134260717-r3.yaml`.
- A source of the update model `Each` adds its buff to the units in reach on
  every tick from the ninth, its side's ground units or the enemies of either
  domain, and stops with its unit's death:
  `tests/technology_buff/mobile-power-station.yaml` and
  `tests/technology_buff/degeneration-beam.yaml`.
- A hit adds its buff to the unit struck, ranged or melee, and a Fortress's
  range reads 70 of 100 under it: `tests/technology_buff/suppression-shots.yaml`,
  `tests/technology_buff/suppression-shots-melee.yaml`.
- Ignite burns a Rhino 3% of its maximum life every half second from the
  Vulcan's first hit, and Field Maintenance repairs nothing while it burns;
  without Ignite the Rhino repairs from tick 106 and wins:
  `tests/technology_buff/ignite.yaml`.
- An uncertain Ignite draws from the struck Rhino's side's stream on each
  hit: 67 of a Wasp's 131 hits add it, 17 of a Fang squad's 97 and 164 of a
  Fire Badger's 236; drawn from the attacker's side, the simulator parts from
  each recording at its first draw:
  `tests/technology_buff/ignite-wasp.yaml`,
  `tests/technology_buff/ignite-fang.yaml`,
  `tests/technology_buff/ignite-fire-badger.yaml`.
- Photon Coating holds its buff from the first tick, and a Vulcan's Ignite
  never reaches the Rhino or War Factory it covers:
  `tests/technology_buff/photon-coating.yaml`,
  `tests/technology_buff/photon-coating-war-factory.yaml`.
- Photon Emission adds its buff at tick 12 to the Overlord's side's other
  units within 100 m, the Wasps and a Rhino but neither a Marksman 155 m off
  nor the Overlord, and to the Farseer's only on the ground; a Vulcan's
  Ignite reaches none of them. Photon Loop adds its buff to the Mountain on
  the first tick and at tick 600, 100 ticks after it ran out:
  `tests/technology_buff/photon-emission.yaml`,
  `tests/technology_buff/photon-emission-farseer.yaml`,
  `tests/technology_buff/photon-loop.yaml`.
- Each of a Scorpion's hits with Acid Attack adds its buff to the Rhino it
  struck and leaves an acid at t131 and t219, which keeps the buff on a Rhino
  standing in it every 19 ticks: `tests/technology_buff/acid-attack.yaml`.
- Electromagnetic Armor's buff is on a Rhino from the tick its blow lands
  on a Void Eye, at t244 and again at t264:
  `tests/technology_buff/electromagnetic-armor.yaml`.
- Electromagnetic Armor's buff on a Fortress whose only technology is
  Barrier deactivates its shield at 60614 and gives it back at 60614, and
  makes its drawn interval of 35 its plain 36; without the refresh the
  simulator parts from the recording at t164:
  `tests/shield/barrier-technology-void_eye.yaml`.
- Chamber Compression's stack goes back to none as each of a Hound's shots
  lands; without the reset the simulator parts from the recording at t189:
  `tests/technology_buff/chamber-compression.yaml`.
- Scanning Radar keeps its range on the Farseer and a Marksman 55 m off, and
  never on a Rhino beside it, whose attack is melee:
  `tests/technology_buff/scanning-radar.yaml`.
- Counter-Fire's buff is on a Fire Badger from the tick a Marksman's hit
  takes life from it, its range 145 of 75; without it the simulator parts
  from the recording on that tick, on that range:
  `tests/technology_buff/counter-fire.yaml`.
- A hit's buff that disables technology switches the struck unit's off, a
  squad's buff written by its first unit, and reaches the unit the hit
  killed: `tests/technology_disable/shot-armor.yaml`,
  `tests/technology_disable/shot-phoenix.yaml`,
  `tests/technology_disable/shot-kills.yaml`.
- A unit killed under Replicate leaves Crawlers by its radius, 7 for a
  Marksman and 12 for a Rhino, which join at once and move from the second
  move before a solve: `tests/technology_buff/replicate.yaml`,
  `tests/technology_buff/replicate-swarm.yaml`.
- Three buffs run on one Rhino, and one ending leaves the others' rates:
  `tests/technology_buff/three-buffs.yaml`.
- A buff stacking on distance adds a metre of range for every 7 metres its
  Steel Ball has rolled, two in a step where it rolled that far, and holds
  while it stands: `tests/technology_buff/kinetic-charge.yaml`,
  `tests/technology_buff/kinetic-charge-stops.yaml`, and to 80 metres
  `tests/corpus/268477093-r4.yaml`.
- A disabling buff takes a plain technology's numbers off its unit as it is
  written, life and current interval with them, and they come back as it runs
  out or as the fight is left:
  `tests/technology_disable/barrage-assault-mode.yaml`,
  `tests/technology_disable/impact-to-fight-end.yaml`,
  `tests/technology_disable/impact-expires.yaml`. An armour's reduction
  leaves with it, a Wasp's 142 on an armoured Rhino becoming 202:
  `tests/technology_disable/impact-armor.yaml`. A unit with no
  technology keeps its drawn interval:
  `tests/technology_disable/impact-on-both-sides.yaml`. A unit that
  dies disabled has its technologies back as its buffs go, and its shots
  still in the air land with them:
  `tests/technology_disable/shot-dies-with-shots-in-flight.yaml`.
- A disabled extra skill lets the burst under way run out, starts no attack
  while disabled, and fires again as it is enabled:
  `tests/technology_disable/barrage-homing-missile.yaml`,
  `tests/technology_disable/barrage-homing-missile-held.yaml`.
- A search technology's ranges and offsets leave with it:
  `tests/technology_disable/impact-ground-targeting.yaml`.
- A stacking buff its unit added writes no stack while the technologies are
  off, takes its life rate out at its next step with a second share of the
  life, and stacks on from the stack it kept:
  `tests/technology_disable/impact-combat-evolvement.yaml`,
  `tests/technology_disable/impact-combat-evolvement-expires.yaml`.
- High-Explosive Ammo's range lands in each skill's `splash_range` beside its
  damage rate, and its shots strike the Crawlers around their target:
  `tests/splash/high-explosive-ammo-wasp.yaml`,
  `tests/splash/high-explosive-ammo-mustang.yaml`,
  `tests/splash/high-explosive-ammo-overlord.yaml`,
  `tests/splash/high-explosive-ammo-stormcaller.yaml`,
  `tests/splash/high-explosive-ammo-war_factory.yaml`,
  `tests/splash/high-explosive-ammo-wraith.yaml`,
  `tests/splash/high-explosive-ammo-tarantula.yaml`,
  `tests/splash/high-explosive-ammo-phantom_ray.yaml`.
- High-Speed Engine's 5 lands in its unit's move speed, a Wasp's and a
  Phoenix's 16 reading 21 and an Overlord's 10 reading 15:
  `tests/modifier/technology-jump-drive-wasp.yaml`,
  `tests/modifier/technology-jump-drive-overlord.yaml`,
  `tests/modifier/technology-jump-drive-phoenix.yaml`.
- A unit's interceptors take rockets out of the air from where it stands, a
  War Factory's four each on its own, and a Mustang's lock its main skill,
  which stands its unit idle on the next update:
  `tests/interceptor/mustang-interception.yaml`,
  `tests/interceptor/sabertooth-interception.yaml`,
  `tests/interceptor/farseer-interception.yaml`,
  `tests/interceptor/war_factory-interception.yaml`.
- A permanent preemptive explosion does not activate, and its unit's death
  neither explodes nor burns, while the technologies are off; switched on,
  both come back:
  `tests/technology_disable/impact-scorching-charge.yaml`,
  `tests/technology_disable/impact-scorching-charge-expires.yaml`.
- Reactive Armor takes the first five hits that take life from its unit at
  a fifth of their damage, the fifth included, and the next one, in the same
  tick, whole; switched off, it keeps its count:
  `tests/reactive_armor/typhoon-wasps.yaml`,
  `tests/reactive_armor/centurion-wasps.yaml`,
  `tests/reactive_armor/typhoon-void_eye.yaml`.
- A Typhoon travelling in takes its whole damage and counts nothing until it
  arrives on tick 161, and has the rate for five hits from then:
  `tests/reactive_armor/typhoon-travelling.yaml`.
- A fire technology's unit leaves its fire where each hit of its main skill
  lands, a Stormcaller's every shell, those that strike nothing too:
  `tests/fire_intensify/stormcaller-rhino.yaml`,
  `tests/fire_intensify/fire_badger-marksman.yaml`.

### Read

- Melee-mode technologies: `MeleeModeEffectSystem.AddOwner`, `AddMeleeSkill`,
  `UpdateMainSkillAmmoCapacity`, `HasIncompatibleSkill`, `OnMeleeSkillActive`,
  `OnMeleeModeTransitionComplete`, `RemoveMeleeModeDataModifier`,
  `AmmoSkillPool`, `FightSkill.IsAmmoConsumer`, `ReduceLoadingRemainCount`,
  `AmmoEmptyController.CheckCanActive`, `SkillManager.ActivePermanentPreemptiveSkill`,
  `PreemptiveSkillController.StartActiveTransition`, `FinishActiveTransition`.
- Shield-spawning technologies: `SpawnAdvancedShieldController.PerformHitEffect`,
  `Enable`, `Disable`, `GetAdvancedEnergyShieldValue`,
  `SpawnAdvancedShieldEffectProvider`, `AdvancedEnergyShieldSystem.Create`,
  `GroupAdvancedEnergyShieldManager.Create`, `OnFightEnd`,
  `FightEnergyShield.IsShortLifeTime`, `IsResetNextRound`.
- Cloak technologies: `MoveAbilityDynamicTech.GetDelayEnter`, `GetDelayExit`,
  `MoveAbilityDynamicProvider.DoActive`, `DoDeactive`, `DisableEffect`,
  `EnableEffect`, `CloakSystem`, `TeamCloakManager.OnEnterFight`,
  `OnFightStart`, `Update`, `CloakController.Update`, `ResetData`,
  `OnISkillOwnerAttack`, `IsMainSearcherSkillIdle`, `IsNothingAround`,
  `PerformChangeVisibility`, `FightController.AddModules`.
- A second damage that writes a buff: `SecondaryDamageIntensifyTech.GetBuffData`,
  `GetBuffTechListener`, `GetEffectTargetTypes`, `GetProbablity`,
  `SecondaryDamageIntensifyTechData.PreProcess`,
  `SecondaryDamageIntensifyEffectProvider.DoActive`,
  `BuffCycleController.PerformHitEffect`,
  `TriggerBuffOrBuffRangeItemFromHit`, `BuffSystem.AddBuff`,
  `DamagePerformer.ApplySecondaryDamageToActors`.
- Chain technologies: `IterationHitEffectProvider.PerformHitEffect`,
  `HitEffectControl.Init`, `Perform`, `OnTimerOver`, `GetTarget`,
  `OnIterationSuccessOnce`, `OnIterationEnd`, `GetDamage`,
  `IterationEffectSystem.IsIteration`, `Add`, `Remove`,
  `FightMechIgnoreTargetManager.AddIgnoreBySkillHit`, `IsContains`,
  `ClearIgnoreByHitEffectEnd`, `ClearTargetRecord`,
  `SkillDamageProvider.CalculateDamagePosition`.
- Additional-damage technologies: `AdditionalDamageTech.GetReduceLifeRate`,
  `AdditionalDamageProvider.PerformHitEffect`, `DoEnableEffect`,
  `DoDisableEffect`, `FightActor.OnHitted`, `FightMech.OnHitted`,
  `HitDamageInfo`, `FightEffectMananger.Sort`.
- Dead-explosion technologies: `DeadExplosiveTechnologyData.GetRange`,
  `DeadExplosiveTech.GetDamageMultiplier`, `HasDeadRangeItem`,
  `DeadExplosiveController.PerformDeadEffect`,
  `DeadExplosiveDamageProvider.GetDamage`, `GetSplashRange`,
  `GetTargetType`, `ExplosiveDamageCondition`, `FightActor.lifeGauge`.
- Searching for the most life: `SearchTargetModifyTechnologyData`,
  `SearchTargetModifyTech.GetSearchTargetType`, `SkillSearchTargetProvider.DoActive`,
  `DoDeactive`, `DoEnable`, `DoDisable`, `FightSkill.ChangeSearchTargetType`,
  `SearchTargetController.Change`, `LifePriorityTargetSelector.Select`,
  `SelectBySearchTargetType`, `FightTransform.Distance2D`,
  `MainSkillSearchTargetController.PrepareSearch`,
  `SkillSearchTargetController.PerformNormalSkillSearch`.
- Ignoring a buff effect: `IgnoreBuffEffectTechnologyData`,
  `IgnoreBuffEffectTech.IsIgnoreBuffEffect`, `GetIgnoredBuffEffectType`,
  `IgnoreBuffEffectProvider.Active`, `Deactive`, `DisableEffect`,
  `EnableEffect`, `IgnoreBuffEffectSystem.Active`, `OnEnterFight`,
  `ApplyIgnoreBuff`, `RemoveIgnoreBuff`, `Disable`, `Enable`,
  `BuffManager.AddIgnoredBuffEffectData`, `BuffManager.AddBuff`,
  `Buff.AddEffect`, `Buff.Reset`, `Buff.RefreshEffect`, `BuffEffectType`,
  `BuffDataFloatRate`.
- Fire technologies: `FireIntensifyTechnologyData.GetRange`, `GetLIfeTime`,
  `FireIntensifyEffectProvider.AddEffect`, `RemoveEffect`,
  `RegisterMechEventInternal`, its `IHitEffectPerformer.PerformHitEffect`,
  `GroundFireController.GetFireMech`, `RangeItemSystem.AddItem`,
  `MechDataModifer.AddData`.
- Reactive armor: `ReactiveArmorTech.GetDamageReduceRate`,
  `GetDamageReduceCount`, `GetDamageReduceValue`,
  `ReactiveArmorTechEffectProvider.DoActive`, `DisableEffect`, `EnableEffect`,
  `ReactiveArmorSystem.AddReactiveArmorOwner`, `OnEnterFight`, `AddEffect`,
  `RemoveEffect`, `OnReactiveArmorOwnerDamaged`, `DisableReactiveArmor`,
  `FightController.IsFighting` (which `DoActive` asks before writing the
  rate at once),
  `EnableReactiveArmor`, `ReactiveArmorSystem.Init` (listening to
  `FightController.OnActorHittedEvent`), `FightController.OnActorHitted`,
  `FightCalculator.PerformHitTargetEffect`, `FightMech.GetDataFloatAddRate`,
  `FightMech.GetDataFloatReduceRate`.
- A technology answers the same correction interface an officer does, so its
  fields land in the same channels: `TechnologyData.lifeChangeRate`,
  `TechnologyData.damageChangeRate`, `TechnologyData.attackRangeChangeValue`.
- An effect is a list indexed by rank, and a growing list is its first entry
  times the rank; `scripts/extract/extract-technology-effects.py` refuses a table where
  that does not hold, and wrote this version's: `TechnologyData.damageChangeRate`.
- A getter reads entry `ISkillOwner.GetLevel()` of its list, the last past it,
  and the first when it is given no unit: `TechnologyData.GetDamageChangeRate`.
- A unit's level changes only between rounds, through `MechTeam.ChangeLevel`,
  which `UnitSystem.ChangeLevel` calls: `UnitSystem.ChangeLevel`.
- A production technology is the source a production item is: `SupportUnitTech`
  and `SupportUnitEquipment` both answer `ISupportEffectDataSource`. A make's
  appearance takes no time for `appearType` 0 and 1, the row's `productTime`
  for 6 and 8 and its `ProductMoveTime` for 7, and `APPEAR_DURATION`
  otherwise; its level is the owner's for `DynamicMechLevel.Parent`:
  `SupportUnitCreator.CreateMech`, `SupportUnitCreator.APPEAR_DURATION`.
- A technology that sets `isInverseIsLockTarget` writes -1 into a skill's
  `SkillDataChangeInt.IsLockTarget` where the unit's main skill's row locks
  and 1 otherwise, and a skill locks while its row's flag plus that is one:
  `SkillDataModifier.AddData`, `FightSkill.IsLockTarget`. Its minimum range
  value is whole metres, which the skill's `MinAttackRangeValue` adds to the
  row's: `TechnologyData.GetMinAttackRangeChangeValue`,
  `FightSkill.GetMinAttackRange`.
- A multi-attack technology adds its count, its duration and its random range
  to the skill's `SkillDataChangeInt.ProjectileCountValue` and
  `SkillDataChangeFloat.ProjectileDurationValue` and `ProjectileRandomRange`,
  which the skill's properties add to its row's: `MultiAttackTech.AddData`,
  `ProjectileCountProperty.Refresh`, `ProjectileDurationProperty.Refresh`,
  `ProjectileRandomRangeProperty.Refresh`.
- A burst's projectiles are `PROJECTILE_INTERVAL` apart when its duration is
  zero or less, and the performer makes a position controller only for a
  random range above zero, which takes turns between the weapons only for a
  skill of two: `ProjectileMultiAttackPerformer.OnStartFirstPerform`,
  `ProjectileMultiAttackPerformer.MultiAttackTargetPositionController.GetAndDeletePositionOffsets`.
- A damage-share technology's units of one type link within its distance
  into groups each update, and a first hit on a member is shared among the
  group's members alive as hits of their own:
  `MechGrounpEffectProvider.DoActive`, `TeamMechGroupManager.UpdateGroupInfo`,
  `TeamMechGroupManager.TrySplitGroup`, `TeamMechGroupManager.RebuildGroup`,
  `MechGroupInternal.Refresh`, `TeamMechGroupManager.LinkMeches`,
  `MechGrounpSystem.ChangeMechGroup`,
  `FightCalculator.CalculateGroupDamage`,
  `FightCalculator.PerformHitTargetEffect`.
- A group whose purpose is `DamageChange` writes on each member's main
  skill its rate times its count less one, the count held to its most, and
  shares no hit: `MechGroupInternal.RefreshMechData`,
  `MechGroupInternal.RemoveMechData`, `FightMech.SetGroup`,
  `FightCalculator.PerformHitTargetEffect`.
- A grouping technology switched off takes its unit out of its side's
  manager, and switched on hands it back:
  `MechGrounpEffectProvider.DisableEffect`,
  `MechGrounpEffectProvider.EnableEffect`, `TeamMechGroupManager.RemoveMech`,
  `TeamMechGroupManager.AddMech`.
- A siege-mode technology digs its unit in as the fight starts and as it
  arrives, counts the time no enemy stands in its main skill's range, sets
  it back at each blow, and takes the trench away once the time reaches the
  duration, the motion idled after the delay; a disable sets the time to
  the duration, and a death and the fight's end take the trench away:
  `SiegeModeEffectSystem.AddSiegeModeOwner`,
  `SiegeModeEffectSystem.OnEnterFight`, `SiegeModeEffectSystem.AddEffect`,
  `SiegeModeEffectSystem.Update`, `SiegeModeEffectSystem.ResetTimer`,
  `SiegeModeEffectSystem.RemoveEffect`, `SiegeModeEffectSystem.EndSiegeMode`,
  `SiegeModeEffectSystem.RemoveSiegeModeOwner`,
  `SiegeModeEffectSystem.OnExitFight`, `SiegeModeEffectProvider.DoActive`,
  `SiegeModeEffectProvider.DisableEffect`,
  `SkillAttackController.AttackingController.Update`,
  `MotionStopState.Enter`, `MotionStopState.Update`,
  `RVOControllerFixed.Lock`.
- A wreckage-recovery technology records each unit its skills' hits strike,
  drops a record after its time, and heals the holders alive, hurt and
  within its distance by the dead unit's maximum life over their count as a
  recorded unit dies; disabled, its hit effect is off:
  `TeamWreckageRecoveryManager.Add`,
  `TeamWreckageRecoveryManager.Update`,
  `TeamWreckageRecoveryManager.PerformTargetDeadEffect`,
  `TeamWreckageRecoveryManager.PerformRecoveryEffect`,
  `TeamWreckageRecoveryManager.RegisterTargetDeadEvent`,
  `TeamWreckageRecoveryManager.OnFightStart`,
  `WreckageRecoveryEffectProvider.DoActive`,
  `WreckageRecoveryEffectProvider.DisableEffect`,
  `WreckageRecoveryEffectProvider.EnableEffect`,
  `WreckageRecoveryTechnologyData.GetDistance`, `SkillManager.AddHitEffect`,
  `FightMech.OnDead`.
- A rebirth technology starts a task as its unit dies, before its `OnDead`,
  while its count lasts, marks a unit that does not follow an ally rising,
  counts the task each update and brings the unit back where it fell after
  its whole seconds over a tick, and a rising unit holds its side:
  `DeadEffectSystem.Update`, `DeadEffectSystem.Init`,
  `DeadRebirthController.PerformDeadEffect`,
  `DeadRebirthController.CostRebirthCount`, `DeadRebirthController.Update`,
  `DeadRebirthController.OnFightExit`, `RebirthTask.StartTask`,
  `RebirthTask.Update`, `RebirthTask.RebirthMech`, `RebirthTask.rebirthCostTime`,
  `RebirthTech.GetRebirthCostTime`, `RebirthTech.GetRebirthCount`,
  `RebirthTech.IsFollowOthers`, `FightMech.isRebirthing`,
  `FightMech.ForceRecoveryLife`, `FightMech.AddRebirthCount`,
  `FightMech.EnterFight`, `FightTeam.ActiveMech`,
  `FightEffectSystem.ActiveEffect`, `FightCoreSystem.TryDstroyTower`,
  `FightCoreSystem.IsStepFinish`, `MotionController.EnterFight`,
  `RVOControllerFixed.Active`.
- A rebirth that follows an ally sends a pilot after the nearest unit of its
  type, to points behind it in rows, flying at speed or after a transfer and
  then by a lerp with a swinging offset drawn from its side's stream:
  `RebirthTask.TryGetNearestTeamMech`, `RebirthTask.UpdateReadyRebirth`,
  `RebirthTask.UpdateMoveState`, `RebirthTask.SetFollowTarget`,
  `FollowPointManager.Update`, `FollowPointManager.CreateNearestPoints`,
  `FollowPointManager.CreateFollowPoint`, `FollowPointManager.Follow`,
  `FollowPoint.UpdatePoint`, `RebirthSurvival.DoMoveSurvival`,
  `RebirthSurvival.MakeRandomOffest`, `RebirthSurvival.DoTransfer`,
  `RebirthSurvival.mRandom`, `GRRandom.NextInternal`, `FVector3.MoveTowards`,
  `FVector3.Lerp`.
- A loose-formation technology puts its unit's agent in its unit type's
  team with its move radius, which two agents of one group and team add in
  place of their own radii, and hands the agent its own inner radius while
  its lock is within the threshold; disabled, the inner radius, and no
  switching: `RVORadiusChangeProvider.DoActive`,
  `RVORadiusChangeProvider.DisableEffect`,
  `RVORadiusChangeProvider.EnableEffect`,
  `MotionController.ActiveRVOChangeRadius`,
  `MotionController.DisableRVOChangeRadius`,
  `MotionController.EnableRVOChangeRadius`,
  `MotionController.TryUpdateRVOChange`,
  `MotionController.IsTargetInRVONearRange`, `MotionController.Update`,
  `MotionController.EnterFight`, `RVOControllerFixed.Active`,
  `RVOAgentFixed.EnableTeamRadius`, `RVOAgentFixed.SetTeamRadius`,
  `RVOAgentFixed.GenerateNeighbourAgentVOs`, `FightMech.GetMechID`,
  `FightMech.GetLockTarget`, `SummonSystem.DoCreateMech`,
  `MotionController.ActiveMoveFunction`,
  `SuperDeploymentController.FinishTranvel`.
- A fire-extinguisher technology's unit clears, every second update of its
  side's manager, the cells of each of its kinds' terrain within its radius
  and the source's, turning a circle into a grid first and taking away a grid
  left with none; disabled, it clears nothing:
  `ClearRangeItemTech.GetRadius`, `ClearRangeItemTechData.GetRangeItemTypes`,
  `ClearRangeItemEffectProvider.AddEffect`,
  `ClearRangeItemEffectProvider.DoDeactive`,
  `ClearRangeItemEffectProvider.DisableEffect`,
  `ClearRangeItemEffectProvider.EnableEffect`,
  `TeamClearRangeItemManager.Update`,
  `TeamClearRangeItemManager.RemoveRangeItemGrids`,
  `FightController.AddModules`, `RangeItemController.Query`,
  `RangeItemController.RemoveGrids`, `RangeItemController.Remove`,
  `RangeItem.GetCircleRange`, `RangeItemEffectLayerGrid.ConvertToGrid`,
  `RangeItemEffectLayerGrid.GenerateGrid`,
  `AdvancedEnergyShieldSystem.GetActiveEnergyShields`,
  `GridBlockInt.TryDisableGrid`.
- A repair technology's unit repairs, each time its clock reaches the
  interval, every unit of its side within its range, at its level's life;
  added during the fight or switched on again, it counts from zero:
  `RecoveryTech.GetLife`, `RecoveryEffectProvider.DoActive`,
  `RecoveryEffectProvider.DoDeactive`,
  `RecoveryEffectProvider.DisableEffect`,
  `RecoveryEffectProvider.EnableEffect`,
  `RecoveryEffectSystem.AddRecoveryOwner`,
  `RecoveryEffectSystem.RemoveRecoveryOwner`,
  `RecoveryEffectSystem.EnableRecovery`,
  `RecoveryEffectSystem.DisableRecovery`,
  `RecoveryEffectSystem.OnEnterFight`, `RecoveryEffectSystem.Update`,
  `RecoveryEffectSystem.DoRecover`, `RecoveryEffectSystem.RecoveryDataInfo.Update`,
  `RangeTargetCalculator.CalculateRangeActors`.
- A kill-explosion technology's hit walks the units it struck up to the
  first alive or killed by another, and each before explodes over its domain
  about where it fell, at the level's damage raised by tower buffs; disabled,
  it sets nothing off: `KillExplosionTech.GetExplosionDamage`,
  `KillExplosionEffectProvider.DoEnableEffect`,
  `KillExplosionEffectProvider.DoDisableEffect`,
  `KillExplosionEffectProvider.PerformHitEffect`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.GetDamage`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.GetTeamController`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.GetEffectTargetType`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.GetTargetType`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.GetSplashRange`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.CalculateDamagePosition`,
  `KillExplosionEffectProvider.KillExplosionDamageProvider.DispatchHitDamageEvent`,
  `SkillManager.AddHitEffect`, `FightActor.ReduceLife`,
  `FightActor.IsAlive`.
- A burrowing technology burrows its unit while the nearest enemy its main
  skill's search measured, and its attack target or lock, stand beyond its
  distance, writing itself as the unit's buff, and brings it up otherwise:
  `BurrowEffectProvider.DoActive`, `BurrowEffectProvider.DisableEffect`,
  `BurrowEffectProvider.DoDeactive`, `BurrowEffectProvider.AddEffect`,
  `BurrowEffectProvider.EnableEffect`, `BurrowSystem.AddMech`,
  `BurrowSystem.Active`, `BurrowSystem.Deactive`,
  `SuperDeploymentController.ExitTravel`, `FightEffectSystem.ActiveEffect`,
  `TeamBurrowManager.Update`, `TeamBurrowManager.GetDistanceToTarget`,
  `TeamBurrowManager.GetDistanceToAttackTarget`,
  `TeamBurrowManager.TryBurrowDown`, `TeamBurrowManager.TryBurrowUp`,
  `TeamBurrowManager.OnFightStart`, `TeamBurrowManager.ActiveEffect`,
  `BurrowTech.GetProbablity`, `BurrowTech.GetBuffDivide`,
  `BurrowData.GetAmplifyDamageRate`, `FightMech.GetNearestActor`,
  `FightSkill.GetNearestActor`, `SkillSearchTargetController.PerformSearch`,
  `SkillSearchTargetController.PerformNormalSkillSearch`,
  `ScoreRatingTargetSelector.Selector.Select`,
  `ScoreRatingTargetSelector.Selector.Calculate`.
- An acid technology leaves its acid where its unit dies, after the buff
  controller and before the explosive one, unless the unit's technologies
  are off: `DeadEffectSystem.Init`, `DeadEffectSystem.Update`,
  `DeadAcidRangeItemController.PerformDeadEffect`,
  `DeadAcidRangeItemTech.GetRangeItemRange`, `DeadAcidRangeItemTech.GetRoundDuration`,
  `DeadAcidRangeItemTechnologyData.PreProcess`,
  `DeadEffectController.IsAvaliable`, `DeadEffectProvider.DisableEffect`,
  `DeadEffectProvider.EnableEffect`, `RangeItemSystem.AddItem`.
- A dead-line technology hands its unit's main skill a pre-hit effect that
  destroys a live unit at or under the line at the owner's level as a
  suicide, before the hit's shield and damage, and charges it the line:
  `DeadLineEffectProvider.RegisterMechEventInternal`,
  `DeadLineEffectProvider.PerformPreHitEffect`,
  `DamagePerformer.PerformHitTargetEffect`, `DeadLineTech.GetDeadLineValue`.
- A stealth technology's unit goes into stealth as a hit leaves its life over
  its maximum no more than its share, once, and is shown once its time is
  past the duration; a disabling buff shows it and counts it as triggered:
  `StealthTechSystem.OnLifeChange`, `StealthTechSystem.CheckActiveStealth`,
  `StealthTechSystem.ActiveStealth`, `StealthTechSystem.Update`,
  `StealthTechSystem.EndStealth`, `StealthTechSystem.DisableStealthTech`,
  `StealthTechSystem.EnableStealthTech`.
- A unit in stealth loses nothing to a hit, no selector's search takes it, and
  an attacker that moves underground does not reach it, though what asks
  `IsValidTarget(Stealth)` strikes it: `FightCalculator.PerformHitTargetEffect`,
  `FightActor.ReduceLife`, `AttackTargetFilter.Check`,
  `SkillAttackRangeChecker.IsActorInAttackRange`.
- A skill's projectiles leave with the row's life at the unit's level, times
  one plus the skill's `SkillDataChangeFloatRate.ProjectileLifeRate`
  enhancements and then their remainder, cut to a whole number and at least 1:
  `FightProjectileSkill.GetMaxLife`, `DataSet.GetDataFloatAddRate`,
  `DataSet.GetDataFloatReduceRate`.
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
- A buff that burns or disables recovery: `Buff.Init` gives a buff whose
  `IBuffData.GetLifeChangeRate` is nonzero an `IBEC_ChangeLIfe`, which
  `Buff.Update` updates each `stepTime`. `FightMech.RecoveryLife` and
  `FightMech.StealLife` return before `FightActor.AddLife` while
  `BuffManager.IsRecoverDisabled`, which `FightMech.IsRecoverDisabled` reads
  as the buffs' `IBuffData.IsDisableRecover` count above zero.
- A buff source's chance: `BuffTechnologyData.PreProcess` stores
  `Utility.ConvertProbability` of its probability, the `FPoint` times 1000
  truncated, which `BuffTech.GetProbablity` answers. `BuffSystem.DoAddBuff`,
  after `FightActor.IsBuffTarget`, its building checks and
  `FightMech.IsIgnoredBuff`, adds nothing when `IBuffDataSource.GetProbablity`
  is not above 0, adds the buff when it is above 999, and otherwise returns
  when `GRRandom.IsProbabilityFail` on the target's `FightTeam.random` holds,
  which is `GRRandom.Next` of 1000 not below the chance.
  `CommanderSkillBase.GetProbablity`, `LandMineContraption.GetProbablity`,
  `TowerStrengthenData.GetProbablity` and `FightTrapSkill.GetProbablity`
  answer 1000.
- A buff source's effect name is the client's: `BuffTech.GetEffectName`
  answers `BuffTechnologyData.get_EffectName`, and the call index names no
  caller of a `GetEffectName` in the fight's assembly; the client's
  `ResourceManager.CreateBuffTechEffect` reads `BuffTechEffect.GetEffectName`.
- A hit source's range item: the `BuffTech` constructor makes a range item
  of its row when the row names a trigger buff, which
  `BuffTech.GetBuffRangeItem` answers, and each of `BuffRangeItem.GetLifeTime`,
  `BuffRangeItem.GetShowRangeItemType`, `BuffRangeItem.GetRangeItemRange` and
  `BuffRangeItem.GetRoundDuration` reads the row. After
  `BuffSystem.AddBuff`, `BuffCycleController.TriggerBuffOrBuffRangeItemFromHit`
  takes `FightSkill.attackTarget`, or `FightSkill.GetLockTarget` when there is
  none, and when it is not `FightActor.IsFly` and the source answers a range
  item, calls `RangeItemSystem.AddItem` at the hit's point with the team of
  the source's unit.
- A source whose listener is `BeHit`: `BuffCycleController.AddListener`
  adds `BuffCycleController.OnBeHit` to the unit's `FightMech.OnMechBeHit`,
  which `FightMech.OnHitted` raises after the hit's shield energy or
  `FightActor.ReduceLife`. `OnBeHit` returns while
  `SuperDeploymentSystem.IsTravelling` holds for its unit, and for
  `OpponentUnits` takes the hit's owner when it is a live `FightMech`. A buff
  whose `IBuffData.IsDisableTechnology` holds goes to the attacker's
  `BuffManager.AddBeHitDelayBuffInfo`, and any other to
  `BuffSystem.AddBuffByCheck`. `BuffManager.Update` ends with
  `BuffManager.InvokeDelayAddBuff`, which adds each queued buff through
  `BuffSystem.DoAddBuff` while its unit lives or
  `BuffSystem.IsAvaliableWhenActorDead` holds, and clears the queue.
- A stack that resets on a hit: `IBEC_AdditiveEffectBuff.Enter` hands
  `IBEC_AdditiveEffectBuff.ResetAdditiveStackNormal` to the reset controller
  `BuffAdditiveStackResetControllerFactory.Create` made, whose
  `BuffAdditiveStackResetHittedController.Register` passes it to
  `FightMech.RegisterMainSkillHittedAttack`, the unit's
  `FightMech.OnMainSkillPerformHitted`, which `FightMech.PerformMainSkillHitted`
  raises. A projectile's dispatch of its hit
  (`FightProjectile.DispatchHitDamageEvent`) calls it on its owner after the
  skill's hit effects when `IProjectileDataSource.IsMainSkill`, and it is the
  only caller: `SkillDamageProvider.DispatchHitDamageEvent`, a direct blow's or
  a laser's, hands its hit to `FightSkill.DispatchHitDamageEvent` alone, so a
  main skill that does not fire projectiles never resets the stack.
  `ResetAdditiveStackNormal` sets
  `IBEC_AdditiveEffectBuff.additiveStack` and
  `IBEC_AdditiveEffectBuff.additiveStackRecord` to zero and calls
  `Buff.RefreshEffect`.
- A buff that summons: `Buff.Init` gives a buff whose `IBuffData.IsSummoning`
  an `IBEC_DeadSummon`, and `BuffSystem.IsAvaliableWhenActorDead` lets
  `BuffSystem.AddBuff` add it to a dead target, as it does one whose
  `IBuffData.IsDisableTechnology` holds. `FightMech.OnDead` calls
  `BuffManager.OnMechDead`, `Buff.OnMechDead` and `IBEC_DeadSummon.OnMechDead`,
  which returns for a dead unit whose `FightMech.mechCreateType` is
  `MechCreateType.ParasiticalSummon` or whose domain the summon's
  `IMechData.IsFly` does not match, and otherwise counts
  `FPoint.Pow` of the radii's ratio and `IBEC_DeadSummon.DEAD_FACTOR`, at
  least 1, for `SummonSystem.CreateMech` at the dead unit's position, with
  its radius as `CreateSummonMechInfo.randomRange` and the buff's
  `Buff.source` as parent. `Buff.GetSummonMechID` answers the source's type
  below 1. `CreateSummonMechInfo`'s constructor sets its rotation to
  `FightUtility.Angle0`, so the summon faces the world's 0 on either side.
  `SummonSystem.CreateMech` makes it for the parent's
  `FightMech.currentTeamController`, so a parent a control has taken summons
  for the side that holds it, and `SummonSystem.DoCreateMech` calls
  `FightController.CreateMech` with that side and
  the parent's `MechData.IsChildInheritTechnologyEffect` and, with no
  `CreateSummonMechInfo.delayTime`, `SummonSystem.AddMech`. `Buff.Reset`
  takes the new source of a buff that summons when its `FightMech.level` is
  the higher. `FPCSMath.PowFastest` is `FPCSMath.LogFastest` times the
  exponent times log2 e through `FPCSMath.Exp2Fastest`.
- A source of the listener `Hit`: `BuffCycleController.RegisterMechEvent`
  calls `FightSkill.AddHitEffectProvider` on the main skill when
  `SkillDataModifier.AvaliableCheck` passes, and on its extra skills, grouped
  or passing the check. `FightSkill.DispatchHitDamageEvent` hands the hit's
  targets to `SkillHitEffectController.PerformHitEffect`, which calls each
  `IHitEffectPerformer.PerformHitEffect`. `BuffCycleController.PerformHitEffect`
  returns when `isTechnologyDisabled` and `IEffectProviderDataSource.CanDisable`,
  or on a secondary hit unless the source is a second damage's, and otherwise
  calls
  `BuffCycleController.TriggerBuffOrBuffRangeItemFromHit`, which hands the
  targets to `BuffSystem.AddBuff`; that calls `BuffSystem.DoAddBuff` for each
  target `FightActor.IsAlive`, under the source actor's
  `FightActor.currentTeamController`.
- The range's corrections: `AttackRangeProperty.GetAttackRange` sums
  `ISkillData.GetAttackRange`, the skill's data and
  `BuffManager.GetAttackRangeAddValue` with `BuffManager.GetAttackRangeReduceValue`;
  `AttackRangeProperty.Refresh` adds `BuffManager.GetAttackRangeAddRate` to
  `FightSkill.GetDataFloatAddRate`, multiplies `FightSkill.GetDataFloatReduceRate`
  by `BuffManager.GetAttackRangeReduceRate`, and multiplies the range by one
  plus the first and then by the second.
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
- A source's controller under the update model `All`:
  `BuffCycleController.Update` hands it to `BuffCycleController.UpdateModel1`,
  which counts `BuffCycleController.timeSum` up a tick. In
  `BuffCycleState.Delaying`, set by `BuffCycleController.TriggerCycleStart`,
  it returns until the count reaches `BuffCycleController.delayTimeConfig`,
  then takes it off and moves to `BuffCycleState.Cycleing` when
  `BuffCycleController.get_IsCycle`, an interval `FPoint`-unequal to zero, and
  otherwise stops; cycling, it returns until the count reaches
  `BuffCycleController.intervalTimeConfig` and takes it off. Each trigger
  gives the buff, through
  `BuffCycleController.TriggerBuffOrBuffRangeItemFromSelector` and
  `BuffSystem.AddBuffByCheck`, to the owner when
  `IEffectBuffDataSource.GetEffectTargetTypes` holds one type and it is
  `MechUnit`, and otherwise to each unit
  `RangeTargetCalculator.CalculateRangeActors` finds within
  `IEffectBuffDataSource.GetMax` that `BuffCycleController.AvailableCheck`
  passes. The constructor sets both configs to `FPoint.op_Division` of the
  time by the tick, its whole part. `AvailableCheck` passes for
  `FriendUnits` a unit of the owner's group whose
  `FightActor.currentTeamController` is not the owner's, and for
  `OtherSelfUnits` one other than the owner of the owner's team.
- A source's distance type: `BuffCycleController.AvailableCheck` returns
  false for a `FightMech` whose `FightMech.GetMainSkill` answers
  `FightSkillBase.IsMeleeAttack` when the type is `remote`, and for one whose
  does not when it is `Melee`, before it reads the target types.
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
- Losing life: `BuffCycleController.AddListener` hands a controller whose
  `IEffectBuffDataSource.GetBuffTechListener` is `BuffTechListener.GetDamage`
  to its unit's `FightActor.OnLifeChange`, which `FightActor.ReduceLife`
  invokes once it has taken life. `BuffCycleController.OnGetDamage` returns
  on no damage, while the unit's technologies are disabled
  (`FightMech.CanDiableTargetTechnology`, `ISkillOwner.IsTechnologyEnterDisabled`)
  and while it travels (`SuperDeploymentSystem.IsTravelling`), and for each of
  `IEffectBuffDataSource.GetEffectTargetTypes` adds the buff through
  `BuffSystem.AddBuffByCheck`: `TargetType.MechUnit` to the unit itself,
  `TargetType.OpponentUnits` to the unit that dealt the damage, or through
  `BuffManager.AddBeHitDelayBuffInfo`. `BuffSystem.AddBuffByCheck` returns for
  a dead target unless `BuffSystem.IsAvaliableWhenActorDead`.

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
  `BuffEffectProvider.DoEnableCycle`, `BuffCycleController.RemoveListener`,
  `BuffManager.ClearSelfResourceBuffByDisableTech`,
  `Buff.ResetAdditiveEffectStackByDisableTech`,
  `IBEC_AdditiveEffectBuff.AddAdditiveStack`,
  `IBEC_AdditiveEffectBuff.RefreshAdditiveEffect`,
  `IBEC_ChangeMaxLife.DoAdditiveEffect`,
  `PermanentPreemptiveActiveConditionLifeController.CheckCanActive`,
  `PreemptiveSkillController.Update`,
  `DeadEffectSystem.OnActorDead`, `DeadEffectController.IsAvaliable`,
  `ExplosionSkillData.IsTechnologyEffect`,
  `FightMech.RefreshLifeData`, `FightSkill.RefreshAttackInterval`,
  `SkillDataModifier.RemoveData`, `FightSkill.RefreshDatas`,
  `ExtraSkillProvider.DisableSkill`, `ExtraSkillProvider.EnableSkill`,
  `FightSkill.Disable`, `FightSkill.Enable`, `SkillIdleState.Update`,
  `SkillAttackState.CheckAttackable`.

- A splash technology writes its numbers as a plain technology does and adds
  its range to the skill's splash: `SplashTech.AddData`,
  `SplashTechnologyData.GetRange`, `Technology.AddData`,
  `SkillDataModifier.AddData`, `FightSkill.GetSplashRange`.
- High-Speed Engine is a plain technology in a fight: `TechnologyFactory.Create`
  makes a `MobilityIntensifyTech`, which declares only its constructor
  (`MobilityIntensifyTech.MobilityIntensifyTech`) over `Technology.AddData`.
- A unit's interceptors: `InterceptMissileTech` answers `IInterceptData`
  from its row, and `InterceptMissileEffectProvider.DoActive` adds the unit's
  group to its side's (`TeamInterceptSourceManager.GetInterceptSource`),
  `InterceptCtrGroup_Mech`'s constructor making `GetWeaponCount` of
  `InterceptEffect_FightMech_Preemptive` or `_NoPreemptive` as `IsPreemptive`
  says, each initialised and added (`InterceptCtr_Group.Init`,
  `InterceptCtrGroup_Mech.DoAdd`, `InterceptEffectBase.DoAdd`).
- They stand where their unit's transform stands, on its side:
  `InterceptEffect_FightMech_Preemptive.GetPos`,
  `InterceptEffect_FightMech_NoPreemptive.GetPos`,
  `InterceptEffect_FightMech_NoPreemptive.GetTeamController`, and a
  projectile joins a unit's group only while the unit stands on that side:
  `TeamInterceptSourceManager.GetCurrentTeamInterceptSources`.
- A preemptive one locks its unit's main skill as it prepares and idles it
  as it goes idle: `InterceptEffectBase.EnterPrepare`,
  `InterceptEffectBase.EnterIdle`,
  `InterceptEffect_FightMech_Preemptive.ChangeMechToLockState`,
  `InterceptEffect_FightMech_Preemptive.ChangeMechToUnlockState`,
  `FightSkill.ChangeToLockState`, `FightSkill.ChangeToIdleState`,
  `SkillLockState.Enter`, `SkillLockState.Exit`.
- A dead unit's group leaves its side's, and nothing else is done to its
  interceptors: `InterceptMissileEffectProvider.DoDeactive`,
  `TeamInterceptSourceManager.DoRemove`, `InterceptCtr_Group.DoRemove`,
  `InterceptEffectBase.DoRemove`. A turned unit's moves to the other side's:
  `InterceptSystem.OnChangeTeam`.

### Not established

- **A second spawned shield, and one standing as the fight ends**: read,
  not recorded.
- **A Phantom Ray switched off while cloaked**, and one whose technologies
  come back while it shows: read from the build, not recorded.
- **A chain whose jump makes the fight's last kill.** A Raiden whose jump
  kills the last Fang, before the main skill's update, attacks a tower that
  is torn down on the next tick: the game keeps its skill cooling on the
  tower, and the simulator lets it go idle.
- **A Raiden with Chain over Crawlers standing under it.** An idle weapon's
  search at t563 of a Raiden with Chain against a Fortress, Fangs, Mustangs
  and Crawlers keeps a Crawler in the game and takes a nearer one in the
  simulator; the same layout without Chain matches throughout, and its fight
  takes another course before then.
- **A tie between points.** The sort that orders points at one distance
  follows `List.Sort` as read; no recording tells it from a stable one.
- **A rebirth beside another dead effect, switched off, or of a turned or
  summoned unit.** No recording holds one. A unit another side turned keeps
  that side as it dies (`RebirthTask.StartTask` sets `rebirthToTeam` when
  `currentTeamController` is not `originTeamController`, before
  `TeamTranslationSystem.OnMechDead` hands it back), follows that side's units
  of its type (`GetTeamAliveMechs`) while keeping an ally only on the side it
  stands on (`UpdateReadyRebirth`), and rises on it, turned again
  (`RebirthMech`: `FightActor.ChangeTeam`,
  `TeamTranslationSystem.AddTranslatedMech`) under a formation of its own:
  Typhoons with Field Reassembly a Hacker turned rise as the Hacker's. A
  turned summon's rebirth (`SummonSystem.RebirthMech`) is refused by name.
  A dead summon or a
  rebirth switched off is read from the build, the acid's alone recorded.
  A unit an explosion kills dies after the acid controller has run and
  leaves no acid, read from the build.
- **A burrowing technology switched on again, a burrowing unit risen
  again, or one a battle skill summons.** Read from the build; no recording
  holds one. One with a grouped main skill or a batch of standalone weapons,
  whose nearest enemy no recording has read, is refused.
- **A loose formation switched on again, or a battle skill's summon that
  holds it.** Read from the build; no recording holds an Electromagnetic
  Impact running out on Crawlers that hold it, and no battle skill summons
  Crawlers.
- **A fire extinguisher switched on again, on a summon, or clearing a fire
  burnt from oil.** Read from the build; no recording holds an
  Electromagnetic Impact running out on Hounds that hold it, a battle skill
  or a dying unit that summons a Hound (none does), nor a Hound clearing a
  fire burnt from an oil beside a shield.
- **A repair switched on again, and two clocks reaching the interval
  together.** Read from the build; no recording holds an Electromagnetic
  Impact running out on Typhoons that hold it, nor two repairing Typhoons
  whose dictionary order a repair's events show, beyond a pair added
  together.
- **A kill explosion raised by buffs, switched on again, chaining, or after
  its unit's death.** Read from the build; no recording holds a tower buff on
  Typhoons that hold it, an Electromagnetic Impact running out on them, a
  row that chains, nor a rocket landing after its Typhoon died, which still
  sets off what it kills, the hit effect staying on the skill, which only
  disabling takes off (`KillExplosionEffectProvider.DoDisableEffect`).

- **The order of a skill's hit effects.** `SkillHitEffectController` runs
  them in the order they were added, and `FightEffectMananger.RegisterMechEvent`
  walks the unit's providers in their order, each registering its own
  (`EffectProvider.RegisterEffectEvent`): a wreckage record's, a lifesteal's,
  a buff source's, a main fire's, then a kill explosion's. A Void Eye with
  Suppression Shots and Energy Absorption takes its life back before its
  buff is written on what it struck. Read from the build; no recording holds
  a unit with two of them.

- **A wreckage record running out, and the share among holders.** Read
  from the build; no recording holds a recorded unit dying after its time,
  nor a share smaller than what each holder lacks.

- **That one a buff summoned summons nothing as it dies.** Read from the
  build, and no recording holds a summoned unit dying under the buff.

- **A hurt unit's life as a disable moves its maximum.** The share rule is
  the one a buff's maximum life measured; no disable has been recorded on a
  hurt unit whose technology moves its maximum.
- **What switching off does beyond numbers**: an extra weapon's production
  line (`SupportUnitCreator`), an explosion or preemptive skill other than a
  permanent preemptive explosion or an around skill, an active permanent preemptive skill
  (`PreemptiveSkillController.Update` gives it up), a group. Refused.
- **A unit's interceptors switched off.** `InterceptMissileEffectProvider.DisableEffect`
  disables each and lets it idle (`InterceptEffectBase.DoDisable`), and
  `EnableEffect` enables each and lets it idle (`DoEnable`); read from the
  build, no recording holds it.
- **What a joining unit's effects do.** A unit made by a line, summoned by
  a battle skill or summoned as another dies is handed its side's loadout
  for its type whole (`TeamFightEffectManager.CreateMechUnitEffectMananger`),
  and `FightEffectSystem.ActiveEffect` hands it to each provider as it
  joins, as it does a unit landing. Read from the build; no recording holds
  a joining unit's interceptors, stealth, group, trench, repair or rebirth.
  It runs the lines its side's technologies and extra weapons hand its type,
  a creator of each made as its effects are added
  (`SupportUnitProvider.AddEffect`, `SupportUnitSystem.AddSkillOwner`), after
  its side's others, except a line that makes its own type, which is passed
  over for a unit created in the fight (`SupportUnitProvider.AvaliableCheck`,
  `FightMech.mechCreateType`): a Vortex Mirage makes no Mirage
  (`tests/production/electromagnetic-twin.yaml`), and a Vulcan's
  Marksman with Shooting Squad makes its Fangs. What it summons as it dies
  is summoned as a placed unit's is (`DeadSummonTech`, `IBEC_DeadSummon`),
  the summon handed its side's loadout for its type: a War Factory's Steel
  Balls with Mechanical Division leave their Crawlers. Read from the build;
  no recording holds it. One whose technologies give it a line as it
  surfaces is refused.
- **A unit's interceptors beside a building's, and on a turned or travelling
  unit.** Here a side's buildings update before its units, which is not read;
  the order the side's records keep units in is read from the build and
  recorded only with one unit's group, or one squad's.
- **How the life share rounds.** The quotient rounded and the product
  truncated is what the recordings fit; the arithmetic of `FPoint` division
  and multiplication was not read.

- **A value applies before a rate, for a technology.** Measured on the
  Sledgehammer's interval technologies, whose fights no test pins because the
  simulator refuses the unit.
- **What the other technologies do**, in the terms a simulator needs. Each
  one owes the mechanism it belongs to: a summon, a skill's own numbers, a
  debuff on the target.
- **`min_attack_range_value`**, which no mechanism in `crates/simulation`
  reads.
