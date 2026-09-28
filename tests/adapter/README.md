# Adapter smoke

Seed 1787720817. `smoke.mcscript` needs the game and
no one watching it: it answers whether the Adapter still installs, reads back
and records everything it could on the build before, once per part of a
layout, and whether every instrument channel's hooks still install.

| Recording | What it exercises |
| --- | --- |
| equipment | two items on one unit, fitted after the officer that adds the slot |
| constructions | a wall and both turrets on each side, at the layout's indices |
| turret | a turret beside a unit |
| contraptions, interceptor | the contraptions a layout can hold |
| terrain-oil, terrain-fire | a retained oil terrain, and fire left by a skill |
| towers | tower strengthening |
| battle-skills | two battle skills released in order |
| readback | round two with constructions, contraptions and energy tower skills |
| ambush | travelling units in the ambush zones |
| replay-round | a round-seven board taken from a replay, 48 units |
| instrumented | one fight with every channel: `target_refs`, `skill_attackable_checker`, `target_search`, `target_candidate`, `rvo_solve`, `rvo_neighbour`, `rvo_vo` |

It pins no hash. What a fight does belongs to the topic that asks it; a
layout here that some topic also records is recorded again, not compared.
Recordings are not tracked.

Four of the layouts it stages are kept here as layouts because
`game.apply_layout` takes a layout and a topic keeps its fights as fight
documents: `two-items.yaml`, `rapid-fire-head-on.yaml`, `both-towers-0-4.yaml`
and `crawlers-vs-marksman.yaml` are the layouts of the pinned fights of the
same names in [`../equipment/`](../equipment/README.md),
[`../turret/`](../turret/README.md), [`../tower/`](../tower/README.md) and
[`../regression/`](../regression/README.md). The fight documents say what each
fight measures. The rest come from [`../../layouts/`](../../layouts/README.md)
and from [`../skill-order/`](../skill-order/).
