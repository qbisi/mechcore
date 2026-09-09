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

The repository pins nightly Rust in `rust-toolchain.toml` and enables Cargo
artifact dependencies in `.cargo/config.toml`. Use a rustup-provided Cargo
(a standalone stable Cargo, including one installed by Nix, does not select
the pinned toolchain). For a Nix environment without rustup, enter
`nix-shell -p rustup` first; its `cargo` proxy selects the pinned nightly.

Build the CLI and its Adapter together from the repository root:

```sh
cargo build -p mechcore --release
```

`cargo run --release -- run <script.mcscript>` also checks and builds the
Adapter before running the CLI. Cargo tracks the Adapter's source and transitive
dependencies; the CLI build script atomically copies the resulting dylib beside
the executable. No runtime Cargo invocation or absolute build path is needed.
The workspace defaults to the CLI; use `--workspace` for workspace-wide checks.
Release build dependencies explicitly use optimization level 3 and one codegen
unit because Cargo otherwise builds build-time artifacts without optimization.
Cargo keeps build dependencies on its unwind panic strategy; the CLI retains
the workspace's abort strategy.
The default release directory contains both files:

```text
target/release/mechcore
target/release/libmechcore_adapter.dylib
```

Debug builds, custom target directories and explicit target triples use their
corresponding output directory. Distribute both files together. After changing
the Adapter, start a new game with `game: launch` (or `shell --launch`): an
already running game keeps its loaded Adapter, including when using `attach`.

`python3 scripts/check-adapter-packaging.py` exercises the real packaging build
script with a small test dylib: source changes, no-op builds, a removed copy,
debug/release profiles, a custom output directory and an explicit target triple.
Run it in the same nightly environment after fetching the workspace dependencies.

The default Steam game executable is home-relative:

```text
Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum
```

`mechcore` launches the game itself. It resolves the Adapter as a sibling of
its own executable and injects it, so no caller assembles that command:

```sh
mechcore shell --launch
```

The equivalent by hand, for a game this tool will later `attach` to:

```sh
DYLD_INSERT_LIBRARIES="$PWD/target/release/libmechcore_adapter.dylib" \
  "$HOME/Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum"
```

[session.md](session.md) defines which of the two applies, what each verb
refuses, and where the launched game's output is written. Both sides resolve
`MECHCORE_ADAPTER_SOCKET` the same way, so an override moves the endpoint for
the Adapter and every client together.

The Adapter reports failures it cannot answer over the socket, such as a
rejected peer or a dropped client, on the game process's standard error.
See [session.md](session.md#diagnostics) for where that stream lands and
how it differs from Unity's own `Player.log`.

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

Request arguments are typed, and their types live in `mechcore-protocol`
alongside the operation names, so both ends of the socket compile against one
definition. Every argument type rejects unknown fields. Naming them in one place
is what keeps a caller from inventing a field the adapter will refuse, which is
otherwise only observable with the game running. `apply_layout` carries a layout
document, whose type the same crate names and `mechcore-document` validates.

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
cross-scene readiness belongs to the session layer, which observes the status
stream before returning from a lifecycle operation.

## Operations

### apply_layout

Neutral map crystals in the global `BuildingSystem` are retained, including
their RVO agents and collision/query indexes. They are native scene inputs, not
layout-side deployments: deleting them solely because they have no team can
change RVO neighbour queries even when they are outside the deployment area.
The prepare result reports their `retained_count` and `rvo_controller_count`
under `retained.neutral_crystals`. The session passes optional `layout.map_id`
to `start_test`, which validates it with `Config.GetMatchSettingOrNull` and
sets `BattleInfo.MapID` before `CreateHost`. Without it Mechcore explicitly uses
map 1021 rather than inheriting a mutable game default.
Replay export reads the actual `Match.GetBattleInfo().MapID`, so replay roundtrip
selects the source map rather than assuming the default scene is equivalent.

Input is the complete layout object defined by [layout.md](layout.md), with a
positive top-level `round` and `sides.blue` and `sides.red` fields.
A layout is valid at any round, but this operation stages every earlier setup
round inside one timeout budget, so it refuses a `round` above
`MAX_STAGED_ROUND`, which is `15`. That budget is an executor limit, not a game
rule or a schema rule.

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

Prepare only counts neutral scene FightCrystals; it does not deactivate, hide,
unregister or remove them. In build 2259, selecting map 1021 produces 27 neutral
crystals with zero RVO controllers, while map 1001 produces 891 with 73 RVO
controllers. These are map inputs, not a Training Ground cleanup category.

Replay and Training Ground capture allocate MCFR Building IDs by
`(team_id, building_type_id, position.x, position.y, position.z)` using raw Q32.32
coordinates; duplicate keys fail closed. Native construction/building counters
and layout order do not determine these IDs. Existing IDs and target/event
references remain stable after allocation, including after object removal.

A structurally valid layout may contain build-2259 retained Sticky Oil Bomb
state in `sides.<side>.terrains`. During activation the Adapter expands each
entry's two ordered control points with the native fixed-point primitives,
creates only the mapped active indexes through `RangeItemSystem.AddItem`, and
restores any final clipped grids with immediate native readback. Simulator
terrain support remains disabled; this Adapter path is validated against a
GRBR replay-round MCFR pair as documented in `terrain.md`.

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

An optional `video_output` enables the logic-frame visual sidecar, and an optional `speed_up`
selects native combat speed-up:

```json
{
  "output":"/absolute/path/battle.mcfr",
  "video_output":"/absolute/path/battle.mov",
  "speed_up":true
}
```

`speed_up` defaults to `true` and applies with or without `video_output`. It is a boolean because
the native path is a per-user vote (`RequestSpeedUp` -> `MH_SpeedUp` -> `MiscManager.ActiveSpeedUp`)
carrying no rate; the only numeric multiplier in the game belongs to the separate automatic
`FightSpeedUpController`, which this operation does not drive. `record_replay_round` accepts the
same field.

The optional path must be absolute, non-existing, distinct from `output`, and use `.mov`. With no
`video_output`, no screenshot metadata is resolved, the camera is untouched, and no visual encoding
work occurs. With it enabled, the adapter uses the deterministic `calibration_topdown` view: native
Cinemachine and mouse/keyboard pan, orbit, and zoom controllers are suspended; the main camera is
fixed at world `(0,1070,-1070)`, rotated to `(45,0,0)`, and given a perspective field of view of
`20` degrees. It points at the battlefield origin along equal Y and Z offsets. The render is fixed at
1920x1080. Because the field of view is vertical and unchanged, vertical coverage is the same as the
earlier 2560x1600 render while horizontal coverage is wider, so frames from the two resolutions are
not pixel-comparable. Native perspective rendering is used so tower shadow decals do not become
opaque black quads. A renderer reproduces the calibration from the reported camera position,
rotation, field of view and media dimensions rather than from a fixed scale factor. A visual
recording temporarily sets `Application.targetFrameRate` to 20. It may still request native
speed-up: the render barrier below paces the logic update, so a sped-up game produces the same frame
count and a bit-identical MCFR, and the gain is small. A main-camera post-render hook releases the next `FightController.Update`; pixel readback at
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

The same shield collection also holds retained Shield Airdrops. Those are
commander-skill objects, so export splits them out of `contraptions` into the
side's `airdrop_shields`, keeping their native full-list order.

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
`terrain_removed.reason=unknown`. These snapshot-difference events are synthesized,
not native callback traces: creation events are appended first, then removal
events, with each batch ordered by stable Terrain ID rather than process-local
pointer address. Previously captured combat callbacks retain their original order.

Native build `1.11.1.3.2259` capture coverage includes `oil`, `fire`, `acid`, and `fog` battle-skill
layouts. In those recordings all four terrain types were present in `terrains.parquet` and their
application lists referenced the affected enemy units. Oil populated the sparse
`remaining_rounds` field; fire, acid, and fog used null.

### record_replay_round

Input identifies an existing native replay, a one-based combat round, and a new
MCFR destination. The round has no upper bound: reading round `N` out of a
replay is decoding, not staging, so `MAX_STAGED_ROUND` does not apply here.

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
be replaced by row number. They remain `physics_result_hash`-bound, non-hashed research
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
before host creation. This operation is the native boundary, so `0` keeps its
native meaning here. The layout schema does not: it rejects `seed: 0` and
expresses the same request by omitting the field, because a document has to
denote one scenario.

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
