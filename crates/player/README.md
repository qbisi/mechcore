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
and previous/next round buttons choose a round and start its timeline
at tick 1, retaining speed and playback state. A trailing state without decisions
and a concession have no fight to play. Each round is simulated from its own
recorded deployment; the page does not advance automatically between rounds.
Round navigation is disabled at the corresponding first/last boundary, and
the previous/next buttons are hidden for a single round. The lower controls
overlay the map on a transparent-to-dark gradient: a full-width seek bar
above the button row shows elapsed progress, loss marks and the settlement
tail. The row contains playback, round navigation, elapsed/total time, round
selection, speed, display settings, fit and SVG export. Life bars, damage
numbers and aim lines live in a settings popover, retaining their keyboard
shortcuts. Clicking outside or pressing Escape closes the popover.
The controls fade out after two seconds without activity and reappear on
pointer movement, pointer press or keyboard input. Hovering or focusing the
controls, or leaving settings open, keeps them visible.

The top HUD shows blue and red reactor cores with the round and elapsed time
between them. Match playback uses each state's core health and the map's
initial health with opening team and specialist corrections as its maximum.
Only current health is written inside each bar, aligned to the outer edge.
The frame and attached name plate are original SVG geometry inspired by the
native HUD, without copying or tracing its artwork. `web/hud.js` constructs
the border for its actual width: the middle length changes while bevels,
rails, stroke widths and the 56-unit overall height stay fixed in design units. A resize
observer updates each SVG viewBox and path. The right frame is mirrored.
Blue and red fills use CSS gradients and a repeating SVG stripe tile with a
fixed 24-unit pitch, so neither changing bar length nor losing health
stretches its texture. The centre places round, a vector clock icon and elapsed
time in one row over a separately sized angular panel.
The whole top HUD scales uniformly with the map's natural fitted view,
using 1.25 CSS pixels per world metre as its design reference. Borders,
text, texture pitch and the timer scale together when the viewport or a
round's field bounds change. Interactive camera zoom and pan do not change
that natural scale. Available viewport width still bounds bar length, and
the lower playback controls retain their own responsive layout.
The rendered ground extends 30 metres beyond each map edge; deployment
regions and simulation bounds stay unchanged. The default camera centres
this ground and fits its height exactly to the browser, without reserving
space for the overlaid HUD or playback controls. Narrow windows may crop
the sides; manual camera movement remains unrestricted.
Numeric text uses a subset of native Xolonium-Regular;
names retain the page's system font. Names over ten grapheme clusters keep their first five and last
four, separated by `*`, with the full name in the badge's title and accessible
label. The maximum still determines the fill and remains in the meter's
accessibility metadata.
Below each name plate, a card button counts that side's held officers.
Hovering or focusing it expands a downward list of their native English names
and effect descriptions. It follows the current round's projected layout,
including officers acquired during that deployment, and preserves duplicates.
An empty list says `No officers`. The text is presentation only, extracted
from the local build's OfficerData localization by `scripts/player/officers.py`
into `web/officers.json`, with description parameters resolved and rich-text
tags removed. The offline page embeds only the officers held by each side.
A supply card sits outside the officer button (blue left, red right), using
the same natural map scale. Its SVG symbol redraws the native `Icon_Supply`'s
gold hexagon and angular cutout without embedding the source bitmap.
Match playback displays supply remaining after
the completed deployment, keeping it fixed during combat and updating it on
round selection. Inputs without economy state display zero supply.

Closed rectangular drawers run down the blue and red edges below the card
buttons. Each shows the native portrait and technology icons without a unit-name
caption. A square formation-count badge sits inside the blue card's upper-left
corner and the red card's upper-right corner. It counts held deployment
formations, including reinforcements, and remains fixed during combat; an
unlocked type without a formation shows zero. Technologies follow each side's match
loadout in its original order: four icons form two rows of two, six form two
rows of three. Larger loadouts widen the drawer, retaining two rows and every
icon. Active technologies light up; inactive choices are dimmed and desaturated.
Frames and positions mirror; artwork stays upright. The drawers do not open.
Each side scrolls independently, including with the keyboard. Technology hover
or focus shows its native name, effect and activation state. Officer popovers
appear above the drawers. Drawer sizes use the core HUD's natural map scale.
Compact portraits and two technology rows sit inside frames whose outer edges
align with the core frames, sharing their 12-pixel viewport inset.

Types are sorted by their document names and stay fixed during combat. Match
playback includes completed deployment unlocks, placed formations and researched
types. Layouts, fights and recordings have no account loadout or unlock list:
they display only known formations and active technologies, without inventing
unrecorded choices.
Inside each unit column, two compact icon strips show battlefield skill slots
and unequipped inventory. Match skills preserve slot order and duplicates.
Available skills are bright, released skills gold, used deployment skills dim,
and cooling skills dim with their remaining round count. These are deployment
states, not timers inferred from combat. Inventory excludes fitted items and
shows each unequipped item separately in a circular frame, preserving inventory
order, duplicates and individual durability without quantity badges. Hover or focus
shows native names, effects and status. Each strip scrolls independently and
has no activation or equipment action. Empty strips disappear.
Either strip can stand alone: its container shrinks to one column, anchored
at the edge nearest the unit roster even after scaling. Both empty hides the
whole container. Standalone
inputs show only their known current-round releases, with no invented unused
skills or inventory. The strips follow natural map scale and mirror placement.
`scripts/player/arsenal.py` extracts native front-facing skill symbols and
equipment thumbnails into `web/arsenal.json`; each page embeds used images once.
The list resets when selecting a round. `scripts/player/unit-panels.py`
extracts 2x WebP portraits and technology icons plus native descriptions
into `web/unit-panels.json`. Each page embeds only needed images, once across
all rounds. These raster UI assets are outside the battlefield SVG export;
the battlefield sprites and drawer frames remain separately drawn vectors.
`web/hud-font.css` is generated by `scripts/player/hud-font.py`: a numeric
WOFF2 subset embedded for offline use, with the original font metadata.
Regeneration and its `--check` need the installed game, UnityPy, FontTools
and Brotli, as its docstring says. Border and texture design need none of
those tools or the original game artwork.
Layout playback assumes one point per side, only for presentation. Independent
fight documents and MCFR recordings have no starting core health: their bars
are dimmed and their numeric health is unknown. Actual settled damage is read
from the recording for every input, as the opposing team's settlement score.
Core health stays fixed through the final recorded tick. A short presentation
tail freezes that frame while survivors' score values fly to the opposing
core and reduce it on arrival. Values use the shared reactor-damage formula,
including summoned, reborn and controlled units, and must sum to the recorded
settlement; when attribution fails, a neutral team-total effect replaces the
unit effects, with a CLI warning. Smaller values launch first, following the
native `ScoreAttackController.PerformNormalAttack` order. The 0.4-second lead-in,
at most 1.2-second launch spread and value-dependent 0.7–1.1-second flights are
presentation choices. Both sides can score in a timeout. No simulation ticks
or events are added, and the reported recording duration remains unchanged.
The seek bar includes a tinted tail; the combat clock stops and the lower
clock labels this interval as settlement. Flights, impacts and deductions are
functions of the playhead, so speed changes, seeks and exports agree.
Health numbers retain negative results after overkill; the bar fill stops at
zero. Damage labels retain the full damage value. A
backwards seek and a round change restore that round's starting health.
The former surviving-unit roster panels and battle-information box are not
shown. The file's title stays in the browser tab and the SVG download name.

The download button pauses at the current integer tick and downloads a vector
image of the battlefield in the current pan and zoom, with the enabled life
bars, aim lines and damage numbers. It carries the ground, grid, buildings,
units and effects as paths, gradients and text, with no embedded bitmap.
The page's HTML controls and core HUD are not part of the image.
`window.player.snapshot(frame)` returns the same SVG for an integer frame
from 1 through `window.player.endTick`, including the presentation tail.
Frames through the recording's last tick are the original combat ticks;
later frames freeze that tick and draw the settlement effects. `player.tau`
remains the combat tick, while `player.playhead` includes the tail.

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
explosion or damage forecast. Circles of at least 100 metres in radius add a
broken outer rail, graduated ticks and longer cardinal marks; the continuous
range contour stays at the configured effect radius.

Electromagnetic Impact, Electromagnetic Blast, Photon Emission and Shield
Airdrop use the same circular release previews and native front-facing icons.
Their ranges are 60, 130, 110 and 70 metres, and their countdowns are 2.7, 2.7,
0.6 and 1 seconds, read from the buff and shield configuration tables. The
preview clears after that nominal release delay, under the same zero-jitter
policy as single impacts. It shows the committed area, without predicting
which units receive a buff or how shields intercept it. It does not persist
for the buff's duration or draw a second shield: actual shields continue to
follow the recording. These additions complete the combat single-point skill
previews; the three-point Mobile Beacon and deployment-only unit operations
are outside this category.

Mobile Beacon and its card variant carry their three committed control points
as two ordered path segments. During combat, each leg has a faint capsule
frame with true semicircular ends. Its 40-metre radius and 80-metre total
width follow `CommanderSkillRangeView.ShowExtraView`, which doubles
`GetSubEffectRange`. Pairs of chevrons at a 46-metre pitch scroll in control-point order inside
each capsule, below units and shields, without an icon, countdown or
endpoint icon. At the route's first and last points, the inward semicircle
completes the capsule end into a full circle without drawing its outer half
twice. The first circle marks the initial selection extent; the last is an
arrival reference and does not select units again. They adapt the native
`CommanderSkillRangeView_WayPointInFight` line, noise and scrolling-arrow
layers (`P_line_01/02`, `P_noise_03`, `P_arrow_01`) as vector strokes.
The chevrons are clipped to each capsule and share a continuous distance
phase over both segments, including reversed releases. The frames stay
separate at the bend. The outlines are static, without a travelling reflection. This is a
stylized instruction path, not a prediction of each unit's offset trajectory.
The 18-metre-per-second texture speed, 23% base outline opacity and at most
19% texture opacity are presentation choices.
The path disappears when all initially selected units have completed it or
died. MCFR has no command track: this display reconstructs completion from
recorded positions, retaining each unit's initial offset and checking the
ordered targets 20 metres past each segment end, within 20 metres plus the
unit's collision radius, as `MoveAttackCommand.Perform` does. Positions are
rounded to centimetres for display, so this is a presentation estimate rather
than a recorded command event. Later summons and rebirth do not join the path;
overlapping releases give a unit to the first matching path. The end tick is
computed once, so seeking backwards restores the same view and SVG export
agrees. An empty selection draws nothing. Coincident points skip
their zero-length segment, and an entirely coincident route draws nothing.

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

`web/skill-icons.js` contains all twenty native deployment symbols, traced
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
