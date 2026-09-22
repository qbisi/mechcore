# Unit levels

Build 1.11.1.3.2259, seed 1787720817. `stats.mcscript` records the control
and the three layouts here. It needs the game; `regressions.mcscript` runs
all four offline and pins both native hash layers.

| Layout | What it separates | Damage | Life |
| --- | --- | ---: | ---: |
| `../regression/marksman-vs-arclight.yaml` | unchanged level-one control | 2329 | 1622 |
| `marksman-2.yaml` | level two from no scaling | 4658 | 3244 |
| `marksman-3.yaml` | linear ratings from doubling each level | 6987 | 4866 |
| `marksman-2-rate.yaml` | rating in the base from a rate in the skill overlay | 6055 | 3244 |

Range remains 140 m, speed 8 m/s and the first interval 55 ticks. The bare
level-two unit has no dynamic modifiers; the officer case has only the
skill's +0.3 damage rate (raw 1288490188). The level is not added to it.
These observations distinguish the channels, not just the final damage.

The base formula and its nine serialized ratings are in
[unit_levels.md](../../docs/rules/unit_levels.md). The tests of `LevelEffects`
also check the highest table row, unchanged range/speed/interval, a zero base,
and refusal of levels outside the table. These are arithmetic and boundary
checks; the native fights cover levels one through three.
