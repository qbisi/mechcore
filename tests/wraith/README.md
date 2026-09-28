# Wraith fixtures

Every layout here exists to measure **how a Wraith's slots choose their
targets**. A Wraith's core is a `SkillGroup` of four `FightSkill`s, one per
weapon slot, each running the same `SkillAttackableChecker.Check` a unit's
skill runs, with a `GroupedSkillAttackBehaviour` over them; what a slot's
search does with the units its siblings already hold is what these fixtures
separate.

| Fixture | What it separates | Reading |
| --- | --- | --- |
| [`../regression/wraith-group-attack.yaml`](../regression/wraith-group-attack.yaml) | a slot re-searching while its siblings hold three of the units in reach | the checker channel's per-slot calls |
| `two-targets.yaml` | two units for four slots: whether a slot doubles up, and on which | the slots' first allocation |

`slots.mcscript` records each layout once with two channels:
`skill_attackable_checker`, which holds every `Check` call with the slot that
made it and the slot's lock before and after it, and `group_slots`, which holds
each slot's lock, attack target and state on every tick. The
regression layout is recorded under the seed the manifest pins it with, so
the recording is the pinned fight.

The Wraith's skill (`config/units/wraith.yaml`, row 18001 of `level0`'s
`MechSkillGroupData` by `scripts/extract-skills.py`): four weapons, one
skill each, weapon mode 1, range 60, interval 1.6 s ± 0.2, prepare 0.4,
`canAttackSameTarget` true, `isEvenlyAllocated` false, quick switch on.

`regressions.mcscript` simulates both layouts without the game and pins their
native hashes: 647 ticks for the regression fight and 96
for two targets, seed 1787857041. The two-target capture assigns u3, u2, u3,
u3; sharing is not even allocation. In the regression capture, u30's fourth
slot switches to u21 at tick 131, excluding u25 held by its sibling. At tick
205, u29's fourth slot takes u55 at an edge distance of 60.9585 m: a main child
skill has its parent's range plus 10 m. The core keeps 60 m.

The original simulator matched the regression fight's positions, life and
events while its mech lock differed on 60 ticks beginning at tick 206. A check
of those alone would miss it, which is why the hash pinned here covers every
field.

The optional checker replay reads the recording's checker channel and restores each call's
before-targets on the slot the call names, on a shadow skill at the kernel's checker site, then restores
the simulated skill before execution continues. It compares the return value,
lock and attack target on every grouped call: 4,188 in the regression capture
and 344 in two targets. It requires the recordings `slots.mcscript` makes where
the game runs; with them in place, run:

```sh
cargo test -p mechcore-simulation grouped_checker_matches_every_captured_call -- --ignored --nocapture
```

`slots.mcscript` also records the Wraith's M2, M3 and M6 standard fights, and
a second ignored test reads every recording it makes. It simulates each fight
and compares, tick by tick, every slot's lock, attack target and state with
the recording's `group_slots`, and the unit's lock and motion beside them, and
names the first tick a fight parts on:

```sh
cargo test -p mechcore-simulation grouped_slots_match_every_recorded_tick -- --ignored --nocapture
```

The two fixtures here and every standard fight agree on every tick.

The replay requires the recording to carry the checker channel and rejects
missing calls. Observed targets are never used to advance the simulation checked by the
gameless regressions. The ordinary Rust tests retain the distinguishing
allocation and child-range cases without needing recordings.
