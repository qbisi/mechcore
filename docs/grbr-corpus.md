# Standard 1v1 GRBR corpus acquisition

[TOC]

This is the build-2259 acquisition path for collecting locally recorded GRBRs
from live, server-provided standard 1v1 matches. It is intentionally narrower
than a general replay downloader: a server-reconstructed replay and a local
observation are different evidence sources.

## Scope and evidence status

| Claim | Status | Evidence |
| --- | --- | --- |
| The watch list can be refreshed with the `MatchFirst` filter and exposes cached `WatchScene` entries | statically confirmed | build-2259 indexed symbols for `LobbyProxy.RequestPageRoomList`, `GetRoomFilterDataByType`, and `LobbyRoomFilterData.GetWatchScenes` |
| `LobbyProxy.WatchScene(sceneid)` enters the spectator path through `StartMatchController.StartWatch(sceneid, source = 4, mapID = -1)` | statically confirmed | bounded IL2CPP excerpt in `work/research/spectator-grbr-2259/evidence/lobby-proxy-watch-scene.md` |
| A watch match is distinguishable through `IsWatchMode`, and `Match.get_IsFinished` marks completion | statically confirmed | build-2259 indexed method signatures |
| `MatchProxy.SaveReplay(Action<String>)` reaches the native replay save command and `MatchUtility.SaveReplay` | statically confirmed | build-2259 indexed symbols and call edges |
| The whole operation enters a live public scene, remains connected through the result, saves a valid local-seat GRBR, and returns to main menu | runtime confirmed | 2026-09-10 final build-2259 smoke: scene `268721197`, map `1011`, 9 recorded rounds, 427,686-byte GRBR, final `main_menu` |

The static evidence is bound to Unity `2022.3.62f3` and game identity
`5b4248d9af138a94e6a809bb0d529e2ecd8ef0ce0a03d33f2e123cb65370ee58`.
Its principal artifacts are:

| Artifact | SHA-256 |
| --- | --- |
| `Contents/Frameworks/GameAssembly.dylib` | `9d2e3f163f74728da73b5dac474f76a0ebdaa3852718fbeb45ad8dec6b4360e5` |
| `Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat` | `2488ba4661958da42438e91cb382259077ef7077dfa2261e74ade1c9b1b1fb43` |

The live row is qualified only for these exact artifacts. It becomes an open
qualification gate again after any game update or Adapter symbol change.

## Native path

One `record_watch_replay` request owns this complete sequence:

1. Require `main_menu` with no current match and snapshot existing `.grbr`
   metadata under `Mechabellum.app/ProjectDatas/Replay`.
2. Wait for the game's own `ProxyGroup` to register `LobbyProxy`, retrieve it
   from `GameFacade` without constructing or registering a competing proxy, and
   call the native UI refresh path `SwitchToRoomListFilter(MatchFirst)`. After
   its five-second throttle, that path clears cached pages and calls
   `RequestPageRoomList(MatchFirst, next = true, block = true, interval = 5)`.
3. Read `GetRoomFilterDataByType(MatchFirst).GetWatchScenes()` and retain only
   scenes satisfying the fixed admission filter below.
4. Select the least occupied candidate, breaking ties by scene ID, and call
   `LobbyProxy.WatchScene(sceneid)`.
5. Require a stable `spectating` status at round one. Then wait until the
   inherited `Match.get_IsFinished()` property getter is true.
6. Look for a new or changed native GRBR and wait for its length and mtime to
   stabilize. If ten seconds of autosave grace yield none, call
   `MatchProxy.SaveReplay(null)` and wait once more.
7. By default retain the native file as the output. If a separate corpus
   directory was requested, copy into it with create-new semantics. Quit the
   match and require a stable `main_menu` status.

Every polling point in that sequence also abandons the sequence when a higher
claim takes the game; the Adapter returns it to the main menu once the
collector's connection is closed.

The Adapter is single-client while this sequence runs. A batch is therefore a
series of isolated matches rather than polling and UI control split across
several clients.

## Stopping a batch

The collector declares `level: 0` and holds the machine, so anything else takes
it back by simply running:

```sh
mechcore run <whatever you actually need the game for>.mcscript
```

The default level of `1` outranks the collector. The Adapter abandons the watch
in progress at its next poll, returns the game to the main menu, and closes the
collector's connection; the collector exits without shutting that game down,
and the new script gets it. One match is lost and no other. See
[session.md](session.md#state-matrix) for the states this moves through, and
[mcscript.md](mcscript.md#acquisition) for the level a script declares.

## Fixed scene admission

The script cannot widen these rules:

- `ERoomListFilter.MatchFirst = 5` and `matchInfo = null`;
- `WatchScene.Round = 1`;
- exactly two members, `ServerSubType = Mod1V1`, and no custom rule deltas;
- map setting `GameMode.Normal = 0` and `MatchMode.VS_1_1 = 0`;
- `0 <= WatcherNum < 300`.

Choosing only at round one prevents a technically valid but incomplete local
recording from entering the corpus. Map configuration is checked through the
game's own `Config.GetMatchSettingOrNull`, not a duplicated map allow-list.

Admission is a property of the scene, not of the bytes. What the game writes
for a match it accepted at these rules is what the corpus holds: the collector
does not decode the file, and does not restate the recorder's own guarantees
about build, seat or round structure. The one shape this path cannot produce is
the downloaded, server-reconstructed replay (`Seat = -1`), because no download
is involved.

## Failure and restart behavior

The collector is fail-closed. No later loop iteration starts after an
entry timeout, match timeout, game rejection, absent or unstable native file,
destination collision, or cleanup failure. When possible, the
Adapter exits any active watch before returning the failure. Being outranked is
not one of these: the run ends successfully, having recorded everything it
completed.

The native replay is never deleted, and neither is a copy that reached the
corpus directory. A published copy remains if only the subsequent match-exit
check fails; the failed JSON result keeps it out of the accepted manifest until
a human reviews it. Rerunning the same script starts a new set of matches and
never overwrites a prior basename.

For provenance, capture stdout as JSONL and retain both game logs named in
[session.md](session.md). Each successful result contains the output path, the
publication mode, the native source, the selected scene metadata, and the final
status.

## Qualification result and repeatable gate

Before treating unattended collection as runtime-qualified, run a small batch
against the exact build-2259 artifacts:

```sh
mechcore run scripts/record-standard-1v1-grbr.mcscript \
  > work/research/grbr-native-2259.jsonl
```

That script declares `level: 0`, so it can be taken over at any point by
ordinary work.

Accept the implementation only after at least one result has all of:

- `operation.recorded = true` and `operation.cleanup.match_exited = true`;
- final `status.status = main_menu`;
- an `operation.output` file that `mechcore convert` can open, with the
  build, a non-negative `Seat` and the `VS_1_1` header the recorder writes;
- no managed exception in the launch log or Unity `Player.log`.

That decode is the reviewer's check on a new build, not a step the collector
performs on every match.

The gate passed on 2026-09-10 with the final reviewed build-2259 binary. The
selected scene was `268721197` on map `1011`; the decoded recorder shape was
`Seat = 2`, `VS_1_1`, two players, and 9 contiguous round records. The copied
artifact was `2259_20260910--268721197_[wesbare]VS[Kaizzum].grbr`, 427,686 bytes,
with SHA-256
`22a05ab79d4e536c6a1fbefbc0b90701b0ef7ddd7c2ef75c4985c227e5b4ac03`.
The native log showed `MatchUtility.SaveReplay`, and the final status was
`main_menu`; no managed exception was found in that run. General
`convert` parsing reached round 4 and then correctly refused commander
skill `800001` because retained shields are absent from replay data. That
converter fidelity gate does not invalidate the scene admission above.
