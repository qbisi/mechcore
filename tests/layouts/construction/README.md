# Construction fixtures

Every layout here exists to measure **what a `constructions` entry becomes in a
fight**, and nothing else. The rule they measure is
[`constructions.md`](../../../docs/rules/constructions.md)'s: a construction is
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

`shape.mcscript`, `wall.mcscript` and `placement.mcscript` record them and
carry every number as an `expect`, so a fixture and the measurement that reads
it are one file to rerun. `regressions.mcscript` needs no game and is what CI
runs: it holds the simulator to the physics hash the game produced for
`wall-aside.yaml`.

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

**What makes a unit attack a wall.** A wall is not searched for, and the
Crawlers in `wall-block.yaml` attack it anyway. `constructions.md` says what is
known; the fixture that separates the mechanism does not exist yet.
