# Releasing contraptions

How many contraptions a side may release in one round. What a contraption is
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

## Evidence

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

### Not established

- **The cap in a recording.** No replay of this version's corpus releases more
  than seven contraptions in a round, counting releases later undone, so no
  replay reaches the eighth, let alone a refused ninth.
- **A construction released alongside contraptions.** No replay releases a
  construction.
