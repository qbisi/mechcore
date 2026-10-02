# Layout fixtures

Each file is a layout document as [layout.md](../docs/spec/document/layout.md)
defines it, used as input to `apply_layout` against the live game, to
`mechcore convert --to mcfr`, or to both. A fixture is a scenario someone chose, so the
reason it exists belongs with it.

These are the layouts built by hand that no script uses, and one captured live.
One a script reads lives with it, in its topic directory under
[`../tests/`](../tests/README.md); the replays a layout can be projected from
are in the corpus [`../scripts/corpus/README.md`](../scripts/corpus/README.md) points at.

`marksman-vs-arclight.yaml` is the layout of the pinned fight
[`../tests/regression/fights/marksman-vs-arclight.yaml`](../tests/regression/fights/marksman-vs-arclight.yaml),
one Marksman against one Arclight. The crates' tests read it as a plain
layout: a small fight that ends, for what `simulate_layout` and the command
line do with any layout, under seeds of their own. They also fight
`experience.yaml` through the simulator, for the experience a formation opens
a fight with.

`tuff-replay-round-7.yaml` is not built by hand: it was captured from the game
at the end of round 7's deployment in the `[crower]VS[[TUFF]MARLFAUX]` replay,
so it is the round's decisions stepped from the position it opened with, both
sides, board included. `crates/document/src/project.rs` holds the projection
to it, and `crates/adapter/src/operations.rs` reads red's two towers from it,
one strengthened to level 2 beside one that is not.

`raiden-wall-slots.yaml` is the probe behind the simulator's refusal of a
grouped slot that strikes a construction: the game's Raiden there strikes a
wall block with its core slot alone, where the simulator's slots would have
struck one each.

`crowd.yaml` is the large scene the simulator's speed is followed on: 1302
units, mirrored, under a fixed seed, whose `convert --to mcfr` profiling
splits what the fight cost by phase and states it per unit-tick.

Most files here are named for what they contain and need no further
explanation. The ones below were built to exercise a specific native path, and
their coordinates are load-bearing: changing a position silently turns the
fixture into a different test that still passes.

`construction-battle.yaml` reproduces opening construction group 28
with the `reverse_x` transform observed in replay R002. In blue's side-local
frame the Defensive Wall sits at `(-140, -55)`, the Rapid-Fire Turret at
`(140, -100)` and the Anti-Armor Turret at `(-140, -100)`. Red uses the
side-local coordinates that compile back to R002's recorded world positions.
Magnetic Barrier is absent because it is not part of that opening group, and
because this build cannot place one: a layout compiles it and the release is
then refused by the game, as
[constructions.md](../docs/rules/constructions.md#where-the-objects-stand)
records.

[`../construction/`](../tests/construction/README.md) is the directory that
measures what a construction becomes; this one is where a construction appears
inside a fixture built for something else.

`interceptor-battle.yaml` places one `30 x 30` interceptor and one `50 x 20`
Stormcaller per side. It is the live regression sample for native placement,
recorder readback, shared footprint validation, round transition and shutdown.

`shield-missile-battle.yaml` gives each side a Stormcaller centred at
`(5,-40)`, a shield at `(1,-81)` and a missile at `(1,-11)`. The Stormcaller's
whole `50 x 20` footprint lies inside its own radius-70 shield, and after the
red-side transform each Stormcaller starts about 51.35 m from the opponent's
missile, inside the reference missile's 100 m trigger range. Both contraption
centres deliberately miss the deployment modulo-10 grid. It covers shield and
missile target-region validation, their exclusion from deployment collisions,
recorder readback, the fight interaction, round transition and shutdown.

`crawler-in-face.yaml` places a `50 x 20` Crawler per side at local `y=-20`,
front edge exactly on `y=-10`. Placement succeeds only while the room keeps the
standard 300 m-deep round-one main regions, so a pass also proves the initial
native constructions were cleared and their manager count read back as zero.

`six-unit.yaml` exercises persistent technology state and side-wide tower
state. Sledgehammer, Marksman, Fang, Wasp and Arclight take their Range
Enhancement technologies `10213`, `10202`, `10209`, `10206` and `10215`, and
red takes Improved Wasp Officer `30602`. Blue strengthens its Research Center
to level 2 and holds attack Officer `20311` and defense Officer `20300`; red
strengthens its Energy Tower to level 1 and activates both Energy Tower skills.
Blue releases `missile_strike` at world `(55,60)`, the centre of red's local
`(-55,-60)` front Fang. Red releases `mobile_beacon` at local `(-55,-60)`,
`(-105,-90)` and `(-105,20)`, compiling to world `(55,60)`, `(105,90)` and
`(105,-20)`, so the selected Fang first retreats briefly away from the adjacent
Wasp and the strike point, then advances on the displaced line.

## Probes a topic reads

Some layouts were read by a topic's measurement and are not fights the
simulator reproduces, so they are layouts rather than fight documents. The
topic's readme says what each reading was:
`construction-shape.yaml` and `wall-line-width.yaml` in
[`../tests/construction/`](../tests/construction/README.md), and
`technology-disabled.yaml` and the four `technology-interval*.yaml` in
[`../tests/modifier/`](../tests/modifier/README.md).

`orbital-first.yaml` and `lightning-first.yaml` hold the same pair of battle
skill releases at the same two positions over a twelve-Crawler block and
differ only in which is declared first; [layout.md](../docs/spec/document/layout.md#battle_skills)
says what the game recorded of them.

## The attack interval's stagger

The three `stagger-*.yaml` measure **the random stagger on a unit's first
attack interval**, the rule
[`combat.md`](../docs/rules/combat.md#the-current-interval-carries-a-per-cycle-stagger)
states: one draw per member, in the order the recording numbers the
deployment, ascending world `z` then `x`, in that member's own
`interval_offset`, and no draw when the offset is zero. Each was read at tick
one, `derived.current_attack_interval`, and every reading is recomputed from
the seed without the game by `crates/simulation/src/fight/random.rs`:

| Layout | What it separates | Reading, seed 1787720817 |
| --- | --- | --- |
| `stagger-singles.yaml` | the order and the per-unit range | 55, 23, 56, 32 by ascending `z` |
| `stagger-three-marksmen.yaml` | a signed draw, one type held constant | 55, **65**, 56 against a description of 62 |
| `stagger-mustang-then-marksman.yaml` | a draw per member against one per formation | Marksman **72**, where per formation would be 55 |

The Rhino is red's unit in each: its offset is zero, so it reads its
description, and red draws from its own stream. How many draws the rest of a
fight takes from the stream is not measured.

## Layout replay coverage

A layout fought without a scene, as a
[layout replay](../docs/spec/document/layout-replay.md), fights as the Training
Ground fights it: every pinned fight under `tests/` hashes the same recorded
both ways. The fields no pinned fight holds were each recorded both ways once,
with one seed, and compared equal field by field:

| Layout | Field |
| --- | --- |
| `experience.yaml` | experience within a level |
| `contraptions.yaml` | contraptions, with gaps in their indices |
| `blueprints.yaml` | both enhancement chains |
| `magnetic-barrier.yaml` | the construction no corpus snapshot holds |
| `round-3.yaml` | a later round, with a settled flank unit |
| `orbital-first.yaml` | two battle skills released in order |
| `energy-tower.yaml` | Energy Tower skills |
| `travelling.yaml` | a travelling unit |
| `travelling-fitted.yaml` | travelling units upgraded, fitted and turned |
| `travelling-behind.yaml` | a travelling unit bought before another unit of the round |
| `delivered-squad.yaml` | a squad an officer hands out as the round opens |
| `airdrop-shields.yaml` | Shield Airdrops left standing |
| `oil.yaml` | an oil area left standing |

**A travelling unit is bought.** A unit the round opens with may not move from
round 2 on, and moving onto a flank from another region is what makes a unit
travel, so the replay buys it during the round and moves it. A purchase takes
the allocator's next index, whatever the record asks for, so the round buys
every unit from its first new index through the last travelling one, in order.

**A delivered squad is the layout's unit.** The snapshot is the side before
its round opens, so an officer whose schedule hands out a squad that round
hands it out again; the replay lets the delivery be the layout's unit, and
upgrades and moves it.

**An oil area's grid is not measured.** The Training Ground rebuilds an oil
line only from control points a release produced, and the corpus holds no oil
area an interception clipped, so `oil.yaml` is whole. A clipped grid is written
the way the replay reader decodes it, and a unit test holds the two to each
other; no fight has checked it.
