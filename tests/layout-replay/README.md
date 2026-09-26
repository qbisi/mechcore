# Layout replays

Does a layout fought without a scene fight as the Training Ground fights it?
`mechcore replay convert <layout.yaml> <replay.grbr>` writes a layout as a
replay whose round is the layout, and `game.record_layout` records the game
fighting that round headlessly
([layout-replay.md](../../docs/spec/document/layout-replay.md)). This
directory holds the evidence that the two recordings are the same fight.

Most of it is elsewhere. Every fight pinned in another directory's
`regressions.mcscript` was recorded in the Training Ground, and each one
recorded headlessly from its layout hashes the same, physics and content:
the units' standard layouts, constructions, equipment, officers, unit
levels, technologies and tower strengthening. The fields no pinned fight
holds are here, one layout each, and
[`equivalence.mcscript`](equivalence.mcscript) records every one both ways
with one seed and compares the two recordings field by field:

| Layout | Field |
| --- | --- |
| `experience.yaml` | experience within a level |
| `contraptions.yaml` | contraptions, with gaps in their indices |
| `blueprints.yaml` | both enhancement chains |
| `magnetic-barrier.yaml` | the construction no corpus snapshot holds |
| `round-3.yaml` | a later round, with a settled flank unit |
| [`../skill-order/orbital-first.yaml`](../skill-order/orbital-first.yaml) | two battle skills released in order |
| `energy-tower.yaml` | Energy Tower skills |
| `travelling.yaml` | a travelling unit |
| `travelling-fitted.yaml` | travelling units upgraded, fitted and turned |
| `delivered-squad.yaml` | a squad an officer hands out as the round opens |
| `airdrop-shields.yaml` | Shield Airdrops left standing |
| `oil.yaml` | an oil area left standing |

Each pair is equal. Two parts are equal only as far as these layouts go.

**A travelling unit is bought.** A unit the round opens with may not move
from round 2 on, and moving onto a flank from another region is what makes a
unit travel, so the replay buys it during the round, where the deployment
area is free, and moves it. `travelling.yaml` failed until it did: the game
refused the move of a unit the snapshot held.

**A delivered squad is the layout's unit.** The snapshot is the side before
its round opens, so an officer whose schedule hands out a squad that round
hands it out again. `delivered-squad.yaml` recorded an extra level 3
Marksman until the replay let the delivery be the layout's level 4 one and
upgraded and moved it.

**An oil area's grid is not measured here.** The Training Ground rebuilds an
oil line only from control points a release produced, and the corpus of this
version holds no oil area an interception clipped, so `oil.yaml` is whole.
A clipped grid is written the way the replay reader decodes it, and a unit
test holds the two to each other; no fight has checked it.
