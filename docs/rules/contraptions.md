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
building's and unit's is.

## A missile

A missile is `FightLandMine`: it stands where it was released and is not a
building, so nothing targets it and it takes no part in movement.
[`config/contraptions.yaml`](../../config/contraptions.yaml) holds its numbers
and the buff its hit writes.

**When it fires.** `MineSystem` updates before `FightCoreSystem`, so on every
tick, before any unit has moved, each side's missiles in the order they were
released ask whether an enemy is in range: the object of the other side whose
edge is nearest, a unit of either domain or a building that is not a tower,
its distance measured in two dimensions from the missile to its centre less
its radius and required to be under the row's `range`. The first of equals is
taken. A missile fires once, and is spent.

**What it fires.** A projectile of its own, which no unit owns: it leaves from
`mineFlyHeight`, 60 metres, above where the missile stands, at the row's
`moveSpeed`, locked on what set it off, with the row's `maxLife`, and an
interceptor may take it out of the air. Its release and its removal name no
source; its damage and the deaths it causes are credited to the projectile.

**What its hit does.** It deals the row's `damage` to what it lands on and
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
[`config/contraptions.yaml`](../../config/contraptions.yaml). It is active from
the start, is no actor, and takes no part in movement. A side's shields are in
the order `CompareEnergyShield` sorts them at the fight's start: their centre's
x, then its z, then their energy and radius. A shield covers what its sphere
holds strictly inside, in three dimensions, while it has energy left.

A hit meant for a unit its side's shield covers lands on the shield:

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
  lands inside a shield reaches the units inside.
- A skill whose hits cross shields (`canCrossAdvancedShield`: a Crawler's, a
  Rhino's) passes every shield, and a unit whose attack does not, aiming at a
  covered unit, fires at the shield: it stops once the point of the shield's
  surface on its way to its target is within its range, and its weapons name no
  target while it does.

A shield takes a hit's damage up to the energy it has left, and a hit that
empties it destroys it for the rest of the fight: the excess goes nowhere. The
recording names the shield as the target of that `damage`, and
`shield_destroyed` comes after everything else the tick did. A skill that broke
it still aims at it for the rest of that tick and loses its lock at its next
check. A shield never regains energy within a fight.

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
- A fallen interceptor intercepts nothing more, falls among the tick's deaths,
  and does not stand into the next round:
  `tests/interceptor/fights/interceptor-falls.yaml`.
- A shield takes projectiles at its surface, a splashing one's included, and
  its side's covered unit takes nothing; units whose target it covers stop at
  its surface: `tests/shield/fights/projectiles.yaml`.
- Blows at a covered unit land on the shield until it breaks, and the unit
  that broke it loses its lock the tick after:
  `tests/shield/fights/blows-break-it.yaml`.
- A beam at a covered unit lands on the shield: `tests/shield/fights/beam.yaml`.
- A skill that crosses shields strikes the covered unit:
  `tests/shield/fights/crawlers-cross.yaml`.
- The hit that empties a shield is absorbed whole, and the shield is gone
  after it: `tests/shield/fights/projectiles-break-it.yaml`.

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
- **Two missiles of one side.** Which asks first is taken as the order the
  layout releases them in, and no recording has two.
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
