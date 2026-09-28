# Construction fixtures

What a `constructions` entry becomes in a fight: how many objects it arrives
as, where they stand, whom they obstruct, and what attacks them.
[`constructions.md`](../../docs/rules/constructions.md) states what these
fights settled and what they leave open. The two probes that are not fights,
`construction-shape.yaml` and `wall-line-width.yaml`, are in
[`../../layouts/`](../../layouts/README.md).

A recording names a building row's `BuildingType` and not which construction
released it; `show --view buildings` matches the rows back to the placements
the layout declares.

The rules on how a unit meets a block and how an attack on one ends were read
from each skill's state and attack phase, tick by tick:

```sh
scripts/record-fights.py --instrument target_refs --out /tmp/mechcore/construction/skill-state tests/construction/fights/*.yaml
```
