# mechcore-player

Plays an MCFR recording back as a flat, top-down picture of the battlefield:
the map's towers, the constructions, the shields, the units and their shots,
moving and fighting as the recording says they did.

```bash
mechcore play crates/player/scenes/six-units.yaml
```

writes `six-units.html` beside the layout: one file that carries its style,
its script and the whole fight, and plays offline in any browser. `play` takes
a recording too, and a layout or fight document it fights in memory first;
[cli.md](../../docs/spec/mechcore/cli.md#play) is its contract. This crate is
the library it plays with: [`timeline`](src/timeline.rs) lays any
`Recording` out for the page and [`page`](src/page.rs) writes it. Space plays and pauses, the arrows step a tick (a second with
Shift), the wheel zooms, a drag pans, and the bar at the bottom seeks, with
each side's losses marked along it.

## What it shows and what it does not

The page reads only what the recording holds. `src/timeline.rs` turns the
recording's snapshots into one track per field of each object, and the page
draws between ticks, so it plays at any speed and seeks anywhere. A shot,
a blow or a death is animated from the event that records it, never inferred:
a weapon recoils when the recording releases its projectile, a melee unit
swings so its blow lands on the tick its damage does, and a construction,
whose facing the recording does not hold, turns its gun toward each shot it
fires.

A unit type with no sprite is still drawn, as a disc of its collision size
under its name. Terrain, buffs and the instrument channels are not drawn.

## The sprites

`web/sprites.js` draws each unit and building after the game's own model,
seen from straight above in its attack pose: the silhouette, the parts that
move in a fight, the white armour, the team colour where the model's texture
masks it, and each unit's emissive colour. The game's art never enters this
directory. [`scripts/player/model-views.py`](../../scripts/player/model-views.py)
renders the views the sprites follow from the installed game, into the
untracked `work/player/models/`, so a sprite is checked against its model
again whenever either changes.

The sprites are also files: [`models/`](../../models/README.md) at the top of
the repository holds each one as SVG, written from this code by
`scripts/player/export-models.mjs`. That script runs `sprites.js` itself
against a context that records SVG, so a sprite draws only with the canvas
calls the script records. CI fails when a sprite changes and its file does
not.

## The directory

| Path | Holds |
| --- | --- |
| `src/` | the timeline the page plays and the page that carries it |
| `web/` | the page's shell, style, sprites and player, built into the binary |
| `scenes/` | `six-units.yaml`, the demo the test fights: the six units with sprites, each side's constructions and a shield |
| `tests/` | the demo fought by the simulator and read back from the page |
