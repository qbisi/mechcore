# Raiden fixtures

Every fixture here exists to measure **how a Raiden's three weapons choose
their targets and fire together**. A Raiden's core is a fusillade
`SkillGroup`: three `FightSkill`s, one per weapon, held to the core by
`GroupedSkillFusilladeBehaviour`, each weapon with a transform of its own
fixed to the body. The rules are in
[`combat.md`](../../docs/rules/combat.md#a-fusillade-fires-with-its-core).

| Fixture | What it separates | Reading |
| --- | --- | --- |
| `fights/fang-in-reach.yaml` | three Raidens with a Fang formation already in reach: whether Raidens avoid one another's targets | the first volley's nine blows |

The fixture and the Raiden's twelve standard fights (`raiden-*.yaml` in
[`../units/fights/`](../units/README.md)) are recorded, each from its fight
document, with two channels:

```sh
scripts/record-fights.py --instrument skill_attackable_checker,group_slots \
    --out /tmp/mechcore/raiden/slots tests/raiden/fights/*.yaml tests/units/fights/raiden-*.yaml
```

`skill_attackable_checker`, which holds every `Check` call with the slot that
made it and its lock before and after, and `group_slots`, which holds each
slot's lock, attack target and state on every tick.

CI verifies the fixture without the game against the hash the game recorded:
99 ticks, seed 4242. In it the three Raidens fire their first
volley on tick 2 at nine different Fangs, and not because they avoid one
another: the first to update kills the three it holds, and the next finds
them dead and searches again. Raidens do not share out targets across units;
in the Raiden's M3 with seed 4242 one Raiden locks, at tick 164, the three
Crawlers another has held since tick 114. The twelve standard fights are
pinned with the other units' in `../units/fights/`.

A second ignored test reads every recording that command makes, beside the
Wraith's: it simulates each fight and compares, tick by tick, every slot's
lock, attack target and state with the recording's `group_slots`. It requires
the recordings, made where the game runs:

```sh
cargo test -p mechcore-simulation grouped_slots_match_every_recorded_tick -- --ignored --nocapture
```

The fixture and all twelve standard fights agree on every tick.
