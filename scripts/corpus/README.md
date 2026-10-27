# Replay corpus

New collected match documents are published to
[qbisi/mechcore-replay](https://github.com/qbisi/mechcore-replay), under
`matches/<version>/<BattleID>-rN.yaml` for spectator captures and
`tournaments/<date>-<region>/<BattleID>.yaml` for server tournament downloads.
Complete prefixes and partial spectator matches share the versioned directory.
The repository is append-only and read at `master` by
`replay.py sync` into `work/replay/`. Native GRBR files are local inputs for
recording studies; the earlier native archive remains in Git history.

| Script | What it does |
| --- | --- |
| `default-loadout.py` | ranks current-version player loadout choices, counting each match's two sides once, and prints the CLI's default four or six technologies per unit |
| `collect-replays.py` | enters live standard 1v1 rooms at round 2 or later, saves the native cache, leaves, then converts it to `BattleID-rN.yaml` in `work/watch/` (or `--output-dir`); it needs the game |
| `tournament-replays.py` | lists recent tournaments with dates and regions, then downloads every subdivision and ranking page of one tournament ID through the Adapter and converts unique replays to `<date>-<region>/<BattleID>.yaml`; `download --publish` uploads after success, and offline `publish` uploads collected tournaments; listing and downloading need an online game |
| `replay.py` | `sync` fetches the corpus into the untracked `work/replay/`; `path` prints its versioned match directory; `publish --source <dir>` validates and uploads collected match YAML, and `--dry-run` checks without publishing |
| `export-replay-corpus.py` | converts this checkout's version's replays into `work/match/<version>/`, writes each match back as a replay and converts it again, and fails on one that does not come back the same |
| `verify-matches.py` | runs `mechcore verify` over those matches, adds up the transition coverage, and lists every round the simulator fights whose result differs from the match, with whether `tests/corpus/` pins it, every round it refuses and every comparison skipped with a warning, each match's last round fought on its own |
| `match-replays.py` | fights every round of both replays, the corpus's and the one its match writes, in the game, and compares the two |
| `corpus-fights.py` | fights every corpus round in the game into the fight document it records, under `work/fight/<version>/`, each a fixture candidate for `tests/corpus/`; it needs the game |
| `divergence-issue.py` | the rounds a master commit fights wrong or refuses that its parent did not, as one issue naming the pull request it merged, from the two runs' `verify-matches.py` reports |

These scripts are manual research tools. Automatic tournament verification
lives in [mechcore-replay](https://github.com/qbisi/mechcore-replay): newly
added tournament match YAML is verified with a release build of mechcore
master, and each failing match creates or updates one `match` issue here.
The issue stays open until that entire match verifies; the corpus workflow's
manual `match` input rechecks a repaired match against current master.
The gate continues to verify the tracked fight fixtures. `verify-matches.py --match-dir <dir>` can measure a
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

Tournament collection uses the Adapter directly, with no new CLI operation:

```sh
target/release/mechcore game launch --online --level 0
uv run --with pyyaml python scripts/corpus/tournament-replays.py list
uv run --with pyyaml python scripts/corpus/tournament-replays.py download --id 12345 --output-dir work/tournaments --publish
uv run --with pyyaml python scripts/corpus/tournament-replays.py publish --source work/tournaments --dry-run
uv run --with pyyaml python scripts/corpus/tournament-replays.py publish --source work/tournaments
```

`list` prints JSON lines containing the tournament ID, localized title, region,
date and start time, plus native subdivision IDs, `subtype` and `source`.
It includes both historical tournaments and ended events from the regular
competition list, where midweek mini tournaments are found. Mini tournaments
use the same download command and public `tournaments/` archive. Dates use local time;
`--timezone Asia/Shanghai` before the subcommand selects an explicit timezone.
`download --id` selects exactly that tournament and deduplicates server replay
paths across both players, all subdivisions and all ranking pages. It names
matches from their embedded `BattleID`, independently of the server filename,
under `<output-dir>/<date>-<region>/`. The date is exactly the one shown by
`list`, formatted `YYYY-MM-DD`; region is `asia`, `europe` or `americas`.
Use an explicit timezone for repeatable dates across machines; one tournament
can be on Sunday in Asia/Shanghai even when the league is called Saturday.
Existing equal matches are skipped; different contents at the same name fail.
Native GRBRs, tournament metadata and a resumable manifest remain under each
directory's `.downloads/`. A replay containing only the opening has no
deployment round to represent as a match: the converter's explicit
opening-only refusal is recorded as `skipped: opening_only` in the manifest
and output, with a separate `skipped` summary count. Its native input is
retained. Every other conversion failure makes the batch exit nonzero.
Restart the same command to resume. Both actions default to scheduler level 0.
Conversion keeps the converter's current-build and standard 1v1 checks;
older-version and 2v2 inputs remain native files with explicit errors.
`download --publish` releases its Adapter connection and uploads only when
all representable replay conversions succeed; opening-only skips do not block
publication. Without that flag, collection remains local.
Offline `publish --source` accepts the collection root or one date-region
directory and uploads only its match YAML to the same path under `tournaments/`
in mechcore-replay. `--dry-run` validates without a game or any Git operation.
Both publishers share append-only Git publication: equal bytes are skipped,
and a differing existing file aborts before copying any new files. Native
files, manifests and reports are never uploaded. Tournament files retain their
`BattleID.yaml` names; spectator publication below keeps its round suffix.

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

**A spectator match names its version in `build` and is filed by version.**
`replay.py publish` uses each match header's `build` for `matches/<build>/`; a
header that omits it uses this checkout's `GAME_VERSION`. A different version warns and remains
eligible, and one source directory may contain several versions. Names must
be `BattleID-rN.yaml`, and the last captured state must be round N. An existing
name under the same version must have the same bytes, and publication never
replaces one with other bytes. Native files are not uploaded by this command.

**A match document is generated and never hand-corrected.** A value that looks
wrong is a claim about the converter and belongs in
`crates/document/src/convert.rs`. A crate's tests keep any synthetic layout
inputs they need under that crate's `tests/fixtures/`.
