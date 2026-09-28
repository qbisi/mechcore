# Construction fixtures

Every layout here exists to measure **what a `constructions` entry becomes in a
fight**, and nothing else. The rule they measure is
[`constructions.md`](../../docs/rules/constructions.md)'s: a construction is
placed once and arrives as `count` objects, each with its own life, its own
box and its own place in the row.

They were read with `show --view buildings`, which matches a recording's
building rows back to the placements the layout declares, because a recording
says a row's `BuildingType` and not which construction released it.

| Fixture | What it separates | Reading |
| --- | --- | --- |
| [`construction-shape.yaml`](../../layouts/construction-shape.yaml) | one placement against the objects it owns | wall **5**, each turret **1** |
| [`construction-shape.yaml`](../../layouts/construction-shape.yaml) (red) | the layout's constructions against the opening's | two towers, no construction |
| `fights/wall-passage.yaml` | a wall against the side that placed it | crosses it, all five at full life |
| `fights/wall-block.yaml` | a wall against the other side | stops and attacks, four down |
| `fights/wall-aside.yaml` | a wall against a fight it cannot reach | the same fight, five more buildings |
| `fights/wall-line-of-fire.yaml` | the nearest wall against the wall in the way | the one in the way, **block 6** |
| `fights/wall-line-tolerance.yaml` | how far off the line a block may be | between **10.41** and **12.59** metres |
| [`wall-line-width.yaml`](../../layouts/wall-line-width.yaml) | whether that width is the attacker's | it is not: Fang, Marksman and Farseer share it |
| `fights/wall-weapon-group.yaml` | an air unit with four weapon slots | every slot takes the wall, the lock follows none, a shot splashes the next block |
| `fights/wall-rhino.yaml` | a blow against a shot | the same rule, one hit a block, the swing held on a fallen block |
| `fights/wall-splash.yaml` | a splash against what it was aimed at | a shot at a block takes the Crawlers on it |
| `fights/wall-splash-line.yaml` | the same, with blocks entering and leaving the line | the order the target trees hold them |
| `fights/wall-splash-behind.yaml` | a shot at a unit against a block out of reach | the block takes the full shot, **747** |
| `fights/wall-laser.yaml` | a beam against a shot, the line's width at the centimetre, a block in the way of a unit's movement | the same rule; the line is **11.5** metres; asked while an attack is prepared; a block is an obstacle to the other side |

Recording the wall fights once more with the `target_refs` instrument
channel, `scripts/record-fights.py --instrument target_refs --out
/tmp/mechcore/construction/skill-state tests/construction/fights/*.yaml`, says
which state each unit's skill is in and which attack phase it is in, tick by
tick; that, not the recording, is where the rules read when a unit meets a
block and how an attack on one ends.
The eleven wall fights the simulator can run are pinned as fight documents in
`fights/`, each holding the layout, the seed 4242 and what the game recorded,
and a comment saying what it measures; the two probes that are not fights are
in [`../../layouts/`](../../layouts/README.md). CI verifies every fight in
`fights/`, holding the simulator to each one's hash over every one of its
ticks.

The hash covers everything a recording carries, a unit's lock, its weapons'
targets and its motion state included, so the simulator carries the same
content as the game on every tick of all eleven, and every reading in the
table above is in that content.

The three `wall-line-*` fixtures are read by which block ends up destroyed,
because that is what a reader answers. The decision itself — a unit whose lock
target stays a unit while its attack target becomes a block — is in the
recording, and no verb reports it.

The two wall fixtures are the same unit and the same geometry asked of both
sides, which is the only way the wall's own description — that it sinks into
the ground for a friendly unit — could have been separated from a wall that
never obstructs anyone. It never obstructs anyone: a Crawler ends up 0.567
metres from a block's centre on the side that placed it and 0.711 on the other,
against a block radius of 4.

`fights/wall-passage.yaml` keeps red's Marksman out of reach of the wall on purpose.
The first version of it put the Marksman opposite the wall, and the Marksman
destroyed a block outright before the Crawlers arrived — one shot of 2329
against a block's 1112 — which measured a different thing.

**Half of each reading was predicted and half was measured.**
`config/constructions.yaml` gave the
counts, the lives and the boxes before the recording existed; where the five
blocks of a wall stand did not come from it, because the two fields that look
like they should say — `block_width` and `space` — span 83 metres across a
footprint 60 wide.

## What is not measured here

**A construction with more than one row.** The Magnetic Barrier is the only one
the build holds, and nothing here has placed one: the release is refused, with
`ConstructionManager` holding no element at the index after the action is
performed. The refusal is not about the barrier — a Defensive Wall released
anywhere but where the seed's own opening already stands answers the same — so
what a release needs is open, and so is the geometry of anything with a second
row.

**A construction anywhere but where the opening put it.** Every fixture here
places what the seed's opening deals, at the position it deals it, because that
is the only release this build has got to work; three other positions were
refused.

**What a construction does on its own.** A turret carries a `skill_id` and
2748 or 82 of damage, and a Magnetic Barrier slows what comes near it. Neither
is here. A wall's own doing is here, because a wall does nothing except stand
and be shot, which is what the two wall fixtures establish.

**A unit that does not fit between two blocks.** Every reading here is a
Crawler, 1.5 metres of inner radius against gaps 4 metres wide. A wall does not
obstruct one, and that says nothing about a unit the gaps could not admit even
if the blocks were solid.

