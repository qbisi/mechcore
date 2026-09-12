# Native GRBR fixtures

These 41 files are unmodified Mechabellum build-2259 standard 1v1 replays
copied incrementally from the local Steam installation. Existing destinations
are never overwritten. Do not rewrite or normalize them; native loadability
and hash identity are part of the fixture contract. [SHA256SUMS](SHA256SUMS)
records the size-independent identity of every tracked replay.

`2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr` is the integration
sample exercised by `record_replay_round`. Its serialized player records contain
indices `0..=9`; native replay capture accepts battle rounds `1..=9` unchanged
and verifies the same value through `Match.get_RoundCount()`.

## Where usable replays come from

The Steam installation keeps its own replay directory, and the files there fall
into two classes that do not agree with each other. Only one of them is evidence.

```
~/Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/ProjectDatas/Replay
```

**Locally recorded.** Written by this machine. `BattleInfo.Seat` holds a real
seat, 0 or greater, and the file keeps the
`<build>_<date>--<id>_[a]VS[b].grbr` name. Its snapshots are the ones
`PlayerSnapshotController` took, so they are the faithful ones. These are the
files this directory tracks, and the ones any further measurement should use.

**Downloaded.** Fetched from the server, named `<version>\<id>.rep.grbr` with a
literal backslash, and carrying `Seat` of -1. Its snapshots are rebuilt server
side through `NetworkMessageData.Convert`, and the rebuild is not faithful. Two
fields differ with no counterexample in either direction, over 282 downloaded
and 218 locally recorded player-rounds measured before this fixture expansion:

| Field | Locally recorded | Downloaded |
| --- | --- | --- |
| `playerData.IsSpecialSupply` | `false` everywhere | `true` everywhere |
| An upgraded blueprint chain | current level only, `401` | both levels, `4` and `401` |

Do not treat a downloaded replay as a snapshot of the match. It is a
reconstruction, and at least these two fields carry the reconstruction's
conventions rather than the game's state.

Every tracked file additionally has build `2259`, match mode `VS_1_1`, exactly
two players, no `matchType`, no custom game rules, and matching consecutive
round lists beginning at zero. Eleven were played from seat 0 or 1. The other
30 have spectator seat 2 and were recorded locally after the standard 1v1 watch
collector admitted the scene. They are classed by their source fields and
snapshot spelling, not by whether this account was one of the players. Three
tracked matches end with a player concession.

Older builds appear in the Steam directory as `.bak` files from 2203 and 2207.
This directory is build 2259 only. Training Ground recordings carry
`matchType=Test` and are not standard 1v1 corpus members.

## Derived corpora

The tracked set contains 41 matches and 750 player-rounds. Battle documents are
generated under `tests/battle`, one per replay, and its README records what the
conversion establishes about them. Native deployment observations are generated under the
ignored local `work/replay-corpus` directory by:

```bash
python3 scripts/export-replay-corpus.py --offline-only --force-battles
python3 scripts/export-replay-corpus.py
```

The second command treats each replay as an independent native run, validates a
complete JSONL summary before resuming past an existing output, and records
source/output hashes and failures in `work/replay-corpus/manifest.json`.

Those observations are what the deployment transition is checked against:

```bash
ls work/replay-corpus/observations/*.jsonl | mechcore verify
```

Each recorded decision is applied to the position it was taken from and every
field of the result compared, and each round's collapsed sequence is applied to
the position the round opened with and compared against the one it closed with.
`docs/spec/document/turn.md` defines both checks. Their counts are a fact about this local
corpus rather than about the build, so they live in the commit that moved them
and not here.

`scripts/local_replay_support.py` and the other `*_support.py` programs
reproduce aggregate document claims from the same provenance class. A claim
resting on one of them says so.
