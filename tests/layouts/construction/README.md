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

`shape.mcscript` records it and carries every number as an `expect`, so the
fixture and the measurement that reads it are one file to rerun.

**Half of each reading was predicted and half was measured**, and the script
says which is which beside each line. `config/constructions.yaml` gave the
counts, the lives and the boxes before the recording existed; where the five
blocks of a wall stand did not come from it, because the two fields that look
like they should say — `block_width` and `space` — span 83 metres across a
footprint 60 wide.

## What is not measured here

**A construction with more than one row.** The Magnetic Barrier is the only one
the build holds, and this build cannot place it: a layout compiles it, but the
release is refused by the game — `ConstructionElement` is created and the
action performed, and `ConstructionManager` then holds no element at that
index. So the row of five is the whole of what the geometry rests on, and
`constructions.md` states the generalisation as open rather than as a formula
fitted to one wall.

**What a construction does.** A turret carries a `skill_id` and 2748 or 82 of
damage, a wall sinks into the ground for a friendly unit, and a Magnetic
Barrier slows what comes near it. None of that is here: this directory is about
what stands, and a fixture measuring what one does belongs beside it under its
own question.
