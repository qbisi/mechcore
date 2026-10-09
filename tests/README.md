# Test fixtures

A fixture has one home, chosen by where it came from. A fight built by hand
lives with the research question it asked: each question is one directory
directly under `tests/`, named for what it studies, holding its fights and
nothing else. A fight taken from a round of the replay corpus lives in
[`corpus/`](corpus/README.md), whatever it turns out to show, and that
directory alone keeps a readme, for the rules of pinning a round:

| Directory | What it holds | Rule |
| --- | --- | --- |
| [`additional_damage/`](additional_damage/) | what a technology's hit takes from its target's life besides the hit | [`technology_effects.md`](../docs/rules/technology_effects.md#additional-damage-technologies) |
| [`anti_air/`](anti_air/) | what a technology changes about its unit's skill against aircraft: whether it attacks them, reach, damage and search | [`combat.md`](../docs/rules/combat.md#aerial-and-ground-targets) |
| [`armor/`](armor/) | what an armour technology takes off each hit on its unit | [`combat.md`](../docs/rules/combat.md#armour) |
| [`battle_skill/`](battle_skill/) | when a released battle skill lands, what it reaches, and what it writes or summons | [`battle_skill.md`](../docs/rules/battle_skill.md) |
| [`burrow/`](burrow/) | when a burrowing technology burrows its unit and brings it up, and what its burrow takes off the hits on it | [`technology_effects.md`](../docs/rules/technology_effects.md#burrowing-technologies) |
| [`chain/`](chain/) | where a chain technology's hits jump on to, when and for how much | [`technology_effects.md`](../docs/rules/technology_effects.md#chain-technologies) |
| [`construction/`](construction/) | what a construction becomes in a fight, and what attacks it | [`constructions.md`](../docs/rules/constructions.md) |
| [`control/`](control/) | what a control beam turns, what it strikes instead, and what an item or a shield changes about it | [`control.md`](../docs/rules/control.md) |
| [`damage_share/`](damage_share/) | which units a damage-share technology links into a group, and how a hit on one is shared or the group raises their damage | [`technology_effects.md`](../docs/rules/technology_effects.md#damage-share-technologies) |
| [`dead_acid/`](dead_acid/) | what a technology leaves where its unit dies, and when it leaves nothing | [`technology_effects.md`](../docs/rules/technology_effects.md#acid-technologies) |
| [`dead_explosion/`](dead_explosion/) | what a technology makes its unit's death strike, and when it strikes nothing | [`technology_effects.md`](../docs/rules/technology_effects.md#dead-explosion-technologies) |
| [`dead_line/`](dead_line/) | the life under which a technology makes its unit's hits destroy what they strike | [`technology_effects.md`](../docs/rules/technology_effects.md#dead-line-technologies) |
| [`dead_summon/`](dead_summon/) | what a technology makes its unit summon where it dies | [`technology_effects.md`](../docs/rules/technology_effects.md#summons-where-a-unit-dies) |
| [`energy_tower/`](energy_tower/) | what the Energy Tower's fight skills write onto a side, and what they leave alone | [`energy_tower_skills.md`](../docs/rules/energy_tower_skills.md) |
| [`energy_shield/`](energy_shield/) | what a unit's own shield holds and takes off a hit, from an item or a technology | [`combat.md`](../docs/rules/combat.md#personal-shield) |
| [`endgame/`](endgame/) | what the tick a side loses its last unit, and the ticks the fight runs on after it, do to the units still standing | [`combat.md`](../docs/rules/combat.md#endgame-ordering-of-a-1v1-direct-kill) |
| [`extra_weapon/`](extra_weapon/) | what an extra weapon technology adds beside a unit's main skill, and how that skill differs | [`extra_weapons.md`](../docs/rules/extra_weapons.md) |
| [`equipment/`](equipment/) | where equipment corrections land and how they compose with officers | [`equipment_effects.md`](../docs/rules/equipment_effects.md) |
| [`equipment_buff/`](equipment_buff/) | what a buff item adds to the unit wearing it, and which buffs it keeps off | [`equipment_effects.md`](../docs/rules/equipment_effects.md#buff-items) |
| [`ground_attack/`](ground_attack/) | what a technology changes about its unit's skill against ground units: reach, damage and search | [`combat.md`](../docs/rules/combat.md#aerial-and-ground-targets) |
| [`ignore_buff/`](ignore_buff/) | which buff effect a technology makes its unit ignore, and when it stops | [`technology_effects.md`](../docs/rules/technology_effects.md#ignoring-a-buff-effect) |
| [`important_unit/`](important_unit/) | what a side does when its last important unit dies | [`equipment_effects.md`](../docs/rules/equipment_effects.md#an-important-unit) |
| [`interceptor/`](interceptor/) | how an interceptor takes projectiles out of the air, and what befalls it | [`contraptions.md`](../docs/rules/contraptions.md) |
| [`level/`](level/) | how a unit level scales base life and damage before overlays | [`unit_levels.md`](../docs/rules/unit_levels.md) |
| [`lifesteal/`](lifesteal/) | how a hit hands life back to the unit whose skill dealt it, and which lifesteal source does | [`combat.md`](../docs/rules/combat.md#lifesteal) |
| [`loose_formation/`](loose_formation/) | how far apart a loose-formation technology keeps its unit's agents, and when they close up | [`technology_effects.md`](../docs/rules/technology_effects.md#loose-formation-technologies) |
| [`missile/`](missile/) | when a missile fires, at what, and what its hit writes | [`contraptions.md`](../docs/rules/contraptions.md) |
| [`fire_extinguisher/`](fire_extinguisher/) | which terrain a fire extinguisher clears about its unit, and when | [`technology_effects.md`](../docs/rules/technology_effects.md#fire-extinguisher-technologies) |
| [`fire_intensify/`](fire_intensify/) | where a fire technology's unit leaves fires as its main skill hits | [`technology_effects.md`](../docs/rules/technology_effects.md#fire-technologies) |
| [`rebirth/`](rebirth/) | when a rebirth technology brings its unit back after it dies, where, and what its side and the score make of it meanwhile | [`technology_effects.md`](../docs/rules/technology_effects.md#rebirth-technologies) |
| [`reactive_armor/`](reactive_armor/) | what a reactive armor technology takes off the hits on its unit, and for how many | [`technology_effects.md`](../docs/rules/technology_effects.md#reactive-armor-technologies) |
| [`multi_attack/`](multi_attack/) | how many projectiles a multi-attack technology adds its unit's bursts, how far apart in time and space, and from which weapon | [`technology_effects.md`](../docs/rules/technology_effects.md#multi-attack-technologies) |
| [`maintenance_array/`](maintenance_array/) | whom a repair technology's unit repairs about it, how often and from when | [`technology_effects.md`](../docs/rules/technology_effects.md#repair-technologies) |
| [`wreckage_detonation/`](wreckage_detonation/) | which units a kill-explosion technology's unit's hits set off and whom each explosion strikes | [`technology_effects.md`](../docs/rules/technology_effects.md#kill-explosion-technologies) |
| [`map/`](map/) | what a map's neutral crystals do to a fight | [`map.md`](../docs/rules/map.md) |
| [`move_ability/`](move_ability/) | what a technology adds to its unit's move ability, and what it makes as the unit surfaces | [`underground.md`](../docs/rules/underground.md) |
| [`modifier/`](modifier/) | how officers and technologies correct a unit's numbers | [`officer_effects.md`](../docs/rules/officer_effects.md) |
| [`splash/`](splash/) | how a correction widens a skill's splash, and a skill with none given one | [`combat.md`](../docs/rules/combat.md#damage-and-death) |
| [`super_deployment/`](super_deployment/) | what a formation deployed on a flank does in a fight, travelling in or settled | [`unit-rules.md`](../docs/spec/simulation/unit-rules.md) |
| [`sweep/`](sweep/) | what a sweep strikes beyond the standard fights: shields, buildings, aircraft beside ground units, its technology | [`sweep.md`](../docs/rules/sweep.md) |
| [`stealth/`](stealth/) | when a stealth or cloak technology hides its unit, what strikes and finds it meanwhile, and what disabling it does | [`technology_effects.md`](../docs/rules/technology_effects.md#stealth-technologies) |
| [`shield/`](shield/) | what a battlefield shield does to the hits meant for what it covers, and when it breaks | [`contraptions.md`](../docs/rules/contraptions.md) |
| [`siege_mode/`](siege_mode/) | when a siege-mode technology digs its unit in, what the trench gives it, and what takes it away | [`technology_effects.md`](../docs/rules/technology_effects.md#siege-mode-technologies) |
| [`search/`](search/) | which positions a skill's target search scores | [`combat.md`](../docs/rules/combat.md) |
| [`secondary_damage/`](secondary_damage/) | what a technology's second damage deals around each of its unit's hits | [`combat.md`](../docs/rules/combat.md#a-second-damage-around-a-hit) |
| [`production/`](production/) | what a production line makes, where its makes stand, and how often they come | [`equipment_effects.md`](../docs/rules/equipment_effects.md#production-lines) |
| [`repair/`](repair/) | how a unit repairs itself while hurt: when it starts, how often and by how much | [`combat.md`](../docs/rules/combat.md#repair) |
| [`projectile/`](projectile/) | how a projectile leaves its weapon, climbs and flies, and a burst still releasing | [`combat.md`](../docs/rules/combat.md) |
| [`technology_buff/`](technology_buff/) | what a buff technology adds, to its unit or the units around it, and how its buff runs | [`technology_effects.md`](../docs/rules/technology_effects.md#buff-technologies) |
| [`technology_disable/`](technology_disable/) | what a buff that disables technology switches off on a unit, and what it gives back | [`technology_effects.md`](../docs/rules/technology_effects.md) |
| [`terrain/`](terrain/) | what a battle skill's terrain does to the units standing in it, and when it goes | [`terrain.md`](../docs/rules/terrain.md) |
| [`tower/`](tower/) | what losing a tower writes on its side, and what strengthening one adds | [`towers.md`](../docs/rules/towers.md) |
| [`turret/`](turret/) | when a turret fires, at what, and how often | [`turrets.md`](../docs/rules/turrets.md) |
| [`raiden/`](raiden/) | how a Raiden's three weapons choose targets and fire together, beside its standard fights | [`combat.md`](../docs/rules/combat.md#a-fusillade-fires-with-its-core) |
| [`wreckage/`](wreckage/) | how a wreckage-recovery technology heals its unit as an enemy it struck dies | [`technology_effects.md`](../docs/rules/technology_effects.md#wreckage-recovery-technologies) |
| [`wraith/`](wraith/) | how a Wraith's four slots choose their targets, beside its standard fights | [`combat.md`](../docs/rules/combat.md#a-grouped-slot-searches-around-its-siblings-locks) |
| [`corpus/`](corpus/README.md) | rounds of the replay corpus the simulator plays back, each named for its replay and round |  |
| every other unit, `arclight/` to `wasp/` | that unit's [standard fights](#standard-unit-layouts) | [`combat.md`](../docs/rules/combat.md) |

**A fixture that bears on several topics is cited, not copied.** It stays in
its home, and every other topic reaches it by its path: a rule's `### Recorded`
evidence in [`docs/rules/`](../docs/rules/) names the fights it rests on, which
`scripts/check/check-docs.py` holds to exist, and a fight's comment names a
fight elsewhere that it takes as its control, as `equipment/` and `level/`
take `marksman/vs-arclight.yaml`. No fixture is copied into a
second directory or linked there: CI finds fights by directory, and a second
path would be a second fixture to keep in step. Two fights that differ in
anything, a `map_id` included, are two fixtures.

A fight's comment says what it measures, its control and what the game
answered; the topic's question is its row above, and the rule it settled is
the rules document beside it. A rule lands in [`docs/rules/`](../docs/rules/),
never here.

A unit's directory holds its [standard fights](#standard-unit-layouts) and,
beside them, the older pairings `vs-*.yaml`: one formation against another,
from before the standard layouts, which the RVO module and the skill state
were first held to.

**A pinned fight is a [fight document](../docs/spec/document/fight.md)** in its
topic's directory: the layout, the seed, and what the game's
recording of them decided, its tick count and hash among it, in one file. A
comment at the top of the file says what the fight measures. A fixture's
`source` is `game`: a fight the simulator computed states what is being
checked, and is never a fixture. A layout stays a layout file only while
something reads it as a layout: a probe no fight pins, or a stage for
`game apply_layout`.

CI verifies every fight: `scripts/check/check-scripts.sh` hands every
`tests/*/*.yaml` to `verify`, which fights each layout with its seed
through the simulator and holds it to what the document states. Where the game
runs, `mechcore verify --backend game` records the fights again and holds each
recording to its fixture, and `--update` writes back each one the game now
records differently, which is how every pin moves to a new MCFR format at
once. Recordings never enter the repository, and are not published anywhere
else: what the repository keeps is what reproduces them, the fights and the
scripts, which any machine with the game records again. The Research section
of [`AGENTS.md`](../AGENTS.md) says who records them and where a pinned hash
may come from.

To pin a fight, record it where the game runs, read the recording back as a
fight, and put the document in its topic's directory:

```sh
mechcore convert <layout> --to mcfr --backend game /tmp/mechcore/<topic>/<name>.mcfr --seed <seed>
mechcore convert /tmp/mechcore/<topic>/<name>.mcfr --to fight tests/<topic>/<name>.yaml
```

`convert` leaves out `game_build`: a document that states none is read
against the build [`GAME_VERSION`](../GAME_VERSION) names, which is the only
place a version is written. The file stays as `convert` wrote it, and a
comment added above it does not change what it states. CI finds it by its
directory; nothing else lists it.

Two kinds of fixture live outside `tests/`. The native replays themselves, and
the match documents converted from them, are the corpus
[`../scripts/corpus/`](../scripts/corpus/README.md) describes: `verify` over the
match documents fights their rounds in order where the corpus is, and only a
round pinned in [`corpus/`](corpus/README.md) is held by CI. Layouts built by
hand that no script uses, and the one the crates' tests read as a plain
layout, are in [`../layouts/`](../layouts/README.md).

## A match needs a witness

A recording that matches is evidence for a mechanism only if it would have
caught the mechanism wrong. So a mechanism is not done when its probes match;
it is done when its probes are shown to see it. Each check runs on the
simulator alone, without the game.

- **Off.** Switched off in the simulator, the mechanism moves the probe's hash.
  If the hash holds, the probe did not exercise it or the recording cannot see
  it. Find a probe that does, or, when none can, propose the admission that
  lets the hash see it.
- **Late.** Shifted by one tick, or with one of its numbers nudged, the
  mechanism's first divergence lands near the tick it first acts. One that
  lands far later, or only in what the round settles, means the hash lacks the
  state that carried the error: propose its admission
  ([mcfr.md](../docs/spec/mcfr/mcfr.md#admission-to-the-hash)). An
  instrument channel may locate it meanwhile, but does not pass this check.
- **Cause.** A damage, buff or unit the mechanism makes carries the cause the
  build has. An event whose cause is null where the build's call has one is a
  gap in the format.
- **Entry.** Every hooked build entry the mechanism passes writes its event.
  One the build passes and the recording lacks is a capture bug, fixed rather
  than worked around.

The pull request that lands a mechanism names its witness: the recorded fields
or events that see it, and for each mutation the tick of its first divergence
beside the tick the mechanism first acts. A format change the witness needs
blocks the mechanism: it lands first, and the mechanism waits.

## Reading a recording

`mechcore query <recording> --schema` lists what a recording holds. A few
things it holds are easy to misread:

- A battle skill's effect names no source: its `buff_applied` events, and an
  air drop's `damage` and the deaths it causes, carry only the team that
  released it. A summon is a unit from its `unit_created` on, though it first
  stands in a snapshot a second later, as it joins the fight.
- A control beam's hit that turns writes no `damage`; the turning shows in the
  unit's `control`, the progress its beams have added and whose they are.
- A missile's projectile is the one whose `projectile_released` names no
  source.
- A reborn unit stands again under the same unit id, with no event: while it
  waits it is a row in `rebirths`, and after, its `rebirth_count` reads 1.
- A building row names its `BuildingType`, not the construction that placed
  it; `show --view buildings` matches the rows back to the layout's
  placements.
- An explosion a kill sets off is `damage` that names the unit and no skill.

## Recording with instrument channels

What a recording does not hash, an instrument channel records beside it
([mcfr.md](../docs/spec/mcfr/mcfr.md#instrument-channels)). Any pinned fight is
recorded again with channels by `scripts/record-fights.py`, and a layout by
`convert`:

```sh
scripts/record-fights.py --instrument <channels> --out /tmp/mechcore/<topic> tests/<topic>/*.yaml
mechcore convert <layout> --to mcfr --backend game <out.mcfr> --seed <seed> --instrument <channels>
```

| Channel | What it shows | Read in |
| --- | --- | --- |
| `target_refs` | each skill's state and attack phase, tick by tick | `construction/`, `control/`, `sweep/`, `turret/`, the unit pairings |
| `target_search`, `target_candidate` | what a search scored, and its best candidates | `anti_air/`, `ground_attack/`, `extra_weapon/` |
| `skill_attackable_checker` | every `Check` call, with the slot that made it and its lock before and after | `raiden/`, `wraith/`, `shield/barrier-fortress-vs-wasps.yaml` |
| `unit_pose` | each unit's animated pose | the player's demo scene (`scripts/player/pose-timing.py`) |

An ignored test replays the checker channel of `wraith/group-attack.yaml` and
`wraith/two-targets.yaml`: before each grouped call it restores the targets
the slot held and compares the call's answer, lock and attack target.

```sh
cargo test -p mechcore-simulation grouped_checker_matches_every_captured_call -- --ignored --nocapture
```

## How a mechanism is researched

[`modifier/`](modifier/)'s composition fights are the worked example: they
were designed so that the outcome separates the candidates, which is why one
reads a tick count and another the life left. A recording carries each unit's
speed and each skill's range, damage and interval as the build computed them,
so a new correction needs no such design: put it on a unit, record one tick,
and read the numbers with `show --view stats`.

1. **Reduce the question to one number.** A question that does not reduce to
   one makes no experiment.
2. **Read what can be read first, and write down what it cannot answer.**
3. **Design fights that tell the hypotheses apart, with a control.** The
   control is recorded twice and the two must agree tick for tick; when they
   do not, stop.
4. **Write each hypothesis's expected value into the script before
   recording.**
5. **A measurement has two halves, and only both agreeing make a rule:** what
   the game stores, and what the game computes.
6. **A rule lands as a table and code, and a refusal comes before a guess.**
   The numbers go into `config/`, their source into `docs/rules/`, and the
   cases not reached are refused by name.

## Standard unit layouts

A unit is supported without technology, at level 1 in round 1 with no officer
or equipment, when three things hold. The six standard layouts below are each
recorded by the game with two seeds, 4242 and 1787720817, and played back by
the simulator tick for tick, in every field. Every skill the build gives the
unit without technology is reproduced by one of them or refused by name. And
the units its layouts face are supported first. Every recording the
simulator reproduces is pinned in the unit's own directory as
`<layout>-<seed>.yaml`, with the layout's own comment at its top. Each is fought without a scene, as a
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

