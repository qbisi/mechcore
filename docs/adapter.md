# Mechcore adapter

[TOC]

`mechcore-adapter` is an in-process Rust `cdylib`. On macOS the release
artifact is `target/release/libmechcore_adapter.dylib`; the artifact name is
not tied to a game version.

The adapter creates a Unix domain socket and waits for one client. When
`MECHCORE_ADAPTER_SOCKET` is absent, the endpoint is
`/tmp/mechcore-adapter-<uid>.sock`. A configured path must be absolute and no
longer than 100 bytes. The adapter refuses to replace a non-socket or a socket
owned by another user, creates the endpoint with mode `0600`, and admits only a
peer with the same effective UID.

## Build and launch

Build the release Adapter from the repository root:

```sh
cargo build -p mechcore-adapter --release
```

The Adapter dylib is repository-relative:

```text
target/release/libmechcore_adapter.dylib
```

The default Steam game executable is home-relative:

```text
Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum
```

The controlling Agent, not MCP, resolves those two roots and launches the game
outside the MCP sandbox:

```sh
DYLD_INSERT_LIBRARIES="$PWD/target/release/libmechcore_adapter.dylib" \
  "$HOME/Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum"
```

For the MCP workflow, leave `MECHCORE_ADAPTER_SOCKET` unset so both processes
use `/tmp/mechcore-adapter-<uid>.sock`. A custom socket remains available to
other Adapter clients, but MCP does not discover custom endpoints.

## Wire protocol

Messages are UTF-8 JSON, one object per line, with a maximum encoded size of
1 MiB. A new connection first receives:

```json
{
  "kind": "hello",
  "protocol": "mechcore.adapter.v1",
  "capabilities": [
    "status",
    "start_test",
    "apply_layout",
    "record_battle",
    "record_replay_round",
    "toggle_fight",
    "speed_up",
    "quit_match",
    "quit_game"
  ]
}
```

A request contains a caller-chosen identifier:

```json
{"id":1,"operation":"status","arguments":{}}
```

Success and failure responses preserve that identifier:

```json
{"kind":"response","id":1,"ok":true,"result":{"status":"main_menu"}}
{"kind":"response","id":2,"ok":false,"error":{"code":"invalid_game_state","message":"no active match"}}
```

The hello capability list is authoritative. Operations not present in that
list are rejected even if private implementation helpers still exist inside
the dylib. Every request is parsed on the socket thread. Individual native
actions execute synchronously on Unity's main dispatch queue; the socket thread
coordinates the multiple short actions and status samples required by a
multi-round `apply_layout`. A failed or disconnected mutation must not be
automatically retried.

The adapter returns after the native call or readback completes. `apply_layout`
also waits until the requested activation-round deployment is stable.
`record_battle` remains active through the complete logic-tick capture and atomic MCFR publication.
`record_replay_round` additionally owns replay loading, round selection, accelerated deployment,
capture, and return to the main menu. Other
cross-scene readiness belongs to `mechcore mcp`, which observes the status
stream before returning from lifecycle tools.

## Operations

### apply_layout

Input is the complete layout object defined by [layout.md](layout.md), with a
top-level activation `round` in `1..=15` and `sides.blue` and `sides.red`
fields.

Typical output:

```json
{"applied":true,"round":3,"formation_count":12,"construction_count":1,"contraption_count":0,"skipped_rounds":[1,2],"stages":[{"stage":"prepare"},{"stage":"activation"}]}
```

The operation begins only during first-round Training Ground deployment. It
compiles and validates the complete layout before mutation, clears both sides,
and expires earlier empty deployment states synchronously. Every formation is
created in declaration/index order after the activation round begins, with its
index set directly through `MAD_AddUnit.UIDX`; missing indices remain absent
without creating/removing placeholder units. The
Adapter sets `MechTeam` experience, explicitly corrects a mismatching native
travelling state, and requires authoritative experience and
`SuperDeploymentSystem.IsTravellingUnit` readback after the final move.
Same-seed opening constructions are retained when `(type, position)` matches
and otherwise removed. Missing constructions are applied in declaration order
with native-allocated indices, without index placeholders. Contraptions and modifiers are then applied in the activation
round, whose deployment timer is reset before the operation returns.

Prepare also deactivates and hides the neutral scene FightCrystals, clears their
global BuildingSystem indexes and removes their owner registrations before the
first fight. The global list must contain only plain FightCrystals with null
current/original teams; mixed ownership fails closed. Team towers and deployed
constructions are not in that cleanup list. This path never runs in replay mode.

Replay and Training Ground capture allocate MCFR Building IDs by
`(team_id, building_type_id, position.x, position.y, position.z)` using raw Q32.32
coordinates; duplicate keys fail closed. Native construction/building counters
and layout order do not determine these IDs. Existing IDs and target/event
references remain stable after allocation, including after object removal.

A structurally valid layout may contain `sides.<side>.terrains` for readback and
offline verification. The Adapter rejects any non-empty terrain list before the
prepare stage and performs no native mutation. Native terrain application stays
disabled until GRBR-derived rounds can be captured as MCFR and compared for
closure.

### quit_game

Input is an empty object.

Typical output:

```json
{"requested":true,"exit_code":0}
```

The operation invokes Unity application shutdown and is valid only at the main
menu. The adapter confirms the request; the MCP layer additionally waits for
the owned process to exit.

### record_battle

Input contains one absolute, non-existing destination with a `.mcfr` suffix:

```json
{"output":"/absolute/path/battle.mcfr"}
```

An optional `video_output` enables the logic-frame visual sidecar:

```json
{
  "output":"/absolute/path/battle.mcfr",
  "video_output":"/absolute/path/battle.mov"
}
```

The optional path must be absolute, non-existing, distinct from `output`, and use `.mov`. With no
`video_output`, no screenshot metadata is resolved, the camera is untouched, and no visual encoding
work occurs. With it enabled, the adapter uses the deterministic `calibration_topdown` view: native
Cinemachine and mouse/keyboard pan, orbit, and zoom controllers are suspended; the main camera is
fixed at world `(0,1070,-1070)`, rotated to `(45,0,0)`, and given a perspective field of view of
`20` degrees. It points at the battlefield origin along equal Y and Z offsets. The render is fixed at
2560x1600; the distance and field of view preserve approximately the previous 1.5x center-plane scale
while using native perspective rendering so tower shadow decals do not become opaque black quads. A
visual recording temporarily sets `Application.targetFrameRate` to 20 and does not request native
speed-up. A main-camera post-render hook releases the next `FightController.Update`; pixel readback at
that following update therefore captures a completed render of the pending MCFR snapshot rather than
the state being advanced.
The normal screen-space UI is included. The terminal snapshot is not returned until its completed
render has also been captured. Frames are JPEG-encoded and written as a QuickTime Motion JPEG stream
whose sample duration equals `D.logic_step`; frame count must equal MCFR tick count. Screen and camera
controller state and the original target frame rate are restored after terminal capture or failure,
and partial media remains unpublished.
The result reports `view`, `projection`, camera position/rotation, and field of view alongside the media
dimensions so a renderer can reconstruct the same world-to-screen calibration.

The operation is valid only after layout completion in Training Ground deployment. It arms native
capture, starts combat, and records from `S(1)` after the first combat update through every subsequent
`FightController.Update` boundary through the unique fighting-to-over transition. When video capture
is disabled, the adapter calls native `RequestSpeedUp` once on the first update that reports
`IsFighting=true`; requesting it during the preceding process-state transition is too early and has no
lasting effect. It calls `mechcore-mcfr::McfrWriter` directly, publishes atomically, reopens the file
structurally, and returns the state/transition counts and all formal hashes.

Projectile release/removal and damage use narrow native hooks so objects created and removed inside
one logic step remain in `E`. The release hook records the native projectile, owner and target; the
removal hook additionally records position and the native `intercepted` argument. During
`DamagePerformer.Perform`, the Adapter scopes the native provider attribution. Each
`FightController.OnActorHitted(HitDamageInfo)` then records one actor target and its positive
`damageReal`; `FightActor.ReduceLife(HitDamageInfo)` scopes that same attribution around synchronous
death processing. Battlefield Shield damage is recorded per actual Shield result. The same native
chain attaches the Shield reference to the subsequent projectile removal as `absorbed_by`.

The current native snapshot closure directly reads units, projectiles, the alive `FightCrystal` union
from every FightTeam's towers, buildings, and constructions, battlefield shields from
`AdvancedEnergyShieldSystem`, dynamic battlefield terrain from `RangeItemSystem`, personal shields, the four native status bits, the three
numeric-modifier channels, and every weapon of every entry returned by `FightMech.GetSkills()`.
Modifier getters are read in full and fail the recording on error; MCFR persistence then encodes
successfully read zero values as nullable sparse fields whose semantic default is no modifier.
Neutral map crystals registered only in `BuildingSystem` are outside the Building domain; their
`FightCrystal.OnDead` calls do not emit `building_destroyed`.

Battlefield shields are the independent `FightEnergyShield` objects managed by
`AdvancedEnergyShieldSystem`; a unit's `EnergyShieldController` remains the `personal_shield`
component of its Unit row. The Adapter obtains each Training Ground side's `IFightGroup` through
`FightTeam.GetFightGroup()` and reads both the full shield list and the active list. It persists
`active` and the nullable active-list index `active_order`, because runtime
reactivation appends an object and can change the interception order independently of Shield ID.
Unknown shield data-source classes, inconsistent active-list membership, dangling projectile birth
containment references, and reused retired shield pointers fail the recording. Shield lifecycle
events are generated from authoritative changes in full-list membership at consecutive native
snapshot boundaries; the current removal source proves destruction but not a narrower cause, so
`shield_destroyed.reason` is `unknown`.

Embedded layout contraptions are read from current native inventory, not
`GetFightObjectRecorder().GeRecords()`: full contraption shields (including
inactive reset-next-round objects), `TeamMineManager.GetLandMines()`, and the
live interceptor subset of `TeamInterceptSourceManager` sources. This retains
earlier-round objects without resurrecting consumed missiles or destroyed
interceptors. Unit interception sources are not layout contraptions. Layout
retains native full-list shield order at deployment and is not delayed or
reordered from S(1).

Native hooks use temporary Shield IDs before S(1). At the first advancing combat
snapshot, initial IDs are assigned once in `(team_id, active_order)` order, with
inactive shields following each team's active shields. First-tick-removed
shields follow all S(1) rows, grouped by team, keeping persisted IDs contiguous.
Those fallback groups sort by source kind, owner reference, position X/Y/Z,
radius, round policy, maximum energy and current energy; indistinguishable keys
fail closed. Cached references and first-tick traces are remapped before state,
projectile, instrumentation and event references are finalized. Removed objects
retain identities for E(1). After that boundary, IDs remain pointer-stable even
when active order changes; later new shields append IDs without reuse.
The layout still uses only `type`, `x`, `y` and ordinary placement bounds.

Dynamic terrain is enumerated by the six native `RangeItemType` controllers. A controller or item
list that has not been instantiated contributes an empty collection; each member returned by
`RangeItemController.GetItems()` is active and receives a stable Terrain ID. The Adapter reads its
team, type, position, radius, optional grid mask, optional cross-round remainder, optional logic
lifetime, and the controller's direct unit applications. `affectedUnits` is joined to Unit IDs;
`affectedUnitTimes` and positive `effectTimeDuration` provide an optional periodic clock.
Terrain creation and removal events follow authoritative item-list membership changes at consecutive
snapshot boundaries. Removed pointers are tombstoned, and the current removal source records
`terrain_removed.reason=unknown`.

Native build `1.11.1.3.2259` capture coverage includes `oil`, `fire`, `acid`, and `fog` battle-skill
layouts. In those recordings all four terrain types were present in `terrains.parquet` and their
application lists referenced the affected enemy units. Oil populated the sparse
`remaining_rounds` field; fire, acid, and fog used null.

### record_replay_round

Input identifies an existing native replay, a one-based combat round, and a new
MCFR destination:

```json
{
  "grbr": "/absolute/path/battle.grbr",
  "round": 6,
  "output": "/absolute/path/round-6.mcfr"
}
```

Both `record_replay_round` and `record_battle` accept an optional research-only
`instrumentation` object. A bounded RVO request is:

```json
{
  "output": "/absolute/path/round-7-rvo.h5",
  "profile": "target_refs_rvo_v1",
  "rvo_scope": {
    "start_tick": 8,
    "end_tick": 14,
    "unit_ids": [124, 282, 363, 246]
  }
}
```

`rvo_scope` requires 1–8 unique positive **MCFR unit IDs**, not formation indices,
and an inclusive window of at most 64 positive **MCFR combat ticks**.
Build 2259 advances `FightController.get_Tick` by 100 per combat tick; the
Adapter converts the window accordingly (8–14 selects native 800–1400).
It filters RVO detail before reading agent state, neighbours, or VO buffers.
Only selected sources are captured; their full neighbour lists may reference
other units/buildings/internal agents. Agent `ordinal` remains the original
simulator-list index, not the index in the filtered result. Updates are selected
by their start tick; publication after the window is still drained and
identified by `publish_native_tick`. Observation `start_native_tick` and
`publish_native_tick` retain native counter values. `agents` holds the pre-solve snapshot,
`published_agents` the state at the native publication boundary; both retain
raw Q32.32 integers. Internal-agent ordinals are capture-local, not normalized
cross-recording identities.

Scoped sidecars are sparse: `records.steps` holds actual MCFR ticks and must not
be replaced by row number. They remain result-hash-bound, non-hashed research
evidence; formal MCFR still records the complete battle. Omitting `rvo_scope`
retains the existing full instrumentation profile. Invalid scope/output is
rejected before replay loading. No Simulator closure is implied.

The Adapter requires `main_menu` and passes the requested round unchanged to
the native `PlayReplayCommand.Execute(IReplay, startRound)` argument. Replay
`Match.get_RoundCount()` must read back the same value before capture is armed.
`ReplayMatchBase.SetReplayTime(false, 0)` then removes recorded deployment
delays. The capture hook reads the embedded layout at entry to the
final player's `PlayerController.FinishDeploy()`, before the native transition
can initialize fighting, but does not persist that pre-update state. `S(1)` is the first state row. Earlier players are rejected unless every other
player has already completed deployment, so a partially replayed deployment
cannot be published. Once fighting begins, the normal native
`RequestSpeedUp()` path accelerates combat; the existing fighting-to-over edge
terminates MCFR recording.

Replay formations remain ordered by and export their stable native unit index,
but those indices may contain gaps left by units removed in earlier rounds.
Each formation also exports its integer `MechTeam` experience. Replay
constructions export only `type/x/y`, sorted by that tuple, without native
construction indices. Negative and duplicate unit indices and negative experience fail
closed. Active commander
abilities enter `battle_skills` only when native
`TryGetReleaseCommanderSkillData` supplies positional release data; active
non-release abilities are outside that layout field.

The operation reopens and verifies the MCFR, exits the replay through the native
match quit path, and returns success only after stable `main_menu` status. It
never quits the game process. Invalid input, unavailable rounds, capture
failure, and timeout paths also attempt replay cleanup before returning an
error.

### quit_match

Input is an empty object.

Typical output:

```json
{"performed":true}
```

The operation requests that the active Training Ground or replay match exit.
The MCP layer additionally waits for `main_menu`.

### speed_up

Input is an empty object.

Typical output:

```json
{"requested":true}
```

The operation submits the native battle speed-up request. The current status
does not expose a speed field, so the native call completing normally is the
authoritative completion condition.

### start_test

Input optionally specifies the native match seed:

```json
{"seed":1787720817}
```

Omitting `seed`, or passing `0`, preserves the game's system-generated seed
behavior. Any nonzero signed 32-bit value is written to `BattleSetting.SystemSeed`
before host creation.

Typical output:

```json
{"created":true,"initial_supply":10000,"requested_seed":1787720817}
```

The operation creates the single fixed Training Ground mode used by
`apply_layout`. Both sides start with 10000 supply; advanced teams and all
reinforcement systems are disabled. Native constructions remain enabled during
room creation to preserve standard round-one deployment depth, then
`apply_layout` clears them before applying the requested layout. The MCP layer
additionally waits for first-round deployment readiness.

### status

Input is an empty object.

Main-menu output:

```json
{"status":"main_menu"}
```

Training Ground output:

```json
{"status":"training_ground","round_count":1,"deploying":true,"fighting":false,"match_seed":1787720817}
```

`status` is exactly one of `main_menu`, `training_ground`, `replay`, or
`unknown`. Training Ground and replay status additionally report `round_count`,
`deploying`, `fighting`, and the effective `match_seed` read from the native
match random stream; a temporarily unavailable native detail is `null`.

### toggle_fight

Input is an empty object.

Typical output:

```json
{"performed":true}
```

The operation changes the Training Ground process state from deployment to
battle. The MCP layer additionally observes the battle transition before
returning.
