# Fight architecture

[简体中文](architecture.zh.md)

[TOC]

## Scope

This contract defines the shape of the simulator: what a mechanism is, where it
lives, how it is driven, and how a unit's numbers are computed. It is the frame
the per-mechanism contracts hang from, and it says nothing about what any one
mechanism does — [rvo.md](rvo.md), [quadtree.md](quadtree.md),
[`docs/rules/`](../../rules) and each mechanism's own index own that.

The shape is not invented. Build `1.11.1.3.2259` has a module architecture, an
object model and a derived-value layer, and this mirrors all three, because a
simulator shaped like the thing it reproduces can be checked against it a piece
at a time. Where the build's structure and a convenient structure disagree, the
build wins and the reason is written down.

**What the evidence covers.** The structure here is read from the local
decompilation index, `work/unity-index/1.11.1.3.2259/index.sqlite`: 36,361
types, 301,615 methods and 662,014 call edges produced by Cpp2IL.
[`scripts/fight-structure.py`](../../../scripts/fight-structure.py) regenerates
every list and table below from it. The index carries **no method bodies**, so
what it establishes is membership and call edges — which type exists, what it
owns, what it calls. It does not establish arithmetic, branch conditions, or
the order of calls inside a body. Every statement below is one of the first
kind, and the ones that would need the second kind are in
[Unresolved](#unresolved).

## The native shape in one picture

```text
              IMatch
                │  holds
                ▼
          IModuleManager ──────────────────────────────────┐
                │  GetModules()                            │
                ▼                                          │
     FightModule (35 of them)                              │
       Init · OnActive/OnDeactive · OnEnterFight/OnExitFight
       OnFightStart/OnFightEnd · Update · IsStepFinish · Stop
                │                                          │
                │ drive                                    │ own
                ▼                                          ▼
          FightActor                                  side objects
       ├── FightMech    (a unit)                 RangeItem · FightEnergyShield
       └── FightCrystal (a building)             Projectile · FightInterceptor
                │
                ├── BuffManager   ── aggregates over active Buffs
                ├── DataSet       ── per-instance overlay, tagged by IDataModifier
                └── FightSkill(s) ── each with its own DataSet over shared ISkillData
                            │
                            ▼
                     FightProperty
            AttackInterval · AttackRange · Damage · MoveSpeed
            ProjectileCount · ProjectileDuration · …
            cached, invalidated by the DataSet's change listeners
```

Three sentences carry it:

- **Everything that runs is a module.** `FightCoreSystem` is one of 35, not a
  privileged loop with the rest hanging off it.
- **Everything a mechanism changes about a unit goes through one of two
  overlays**, the actor's `DataSet` or its skills' `DataSet`, or through the
  `BuffManager`'s aggregate of active buffs. Nothing writes a base number.
- **Nothing reads a number directly.** A `FightProperty` composes the shared
  description with those overlays, caches the result, and is invalidated by
  change events.

## Where the code lives

`crates/simulation/src/fight/` is the fight, split by what each part mirrors in
the build, and each file declares the data it owns. A unit is `Actor` in
`mod.rs` — the mech: its placement, position, facing and life — holding a
`Motion` (`motion.rs`, the `MotionController`: its state, where it was asked to
go, and what the RVO solver made of that) and a `Skill` (`skill/mod.rs`, the
`FightSkill`: the lock and what the weapons fire at, the skill's state, the
attack being made). A path through a unit reads as the build's does:
`actor.skill.lock_target`, `actor.motion.state`.

A skill has one machine whoever owns it. A construction that fires
(`construction.rs`, the `FightConstruction`) holds the same `Skill`, and
`update_skill` is `SkillManager.Update` for either owner. What differs between
the two reaches the skill only through `attacker.rs`, the build's
`ISkillOwner` and `IAttacker`: where the owner stands, its radius and reach,
what its attack angle is measured against, how fast its weapons turn, and
whether it searches. A construction's `MotionController` is never updated, so
its motion reads idle. The kernel asks a unit's skill to start its attack from
the unit's motion (`attack_in_range`); a construction's asks it after the
skill's update (`attack_in_reach`), with the same `try_start_attack`.

| File | Mirrors |
| --- | --- |
| `mod.rs` | the objects, and `FightCoreSystem`'s advance: `step` |
| `deploy.rs` | deployment: formations, the initial actors and buildings, the presearch |
| `mech.rs` | `FightMech`: position, facing, life, and its snapshot |
| `construction.rs` | `FightConstruction`: a building that owns a skill, and its update |
| `attacker.rs` | `ISkillOwner` and `IAttacker`: what a skill reads of its owner, and the reach, angle, weapon turn and interval draw written once against it |
| `search.rs` | the target quadtrees and the target selector |
| `motion.rs` | `MotionController`: holding a dead target, attacking one in range, leaving or approaching one; the RVO submission |
| `damage.rs` | `DamagePerformer`, [below](#damage) |
| `projectile.rs` | `ProjectileSystem` |
| `skill/mod.rs` | `SkillManager.Update` for any owner: `FightSkill`'s update as named parts, its `SkillState` with the reload, the search timer, `TryStartAttack` |
| `skill/check.rs` | `SearchAttackTarget`, `SkillAttackableChecker`, `WallConstructionTargetChecker`, `SkillAttackState.Finish` |
| `skill/perform.rs` | the attack performers: a blow, a shot, a burst |
| `skill/group.rs` | a grouped skill's slots |
| `rvo.rs` | `RVOSimulatorFixed`, the sampled RVO the motion submits to; [rvo.md](rvo.md) |
| `random.rs` | `GRRandom`, the stream an attack interval's stagger draws from |
| `math.rs` | the build's fixed-point arithmetic |
| `run.rs` | running a layout and comparing it with a recording |

Outside the fight, the crate holds what builds one. `layout/` compiles a
layout into placements and, in `layout/constructions.rs`, the buildings a
construction releases. `modifier/` is the `Modifier` step: the officers and
technologies written onto a unit before it fights. `data.rs` is the data layer
both of them and the fight read, `rules.rs` the unit descriptions, and
`module.rs` the registry of which module claims which layout field.

## Modules

`FightModule` is the base of every system, and `IModuleManager.GetModules()`
holds them. The lifecycle is the module's whole contract with the fight:

| Hook | When |
| --- | --- |
| `Init(IModuleManager)` | the match builds its modules |
| `OnActive` / `OnDeactive` | the module is switched on or off |
| `OnEnterFight` / `OnExitFight(isInterrupted)` | the fight scene is entered and left |
| `OnFightStart` / `OnFightEnd` | the fight itself begins and ends |
| `Update` | one logic advance |
| `IsStepFinish` | whether this module has settled this advance |
| `Stop` | the match tears down |

A module is a `GameRiver.Fight` type whose name ends in `System` and whose
constructor takes the match. All 35 do, and the only other types built that way
are `FightController`, `FightModule` itself and two generic bases, so the set is
exactly these. Twenty-five of them override at least one lifecycle hook and ten
override none, which says those ten are driven by the calls made into them
rather than by the advance.

The simulator carries a module for each one, implemented or not:

```text
AdvancedEnergyShieldSystem  AutoRecoverySystem      BuffSystem
BuildingSystem              BurrowSystem            ClearRangeItemSystem
CloakSystem                 CommanderSkillSystem    DeadEffectSystem
ExpSystem                   ExtraSkillSystem        FightConstructionSystem
FightCoreSystem             FightEffectSystem       FightGroupSystem
FightTeamSystem             InterceptSystem         IterationEffectSystem
LifeChangeEffectSystem      MechGrounpSystem        MineSystem
MoveAbilityRangeItemSystem  MoveAbilitySummonSystem ProjectileSystem
RangeItemSystem             ReactiveArmorSystem     RecoveryEffectSystem
SiegeModeEffectSystem       StealthTechSystem       SummonSystem
SuperDeploymentSystem       SupportUnitSystem       TeamTranslationSystem
TechnologySystem            WreckageRecoverySystem
```

**A module that does nothing still exists.** It declares the layout fields it
would claim and refuses them, which is what makes the closure a property of the
registry rather than a hand-written list: a layout compiles when every field it
carries is claimed by a module that understands it, and is refused naming the
field and the module otherwise. Implementing a mechanism is filling in its
module, never editing the loop that drives it.

**A module is not all or nothing.** It claims fields and understands some of
them; the rest are refused exactly as an empty module's claims are. `Modifier`
claims officers, technologies and equipment, and understands officers and
technologies today, because two of the four effect tables are extracted. A
field it understands can still refuse one particular layout: an officer or a
technology whose effect this build cannot compose refuses the side holding it,
by name and by field, rather than being half applied.

A unit's level is not a field, and not a correction. `FightMech` is built
with its `IMechLevelData`, and its `GetBaseLife` and `GetBaseDamage` multiply
the description by the level's rating before any `DataSet` applies; build
2259's ratings are the levels themselves. So the level travels with the unit
into `Stats` as its own multiplier, and every overlay applies to the product
([unit_levels.md](../../rules/unit_levels.md)).

One module is not the build's. Officers, technologies and equipment are
applied to a unit **before** the fight rather than inside it — the build's
`TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is called
from the deployment's `MAP_AddUnit` — so the simulator applies them as the fight
is built, in a step of its own called `Modifier`. Which module claims which field
is otherwise this simulator's arrangement; the names are the build's, and two of
the arrangements are the build's too, `RangeItemSystem` owning terrain and
`SuperDeploymentSystem` owning a travelling unit because
`FightCoreSystem.PreCalculate` asks it `IsTravelling`.

A refusal names every field both sides carry at once rather than the first,
because what a caller wants to know is how far a deployment is from being
fought:

```text
side blue needs modules this build has not implemented: constructions
(FightConstructionSystem), battle skills (CommanderSkillSystem); side red needs
modules this build has not implemented: unit equipment (Modifier)
```

A refusal from inside a field the registry lets through names the thing rather
than the field, because the field is understood and this one member of it is
not:

```text
side blue: officer 20006 (先进瞄准系统) writes attack_range_value, and how a
value composes with a description is not measured
```

## Objects

| Native | What it is |
| --- | --- |
| `FightActor` | anything with life, a team, bounds, buffs and shields |
| `FightMech` | a unit; carries skills, motion and a formation |
| `FightCrystal` | a building: a tower (`IsTower`, `BuildingType` `EnergyTower` or `ResearchCenter`), or a construction (`Special`) |
| `FightTeam` / `FightTeamController` | one side, and what it owns |
| `IFightGroup` | a formation, the unit of group behaviour |
| `FightSkill` | one skill channel of an owner, over shared `ISkillData` |
| `RangeItem` | a terrain area, held by a `RangeItemController` per type |
| `FightEnergyShield` | a standalone shield, held by `AdvancedEnergyShieldSystem` |
| `FightInterceptor` | an interception source, held by `InterceptSystem` |
| `Projectile` | a shot in flight, held by `ProjectileSystem` |

Each of those has a column in a recording, which is what makes a mechanism
acceptable on its own: [mcfr.md](../mcfr/mcfr.md) records units, buildings,
shields, terrains and projectiles as separate collections, and the object model
above is one-to-one with them.

## The data layer

This is the part worth copying exactly, because it is what makes an effect
attributable after the fact.

**A description is shared.** One `ISkillData` and one unit description serve
every instance of a type. Nothing may write to them.

**A change is an overlay entry with an owner.** `DataSet` holds three kinds of
entry, addressed by an index:

| Kind | Reader | What it holds |
| --- | --- | --- |
| Float | `GetDataFloat(index)` | an additive value |
| FloatRate | `GetDataFloatAddRate(index)` / `GetDataFloatReduceRate(index)` | a pair, the increase and the decrease |
| Int | `GetDataInt(index)` | an integer |

Every write carries its source: `ChangeDataFloat(index, IDataModifier, value)`.
That is how a technology, an equipment or a buff can be added and removed
without anyone recomputing a base, and it is why a recording can say which
channel a correction came from.

The indices are the `MechDataChange{Float,FloatRate,Int}` and
`SkillDataChange{Float,FloatRate,Int}` enums, and a skill's own mutation is
`FightSkill.AddData(SkillDataChangeFloat, IDataModifier, value)`. Those enums
are exactly the field sets MCFR records as `unit_dynamic_modifiers` and
`skill_dynamic_modifiers`.

**Buffs aggregate separately.** `BuffManager` sums the active `Buff`s of an
owner and exposes the total through getters — `GetAmplifyDamageAddRate`,
`GetAttackIntervalChangeAddRate`, and the rest of the set MCFR records as
`buff_modifiers`. It is not a `DataSet`, and that is the point: a buff and a
data change that produce the same number stay distinguishable.

So a unit's state is **one shared description and three overlays**, and a
recording dumps all three every tick.

## Derived values

A `FightProperty` is a cached derived number. It registers as a listener on the
overlay entries it depends on — `RegisterDataChangedEventFloat`,
`…FloatRate`, `…Int` — is marked dirty when one changes, and recomputes in
`Refresh()`. `MechPropertyFloat` hangs off a `FightMech`, `SkillPropertyFloat`
off a `FightSkill`.

What each property reads is a call-edge fact, and therefore established. The
arithmetic that combines them is not:

| Property | Reads |
| --- | --- |
| `AttackIntervalProperty` | skill `GetDataFloatAddRate` / `GetDataFloatReduceRate`; `BuffManager` attack-interval and extra attack-interval add/reduce rates; `FPoint.Max` |
| `AttackRangeProperty` | skill `GetDataFloatAddRate` / `GetDataFloatReduceRate`; `BuffManager` attack-range and extra attack-range add/reduce rates |
| `DamageProperty` | `FightMech.GetBaseDamage`; skill `GetDataFloatReduceRate`; its own `CalculateBaseDamage` and `CalculateDamage` |
| `MoveSpeedProperty` | `FightMech` `GetDataFloatAddRate` / `GetDataFloatReduceRate` / `GetDataInt`; a `DataSet`'s add and reduce rates; `BuffManager.GetMoveSpeedChangeValue` |
| `ProjectileCountProperty` | `DataSet.GetDataInt` |

### How a correction composes

The build says this in its own type names. `DataSet` keeps three lists:

```text
List<DataInt>                 intDatas        a plain integer
List<AdditiveDataFloat>       floatDatas      ChangeDataFloat      — a value
List<MultiplicativeDataFloat> floatRateDatas  ChangeDataFloatRate  — a rate
```

`DataInt` is abstract, and which of its three subclasses an index uses is the
whole of that index's arithmetic: `DataIntGroup` sums its entries and clamps
them between the two bounds its constructor takes, `DataIntSingleMax` keeps the
largest single entry, `DataIntSingleMin` the smallest. `FightMech`'s
constructor builds `DataIntGroup(0x80000000, 0x7FFFFFFF, 0)`, so a unit's
integers are a plain sum whose clamp is the whole `Int32` range and never
binds.

`AdditiveDataFloat.Refresh` sums its entries — the ISIL is an `add` in a loop
with saturation guards — and the class carries `Min` and `Max` with
`FPoint.Clamp` in its call list, so a value is a sum that can be clamped.

`MultiplicativeDataFloat.Refresh` keeps **two** accumulators. It resets one to
zero and the other to a metadata constant of one, then walks its entries and
routes each by `FPoint.op_GreaterThan` — by its sign — summing into the first
and multiplying into the second. `GetDataFloatAddRate` returns the first and
`GetDataFloatReduceRate` the second, which is why one recorded `reduce` can
stand for several impairments: the build has already multiplied them together,
and MCFR stores `1 − that product`.

`FightMech.CalculateMaxLife` shows the assembly: it loads `0x100000000`, the
Q32.32 one, adds the add-rate to it, shifts an integer from the data source
left by 32 to make it an `FPoint`, and multiplies. So:

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

truncated toward zero once, where the build casts back to `Int32`. **An
impairment is not a negative enhancement**: two of `0.11` leave `0.89 × 0.89`,
not `1 − 0.22`. `tests/modifier/` holds the fixtures that measure each
clause against the game, and
[`officer_effects.md`](../../rules/officer_effects.md) records what they
answered.

Two things fall out of that table. An attack interval is clamped from below and
a range is not, which is a difference in kind and not an accident. And a
property's inputs are exactly the recorded columns, so **a mechanism can be
checked before its arithmetic is known**: whether the table it fills produces
the number the game stored is one question, and whether the composition turns
that number into the right damage is another.

## Damage

Every way a fight deals damage is one provider handed to one performer. The
build's `IDamageProvider` describes a hit — `GetDamage`, `GetSplashRange`,
`GetMainTarget`, `GetMaxTargetCount`, `GetEffectTargetType` and the rest — and
is implemented by skills, projectiles, commander skills, kill explosions,
mines, support units and constructions. `DamagePerformer` resolves any of them:
`PrepareRangeTargets` decides who is struck, and `PerformSingleEffect` /
`PerformRangeEffect` take the life. What it strikes is a `FightActor`, so a
unit and a building are struck by the same code.

The simulator mirrors that split:

| Native | Simulator |
| --- | --- |
| `IDamageProvider` | `DamageHit`: source, amount, what it was aimed at, whether that is struck wherever it stands, splash centre and radius, the domains it reaches |
| `DamagePerformer.PrepareRangeTargets` | `damage_targets`, the one place that decides who a hit lands on |
| a single target's life change | `strike`, the one place life is taken, from a unit or a building |
| `PerformRangeEffect` | `perform_damage`, which strikes every target in order and records `damage` for each that lost life |

**A way of dealing damage is a provider, never a new resolution.** A direct
strike, a projectile arriving and a laser each describe their hit and hand it
over; a laser, having no range, takes the stroke without the range step. What
differs between them is only where they record what follows — a projectile
records its own removal before the deaths it caused — so deaths and fallen
buildings are handed back to the caller rather than recorded by the performer.

**A splash takes both kinds, in the order the trees hold them.**
`damage_targets` walks each enemy team's target tree, where walls stand beside
units, and takes every unit its reach and domain allow and every alive,
targetable building whose edge the splash reaches, each for the full amount,
whatever the hit was aimed at. The deaths and falls a strike causes are handed
back in the order struck and recorded at the end of the tick, after the deaths
of the step; a unit that died leaves its tree then, keeping the order of the
rest.

## Attack target

A skill decides what its weapons fire at in one place, and every construction
rule the fight knows reaches the weapons through it. The build's
`FightSkill.SearchAttackTarget` takes the lock and hands the weapons a shield,
else a wall in the way (`WallConstructionTargetChecker`, which has no other
caller), else the lock itself. It is reached from the idle state every update
the skill holds a lock, and from `SkillAttackableChecker.Check`, which the
prepare state runs every update and the attack state runs between blows and
while a blow winds up. `SkillAttackState.CheckAttackable` rejects a dead
building attack target before asking the checker, and a failed check finishes
the attack: `StopAttack` clears the lock, the skill cools, and it enters idle
with its targets cleared.

| Native | Simulator |
| --- | --- |
| `FightSkill.SearchAttackTarget` | `search_attack_target`, the only caller of `wall_in_the_way` |
| `SkillAttackableChecker.Check` | `check_attackable` |
| `SkillAttackState.CheckAttackable` | `attack_state_check_attackable`, asked where `between_blows` holds |
| `SkillAttackState.Finish` | `finish_attack`, into the cooling hold or straight to idle |
| a failed check in `SkillPrepareState` | `enter_idle_clearing_targets` |

**Nothing here is about walls.** A wall differs from a unit only in being an
answer `SearchAttackTarget` can give, and in the building test the attack state
applies to a dead target; both are the build's. The skill states the checks run
in were read from a capture of each unit's `SkillStateController` state and
`SkillAttackController` phase (the `target_refs_v1` instrumentation profile),
beside the recordings under `tests/construction/`.

**How it was held to the game.** Two captures of the whole regression
manifest are the oracle: each unit's skill state and attack phase per tick
(`tests/regression/skill-state.mcscript`), and every `Check` call with the
skill's lock and attack target on either side of it (the
`skill_attackable_checker_v1` profile). Grouped skills use the same checker
per slot, with the siblings' lock holdings and each slot's own range. The
[Wraith fixtures](../../../tests/wraith/README.md) describe the independent
per-call replay and the physics/content checks. Beyond the checker itself:

- the checker runs between blows, through the wait before a blow, and on the
  update the blow lands, before it is performed;
- a backswing ends by the next blow: the one a description states is cut to
  the attack interval, as a Wasp's is;
- the attack state outlives the backswing, for every unit;
- a state is not updated on the tick it is entered: the skill enters its
  prepare or attack state the tick the unit comes into range, even while its
  facing is still being corrected, and the first blow waits for the tick after
  the prepare ends.

With those, the stale-target, quick-switch-out-of-range and stale-replacement
paths that answered a lock dying or walking away are gone: the checker answers
all of it.

**Grouped search boundary.** A group searches and checks its slots in order;
each settled search publishes its lock to the mech. The ordinary search
excludes sibling locks and permits sharing according to the skill data.
The child-range rule is in [combat](../../rules/combat.md#a-grouped-slot-searches-around-its-siblings-locks).
The mirror refuses live shared-lock redistribution onto an available unheld
target and a child leaving its attack area. Grouped fusillade is outside the
supported configs. The existing prepare offset and per-slot wall checks are
separate from this target-search contract.

**What the mirror does not carry yet.** A skill
that cannot switch quickly keeps a live lock out of reach. That is measured,
and the path is read: the build reads the quick-switch flag only in the
research branch of `Check`, and a live lock sends `CheckWhenLoseTarget` to the
synchronous search (`PerformNormalSkillSearch` with no prepared job), which
answers the lock. Why the selector's score favours it there is not read. The
mirror states the measured rule in place of that score.

## The mirror, and what is dummy

The simulator carries the same three layers under the same names, and grows by
filling modules rather than by editing the kernel:

| Native | Simulator |
| --- | --- |
| `IModuleManager` + `FightModule` | the module registry, one entry per system, each declaring the layout fields it claims |
| `DataSet` over a shared description | the two overlays, per actor and per skill, each entry tagged with the mechanism that wrote it |
| `BuffManager` aggregates | the buff channel, summed per actor |
| `FightProperty` | a derived value read through `stat(...)`, never a direct field read |
| `IDamageProvider` + `DamagePerformer` | one hit description and one resolution, [above](#damage) |
| `SearchAttackTarget` + `SkillAttackableChecker` | one answer to what the weapons fire at, [above](#attack-target) |
| the recording's columns | what each layer is accepted against, per tick |

A module that is not implemented is present, claims its fields, and refuses
them. The count of tracked rounds a layout compiles for is the progress bar, and
[`scripts/fight-coverage.py`](../../../scripts/fight-coverage.py) reports it.

## Determinism invariants

The frame inherits the simulator's existing invariants and adds three of its
own:

- **Module order is fixed and explicit.** Whatever order the modules are driven
  in, it is a constant of the build rather than a map iteration, and two runs of
  one seed drive them identically.
- **An overlay is a set of tagged entries, not a running total.** Adding then
  removing a modifier restores the exact previous value, because the value is
  recomputed from the entries and never adjusted in place.
- **A derived value is a pure function of the description and the overlays.** A
  property may be cached and invalidated, but a cache that is dropped and
  recomputed at any moment gives the same number, so caching cannot change a
  result.

## Fidelity boundary

The frame reproduces the build's structure, not its implementation. A module
here is a place for a mechanism, and a mechanism is only as faithful as its own
contract says. Three things are deliberately not claimed:

- **No timing claim comes from this document.** That a module has an `Update`
  says it runs each advance; it does not say when within the advance.
- **No arithmetic claim comes from this document.** A property's input list is
  what the call graph shows; how the inputs combine is each stat's own
  question.
- **A dummy module is a refusal, never an approximation.** A fight that needs an
  unimplemented mechanism is not fought.

## Unresolved

- **What a property does with two channels' aggregates.** One channel is
  settled, below; what `AttackIntervalProperty` does when a skill's `DataSet`
  and the `BuffManager` both answer is not, and `data.rs` refuses a number
  corrected in two channels at once rather than assuming the same shape
  extends across them.
- **The order the modules are driven in, and the order of work inside one
  advance.** `FightCoreSystem.Update` calls `TeamUpdate` then `GroupUpdate`, and
  `PreCalculate` exists beside `Update`, but a body's call order is not in the
  index. It closes by measurement against a recording.
- **Which enum index is which field.** The `MechDataChange*` and
  `SkillDataChange*` names are known and so are the recorded field names; the
  numeric indices behind them are not read yet.
- **What `IsStepFinish` decides.** Every module has one, and the terminal
  condition a fight ends by is currently the simulator's own.
