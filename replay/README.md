# Replay scripts

Scripts that drive the game through native replays rather than through a
layout. They are a workflow of their own, beside the tests and not part of
them: nothing here is run by CI beyond being parsed, because every one needs
the game.

| Script | What it does |
| --- | --- |
| `roundtrip-bork-map.mcscript`, `roundtrip-indices.mcscript`, `roundtrip-tuff.mcscript` | record one round of a tracked replay, then play the layout embedded in that recording on the Training Ground and require the same fight |
| `record-standard-1v1.mcscript` | watch live standard 1v1 matches unattended and keep each as a replay |

Their replays are the tracked ones in [`../tests/grbr/`](../tests/grbr/README.md).
`mechcore doc verify` checks the battle documents converted from those replays;
that is a different process from this one, and `scripts/verify-battles.py`
runs it.
