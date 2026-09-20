# Attack interval fixtures

Every layout here exists to measure **the random stagger on a unit's first
attack interval**, and nothing else. The rule they measure is
[`combat.md`](../../../docs/rules/combat.md#the-stored-interval-carries-a-per-unit-stagger)'s:

```text
stored interval = description + GRRandom(round + team) × 4444).next_in_range(offset)
```

one draw per **member**, taken in the order the recording numbers the
deployment — ascending world `z`, then `x` — in that member's own
`interval_offset`, and no draw at all when the offset is zero.

They are read, not fought. What each one answers is
`derived.attack_interval`, which a recording has carried since MCFR 0.5.0, at
any tick: the number is stored once and never moves. Nothing here needs the
fight to reach any particular state, so the layouts are arranged for
legibility rather than for a battle.

| Fixture | What it separates | Reading |
| --- | --- | --- |
| `stagger-singles.yaml` | the order and the per-unit range | 55, 23, 56, 32 by ascending `z` |
| `stagger-three-marksmen.yaml` | signed draw, one type held constant | 55, **65**, 56 against a description of 62 |
| `stagger-mustang-then-marksman.yaml` | a draw per member against a draw per formation | Marksman **72**, where per formation would be 55 |

**Each one is predicted before it is read.** `GRRandom` is implemented in
`crates/simulation/src/random.rs` and its first draws are pinned by a test
there, so every number above is recomputed offline from the seed and compared
with what the game stored. A fixture whose reading cannot be recomputed that
way does not belong here.

The three fixtures do different work and none of them is redundant. The
singles fix the walk order with nothing to interleave; the three Marksmen hold
the type constant and catch the sign, which a single fixture would have missed
because one of its three draws is positive; the Mustang stands in front of a
Marksman precisely so that the two consumption rules answer different numbers,
72 against 55.

The Rhino appears in each of them as red's unit. Its offset is zero, so it
reads its description exactly, and red draws from its own stream.

## What is not measured here

How many draws the **rest of a fight** consumes from the same stream. This
directory covers the stagger at deployment; `combat.md`'s first rule names the
stream itself, and what else takes from it during a fight is nobody's
measurement yet.
