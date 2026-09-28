# Native regression fights

Fights that exercise the kernel as a whole rather than one rule: movement,
targeting, splash and death across a handful of unit pairs, one seed each.
Each is a [fight document](../../docs/spec/document/fight.md) in `fights/`,
named for its layout.

Three topics read their control from here rather than pinning it twice:
[`../equipment/`](../equipment/README.md) and [`../level/`](../level/README.md)
take `marksman-vs-arclight.yaml` as the unmodified Marksman, and
[`../wraith/`](../wraith/README.md) records `wraith-group-attack-01.yaml` with
its slot channels.

CI verifies every fight through the simulator, with every other topic's, as
[`../README.md`](../README.md) says. Where the game runs, they are recorded
again and held to their fixtures, and recorded with the `target_refs` channel
for each skill's state tick by tick:

```sh
mechcore verify --backend game tests/regression/fights/*.yaml
scripts/record-fights.py --instrument target_refs --out /tmp/mechcore/regression/skill-state tests/regression/fights/*.yaml
```

`crates/simulation/tests/fight.rs` fights `marksman-vs-arclight.yaml`,
`rhino-vs-arclight.yaml` and `rhino-retarget.yaml` and reads named fields out
of the result, a unit's lock and its motion state, so that a failure says which
one moved; the hash says only that something did.
