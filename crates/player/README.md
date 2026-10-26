# mechcore-player

Plays an MCFR recording back as a flat, top-down picture of the battlefield:
the map's towers, the constructions, the shields, the units and their shots,
moving and fighting as the recording says they did.

```bash
mechcore play crates/player/scenes/six-units.yaml
```

writes `six-units.html` in the system temporary directory: one file that
carries its style, its script and the whole fight, and plays offline in any browser. `play` takes
a recording too, and a layout or fight document it fights in memory first;
[cli.md](../../docs/spec/mechcore/cli.md#play) is its contract. This crate is
the library it plays with: [`timeline`](src/timeline.rs) lays any
`Recording` out for the page and [`page`](src/page.rs) writes it. Space plays and pauses, the arrows step a tick (a second with
Shift), the wheel zooms, a drag pans, and the bar at the bottom seeks, with
each side's losses marked along it.

A match file plays every completed deployment in one page. The round dropdown
immediately left of the speed selector chooses a round and starts its timeline
at tick 1, retaining speed and playback state. A trailing state without decisions
and a concession have no fight to play. Each round is simulated from its own
recorded deployment; the page does not advance automatically between rounds.

The download button pauses at the current integer tick and downloads a vector
image of the battlefield in the current pan and zoom, with the enabled life
bars, aim lines and damage numbers. It carries the ground, grid, buildings,
units and effects as paths, gradients and text, with no embedded bitmap.
The page's HTML controls and roster panels are not part of the image.
`window.player.snapshot(tick)` returns the same SVG for an integer tick
from 1 through the recording's last tick.

## What it shows and what it does not

The page reads only what the recording holds. `src/timeline.rs` turns the
recording's snapshots into one track per field of each object, and the page
draws between ticks, so it plays at any speed and seeks anywhere. A shot,
a blow or a death is animated from the event that records it, never inferred:
a weapon recoils when the recording releases its projectile, a melee unit
swings so its blow lands on the tick its damage does, and a construction,
whose facing the recording does not hold, turns its gun toward each shot it
fires. A shot flies its recorded path, its shadow cast by its height, and
looks as the effect the game flies for its weapon: a streak, a shell, a
missile, a jet of fire or a ball of light, in that effect's colours, and a
shot or a blow that splashes rings the ground it covers, as wide as its
weapon's skill splashed when it fired. A
weapon that strikes at range without a projectile, as a Vortex's or a
Melting Point's does, draws its blow from the attacker to its target, an
arc or a beam. The recording releases every shot at its unit's centre; the
skills of a weapon group, a Wraith's eight launchers and a Melting Point's
Energy Diffraction beams, leave from launchers of their own, the shot
carried out to its launcher by less as it nears where it lands.

A recording made with the `unit_pose` instrument channel also says how the
game drew each unit: the clip its model played and how far through. The page
then takes a unit's stance and the phase of its attack from that clip, so a
Rhino carries its chainsaws ahead to walk and lowers them to its sides to
fight, and slashes with its left arm and its right in turn, as the game
showed it. A recording with no poses, as every
fight the simulator makes, is animated from its units' motion and its events,
with the timings `scripts/player/pose-timing.py` reads off a recording that
has them: `SWING` and `ACTIONS` in `web/player.js`.
A Rhino's Whirlwind spins it once round with both chainsaws out, its blow
landing half way through the turn, and whirls about it out to the skill's
splash, as the game's TechAttack clip and its whirlwind effect do: `SPINS`.

A unit type with no sprite is still drawn, as a disc of its collision size
under its name. The ground is simplified, under each side's three deployment
regions dashed in its colour: its main half and the two flanks beside the
other side's, where a layout may place a formation, as `Region` in
`mechcore-document` bounds them; the page frames all six. Fire, oil, acid,
smoke and sandstorm are drawn on a five-metre grid. Recorded cut masks retain
their surviving cells, and whole circles use a radius template placed on the
same world lattice.
This is a display of the circle, not a change to its simulation shape.
The surfaces are kept quieter than the units on them: oil is a still,
near-black pool with a thin rim and faint sheens; fire is scorched ground with slow ember
beds, a hot edge and small, sparse flames and sparks.
Acid is a green pool with small bubbles; smoke has drifting grey-blue wisps;
sandstorm has warm haze, wind trails and blowing grit. Smoke is recorded as
`fog`, sandstorm as `fog_sand`. They remain separate materials when overlapping.
Smoke and sandstorm draw above ground and airborne units, with cloud-layer
opacity of 95% and 90% respectively, multiplied by their translucent textures.
Life bars, aim lines and damage numbers remain above the clouds. The other
terrain materials draw below units.
At each tick, same-type terrain cells are united before drawing: overlapping
circles have one surface and only the union's exposed edges get a rim.
Texture samples are anchored to world cells, so adding another circle does
not move the existing sheens or flames. Holes in cut masks stay empty.
Their appearance, ignition and disappearance follow the recorded snapshots,
and seeking or exporting SVG draws the same shapes. Recovery zones,
buffs and the other instrument channels are not drawn.

## The sprites

`web/sprites.js` draws each unit and building after the game's own model,
seen from straight above in its attack pose: the silhouette, the parts that
move in a fight, the white armour, the team colour where the model's texture
masks it, and each unit's emissive colour. The game's art never enters this
directory. [`scripts/player/model-views.py`](../../scripts/player/model-views.py)
renders the views the sprites follow from the installed game, into the
untracked `work/player/models/`, so a sprite is checked against its model
again whenever either changes.

The sprites are also files: [`assets/models/`](../../assets/models/README.md)
holds each one as SVG, written from this code by
`scripts/player/export-models.mjs`. That script runs `sprites.js` itself
against a context that records SVG, so a sprite draws only with the canvas
calls the script records. CI fails when a sprite changes and its file does
not.

`web/snapshot.js` is the Canvas drawing vocabulary used by both the model exporter
and the page's download button. Exporting a battle runs the page's normal frame
drawing with that context, including a vector version of the cached ground;
it does not assemble a separate illustration from the recording.

## The directory

| Path | Holds |
| --- | --- |
| `src/` | the timeline the page plays and the page that carries it |
| `web/` | the page's shell, style, sprites and player, built into the binary |
| `scenes/` | `six-units.yaml`, the demo the test fights: six of the units with sprites, each side's constructions and a shield |
| `tests/` | the demo and terrain shapes fought by the simulator and read back from the page; Canvas/SVG semantics and terrain masks, checked with `node --test crates/player/tests/*.test.mjs` |
