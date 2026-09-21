# Replays

Native replays and what is converted from them, kept together because each
one is a function of the one before it:

| Directory | What it holds | How it is made |
| --- | --- | --- |
| [`grbr/`](grbr/README.md) | the tracked native replays, byte for byte as the game wrote them | copied from the Steam installation, never rewritten |
| [`battle/`](battle/README.md) | one battle document per replay | `mechcore replay convert`, over all of them by `scripts/export-replay-corpus.py` |
| `layout/` | the position one round of a battle opens its fight from | `mechcore doc project <battle> --round <n>` |

Nothing here is a test of its own. CI requires `battle/` to be exactly what
the converter writes from `grbr/`, and `scripts/verify-battles.py` runs
`mechcore doc verify` over every battle; that is a different process from the
tests under [`../tests/`](../tests/README.md). Layouts built by hand, rather
than converted, are in [`../layouts/`](../layouts/README.md).

`record-standard-1v1.mcscript` is how new replays are made: it watches live
standard 1v1 matches unattended and keeps each one. It needs the game, so CI
only parses it.
