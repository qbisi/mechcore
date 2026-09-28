# Wraith fixtures

How a Wraith's four slots choose their targets, given what the others hold:
the grouped search
[`combat.md`](../../docs/rules/combat.md#a-grouped-slot-searches-around-its-siblings-locks)
describes. `fights/two-targets.yaml` and the regression fight
[`wraith-group-attack-01.yaml`](../regression/fights/wraith-group-attack-01.yaml)
ask it; the other twelve fights are the Wraith's
[standard fights](../README.md#standard-unit-layouts).

All fourteen are recorded with two channels: `skill_attackable_checker`, every
`Check` call with the slot that made it and its lock before and after, and
`group_slots`, each slot's lock, attack target and state on every tick.

```sh
scripts/record-fights.py --instrument skill_attackable_checker,group_slots \
    --out /tmp/mechcore/wraith/slots tests/regression/fights/wraith-group-attack-01.yaml \
    tests/wraith/fights/*.yaml
```

Two ignored tests read those recordings. The first replays the checker
channel: before each grouped call it restores the targets the slot held, on a
shadow skill at the kernel's checker site, and compares the call's answer,
lock and attack target, 4,188 calls in the regression fight and 344 in two
targets. Observed targets never advance the simulation itself. The second
simulates each fight and compares every slot's lock, attack target and state
with `group_slots` on every tick, beside the unit's lock and motion, and names
the first tick a fight parts on. Both fixtures and every standard fight agree.

```sh
cargo test -p mechcore-simulation grouped_checker_matches_every_captured_call -- --ignored --nocapture
cargo test -p mechcore-simulation grouped_slots_match_every_recorded_tick -- --ignored --nocapture
```
