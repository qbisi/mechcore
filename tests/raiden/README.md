# Raiden fixtures

How a Raiden's three weapons choose their targets and fire together, the
fusillade [`combat.md`](../../docs/rules/combat.md#a-fusillade-fires-with-its-core)
describes. `fang-in-reach.yaml` asks it; the other twelve fights are
the Raiden's [standard fights](../README.md#standard-unit-layouts).

Every slot's lock, attack target, state and clock are in each recording's
`skills` on every tick, and in its hash, so verifying a fight compares them.
The checker calls are the `skill_attackable_checker` channel: every `Check`
call with the slot that made it and its lock before and after.

```sh
scripts/record-fights.py --instrument skill_attackable_checker \
    --out /tmp/mechcore/raiden/slots tests/raiden/*.yaml
```
