# Wraith fixtures

Every layout here exists to measure **how a Wraith's slots choose their
targets**. A Wraith's core is a `SkillGroup` of four `FightSkill`s, one per
weapon slot, each running the same `SkillAttackableChecker.Check` a unit's
skill runs, with a `GroupedSkillAttackBehaviour` over them; what a slot's
search does with the units its siblings already hold is what these fixtures
separate.

| Fixture | What it separates | Reading |
| --- | --- | --- |
| [`../regression/wraith-group-attack.yaml`](../regression/wraith-group-attack.yaml) | a slot re-searching while its siblings hold three of the units in reach | the checker sidecar's per-slot calls |
| `two-targets.yaml` | two units for four slots: whether a slot doubles up, and on which | the slots' first allocation |

`slots.mcscript` records each layout twice, once under
`skill_attackable_checker_v1`, which holds every `Check` call with the slot's
lock before and after it, and once under `target_refs_v1`, which holds every
other unit's lock and attack target and nothing for the Wraith's slots. The
regression layout is recorded under the seed the manifest pins it with, so
the recording is the pinned fight.

The Wraith's skill (`config/units/wraith.yaml`, row 18001 of `level0`'s
`MechSkillGroupData` by `scripts/extract-skills.py`): four weapons, one
skill each, weapon mode 1, range 60, interval 1.6 s ± 0.2, prepare 0.4,
`canAttackSameTarget` true, `isEvenlyAllocated` false, quick switch on.

Nothing here runs the simulator: the offline `regressions.mcscript` that
pins the answer is the claimant's to add, beside the manifest entry that
already pins the regression fight.
