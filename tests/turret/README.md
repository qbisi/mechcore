# Turret fixtures

What a turret's skill does: when it fires, at what, and how often.
[`turrets.md`](../../docs/rules/turrets.md) states what these fights settled,
and [`config/constructions.yaml`](../../config/constructions.yaml) holds each
turret's numbers. They were read from each skill's state tick by tick:

```sh
scripts/record-fights.py --instrument target_refs --out /tmp/mechcore/turret/skill-state tests/turret/fights/*.yaml
```
