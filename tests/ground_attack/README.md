# Ground attack

What a technology changes about its unit's skill against ground units: how
far it reaches one, what it deals one, and how its search weighs one against
an aircraft. [`combat.md`](../../docs/rules/combat.md#aerial-and-ground-targets)
is the rule; [`anti_air/`](../anti_air/README.md) measures the same numbers
against aircraft.

Each fight puts a ground technology on the unit that researches it against an
Arclight and Phoenixes: Ground Specialization on a Wasp squad, and Ground
Targeting on a Phantom Ray.

A recording holds the range and damage the technology comes to in each skill's
`attack_range` and `attack_damage`, and what a search scored in the
`target_search` and `target_candidate` instrument channels:

```sh
mechcore convert <layout> --to mcfr --backend game <abs path>.mcfr --instrument target_search,target_candidate
```
