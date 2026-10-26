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

The page reads combat state from the recording. `src/timeline.rs` turns the
recording's snapshots into one track per field of each object, and the page
draws between ticks, so it plays at any speed and seeks anywhere. A unit's shot,
blow or death is animated from the event that records it, never inferred:
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

Incendiary Bomb, Sticky Oil Bomb, Acid Blast, Smoke Bomb and Ion Blast show their
committed release previews from the embedded layout. A rounded rectangle
follows the two control points: its length is their distance plus two terrain
radii, and its width is the terrain diameter. It frames the extent of the
impact circles with a faint team-coloured fill and accented end caps,
rather than following the terrain cells. A reflection sweeps along its
contour from the first release point towards the last, including reversed
and diagonal releases. The two control points are marked inside the end caps.
The preview does not predict a shield's future interception or terrain
clipping; the recorded terrain shows what actually landed.
A world-sized, upright vector symbol at the midpoint has one quiet ring
and a countdown arc on that ring. The glyphs share an optical size
independent of effect radius; the symbol and its ring scale together with
the battlefield when zooming. All five use the game's front-facing card
silhouettes: `SW_Fire_f`, `SW_Oil_f`, `SW_Acid_f`, `SW_Fog_f` and `SW_Ion_f`.
They are traced from their alpha channels into cubic vector paths and tinted
in their team's colour. The `*_SBUI` artwork is already skewed for the native
ground view and is not used. Native bitmaps are not embedded in the page or snapshots.

Radius, delay, interval and effect count come from the build's existing
`config/commander_skill_effects.yaml` table. Each preview stays separate from
other releases, even where their resulting terrain merges. Standing oil has
no new release preview. Native unrecorded presentation jitter is set to zero:
line previews clear after their start delay plus interval times count, with the
same deterministic result when seeking and exporting SVG. Playback starts
with the committed deployment; it does not invent cursor or placement history.

Underground Threat, Rhino Assault, Wasp Swarm, Mobilize Battleship and Vulcan's
Descent show circular summon previews with their native front-facing symbols.
Their deployment radii are 32, 20, 25, 25 and 25 metres; a single-unit summon
uses that deployment range, not a scatter prediction. Their countdown adds
the native creator's one-second `APPEAR_DURATION` to the configured 1.3-second
delay and fades out as the first units arrive. Later summon batches are shown
by their recorded units rather than extending the release preview.

Every recorded `unit_created` becomes a creation cue. Once that unit reaches
its first visible snapshot, its sprite and shadow fade in and grow to full
size over 0.75 seconds, accompanied by a ground ring and dust for ground units
or short light trails for aircraft. This applies equally to commander summons,
production lines and units generated on death. A summon's creation may precede
its first snapshot by a second: the effect waits for visibility and never
draws a unit early. Deployed units have no creation cue, and a returning unit
under its existing identity does not replay this first-appearance animation.
The animation changes no recorded position, height, life or attack timing;
seeking and SVG export use the same deterministic drawing.

Missile Strike, Heavy missile strike, Nuke and Orbital Javelin show circular
release previews at their recorded target positions. Their range radii are
40, 40, 100 and 30 metres. A quiet clockwise reflection follows the range
contour at the same progress as the central native symbol's countdown ring:
3 seconds for the missiles and javelin, 15 seconds for Nuke. The preview clears at the
configured release time under the same zero-jitter policy. This adds no new
explosion or damage forecast. Other point-skill categories beyond the damage
and summon previews described here are not drawn yet. Circles of at least 100 metres in radius add a
broken outer rail, graduated ticks and longer cardinal marks; the continuous
range contour stays at the configured effect radius.

Orbital Bombardment and Lightning Storm preview their full 130-metre circular
footprint with `SW_Rockets_f` and `SW_Thunder_Storm_f`. The native range view
uses `effectRange`, which already includes each 30-metre sub-effect: its
random impact centres lie within 100 metres. Their configured delay is three seconds.
The native random-damage view hides its main frame and symbol at its first
sub-effect; the player fades them out over 0.25 seconds from the reconstructed
first arrival, independently of the 15 or 30 repeated strikes.

`mechcore play` reconstructs these skills' ground-arrival plans by initializing
the simulator from the embedded layout and seed, without advancing the fight.
It uses the existing placement code and side streams, seeded from
`(round + team) * 4444` and consumed in release order before the units' first
attack-interval draws. Repeated releases keep separate plans. The page carries
the planned coordinates and arrival ticks; it does not draw another random
sequence or interpolate between scattered points. A bombardment draws a warm
flash and expanding blast ring, while a storm draws a brief lightning bolt
and electrical traces. Each 30-metre circle appears only when its planned
strike arrives, including strikes that hit nothing. Seeking and SVG export
produce the same effects.

These are simulator-reconstructed ground arrivals, not recorded commander
events. A shield may intercept a falling effect before it reaches that point;
the forecast does not reconstruct interception. Shield state, damage and buffs
remain those of the recording. If the current simulator cannot initialize the
embedded layout, `play` warns and retains the release preview alone, timed by
its nominal configured delay. MCFR is unchanged.

`web/skill-icons.js` contains all sixteen native deployment symbols, traced
from the front-facing high-resolution `iconName + "_f"` alpha silhouettes
by `scripts/player/skill-icons.py`. The four single-impact skills use
`SW_Rockets_Little_f`, `SW_Rockets_Heavy_f`, `SW_Nuclear_Bomb_f` and
`SW_Orbital_Javelin_f`.
The script requires the installed game, its matching local data export and
Potrace; its docstring gives the `uv run --with` invocation. Original bitmaps
and intermediate traces stay local under `work/player/skill-icons/`.

Ion Blast additionally displays a forecast of its moving strike area. This
is presentation inferred from the committed layout and build configuration,
separate from the event-driven weapon impacts above. Its 20-metre circle
moves smoothly along the 60 configured impact points, starting after the
three-second delay, with a short flash at each 0.3-second interval and a
fading residual lasting at most 0.6 seconds. The placement band and central
symbol dim while the forecast moves. All of these ground overlays draw below
the units, and their radius remains in world metres when zooming.

MCFR does not identify commander sub-effects or record empty impacts and
their shield-interception positions. The forecast therefore does not claim
observed damage timing or predict an interception. Shield state and damage
feedback continue to come from the recording; a forecast flash creates no
damage event or damage number. The Ion forecast uses nominal configuration time,
so it does not reproduce the simulation's fixed-point update offsets.

## The sprites

`web/sprites.js` draws each unit and building after the game's own model,
seen from straight above in its attack pose: the silhouette, the parts that
move in a fight, the white armour, the team colour where the model's texture
masks it, and each unit's emissive colour. Original model meshes and textures
are not embedded here. [`scripts/player/model-views.py`](../../scripts/player/model-views.py)
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
