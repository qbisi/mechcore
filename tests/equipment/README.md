# Equipment corrections

Build 1.11.1.3.2259, seed 1787720817. `stats.mcscript` records the eight
fixtures, then checks the native unit and skill channels. It needs the
game and is only parsed by a claimant. `regressions.mcscript` is offline.
The layouts use round one and level-one units.

| Fixture | What it separates | Ticks |
| --- | --- | ---: |
| control (regression/marksman-vs-arclight.yaml) | the unmodified baseline | 91 |
| heavy-armor | equipment life rate in the unit channel | 139 |
| heavy-armor-officer | summed life rates versus multiplied brackets | 139 |
| firepower | equipment damage rate in the skill channel | 83 |
| firepower-officer | summed damage rates versus multiplied brackets | 83 |
| laser-sights | range value in the skill channel | 91 |
| far-control | approaching from outside the original range | 165 |
| laser-sights-far | the increased range changes attack eligibility | 140 |

The far pair starts 170 metres centre to centre, 153 edge to edge. The
near Laser Sights fight is the control's physics trajectory because the
target is already in range. Life is read from the first tick; damage and
range are also exposed by `fight stats`.

All eight physics hashes agree, covering 931 ticks. The two controls also
agree in content and are pinned. The six equipped fixtures are unpinned:
from tick one, Actor::snapshot omits the existing unit life-rate aggregate
or the whole skill modifier list. This is outside the equipment modifier
scope. No earlier hash is changed to accommodate the omission. The claim's
GitHub thread holds the first divergences and the keeper decision; these
fixtures remain the reproduction independently of that thread.

The rule is [equipment_effects.md](../../docs/rules/equipment_effects.md).
Recordings and extracted raw research artifacts are not tracked here.
