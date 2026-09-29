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

### Replayed

- A shield, an interceptor and a missile each stand into the next round as the
  section above says: `scripts/corpus/match-replays.py` with `--recordings` converts every
  recorded round of the corpus to its fight document, which reads a missile as
  fired when a projectile no unit owns of its side is first recorded beside it,
  and compares which contraptions it keeps with the next state in the match
  document. The corpus holds each kind both kept and gone.

### Read

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

### Not established

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
