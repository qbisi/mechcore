# Releasing contraptions

How many contraptions a side may release in one round, and which of them a
fight leaves standing into the next. What a contraption is
and where one may stand are [layout.md](../spec/document/layout.md)'s; what one
costs is [`config/economy.yaml`](../../config/economy.yaml)'s.

## Eight a round

A side may release eight contraptions in a round. `ContraptionManager` keeps
two counts: `BuyCount`, which its constructor sets to 8, and `RemainCount`,
what the round has left. As each deployment opens,
`ContraptionSystem.OnEnterDeployment` restores every player's `RemainCount` to
its `BuyCount`. Releasing a contraption spends one, and undoing the release
gives it back.

`IsReadyToRelease` refuses a release once `RemainCount` is spent, after it has
checked that the side holds the contraption and can pay for it. A state does not
write what is left: every round opens with eight, as it opens with its
purchases.

**A construction spends the same count.** Releasing a construction spends one
of the eight as a contraption does, and undoing it gives it back. No standard
decision releases a construction: [constructions.md](constructions.md) says
what a release beyond the opening's needs is not established, and the document
format has no such decision.

**Only an officer raises it.** `ChangeBuyCount` adds an officer's
`contraptionBuyCount` to both counts, and takes it away again with the officer.
No officer a standard match deals carries one, so the count is eight in every
round of a standard match.

## Three kinds, three mechanisms

A contraption is one of three kinds, and each is made by a module of its own
when the fight is built: a shield by `AdvancedEnergyShieldSystem`, the module
that also holds a Shield Airdrop's shield; an interceptor by `InterceptSystem`;
a missile by `MineSystem`. A side that releases one is fought only once the
module of its kind is, and a refusal names the kind with its module.

## An interceptor

An interceptor is a building of its side: `InterceptSystem` builds it through
`FightController.CreateFightBuilding` at its placement's centre, with the
row's life, a box and an RVO radius of its `pathRadius`, and its
`pathfindingColliderPriority`. Units may target it as any building, and it is an
obstacle to both sides, its own included: unlike a construction, it does not let
its own side through. [`config/contraptions.yaml`](../../config/contraptions.yaml)
holds its numbers.

**What it reaches.** A projectile its skill marks `canBeIntercept`, aimed at the
other side, is among an interceptor's once it has moved to within the reach:
at least `radiusRangeMin` and under `radiusRangeMax`, measured in three
dimensions from the interceptor's centre on the ground. It checks this after
each move, leaves an interceptor whose reach it has left, and joins in the order
it came into reach. A projectile still climbing to its flight height is among
none.

**How it intercepts.** An interceptor updates after the tick's projectiles. Idle,
it locks the nearest projectile in its reach whose life the attack already
locked on it does not cover, the first of equals, and prepares: it draws
whether the attack will hit from its side's stream, `Next(1000)` against the
row's probability in thousandths, even when that is a certainty. After
`prepareTime` it attacks: a hit takes its attack off the projectile's life,
and a projectile with no life left is removed at once as intercepted. It then
resets for `interval` less `prepareTime`, which for the row is no time at all,
and is idle again; one lock and one attack take three ticks. A projectile that
leaves its reach or lands takes the lock with it.

**Its attack falls and comes back.** The attack starts each fight at `attackNum`.
Every hit takes `attackNum × decline` off it, down to `attackNum × lowerLimit`,
each product truncated to whole points. An interceptor idle with nothing to lock
cools for `coolingTime` and then gives back `attackNum × rise` every
`riseInterval`, up to `attackNum`; locking anything ends the cooling.

**When it falls.** It intercepts nothing more, and its `building_destroyed` is
read among the tick's deaths and falls in the order they came, as every
building's and unit's is. It is no construction: `CreateFightBuilding` makes it
a plain `FightCrystal`, and `SkillAttackState.CheckAttackable` rejects a dead
attack target outright only of the `FightConstruction` class. So a unit
attacking it when it falls goes on to `FightSkill.CheckAttackable`, as for a
fallen tower or a dead unit, and searches again; a skill that cannot switch
quickly finds its target changed and finishes the attack, its weapons naming
what the search found while it cools. A fallen block of a construction ends
the attack without a search, and the weapons go on naming the block.

## A missile

A missile is `FightLandMine`: it stands where it was released and is not a
building, so nothing targets it and it takes no part in movement.
[`config/contraptions.yaml`](../../config/contraptions.yaml) holds its numbers
and the buff its hit writes.

**When it fires.** `MineSystem` updates before `FightCoreSystem`, so on every
tick, before any unit has moved, each side's missiles ask whether an enemy is
in range, the last released first (`TeamMineManager.Update` walks its list
from the end): the object of the other side whose
edge is nearest, a unit of either domain or a building that is not a tower,
its distance measured in two dimensions from the missile to its centre less
its radius and required to be under the row's `range`. The first of equals is
taken. A missile fires once, and is spent.

**What it fires.** A projectile of its own, which no unit owns: it leaves from
`mineFlyHeight`, 60 metres, above where the missile stands, at the row's
`moveSpeed`, locked on what set it off, with the row's `maxLife`, and an
interceptor may take it out of the air. Its reach (`FightProjectile.Init`'s
`moveRange`) is the row's trigger range and its target's radius, as a unit's
projectile's is its skill's range and its target's; no owner holds it to it.
Its release and its removal name no
source; its damage and the deaths it causes are credited to the projectile.
It meets shields as a unit's shot that crosses none does: `FightLandMine`
answers `CanCrossAdvancedEnergyShield` no, so the first enemy shield it flies
into takes it at the surface, and its splash, landing outside a shield, strikes
the shield it reaches and spares what the shield covers (below, under A
shield).

**What its hit does.** It deals the row's `damage`, raised by its side's
officers (below), to what it lands on and
splashes it over `damageRange`, as a unit's projectile does. Then
`FightLandMine.DispatchHitDamageEvent` writes the row's buff on every unit it
struck that still stands, after the damage and before the projectile is
removed; the buff is a slow in the buff channel, of divide 0, so it merges
only with one of its own row already running
([`battle_skill.md`](battle_skill.md) states the rule), and it expires on the
unit's update.
No unit made a kill a missile makes, so its experience goes to the pool
[`unit_experience.md`](unit_experience.md) shares out, on the missile's side.

## A shield

A shield is a sphere of its side standing on the ground at its placement's
centre, as wide as its row's `range` and holding its row's `energy`, both in
[`config/contraptions.yaml`](../../config/contraptions.yaml), the energy raised
by its side's officers (below). It is active from
the start, is no actor, and takes no part in movement. A side's shields are in
the order `CompareEnergyShield` sorts them at the fight's start: their centre's
x, then its z, then their energy and radius. A shield covers what its sphere
holds strictly inside, in three dimensions, while it has energy left.

A hit meant for a unit or a tower its side's shield covers lands on the
shield, since the build asks whether any `FightActor` is covered, and a tower
is one as a unit is:

- **A projectile** is tested where each move leaves it: the first enemy shield
  that holds it and did not hold it as it was made takes it, and it is removed
  where its way crosses the shield's surface, half a metre beyond it. One that
  lands on a covered unit without having entered a shield, and has no splash,
  is taken by the shield covering the unit; neither happens when the one who
  fired it stands inside that shield.
- **A blow or a beam** at a covered unit lands on the shield, unless the
  attacker stands inside it.
- **A splash** that lands outside a shield leaves the units it covers alone,
  and the shield takes the hit if the splash reaches it in the plane; one that
  lands inside a shield reaches the units inside. A splash that lands on a
  unit no shield covers, beside a shield it reaches, strikes both.
- A skill whose hits cross shields (`canCrossAdvancedShield`: a Crawler's, a
  Rhino's) passes every shield, and a unit whose attack does not, aiming at a
  covered unit or tower, fires at the shield: it stops once the point of the
  shield's surface on its way to its target is within its range, and its
  weapons name no target while it does, cooling included. It keeps the shield
  it last found until its skill searches again, one broken since included,
  and goes on attacking it, measured to where its surface stood: a Wasp, which
  checks once a blow, stands attacking a shield that broke after its last
  check, its lock out of its reach, until its next. Each weapon of a grouped
  unit searches its own shield, with its own range, and keeps the one it last
  found until it searches again itself, one broken since included.

A Shield Airdrop's shield is the same object, made where the skill lands;
[`battle_skill.md`](battle_skill.md) states when.

**A shield a unit carries is the same object again, with its unit as its
owner.** A Barrier on a huge ground unit makes one of the item's radius and
energy, among its side's shields at the fight's start, where the unit stands.
It stands where its owner stands, moved as the owner moves, and once the owner
dies it stays where it last stood, still naming it. A hit that empties a shield
with an owner deactivates it rather than destroying it: it stays on the board,
recorded inactive with no energy, shields nothing, and no `shield_destroyed`
is written. The Barrier's shield neither recovers nor refills.

A shield takes a hit's damage up to the energy it has left, and a hit that
empties it destroys it for the rest of the fight: the excess goes nowhere. The
recording names the shield as the target of that `damage`, and records none
for a hit that takes nothing (a Disintegration wave's), and removes a
projectile whose hit took energy from a shield `absorbed_by` the last shield it
took energy from, whether the projectile crossed that shield's surface or its
splash reached it from outside.
`shield_destroyed` comes after everything else the tick did. A skill that broke
it still aims at it for the rest of that tick and loses its lock at its next
check. A shield never regains energy within a fight.

## What an officer adds

Two officers raise a side's contraptions rather than its units. Advanced Shield
Device adds its `energy_shield_rate` of `+0.4` to the side's shield, and
Advanced Missile Device its `land_mine_rate` of `+2` to the side's missile. A
side holds one contraption of each kind, which every placement of the kind
reads, so the rate reaches each of them whether it was placed before the
officer was taken or after. Two officers' rates sum.

The shield's energy and the missile's damage are then the row's number times
`1 + rate` in `FPoint`, cut to an integer. The shield's `+0.4` is stored a hair
under it, so a shield of 40000 holds 55999; the missile's `+2` is exact, and
its 5000 becomes 15000.

## What a fight leaves

A contraption stands into the next round unless the fight ends it, and each
kind ends its own way:

- **A shield** stands until the fight destroys its shield.
- **An interceptor** is a building of its side, and stands until the fight
  destroys that building.
- **A missile fires once.** When an enemy comes within its trigger range it
  launches one missile, a projectile no unit owns, from where it stands, and is
  spent. A missile that fired is gone from the next round, and one that did not
  fire stands into it.

## Evidence

### Recorded

- An interceptor takes projectiles out of the air as the section above says:
  its reach, its lock, one draw a lock from its side's stream, the hit on a
  projectile's life, its attack falling with hits and rising while idle, and
  its standing as an obstacle to both sides:
  `tests/interceptor/fights/stormcallers.yaml`.
- A missile fires once, at tick one when an enemy is in range, at the enemy
  whose edge is nearest, air or ground, or a building that is not a tower; its
  projectile leaves from 60 metres above it, lands for its damage and splash,
  and writes its slow on every unit it struck that still stands:
  `tests/missile/fights/crawlers.yaml`, `tests/missile/fights/rhino-slowed.yaml`,
  `tests/missile/fights/wasps.yaml`, `tests/missile/fights/turret.yaml`.
- Interceptors take missiles' projectiles out of the air:
  `tests/interceptor/fights/missiles.yaml`.
- Two missiles of one side firing on one tick fire the last released first:
  `tests/missile/fights/two-at-once.yaml`.
- A missile's projectile that flies into the shield over its target is taken
  at the surface, the shield losing the 5000 and the Rhino under it nothing:
  `tests/missile/fights/into-shield.yaml`. One that lands on a Rhino beside a
  shield strikes the Rhino and the shield its splash reaches, and spares the
  Marksman the shield covers: `tests/missile/fights/splash-beside-shield.yaml`.
- A fallen interceptor intercepts nothing more, falls among the tick's deaths,
  and does not stand into the next round:
  `tests/interceptor/fights/interceptor-falls.yaml`.
- A unit whose shot fells the interceptor it attacks searches again, and
  cools naming what the search found:
  `tests/interceptor/fights/fortress-fells-interceptor.yaml`.
- An officer's rate raises a side's shield's energy and its missile's damage,
  cut to an integer: `tests/shield/fights/advanced-shield-device.yaml` against
  `tests/shield/fights/projectiles.yaml`, and
  `tests/missile/fights/advanced-missile-device.yaml`.
- A shield takes projectiles at its surface, a splashing one's included, and
  its side's covered unit takes nothing; units whose target it covers stop at
  its surface: `tests/shield/fights/projectiles.yaml`.
- Blows at a covered unit land on the shield until it breaks, and the unit
  that broke it loses its lock the tick after:
  `tests/shield/fights/blows-break-it.yaml`.
- A beam at a covered unit lands on the shield: `tests/shield/fights/beam.yaml`.
- Wasps attacking a shield that breaks between their checks stand attacking
  it until the next: `tests/corpus/fights/201370830-r5.yaml`, ticks 203 to
  218.
- A grouped unit's weapons each fire at the shield covering their own lock,
  and name no target while they do, cooling included, until they search
  again: `tests/shield/fights/grouped-weapons.yaml`.
- A skill that crosses shields strikes the covered unit:
  `tests/shield/fights/crawlers-cross.yaml`.
- A unit locking a covered tower fires at the shield, its weapon naming no
  target, and the shield takes every shot:
  `tests/shield/fights/tower-covered.yaml`.
- The hit that empties a shield is absorbed whole, and the shield is gone
  after it: `tests/shield/fights/projectiles-break-it.yaml`.
- A splash that lands on an uncovered unit beside a shield strikes both, and
  its projectile is removed `absorbed_by` the shield it never entered:
  `tests/shield/fights/splash-beside.yaml`.

- A shield a unit carries follows it, shields it from shots from outside,
  is deactivated when emptied and stays where its owner fell:
  `tests/shield/fights/barrier-wasps.yaml`; blows from inside it reach its
  owner: `tests/shield/fights/barrier-crawlers.yaml`.

### Replayed

- A shield, an interceptor and a missile each stand into the next round as the
  section above says: `scripts/corpus/match-replays.py` with `--recordings` converts every
  recorded round of the corpus to its fight document, which reads a missile as
  fired when a projectile no unit owns of its side is first recorded beside it,
  and compares which contraptions it keeps with the next state in the match
  document. The corpus holds each kind both kept and gone.

### Read

- Each kind is made by its own module: `CRC_EnergyShield.Perform` calls
  `AdvancedEnergyShieldSystem.Create`, `CRC_Interceptor.Perform` calls
  `InterceptSystem.DoCreateFightInterceptor`, and `CRC_Mine.Perform` calls
  `MineSystem.Create`.
- The count starts at eight and is restored as each round opens:
  `ContraptionManager.ContraptionManager`, `ContraptionManager.BuyCount`,
  `ContraptionManager.RemainCount`, `ContraptionSystem.OnEnterDeployment`.
- A release is refused once the count is spent:
  `ContraptionManager.IsReadyToRelease`, `ContraptionManager.CanRelease`.
- Releasing a contraption or a construction spends one, and undoing it gives it
  back: `ContraptionManager.ReduceContraptionCount`,
  `ContraptionManager.AddContraptionCount`, `PAP_ReleaseContraption.Perform`,
  `PAP_ReleaseConstruction.Perform`.
- An officer raises both counts: `ContraptionManager.ChangeBuyCount`,
  `SystemOfficerController.ChangeContraptionBuyCount`,
  `OfficerData.contraptionBuyCount`.
- An officer's rate is added onto the side's one contraption of the kind,
  and its number read times one plus it:
  `SystemOfficerController.ChangeConstraptionEnergyShield`,
  `SystemOfficerController.ChangeConstraptionLandMine`,
  `ContraptionManager.ChangEnergyShieldValue`,
  `ContraptionManager.ChangeLandMineValue`,
  `EnergyShieldContraption.ChangeEnergy`,
  `EnergyShieldContraption.GetAdvancedEnergyShieldValue`,
  `LandMineContraption.ChangeDamage`, `LandMineContraption.GetDamage`.
- A missile fires when an enemy comes within its trigger range, as a
  projectile made with the missile as its data source:
  `TeamMineManager.TryActiveMine`, `FightLandMine.GetTriggerRange`,
  `MineSystem.ActiveMine`, `ProjectileSystem.Create`.
- A missile is created where it is released, and fires when it finds a
  target: `CRC_Mine.Perform`, `MineSystem.Create`, `TeamMineManager.Update`,
  `TeamMineManager.TryActiveMine`, `FightUtility.CalculateDistance2D`,
  `FightCrystal.IsTower`, `MineSystem.ActiveMine`.
- Its projectile is its own: `FightLandMine.IsLockTarget`,
  `FightLandMine.GetEffectTargetType`, `FightLandMine.CanAttackConstruction`,
  and it leaves from `Config.mineFlyHeight` through
  `IFightSetting.GetMineFlyHeight`.
- Its hit writes its buff: `FightLandMine.DispatchHitDamageEvent`,
  `BuffSystem.AddBuff`.
- It updates before the units: `FightController.AddModules` adds `MineSystem`
  before `FightCoreSystem`.
- A shield is a sphere of its side on the ground, active from its creation,
  as wide as its row's range and holding its energy:
  `AdvancedEnergyShieldSystem.Create`, `FightEnergyShield.Active`,
  `EnergyShieldContraption.GetAdvancedEnergyShieldRadius`,
  `EnergyShieldContraption.GetAdvancedEnergyShieldValue`.
- A projectile is tested against enemy shields after each move, as a point,
  past those that held it as it was made: `FightProjectile.Update`,
  `FightProjectile.CheckIsHitEnergyShield`,
  `ProjectileController.IsProjectileHitEnergyShield`,
  `FightCalculator.IsInEnergyShield`,
  `FightUtility.GetAttackPositionOnEnergyShieldOuter`,
  `FightUtility.CalculatePointOnCricle`.
- A blow at a covered unit lands on its shield, which the skill's search
  hands it in place of an attack target: `DamageEffect.Perform`,
  `FightSkill.GetTargetEnergyShield`, `FightSkill.IsActorProtectedByEnergyShield`,
  `SkillSearchTargetController.SearchTargetShield`,
  `SkillAttackRangeChecker.IsAttackTargetInAttackRange`.
- Whether a shield covers something is asked of any `FightActor`, a tower
  included: `FightCalculator.IsActorInEnergyShield(FightActor, FightEnergyShield)`,
  called by `FightSkill.GetTargetEnergyShield(FightActor)`,
  `DamageEffect.Perform` and `FightSkill.IsTowerAttackable`.
- A splash spares what a shield covers unless it lands inside it:
  `DamagePerformer.ProcessAdvancedEnergyShieldEffect`,
  `DamagePerformer.PerformRangeEffect`.
- A hit takes the energy it can, and one that empties a shield destroys it:
  `DamagePerformer.PerformHitAdvancedEndergyShieldEffect`,
  `FightEnergyShield.ReduceEnergy`, `AdvancedEnergyShieldSystem.Destroy`.
- A contraption's shield regains nothing within a fight:
  `GroupAdvancedEnergyShieldManager.Update`.
- An interceptor is a building of its side: `FightInterceptor.Building`,
  `FightInterceptor.OnDestroy`.
- It is built with the row's life, its `pathRadius` for a radius and its
  collider priority: `InterceptSystem.DoCreateFightInterceptor`,
  `FightController.CreateFightBuilding`, `InterceptContraption.GetBuildingData`.
- It is a plain `FightCrystal`, which `FightController.CreateFightBuilding`
  constructs, where a construction's block is a `FightConstruction` from
  `FightController.CreateFightConstruction`; an attack on a dead one fails
  only for the latter: `SkillAttackState.CheckAttackable`,
  `FightSkill.CheckAttackable`.
- A projectile that can be intercepted and is aimed at the other side joins the
  interceptors it is in reach of after it moves: `FightProjectile.Update`,
  `ProjectileController.UpdateIsInInterceptSources`,
  `ProjectileController.GetInInterceptSources`, `FightProjectile.GetCanIntercept`,
  `InterceptEffectBase.IsInSourceRange`, `InterceptEffectBase.AddProjectileController`,
  `InterceptEffectBase.RemoveProjectileController`.
- The lock, the draw, the attack and the reset: `InterceptEffectBase.Update`,
  `InterceptEffectBase.GetTarget`, `InterceptEffectBase.GetAllAttackNum`,
  `InterceptEffectBase.EnterPrepare`, `GRRandom.IsProbabilityPass`,
  `InterceptEffectBase.TryAttack`, `FightProjectile.ReduceLife`.
- The attack's fall and rise: `InterceptEffectBase.Init`,
  `InterceptEffectBase.DropAtk`, `InterceptEffectBase.RecorveAtk`,
  `InterceptEffectBase.UpdateForIdleCooling`.
- It updates after the projectiles: `FightController.AddModules` adds
  `ProjectileSystem` and then `InterceptSystem`.

- A carried shield: `AdvancedEnergyShieldProvider` is a
  `SingleEffectProvider` of `IAdvancedEnergyShieldSource`, which
  `AdvancedEnergyShieldEquipment.GetRadius` and `GetShieldValue` answer from
  its row, and `GetRecoverTime` and `GetEnergyChangeValue` with zero;
  `DamagePerformer.PerformHitAdvancedEndergyShieldEffect` calls
  `AdvancedEnergyShieldSystem.DeactiveEnergyShield` for an emptied shield
  with an owner and `AdvancedEnergyShieldSystem.Destroy` for one without.

### Not established

- **A projectile climbing to its height.** A Farseer's shot took no hit from an
  interceptor in reach until it flew, in one recording this build reproduces up
  to a later tick at which the Farseer's own projectile parts from the game
  without any interceptor; no fight pins it.
- **Two interceptors of one side.** Which of them updates first is taken as the
  order the layout releases them in, and no recording has two.
- **A tower's loss while an interceptor stands.** Whether the buff reaches it
  is not read, and the simulator refuses the fight when it happens.
- **A row whose hit can miss.** The one interceptor's probability is a
  certainty, and a row that is not is refused by name.
- **A missile, or a battle skill, in a fight with a shield.** A missile's
  projectile and a falling sub-effect each have shield branches no fight
  pins; the simulator refuses both.
- **A splashing blow or beam at a covered unit.** Its damage point on the
  shield's surface is read but no fight pins it; the simulator refuses it.
- **A projectile fired from inside an enemy shield,** or across two shields.
  The exemption for the shields that held it as it was made is read, and no
  fight pins it.
- **An officer's rate on a shield's energy** (`ChangEnergyShieldValue`). The
  simulator refuses the officer.

- **Where 60 metres comes from.** `Config.mineFlyHeight` is private and the
  export does not carry it; the height is what the recordings show.
- **Which call spends a fired missile.** `TeamMineManager.Remove` takes a
  missile off its side's list and has no caller the index resolves, so the
  step from firing to being gone is read from the corpus, not the build.
- **A missile cleared without firing.** `MineSystem.ClearLandMine` destroys a
  side's missiles without a projectile; no fight of the corpus does it, and a
  reader of a recording would take such a missile as standing.

- **The cap in a recording.** No replay of this version's corpus releases more
  than seven contraptions in a round, counting releases later undone, so no
  replay reaches the eighth, let alone a refused ninth.
- **A construction released alongside contraptions.** No replay releases a
  construction.
