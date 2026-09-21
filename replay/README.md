# Replay scripts

Scripts that drive the game through native replays rather than through a
layout. They are a workflow of their own, beside the tests and not part of
them: nothing here is run by CI beyond being parsed, because every one needs
the game.

| Script | What it does |
| --- | --- |
| `record-standard-1v1.mcscript` | watch live standard 1v1 matches unattended and keep each as a replay |

The replays it keeps are what [`../tests/grbr/`](../tests/grbr/README.md) is
copied from. `mechcore doc verify` checks the battle documents converted from
those replays; that is a different process from this one, and
`scripts/verify-battles.py` runs it.
