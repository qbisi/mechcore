# Adapter smoke

Seed 1787720817. `smoke.mcscript` needs the game and
no one watching it: it answers whether the Adapter still installs, reads back
and records everything it could on the build before, once per part of a
layout, and whether every instrumentation profile's hooks still install.

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
| one per profile | `target_refs_v1`, `target_refs_rvo_v1`, `skill_attackable_checker_v1`, `selector_score_v1`, `selector_score_rvo_v1` |

It pins no hash. What a fight does belongs to the topic that asks it; a
layout here that some topic also records is recorded again, not compared.
Recordings are not tracked.
