# Modifier fixtures

Every layout here exists to measure **how a correction composes with the
description it corrects**, and nothing else. The rule they are measuring
against is [`officer_effects.md`](../../../docs/rules/officer_effects.md)'s and
`crates/simulation/src/data.rs`'s:

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

The shape is the build's own. `DataSet` keeps values in a
`List<AdditiveDataFloat>` whose `Refresh` sums them, and rates in a
`List<MultiplicativeDataFloat>` whose `Refresh` holds one accumulator reset to
zero and one reset to one, routing each entry by its sign. Each fixture here
puts exactly one clause of that formula on one unit.

**A fixture in this directory changes one thing.** Every one of them is the
same fight — one Marksman shooting one Rhino, the Marksman at `(0, -50)` and
the Rhino at `(5, -55)` — and differs only in the `officers` line. That is what
makes the difference between two of them attributable: the Rhino outlives every
one of these fights, so the reading is the life it has left of its 19297, and
nothing else in the layout can have moved it.

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

`officer-impair-once.yaml` cannot separate the two rules — one impairment is
`0.89` either way — which is what makes it this experiment's control.
`officer-impair-twice.yaml` is the measurement: two impairments of `0.11`
leave `0.89 × 0.89 = 0.7921` of the number under the rule above, and
`1 − 0.22 = 0.78` if impairments summed the way enhancements do. On a Rhino's
19297 that is **15285 against 15051**, and the fight carries the difference to
the end.

**Record before you believe.** Every fixture's expected numbers are written
into `scripts/officer-composition.mcscript` before the recording exists, and
the recordings' hashes live in `tests/mcfr-regressions.yaml` so the simulator
has to reproduce the game tick for tick rather than merely agree about the last
number.
