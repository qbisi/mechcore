# Test fixtures

A fixture lives with the scripts that use it. Each research question is one
directory directly under `tests/`, named for what it studies, holding its
layouts, the `.mcscript` files that record and check them, and a readme that
says what was measured:

| Directory | What it holds |
| --- | --- |
| [`construction/`](construction/README.md) | what a construction becomes in a fight, and what attacks it |
| [`interval/`](interval/README.md) | how an attack interval is staggered |
| [`modifier/`](modifier/README.md) | how officers and technologies correct a unit's numbers |
| `skill-order/` | whether the release order of two battle skills changes the fight |
| `regression/` | the native regression table, its layouts, and the two scripts that read it |
| [`grbr/`](grbr/README.md) | the tracked native replays |
| `battle/` | the battle documents converted from `grbr/` |
| [`layouts/`](layouts/README.md) | layouts no script here uses: fixtures for the CLI and for live checks |

A script that needs no game is run by CI; one that needs the game is only
parsed there. `regressions.mcscript` in a topic directory, and
`regression/simulate.mcscript`, are the offline ones: they hold the simulator
to the physics hash the game recorded. Recordings never enter the repository;
what does is what reproduces them.

Playing native replays back through the game is not a test and does not live
here: [`../replay/`](../replay/README.md) holds those scripts.
