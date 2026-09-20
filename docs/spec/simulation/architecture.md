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
       └── FightCrystal (a tower, a building)    Projectile · FightInterceptor
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
carries is claimed by a module that implements it, and is refused naming the
field and the module otherwise. Implementing a mechanism is filling in its
module, never editing the loop that drives it.

One module is not the build's. Officers, technologies, equipment and levels are
applied to a unit **before** the fight rather than inside it — the build's
`TechnologySystem.AddTechnologyEffect` takes a `PlayerController` and is called
from the deployment's `MAP_AddUnit` — so the simulator applies them as the fight
is built, in a step of its own called `Loadout`. Which module claims which field
is otherwise this simulator's arrangement; the names are the build's, and two of
the arrangements are the build's too, `RangeItemSystem` owning terrain and
`SuperDeploymentSystem` owning a travelling unit because
`FightCoreSystem.PreCalculate` asks it `IsTravelling`.

A refusal names every field both sides carry at once rather than the first,
because what a caller wants to know is how far a deployment is from being
fought:

```text
side blue needs modules this build has not implemented: officers (Loadout),
constructions (FightConstructionSystem); side red needs modules this build has
not implemented: officers (Loadout)
```

## Objects

| Native | What it is |
| --- | --- |
| `FightActor` | anything with life, a team, bounds, buffs and shields |
| `FightMech` | a unit; carries skills, motion and a formation |
| `FightCrystal` | a tower, a building, a construction |
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

Two things fall out of that table. An attack interval is clamped from below and
a range is not, which is a difference in kind and not an accident. And a
property's inputs are exactly the recorded columns, so **a mechanism can be
checked before its arithmetic is known**: whether the table it fills produces
the number the game stored is one question, and whether the composition turns
that number into the right damage is another.

## The mirror, and what is dummy

The simulator carries the same three layers under the same names, and grows by
filling modules rather than by editing the kernel:

| Native | Simulator |
| --- | --- |
| `IModuleManager` + `FightModule` | the module registry, one entry per system, each declaring the layout fields it claims |
| `DataSet` over a shared description | the two overlays, per actor and per skill, each entry tagged with the mechanism that wrote it |
| `BuffManager` aggregates | the buff channel, summed per actor |
| `FightProperty` | a derived value read through `stat(...)`, never a direct field read |
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

- **How a value composes, and what order two channels apply in.** A rate is
  settled: `scripts/officer-composition.mcscript` measured
  `base × (1 + Σ add − Σ reduce)` within one channel, truncated toward zero,
  and [`officer_effects.md`](../../rules/officer_effects.md) records it. That
  capture put both corrections in one channel and both were rates, so a Float
  entry beside a FloatRate at one index, and the order the unit, skill and buff
  channels apply in, are still nobody's measurement — `data.rs` refuses each
  rather than extending the rule to it.
- **The order the modules are driven in, and the order of work inside one
  advance.** `FightCoreSystem.Update` calls `TeamUpdate` then `GroupUpdate`, and
  `PreCalculate` exists beside `Update`, but a body's call order is not in the
  index. It closes by measurement against a recording.
- **Which enum index is which field.** The `MechDataChange*` and
  `SkillDataChange*` names are known and so are the recorded field names; the
  numeric indices behind them are not read yet.
- **What `IsStepFinish` decides.** Every module has one, and the terminal
  condition a fight ends by is currently the simulator's own.
