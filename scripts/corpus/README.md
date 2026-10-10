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
| `verify-matches.py` | runs `mechcore verify` over those matches, adds up the transition coverage, and lists every round the simulator fights whose result differs from the match, with whether `tests/corpus/` pins it, and every round it refuses, each match's last round fought on its own |
| `match-replays.py` | fights every round of both replays, the corpus's and the one its match writes, in the game, and compares the two |
| `corpus-fights.py` | fights every corpus round in the game into the fight document it records, under `work/fight/<version>/`, each a fixture candidate for `tests/corpus/`; it needs the game |
| `divergence-issue.py` | the rounds a master commit fights wrong or refuses that its parent did not, as one issue naming the pull request it merged, from the two runs' `verify-matches.py` reports |

The test suite and the gate read no replay: the converter is not bound to
read every version the corpus holds, and a replay added there must not keep a
change from merging. [`corpus.yml`](../../.github/workflows/corpus.yml) reads
it outside the gate, on master commits alone. On each it fetches the corpus,
converts this version's replays, runs `verify-matches.py` and keeps its
reports; `divergence-issue.py` compares them with the parent commit's, and the
workflow opens one issue for the rounds the commit newly fights wrong or
refuses and comments on the pull request it merged with the issue's link.

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
