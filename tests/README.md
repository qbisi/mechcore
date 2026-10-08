# Test fixtures

A fixture has one home, chosen by where it came from. A fight built by hand
lives with the research question it asked: each question is one directory
directly under `tests/`, named for what it studies, holding its fights and,
where a fight's own comment cannot say it, a readme. A fight taken from a round
of the replay corpus lives in [`corpus/`](corpus/README.md), whatever it turns
out to show:

| Directory | What it holds |
| --- | --- |
| [`anti_air/`](anti_air/README.md) | what a technology changes about its unit's skill against aircraft: whether it attacks them, reach, damage and search |
| [`armor/`](armor/README.md) | what an armour technology takes off each hit on its unit |
| [`battle_skill/`](battle_skill/README.md) | when a released battle skill lands, what it reaches, and what it writes or summons |
| [`construction/`](construction/README.md) | what a construction becomes in a fight, and what attacks it |
| [`control/`](control/README.md) | what a control beam turns, what it strikes instead, and what an item or a shield changes about it |
| [`dead_summon/`](dead_summon/README.md) | what a technology makes its unit summon where it dies |
| [`energy_tower/`](energy_tower/README.md) | what the Energy Tower's fight skills write onto a side, and what they leave alone |
| [`energy_shield/`](energy_shield/README.md) | what a unit's own shield holds and takes off a hit, from an item or a technology |
| [`extra_weapon/`](extra_weapon/README.md) | what an extra weapon technology adds beside a unit's main skill, and how that skill differs |
| [`equipment/`](equipment/README.md) | where equipment corrections land and how they compose with officers |
| [`equipment_buff/`](equipment_buff/README.md) | what a buff item adds to the unit wearing it, and which buffs it keeps off |
| [`ground_attack/`](ground_attack/README.md) | what a technology changes about its unit's skill against ground units: reach, damage and search |
| [`important_unit/`](important_unit/README.md) | what a side does when its last important unit dies |
| [`interceptor/`](interceptor/README.md) | how an interceptor takes projectiles out of the air, and what befalls it |
| [`level/`](level/README.md) | how a unit level scales base life and damage before overlays |
| [`lifesteal/`](lifesteal/README.md) | how a hit hands life back to the unit whose skill dealt it, and which lifesteal source does |
| [`missile/`](missile/README.md) | when a missile fires, at what, and what its hit writes |
| [`map/`](map/README.md) | what a map's neutral crystals do to a fight |
| [`modifier/`](modifier/README.md) | how officers and technologies correct a unit's numbers |
| [`splash/`](splash/README.md) | how a correction widens a skill's splash, and a skill with none given one |
| [`super_deployment/`](super_deployment/README.md) | what a formation deployed on a flank does in a fight, travelling in or settled |
| [`sweep/`](sweep/README.md) | what a sweep strikes beyond the standard fights: shields, buildings, aircraft beside ground units, its technology |
| [`shield/`](shield/README.md) | what a battlefield shield does to the hits meant for what it covers, and when it breaks |
| [`search/`](search/README.md) | which positions a skill's target search scores |
| [`secondary_damage/`](secondary_damage/README.md) | what a technology's second damage deals around each of its unit's hits |
| [`production/`](production/README.md) | what a production line makes, where its makes stand, and how often they come |
| [`repair/`](repair/README.md) | how a unit repairs itself while hurt: when it starts, how often and by how much |
| [`projectile/`](projectile/README.md) | how a projectile leaves its weapon, climbs and flies, and a burst still releasing |
| [`technology_buff/`](technology_buff/README.md) | what a buff technology adds, to its unit or the units around it, and how its buff runs |
| [`technology_disable/`](technology_disable/README.md) | what a buff that disables technology switches off on a unit, and what it gives back |
| [`terrain/`](terrain/README.md) | what a battle skill's terrain does to the units standing in it, and when it goes |
| [`tower/`](tower/README.md) | what losing a tower writes on its side, and what strengthening one adds |
| [`turret/`](turret/README.md) | when a turret fires, at what, and how often |
| [`raiden/`](raiden/README.md) | how a Raiden's three weapons choose targets and fire together, beside its standard fights |
| [`wraith/`](wraith/README.md) | how a Wraith's four slots choose their targets, beside its standard fights |
| [`regression/`](regression/README.md) | fights that exercise the kernel rather than one rule |
| [`corpus/`](corpus/README.md) | rounds of the replay corpus the simulator plays back, each named for its replay and round |
| every other unit, `arclight/` to `wasp/` | that unit's [standard fights](#standard-unit-layouts) |

**A fixture that bears on several topics is cited, not copied.** It stays in
its home, and every other topic reaches it by its path: a rule's `### Recorded`
evidence in [`docs/rules/`](../docs/rules/) names the fights it rests on, which
`scripts/check/check-docs.py` holds to exist, and a topic's readme names a
fight elsewhere that it reads, as `equipment/` and `level/` name the
regression fight they take as their control. No fixture is copied into a
second directory or linked there: CI finds fights by directory, and a second
path would be a second fixture to keep in step. Two fights that differ in
anything, a `map_id` included, are two fixtures.

A fight's comment says what it measures and what the game answered, and a
topic's readme says only what no one fight can: the question, the rule it
settled, and the commands that record it with instrument channels or read
those recordings. A rule lands in [`docs/rules/`](../docs/rules/), never in a
readme here.

**A pinned fight is a [fight document](../docs/spec/document/fight.md)** in its
topic's `fights/` directory: the layout, the seed, and what the game's
recording of them decided, its tick count and hash among it, in one file. A
comment at the top of the file says what the fight measures. A fixture's
`source` is `game`: a fight the simulator computed states what is being
checked, and is never a fixture. A layout stays a layout file only while
something reads it as a layout: a probe no fight pins, or a stage for
`game.apply_layout`.

CI verifies every fight: `scripts/check/check-scripts.sh` hands every
`tests/*/fights/*.yaml` to `verify`, which fights each layout with its seed
through the simulator and holds it to what the document states. Where the game
runs, `mechcore verify --backend game` records the fights again and holds each
recording to its fixture, and `--update` writes back each one the game now
records differently, which is how every pin moves to a new MCFR format at
once. A run script that is still tracked needs the game, and CI only parses
it. Recordings never enter the repository, and are not published anywhere
else: what the repository keeps is what reproduces them, the fights and the
scripts, which any machine with the game records again. The Research section
of [`AGENTS.md`](../AGENTS.md) says who records them and where a pinned hash
may come from.

To pin a fight, record it where the game runs, read the recording back as a
fight, and put the document under its topic's `fights/`:

```sh
mechcore convert <layout> --to mcfr --backend game /tmp/mechcore/<topic>/<name>.mcfr --seed <seed>
mechcore convert /tmp/mechcore/<topic>/<name>.mcfr --to fight tests/<topic>/fights/<name>.yaml
```

Then delete its `game_build` line: a document that states none is read
against the build [`GAME_VERSION`](../GAME_VERSION) names, which is the only
place a version is written. Otherwise the file stays as `convert` wrote it,
and a comment added above it does not change what it states. CI finds it by
its directory; nothing else lists it.

Two kinds of fixture live outside `tests/`. The native replays themselves, and
the match documents converted from them, are the corpus
[`../scripts/corpus/`](../scripts/corpus/README.md) describes: `verify` over the
match documents fights their rounds in order where the corpus is, and only a
round pinned in [`corpus/`](corpus/README.md) is held by CI. Layouts built by
hand that no script uses, and the one the crates' tests read as a plain
layout, are in [`../layouts/`](../layouts/README.md).

## Standard unit layouts

[`plan/units.md`](../plan/units.md) defines when a unit is supported without
technology: six standard layouts, each recorded by the game with two seeds,
4242 and 1787720817, and played back by the simulator tick for tick, in every
field. Every recording the simulator reproduces is pinned in the unit's own
directory as `fights/<layout>-<seed>.yaml`, with the layout's own comment at
its top. Each is fought without a scene, as a
[layout replay](../docs/spec/document/layout-replay.md), and hashes the same
recorded that way as in the Training Ground.

| Layout | What it measures |
| --- | --- |
| M1 `m1-mirror` | the unit against itself, 200 m apart: formation, approach, first blow, damage, death |
| M2 `m2-rhino` | a single large ground target |
| M3 `m3-crawler` | a swarm of ground targets |
| M4 `m4-wasp` / `m4-marksman` | the air relation: a ground unit against Wasps, an air unit against a Marksman |
| M5 `m5-rotated` | the formation rotated, its target off to one side |
| M6 `m6-formations` | two formations against a Crawler swarm and a Marksman |

**A standard layout ends before any tower falls.** A tower's loss puts a
debuff on its side, which is a mechanism of its own and not the unit's. A side
that cannot hit the other walks to the enemy towers, so where one side cannot
hit back the other fields enough formations to win first: a Rhino or a Crawler
against three Wasp formations, three Wasp formations against a Rhino or a
Crawler, and five Wraith formations against a Rhino, where three lost a tower.
M5's target stands nearer than either enemy tower, and an air unit's M5 target
is a Marksman, which shoots back. With one Wasp formation, or with the first
M5 placement, a tower fell in every one of those fights and the recording
parted from the simulator on that tick.

A unit that is its own opponent in a cell does not record it twice: the Rhino
has no M2, the Crawler no M3. Rhino, Crawler, Wasp and Marksman are the
opponents every other unit's layouts use, so they were held to the definition
first.

Without technology a unit has only its main skill: `FightMech`'s constructor
adds one skill, `mechData.GetMainSkillID()`, and extra skills reach a mech
only through a technology's `ExtraSkillSystem`. So the definition's second
condition is met by the main attack alone, and each unit's main skill row
agrees with its `config/units/` file.

Every unit is pinned. Each mechanism the recordings exposed is named in
[`combat.md`](../docs/rules/combat.md). Two of them were read off five of the
fights recorded with the `target_refs` channel,
`scripts/record-fights.py --instrument target_refs`, each skill's state beside
the turret's rotation, which the MCFR carries as `turret_rotation`: the attack
angle is measured from it, and it showed the game turning exactly as the
simulator did and parting only on the state change.

