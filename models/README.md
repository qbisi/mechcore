# Models

The player's sprites as SVG files: each unit and building it draws, seen from
straight above in its rest pose. Every file here is written by
[`scripts/player/export-models.mjs`](../scripts/player/export-models.mjs) from
[`crates/player/web/sprites.js`](../crates/player/web/sprites.js), and none is
edited by hand. The sprites are the source: a shape that looks wrong is
changed there, and the script writes the files again.

```bash
node scripts/player/export-models.mjs
```

The docs workflow runs it with `--check`, which fails when a file differs from
what the sprites draw, so a sprite and its file cannot drift apart.

A file is `<sprite>.svg`, in the first team's blue; the page draws the other
team the same, red where these are blue. Lengths are metres with the front up
the page, rendered at 16 pixels to the metre, so the files are to scale with
one another. The sprites are drawn after the game's models, assembled and
posed as [`scripts/player/model-views.py`](../scripts/player/model-views.py)
renders them, but they are drawings of their own: no geometry or texture of
the game is in them.

| Sprite | Name | |
| --- | --- | --- |
| `marksman` | Marksman | ![](marksman.svg) |
| `arclight` | Arclight | ![](arclight.svg) |
| `rhino` | Rhino | ![](rhino.svg) |
| `crawler` | Crawler | ![](crawler.svg) |
| `sledgehammer` | Sledgehammer | ![](sledgehammer.svg) |
| `wasp` | Wasp | ![](wasp.svg) |
| `energy_tower` | Energy Tower | ![](energy_tower.svg) |
| `research_center` | Research Center | ![](research_center.svg) |
| `anti_armor_turret` | Anti-Armor Cannon | ![](anti_armor_turret.svg) |
| `rapid_fire_turret` | Rapid-Fire Cannon | ![](rapid_fire_turret.svg) |
| `defensive_wall` | Defensive Wall, one block | ![](defensive_wall.svg) |
