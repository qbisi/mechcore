# Replays

The native replays this repository is held to live in
[qbisi/mechcore-replay](https://github.com/qbisi/mechcore-replay), one
directory per game version, `replays/<version>/<name>.grbr`, added to and
never rewritten. That repository holds replays and nothing generated from
them, so it is read at `master`: nothing here pins a commit of it.

| File | What it is |
| --- | --- |
| `record-standard-1v1.mcscript` | how new replays are made: it watches live standard 1v1 matches unattended and keeps each one; it needs the game, so CI only parses it |

`scripts/replay.py sync` fetches the corpus into the untracked `work/replay/`.
The version this checkout describes selects its directory, and
`scripts/export-replay-corpus.py` converts those replays into
`work/battle/<version>/`, where `scripts/verify-battles.py` and
`scripts/fight-coverage.py` read them. Neither the test suite nor CI reads a
replay: the converter is not bound to read every version the corpus holds, and
a replay added there must not turn this repository's CI red.

**A replay is evidence.** It is copied from the Steam installation byte for
byte and never rewritten. Only a locally recorded replay is admitted: the
downloaded class, named `<version>\<id>.rep.grbr` and carrying `Seat` -1, is a
server-side reconstruction whose `IsSpecialSupply` and blueprint chains do not
agree with the game's state.

**A replay's version is its directory.** The game's version string is what the
server matches players and admits spectators by, and every client of a match
runs the same simulation, so it is the unit of one set of rules. Steam can ship
new files under an unchanged version; the replays from before and after such an
update share a directory. A replay is filed under the version of the game that
recorded it; its header carries that version's last component (`2324`).

**A battle document is generated and never hand-corrected.** A value that looks
wrong is a claim about the converter and belongs in
`crates/document/src/convert.rs`. Layouts built by hand, and the one captured
live from a replay's round, are in [`../layouts/`](../layouts/README.md).
