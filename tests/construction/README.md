# Construction fixtures

Every layout here exists to measure **what a `constructions` entry becomes in a
fight**, and nothing else. The rule they measure is
[`constructions.md`](../../docs/rules/constructions.md)'s: a construction is
placed once and arrives as `count` objects, each with its own life, its own
box and its own place in the row.

They are read, not fought. What each one answers is `fight buildings`, which
matches a recording's building rows back to the placements the layout declares,
because a recording says a row's `BuildingType` and not which construction
released it.

| Fixture | What it separates | Reading |
| --- | --- | --- |
| `shape.yaml` | one placement against the objects it owns | wall **5**, each turret **1** |
| `shape.yaml` (red) | the layout's constructions against the opening's | two towers, no construction |
| `wall-passage.yaml` | a wall against the side that placed it | crosses it, all five at full life |
| `wall-block.yaml` | a wall against the other side | stops and attacks, four down |
| `wall-aside.yaml` | a wall against a fight it cannot reach | the same fight, five more buildings |
| `wall-line-of-fire.yaml` | the nearest wall against the wall in the way | the one in the way, **block 6** |
| `wall-line-tolerance.yaml` | how far off the line a block may be | between **10.41** and **12.59** metres |
| `wall-line-width.yaml` | whether that width is the attacker's | it is not: Fang, Marksman and Farseer share it |
| `wall-weapon-group.yaml` | an air unit with four weapon slots | every slot takes the wall, the lock follows none, a shot splashes the next block |
| `wall-rhino.yaml` | a blow against a shot | the same rule, one hit a block, the swing held on a fallen block |
| `wall-laser.yaml` | a beam against a shot, the line's width at the centimetre, a block in the way of a unit's movement | the same rule; the line is **11.5** metres; asked while an attack is prepared; a block is an obstacle to the other side |

`shape.mcscript`, `wall.mcscript`, `placement.mcscript`, `line-of-fire.mcscript`
and `attacks.mcscript` record them and carry every number as an `expect`, so a
fixture and the measurement that reads it are one file to rerun.
`regressions.mcscript` needs no game and is what CI runs: it holds the
simulator to the physics hash the game produced for `wall-aside.yaml`,
`wall-line-of-fire.yaml`, `wall-line-tolerance.yaml`, `wall-weapon-group.yaml`,
`wall-rhino.yaml`, `wall-laser.yaml`, `wall-passage.yaml` and `wall-block.yaml`
— every wall fight here the simulator can run, each over every one of its
ticks.

A physics hash does not cover a unit's lock, its weapons' targets or its motion
state, which are content-layer fields, so every fight above is pinned by its
content hash as well, the game's too: the simulator carries the same content
as the game on every tick of all eight. The measurement scripts also compare
each fight they record with the simulator after recording it.

The three `wall-line-*` layouts are read by which block ends up destroyed,
because that is what a reader answers. The decision itself — a unit whose lock
target stays a unit while its attack target becomes a block — is in the
recording, and no verb reports it.

The two wall fixtures are the same unit and the same geometry asked of both
sides, which is the only way the wall's own description — that it sinks into
the ground for a friendly unit — could have been separated from a wall that
never obstructs anyone. It never obstructs anyone: a Crawler ends up 0.567
metres from a block's centre on the side that placed it and 0.711 on the other,
against a block radius of 4.

`wall-passage.yaml` keeps red's Marksman out of reach of the wall on purpose.
The first version of it put the Marksman opposite the wall, and the Marksman
destroyed a block outright before the Crawlers arrived — one shot of 2329
against a block's 1112 — which measured a different thing.

**Half of each reading was predicted and half was measured**, and the script
says which is which beside each line. `config/constructions.yaml` gave the
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
is the only release this build has got to work. `placement.mcscript` records
the three positions that were refused.

**What a construction does on its own.** A turret carries a `skill_id` and
2748 or 82 of damage, and a Magnetic Barrier slows what comes near it. Neither
is here. A wall's own doing is here, because a wall does nothing except stand
and be shot, which is what the two wall fixtures establish.

**A unit that does not fit between two blocks.** Every reading here is a
Crawler, 1.5 metres of inner radius against gaps 4 metres wide. A wall does not
obstruct one, and that says nothing about a unit the gaps could not admit even
if the blocks were solid.

**Who a Crawler locks at the end of `wall-passage.yaml`.** Its physics agrees
with the game on all 341 ticks; its content parts for four ticks from 327, in
the locks and motion of Crawlers retargeting as the last enemy dies.

**How long a unit stays idle once its block falls.** That it goes idle, drops
its lock and keeps the fallen block in its weapon is reproduced. How long it
stays so is not: the two recordings where it lasts more than a tick carry a
Farseer and a Fortress, which the simulator cannot run.

**A splash that reaches across a wall.** A shot at a block splashes the enemy
buildings around it, and that is measured and reproduced. Two neighbouring
cases are not: whether a shot at a block also takes a unit standing by it, and
what a shot at a unit takes when its splash reaches a block. The simulator
refuses both rather than guess.
