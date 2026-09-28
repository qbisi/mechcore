# Test fixtures

A fixture lives with the scripts that use it. Each research question is one
directory directly under `tests/`, named for what it studies, holding its
layouts, the `.mcscript` files that record and check them, and a readme that
says what was measured:

| Directory | What it holds |
| --- | --- |
| [`adapter/`](adapter/README.md) | whether the Adapter still installs, reads back and records every part of a layout, and every instrumentation profile |
| [`construction/`](construction/README.md) | what a construction becomes in a fight, and what attacks it |
| [`equipment/`](equipment/README.md) | where equipment corrections land and how they compose with officers |
| [`interval/`](interval/README.md) | how an attack interval is staggered |
| [`layout-replay/`](layout-replay/README.md) | whether a layout fought without a scene fights as the Training Ground fights it |
| [`level/`](level/README.md) | how a unit level scales base life and damage before overlays |
| [`modifier/`](modifier/README.md) | how officers and technologies correct a unit's numbers |
| `skill-order/` | whether the release order of two battle skills changes the fight |
| [`tower/`](tower/README.md) | what losing a tower writes on its side, and what strengthening one adds |
| [`turret/`](turret/README.md) | when a turret fires, at what, and how often |
| [`units/`](units/README.md) | each unit's standard layouts, the definition of basic support |
| [`wraith/`](wraith/README.md) | how a Wraith's four slots choose their targets, given what the others hold |
| [`regression/`](regression/README.md) | fights that exercise the kernel rather than one rule, and the three scripts that read them: gameless, re-recorded, and re-recorded with each unit's skill state |

**A pinned fight is a [fight document](../docs/spec/document/fight.md)** in its
topic's `fights/` directory: the layout, the seed, and what the game's
recording of them decided, its tick count and hash among it, in one file. A
comment at the top of the file says what the fight measures. A fixture's
`source` is `recording`: a fight the simulator computed states what is being
checked, and is never a fixture. A layout stays a layout file only while
something reads it as a layout: a probe no fight pins, or a stage for
`game.apply_layout`.

A script that needs no game is run by CI; one that needs the game is only
parsed there. `regressions.mcscript` in a topic directory, and
`regression/simulate.mcscript`, are the gameless ones: each runs `verify` over
its `fights/`, which fights every layout with its seed through the simulator
and holds it to what the document states. Recordings never enter the
repository, and are not published anywhere else: what the repository keeps is
what reproduces them, the fights and the scripts, which any machine with the
game records again. The Research section of [`AGENTS.md`](../AGENTS.md) says
who records them and where a pinned hash may come from.

To pin a fight, record it where the game runs, read the recording back as a
fight, and put the document under its topic's `fights/`:

```sh
mechcore game record <layout> /tmp/mechcore/<topic>/<name>.mcfr --seed <seed>
mechcore convert /tmp/mechcore/<topic>/<name>.mcfr --to fight tests/<topic>/fights/<name>.yaml
```

Then delete its `game_build` line: a document that states none is read
against the build [`GAME_VERSION`](../GAME_VERSION) names, which is the only
place a version is written. Otherwise the file stays as `convert` wrote it,
and a comment added above it does not change what it states. The topic's
`regressions.mcscript` finds it by its directory; nothing else lists it.

Two kinds of fixture live outside `tests/` because no topic owns them. The
native replays, and everything converted from them, are in the corpus
[`../replay/`](../replay/README.md) pins. Layouts built by hand that no script
uses, and the one the crates' tests read as a plain layout, are in
[`../layouts/`](../layouts/README.md).
