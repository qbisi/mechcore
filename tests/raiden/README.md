# Raiden fixtures

How a Raiden's three weapons choose their targets and fire together, the
fusillade [`combat.md`](../../docs/rules/combat.md#a-fusillade-fires-with-its-core)
describes. `fights/fang-in-reach.yaml` asks it; the other twelve fights are
the Raiden's [standard fights](../README.md#standard-unit-layouts).

All thirteen are recorded with two channels: `skill_attackable_checker`, every
`Check` call with the slot that made it and its lock before and after, and
`group_slots`, each slot's lock, attack target and state on every tick.

```sh
scripts/record-fights.py --instrument skill_attackable_checker,group_slots \
    --out /tmp/mechcore/raiden/slots tests/raiden/fights/*.yaml
```

An ignored test reads those recordings, beside the Wraith's, simulates each
fight and compares every slot's lock, attack target and state with
`group_slots` on every tick. All thirteen agree.

```sh
cargo test -p mechcore-simulation grouped_slots_match_every_recorded_tick -- --ignored --nocapture
```
