# Anti-air

What a technology changes about its unit's skill against aircraft: how far it
reaches one, what it deals one, and how its search weighs one against a ground
target. [`combat.md`](../../docs/rules/combat.md#aerial-and-ground-targets) is
the rule.

Each fight puts Aerial Specialization on the units that research it against an
Overlord, with a ground unit beside it where the search is what it measures.
The same layouts without the technology are the control each fight's comment
cites; the simulator plays them back already, and no test pins them.

A recording holds the technology's numbers in the skill's
`attack_air_range_add_value`, `damage_change_rate_air` and
`attack_range_value_air` modifiers, and what a search scored in the
`target_search` and `target_candidate` instrument channels:

```sh
mechcore convert <layout> --to mcfr --backend game <abs path>.mcfr --instrument target_search,target_candidate
```
