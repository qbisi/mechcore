# Modifier fixtures

Every layout here exists to measure **how a correction composes with the
description it corrects**, and nothing else. The rule they are measuring
against is [`officer_effects.md`](../../../docs/rules/officer_effects.md)'s and
`crates/simulation/src/data.rs`'s:

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

The shape is the build's own. `DataSet` keeps three lists, and each one is
named for its own arithmetic: `List<AdditiveDataFloat>` sums, `List<DataInt>`
sums too — a unit's are `DataIntGroup`, clamped across the whole `Int32` range
— and `List<MultiplicativeDataFloat>` holds one accumulator reset to zero and
one reset to one, routing each entry by its sign. Each fixture here puts
exactly one clause of that formula on one unit.

**A fixture in this directory changes one thing.** Every one of them is the
same fight — one Marksman shooting one Rhino, the shooter at `(0, -50)` and
the Rhino at `(5, -55)` — and differs only in the `officers` line, or in the
one case of the range fixtures in the shooter being an Arclight. That is what
makes the difference between two of them attributable: the Rhino outlives every
one of these fights, so the reading is the life it has left of its 19297, and
nothing else in the layout can have moved it.

Two of them read the clock rather than the life: a movement officer does not
change what the Marksman does, it changes when the Rhino arrives, so the
reading is the tick the fight ends at.

The positions are load-bearing in the ordinary way: `x ≡ 5, y ≡ 5 (mod 10)` for
a `30 x 30` footprint, and far enough from `y = -300` that the Marksman's
Normal selector takes the Rhino rather than a building.

| Fixture | Clause it measures | What the game answered |
| --- | --- | ---: |
| `officer-composition-none.yaml` | none: the control | 14639 left |
| `officer-composition-once.yaml` | one enhance, `+0.3` damage | 13243 |
| `officer-composition-twice.yaml` | two enhances sum | 11845 |
| `officer-life-rate.yaml` | an enhance in the unit channel, on life | 20428 of 25086 |
| `officer-impair-once.yaml` | one impair, `−0.11` | see below |
| `officer-impair-twice.yaml` | **two impairs compound, not sum** | see below |
| `officer-range-none.yaml` | the control for the value fixture | 16377 left, 137 ticks |
| `officer-range-value.yaml` | a value is added in the number's own unit | 16961 left |
| `officer-speed-once.yaml` | a plain integer, in `DataSet.intDatas` | ends at tick 104 |
| `officer-speed-twice.yaml` | **two integers sum, rather than the larger winning** | ends at tick 92 |

`officer-impair-once.yaml` cannot separate the two rules — one impairment is
`0.89` either way — which is what makes it this experiment's control.
`officer-impair-twice.yaml` is the measurement: two impairments of `0.11`
leave `0.89 × 0.89 = 0.7921` of the number under the rule above, and
`1 − 0.22 = 0.78` if impairments summed the way enhancements do. On a Rhino's
19297 that is **15285 against 15051**, and the fight carries the difference to
the end.

## The scripts beside them

| Script | Needs the game | What it does |
| --- | --- | --- |
| `composition.mcscript` | yes | records the control and the two enhancement fixtures |
| `impairment.mcscript` | yes | records the two impairment fixtures |
| `value.mcscript` | yes | records the range control and the value fixture |
| `speed.mcscript` | yes | records the two movement fixtures |
| `regressions.mcscript` | **no** | replays every fixture through the simulator and asserts the hash the game produced |

The three recording scripts are this directory's experiments: each one writes
its expected numbers down before the game is started, records, and asserts
both halves against the recording — what the build stored, and what it then
computed. `regressions.mcscript` is the other side of the same table, and it
is what CI runs: the simulator has to reproduce each recording tick for tick
from the layout and the seed alone, on a machine that has no game at all.

These fixtures are deliberately **not** in `tests/mcfr-regressions.yaml`. The
table that holds them lives here, beside them and beside the scripts that
produced them, so that a fixture, its measurement and its regression are one
thing to read and one thing to move.

**Record before you believe.** A recording under `work/` is not tracked; what
is tracked is what it decided.
