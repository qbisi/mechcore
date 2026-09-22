# Test fixtures

A fixture lives with the scripts that use it. Each research question is one
directory directly under `tests/`, named for what it studies, holding its
layouts, the `.mcscript` files that record and check them, and a readme that
says what was measured:

| Directory | What it holds |
| --- | --- |
| [`construction/`](construction/README.md) | what a construction becomes in a fight, and what attacks it |
| [`equipment/`](equipment/README.md) | where equipment corrections land and how they compose with officers |
| [`interval/`](interval/README.md) | how an attack interval is staggered |
| [`modifier/`](modifier/README.md) | how officers and technologies correct a unit's numbers |
| `skill-order/` | whether the release order of two battle skills changes the fight |
| [`turret/`](turret/README.md) | when a turret fires, at what, and how often |
| [`wraith/`](wraith/README.md) | how a Wraith's four slots choose their targets, given what the others hold |
| `regression/` | the native regression table, its layouts, and the three scripts that read it: offline, re-recorded, and re-recorded with each unit's skill state |

A script that needs no game is run by CI; one that needs the game is only
parsed there. `regressions.mcscript` in a topic directory, and
`regression/simulate.mcscript`, are the offline ones: they hold the simulator
to the physics hash the game recorded. Recordings never enter the repository;
what does is what reproduces them. The recordings a research question is
answered against are published as an oracle release by `scripts/oracle.py`,
which is not the repository either; [`.github/CONTRIBUTING.md`](../.github/CONTRIBUTING.md)
says how one is used.

Two kinds of fixture live outside `tests/` because no topic owns them. The
native replays, and everything converted from them, are in the corpus
[`../replay/`](../replay/README.md) pins. Layouts built by hand that no script
uses are in [`../layouts/`](../layouts/README.md).
