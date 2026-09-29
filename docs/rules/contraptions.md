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
- A fallen interceptor intercepts nothing more, falls among the tick's deaths,
  and does not stand into the next round:
  `tests/interceptor/fights/interceptor-falls.yaml`.

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
