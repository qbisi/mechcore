# Unit levels

Seed 1787720817. The control and the three fights in `fights/` are fight
documents, and CI verifies them without the game, against the native hash of
each.

| Fight | What it separates | Damage | Life |
| --- | --- | ---: | ---: |
| [`../regression/fights/marksman-vs-arclight.yaml`](../regression/fights/marksman-vs-arclight.yaml) | unchanged level-one control | 2329 | 1622 |
| `fights/marksman-2.yaml` | level two from no scaling | 4658 | 3244 |
| `fights/marksman-3.yaml` | linear ratings from doubling each level | 6987 | 4866 |
| `fights/marksman-2-rate.yaml` | the level as its own multiplier from a rate in the skill overlay | 6055 | 3244 |

Range remains 140 m, speed 8 m/s and the first interval 55 ticks. The bare
level-two unit has no dynamic modifiers; the officer case has only the
skill's +0.3 damage rate (raw 1288490188). The level is not added to it.
These observations distinguish the channels, not just the final damage.

The rule, and why the level is its own multiplier and not a correction, are
in [unit_levels.md](../../docs/rules/unit_levels.md). `data.rs`'s tests check
the same numbers without the game: a level multiplies base life and damage
and nothing else, and it is applied before the overlays. A level outside one
to nine is refused by the layout, by name.
