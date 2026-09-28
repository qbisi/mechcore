# Equipment corrections

Seed 1787720817. Each fight is a fight document in `fights/`, and
`stats.mcscript` records the nine fights from them, then checks the native
unit and skill channels. It needs the game. CI verifies the fights without
it. The layouts use round one and level-one units.

| Fixture | What it separates | Ticks |
| --- | --- | ---: |
| control ([`../regression/fights/marksman-vs-arclight.yaml`](../regression/fights/marksman-vs-arclight.yaml)) | the unmodified baseline | 91 |
| heavy-armor | equipment life rate in the unit channel | 139 |
| heavy-armor-officer | summed life rates versus multiplied brackets | 139 |
| firepower | equipment damage rate in the skill channel | 83 |
| firepower-officer | summed damage rates versus multiplied brackets | 83 |
| laser-sights | range value in the skill channel | 91 |
| far-control | approaching from outside the original range | 165 |
| laser-sights-far | the increased range changes attack eligibility | 140 |
| two-items | two items on one formation, the second slot from Equipment Expansion | 83 |

The far pair starts 170 metres centre to centre, 153 edge to edge. The
near Laser Sights fight moves as the control does because the
target is already in range. Life is read from the first tick; damage and
range are also exposed by `show --view stats`.

CI verifies the eight fights in `fights/` and the control against the hashes
the game recorded; the simulator reproduces each tick for tick. The hash covers the
unit and skill `DataSet` aggregates, so an equipment written into the wrong
channel fails there even where no unit would move differently.

The rule is [equipment_effects.md](../../docs/rules/equipment_effects.md).
Recordings and extracted raw research artifacts are not tracked here.
