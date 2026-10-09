# Native regression fights

Fights that exercise the kernel as a whole rather than one rule: movement,
targeting, splash and death across a handful of unit pairs, one seed each.

Three topics read their control from here rather than pinning it twice:
[`../equipment/`](../equipment/README.md) and [`../level/`](../level/README.md)
take `marksman-vs-arclight.yaml` as the unmodified Marksman, and
[`../wraith/`](../wraith/README.md) records `wraith-group-attack-01.yaml` with
its slot channels. Recorded with each skill's state tick by tick:

```sh
scripts/record-fights.py --instrument target_refs --out /tmp/mechcore/regression/skill-state tests/regression/*.yaml
```

`crates/simulation/tests/fight.rs` fights `marksman-vs-arclight.yaml`,
`rhino-vs-arclight.yaml` and `rhino-retarget.yaml` and reads named fields out
of the result, a unit's lock and its motion state, so that a failure says which
one moved; the hash says only that something did.
