# Replay corpus

The native replays this repository is held to live in
[qbisi/mechcore-replay](https://github.com/qbisi/mechcore-replay), one
directory per game version, `replays/<version>/<name>.grbr`, added to and
never rewritten. That repository holds replays and nothing generated from
them, so it is read at `master`: nothing here pins a commit of it. The scripts
here make the corpus, fetch it, and read it:

| Script | What it does |
| --- | --- |
| `collect-replays.py` | watches live standard 1v1 matches unattended and keeps each one in the game's replay directory; it needs the game |
| `replay.py` | `sync` fetches the corpus into the untracked `work/replay/`; `publish` adds the replays this machine's game recorded under the installed version, and pushes them |
| `export-replay-corpus.py` | converts this checkout's version's replays into `work/match/<version>/`, writes each match back as a replay and converts it again, and fails on one that does not come back the same |
| `verify-matches.py` | runs `mechcore verify` over those matches and adds up the transition coverage |
| `match-replays.py` | fights every round of both replays, the corpus's and the one its match writes, in the game, and compares the two |
| `fight-coverage.py` | how many recorded rounds the simulator fights, and what it names as missing for the rest |
| `distance-report.py` | the two reports above as one Markdown table, beside an earlier commit's |

The test suite and the gate read no replay: the converter is not bound to
read every version the corpus holds, and a replay added there must not keep a
change from merging. [`corpus.yml`](../../.github/workflows/corpus.yml) reads
it outside the gate. On every pull request and master commit it fetches the
corpus, converts this version's replays, runs `fight-coverage.py` and
`verify-matches.py`, and keeps the reports; on a pull request it also keeps one
comment with `distance-report.py`'s table, compared with the master commit the
pull request is based on.

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

**A match document is generated and never hand-corrected.** A value that looks
wrong is a claim about the converter and belongs in
`crates/document/src/convert.rs`. Layouts built by hand, and the one captured
live from a replay's round, are in [`../../layouts/`](../../layouts/README.md).
