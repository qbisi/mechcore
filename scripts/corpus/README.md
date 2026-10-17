# Replay corpus

New collected match documents are published to
[qbisi/mechcore-replay](https://github.com/qbisi/mechcore-replay), under
`matches/<version>/<BattleID>-rN.yaml`. Complete prefixes and partial matches
share that directory. The repository is append-only and read at `master` by
`replay.py sync` into `work/replay/`. Native GRBR files are local inputs for
recording studies; the earlier native archive remains in Git history.

| Script | What it does |
| --- | --- |
| `collect-replays.py` | enters live standard 1v1 rooms at round 2 or later, saves the native cache, leaves, then converts it to `BattleID-rN.yaml` in `work/watch/` (or `--output-dir`); it needs the game |
| `replay.py` | `sync` fetches the corpus into the untracked `work/replay/`; `path` prints its versioned match directory; `publish --source <dir>` validates and uploads collected match YAML, and `--dry-run` checks without publishing |
| `export-replay-corpus.py` | converts this checkout's version's replays into `work/match/<version>/`, writes each match back as a replay and converts it again, and fails on one that does not come back the same |
| `verify-matches.py` | runs `mechcore verify` over those matches, adds up the transition coverage, and lists every round the simulator fights whose result differs from the match, with whether `tests/corpus/` pins it, and every round it refuses, each match's last round fought on its own |
| `match-replays.py` | fights every round of both replays, the corpus's and the one its match writes, in the game, and compares the two |
| `corpus-fights.py` | fights every corpus round in the game into the fight document it records, under `work/fight/<version>/`, each a fixture candidate for `tests/corpus/`; it needs the game |
| `divergence-issue.py` | the rounds a master commit fights wrong or refuses that its parent did not, as one issue naming the pull request it merged, from the two runs' `verify-matches.py` reports |

Corpus checks are manual research tools; there is no automatic corpus CI job
or automatic divergence issue publication. The gate continues to verify the
tracked fight fixtures. `verify-matches.py --match-dir <dir>` can measure a
collected match directory without fetching or converting native replays.
The scripts that export, compare or record GRBRs need a local native archive;
partial matches do not reconstruct missing native history.

Server downloads are also local inputs: the game keeps the original protobuf
as `.rep.bak` and writes its reconstructed GRBR as `.rep.grbr`. Convert the
latter with `mechcore convert <download.rep.grbr> --to match <match.yaml>`.
The converter restores the server snapshot conventions described in
[`match.md`](../../docs/spec/document/match.md#server-snapshot-reconstruction),
including paid income and omitted action operands; it checks the random
streams and offers rather than treating `Seat = -1` as grounds for refusal.
Only the resulting match YAML is eligible for corpus publication.

`collect-replays.py --duration-seconds 120` collects for two minutes and finishes
the last room's cleanup. It does not wait for the round or match to finish, or
write to the game's Replay directory. `game record --watch` itself saves only
native GRBR. After it returns, the script converts that file, atomically publishes
the match without overwriting, and removes GRBR on success. Conversion failure
stops collection and retains GRBR for diagnosis. `N` is the captured spectator round,
which may lag the lobby round. The output directory's `.watch-visits.json`
excludes rooms for 60 seconds after leaving and gives unvisited rooms priority;
an existing `BattleID-rN.yaml` is a duplicate and is never overwritten. These
partial match captures stay in the work directory until an explicit
`replay.py publish --source <dir>` uploads them.

**A match's version is its directory.** Publication files new matches under
`matches/<GAME_VERSION>/`. `publish --source <dir>` reads that checkout's
version, so the source must contain only captures made under it; an explicit
`game_build` naming another build is refused. Names must be `BattleID-rN.yaml`,
and the last captured state must be round N. An existing name must have the
same bytes, and publication never replaces one with other bytes. Native files
are not uploaded by this command.

**A match document is generated and never hand-corrected.** A value that looks
wrong is a claim about the converter and belongs in
`crates/document/src/convert.rs`. A crate's tests keep any synthetic layout
inputs they need under that crate's `tests/fixtures/`.
