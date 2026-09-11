# Native GRBR fixtures

These are unmodified Mechabellum build-2259 replays copied incrementally from
the local Steam installation. Existing destinations are never
overwritten. Do not rewrite or normalize these files; native loadability and
hash identity are part of the fixture contract.

| Repository file | Original Steam basename | Size | SHA-256 |
| --- | --- | ---: | --- |
| `2259_26-09-03__13-54-16-770_[crower]VS[电脑].grbr` | same | 109420 | `2d2cd66a5ba9b6f295bd027f2e87aec230f8d1420c1c17bbbfea1156251dc8cc` |
| `2259_26-09-03__13-25-40-032_[crower]VS[电脑].grbr` | same | 85695 | `3757b76ac8789e8271ce8bb108c5e9bc29d9efd8b4088a1a85cee84273682c1d` |
| `2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr` | same | 604429 | `90baee4232b812e012fd2e309a54b0bb4a8f8642731436d35622b0b67779c335` |
| `2259_20260823--67294111_[你是蓬莱花仙]VS[crower].grbr` | same | 483172 | `d30eea2b61a7afdc101ba6391a94d67ee79cf574b7b0f46c0124f4131d7e4630` |
| `2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr` | same | 485258 | `f16db1bd8ee2ba4b9d144d5ef314081764b64cec01f61b1d8b065624cb84ba64` |
| `2259_20260901--67344528_[crower]VS[Charon].grbr` | same | 490505 | `c0468f0b3c63512d683fe96a3f8d1e0894018e00ff835212b5c5bc0e0d47be55` |

`2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr` is the integration sample exercised by
`record_replay_round`. Its serialized player records contain indices `0..=9`;
native replay capture accepts battle rounds `1..=9` unchanged and verifies the
same value through `Match.get_RoundCount()`.

## Where usable replays come from

The Steam installation keeps its own replay directory, and the files there fall
into two classes that do not agree with each other. Only one of them is evidence.

```
~/Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/ProjectDatas/Replay
```

**Locally recorded.** Written by this machine while playing. `BattleInfo.Seat`
holds a real seat, 0 or greater, and the file keeps the
`<build>_<date>--<id>_[a]VS[b].grbr` name. Its snapshots are the ones
`PlayerSnapshotController` took, so they are the faithful ones. These are the
files this directory tracks, and the ones any further measurement should use.

**Downloaded.** Fetched from the server, named `<version>\<id>.rep.grbr` with a
literal backslash, and carrying `Seat` of -1. Its snapshots are rebuilt server
side through `NetworkMessageData.Convert`, and the rebuild is not faithful. Two
fields differ with no counterexample in either direction, over 282 downloaded and
218 locally recorded player-rounds:

| Field | Locally recorded | Downloaded |
| --- | --- | --- |
| `playerData.IsSpecialSupply` | `false` everywhere | `true` everywhere |
| An upgraded blueprint chain | current level only, `401` | both levels, `4` and `401` |

Do not treat a downloaded replay as a snapshot of the match. It is a
reconstruction, and at least these two fields carry the reconstruction's
conventions rather than the game's state.

Older builds appear there too, as `.bak` files from 2203 and 2207. This
directory is build 2259 only.

## Supporting corpus

Claims in `docs/spec/document/state.md` that need more rounds than the six files here provide
are measured over the locally recorded replays in the Steam directory, currently
11 ranked matches and 202 player-rounds, without copying them in.
`work/research/local_replay_support.py` reproduces those measurements and shows
how the two classes are told apart. A claim resting on it says so.
`work/research/random_state_support.py` does the same for the random-state
claims, and reimplements `GRRandom` in Python to do it.
`work/research/reinforce_pool_support.py` does the same for the reinforcement
pool log, and `work/research/battle_invariant_support.py` for the match-level
and cross-round claims of `docs/spec/document/battle.md`.
