# Native regression fights

Fights that exercise the kernel as a whole rather than one rule: movement,
targeting, splash and death across a handful of unit pairs, most of them under
many seeds. Each is a [fight document](../../docs/spec/document/fight.md) in
`fights/`, named for its layout and, where one layout is fought under several
seeds, numbered: `rhino-vs-crawlers-01.yaml` to `-10.yaml` are one layout
under ten seeds.

Three topics read their control from here rather than pinning it twice:
[`../equipment/`](../equipment/README.md) and [`../level/`](../level/README.md)
take `marksman-vs-arclight.yaml` as the unmodified Marksman, and
[`../wraith/`](../wraith/README.md) records `wraith-group-attack-01.yaml` with
its slot channels.

| Script | Needs the game | What it does |
| --- | --- | --- |
| `simulate.mcscript` | **no** | verifies every fight through the simulator, and the one simulator pin below |
| `refresh.mcscript` | yes | records every fight again, reads each recording back as a fight, and requires it to state what its fixture states |
| `skill-state.mcscript` | yes | records every fight again with the `target_refs` channel, each skill's state tick by tick |

The whole directory verifies in about sixteen seconds, so there is no tier of
it to save; `refresh.mcscript` records every fight too, since a fight that
drifted on one seed is the answer.

`crates/simulation/tests/fight.rs` fights `marksman-vs-arclight.yaml`,
`rhino-vs-arclight.yaml` and `rhino-retarget.yaml` and reads named fields out
of the result, a unit's lock and its motion state, so that a failure says which
one moved; the hash says only that something did.

`rhino-vs-two-arclights.yaml` is not a fight the game recorded. It is a layout
pinned as the simulator plays it: when sampled RVO movement was modelled, its
321 ticks were reviewed by hand, and `simulate.mcscript` holds the simulator to
that reviewed run. Its hash is the simulator's own, so a change that moves it
has to say why the new run is the right one.
