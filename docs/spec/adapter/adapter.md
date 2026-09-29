# Mechcore adapter

[TOC]

## Scope

This contract defines the socket an in-process adapter exposes to one client at
a time: how a client claims the game, which operations it may then request,
what each returns, and what each refuses.

It does not define the documents that cross that socket. A layout is
[layout.md](../document/layout.md) and a recording is
[mcfr.md](../mcfr/mcfr.md); the adapter validates against those rather than
restating them. Cross-scene readiness is not here either. A lifecycle operation
returns when its own native work completes, and waiting for the game to settle
afterwards belongs to [session.md](../mechcore/session.md).

Every operation mutates a live game and reads the result back. All of them are
fail-closed: an accepted native action is never sufficient on its own, and
nothing partial is ever published.

## Build and launch

`mechcore-adapter` is an in-process Rust `cdylib`. On macOS the release
artifact is `target/release/libmechcore_adapter.dylib`; the artifact name is
not tied to a game version.

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

Packaging the Adapter is the CLI's default `adapter` feature, and the only part
of the workspace that needs macOS. Everything that needs no game — the
simulator, the readers, a gameless `.mcscript` — builds anywhere without it:

```sh
cargo build -p mechcore --release --no-default-features
cargo test --workspace --exclude mechcore-adapter --no-default-features
```

Such a binary refuses `game: launch` with `launch_failed`, naming the missing
feature.
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
corresponding output directory. Distribute both files together. A running game
keeps the Adapter it loaded, including when using `attach`. A game `mechcore`
launched lingers after its last client, and the next `game: launch` (or
`game launch` in a shell) retires it and starts a new one when the Adapter it
loaded is not the one beside `mechcore`, so a rebuilt Adapter is loaded without
quitting anything by hand. A game started any other way is joined with the
Adapter it has; `quit_game` is a plain operation any client can call for that
reason. When the wire contract itself changed, the running game answers with
the old `protocol` name and every new client is refused with
`protocol_mismatch`; a lingering game quits itself within 30 s, and any other
is quit from the game's own menu, before launching again.

`python3 scripts/check/check-adapter-packaging.py` exercises the real packaging build
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
mechcore shell
> game launch
```

The equivalent by hand, for a game this tool will later `attach` to:

```sh
DYLD_INSERT_LIBRARIES="$PWD/target/release/libmechcore_adapter.dylib" \
  "$HOME/Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum"
```

A headless game is the same command with Unity's `-batchmode -nographics`
after the executable. The Adapter reads those two switches off the game's own
command line: `-batchmode` keeps the process out of the Dock, and
`-nographics` refuses a video and lets the game past the resolution check
that would stop it before logging in
([session.md](../mechcore/session.md#headless)). An offline game is started
inside `sandbox-exec` with rules that refuse every IP connection, and the
Adapter refuses `record_watch_replay` in a sandboxed process
([session.md](../mechcore/session.md#offline)).

[session.md](../mechcore/session.md) defines which of the two applies, what each verb
refuses, and where the launched game's output is written. Both sides resolve
`MECHCORE_ADAPTER_SOCKET` the same way, so an override moves the endpoint for
the Adapter and every client together.

The Adapter reports failures it cannot answer over the socket, such as a
rejected peer or a dropped client, on the game process's standard error.
See [session.md](../mechcore/session.md#diagnostics) for where that stream lands and
how it differs from Unity's own `Player.log`.

## Wire protocol

The adapter creates a Unix domain socket and waits for one client. When
`MECHCORE_ADAPTER_SOCKET` is absent, the endpoint is
`/tmp/mechcore-adapter-<uid>.sock`. A configured path must be absolute and no
longer than 100 bytes. The adapter refuses to replace a non-socket or a socket
owned by another user, creates the endpoint with mode `0600`, and admits only a
peer with the same effective UID.

Messages are UTF-8 JSON, one object per line, with a maximum encoded size of
1 MiB. A new connection speaks first, and says what it is worth:

```json
{"kind":"claim","protocol":"mechcore.adapter.v7","level":1}
```

The level is `0..=4`. It orders clients and nothing else: a claim strictly
above the level of the client being served takes the game from it, and an equal
or lower one is refused. Two clients that matter the same amount cannot each
decide the other should stop. A connection that says nothing within three
seconds is dropped, and anything that is not a claim is answered
`{"kind":"refused","protocol":"...","reason":"..."}` so it is not mistaken for
an adapter that stopped answering.

An admitted claim receives:

```json
{
  "kind": "hello",
  "protocol": "mechcore.adapter.v7",
  "capabilities": [
    "status",
    "start_test",
    "apply_layout",
    "record_fight",
    "record_replay_round",
    "record_watch_replay",
    "toggle_fight",
    "speed_up",
    "quit_match",
    "quit_game"
  ],
  "game": {
    "adapter": "9f2c…",
    "headless": true,
    "offline": true,
    "linger_seconds": 30
  }
}
```

`game` is what the claim was admitted to. `adapter` is the BLAKE3 of the
Adapter library the game loaded, read when the Adapter started; `headless` is
whether the game was started with `-nographics`; `offline` is whether the
process runs in a sandbox, which is how `mechcore` takes the network away and
nothing else puts the game in one; `linger_seconds` is how long
the game waits for its next client before it quits itself, and is `null` for a
game that waits for ever. A launch reads it to decide whether a game left
running can do its work
([session.md](../mechcore/session.md#leaving-the-game)).

A game lingers only when it was started with `MECHCORE_ADAPTER_LINGER_SECONDS`,
a positive number of seconds, which `mechcore` sets on every game it launches;
any other value stops the Adapter from starting. Once a lingering game's client
leaves, the Adapter takes the game back to the main menu, since nobody else is
left to. When no claim arrives for that long, it quits the game from the main
menu, answers every claim that arrives meanwhile `evicting`, and ends the
process itself if it has not exited 30 s later.

A claim that does not win is answered instead:

```json
{"kind":"busy","protocol":"mechcore.adapter.v7","holder_level":1,"evicting":true}
```

`holder_level` is what the claim lost to, or is taking the game from.
`evicting` says the claim did win: the game is being handed back right now, and
the client is expected to connect again rather than give up. The adapter admits
its next client only once the interrupted match has been left and the main menu
is up, which is why the winner is told to come back rather than handed a
connection immediately.

The client being served is told before its connection closes:

```json
{"kind":"evicted","protocol":"mechcore.adapter.v7","by_level":3}
```

That notice is the difference between a taken game and a crashed one. A client
that reads it must leave the game process alone: the adapter is keeping it at
the main menu for whoever claimed it. An operation still in flight is answered
first, with the error code `evicted`.

Eviction is the adapter's, not the client's, and it is unconditional. Every
wait inside a long operation stops at its next polling point, a capture
included; a client that is between operations is closed without waiting for it
to speak; and the game is returned to the main menu before the next client is
admitted. Nothing partial is ever published: an abandoned capture is torn down
and an abandoned match simply produces no recording.

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
`record_fight` remains active through the complete logic-tick capture and
atomic MCFR publication. `record_replay_round` additionally owns replay
loading and round selection; it fights the replay without a scene, so it starts
and ends at the main menu.
`record_watch_replay` owns live matchmaking-scene selection, the complete
spectated match, native GRBR publication, and return to the main menu; a higher
claim ends it early and without a recording.
Other cross-scene readiness belongs to the session layer, which observes the status
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

Input is the complete layout object defined by [layout.md](../document/layout.md), with a
positive top-level `round` and `sides.blue` and `sides.red` fields.
A layout is valid at any round, but this operation stages every earlier setup
round inside one timeout budget, so it refuses a `round` above
`MAX_STAGED_ROUND`, which is `15`. That budget is an executor limit, not a game
rule or a schema rule.

Typical output:

```json
{"applied":true,"round":3,"unit_count":12,"construction_count":1,"contraption_count":0,"skipped_rounds":[1,2],"stages":[{"stage":"prepare"},{"stage":"activation"}]}
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
unregister or remove them. Selecting map 1021 produces 27 neutral
crystals with zero RVO controllers, while map 1001 produces 891 with 73 RVO
controllers. These are map inputs, not a Training Ground cleanup category.

Replay and Training Ground capture allocate MCFR Building IDs by
`(team_id, building_type_id, position.x, position.y, position.z)` using raw Q32.32
coordinates; duplicate keys fail closed. Native construction/building counters
and layout order do not determine these IDs. Existing IDs and target/event
references remain stable after allocation, including after object removal.

A structurally valid layout may contain a standing Sticky Oil Bomb area as a
`sticky_oil_bomb` entry of a side's `battle_skills` carrying `standing`. During
activation the Adapter installs every standing shield, then every standing oil
area, after the contraptions and before any release. It expands each oil
area's two ordered control points with the native fixed-point primitives,
creates only the mapped active indexes through `RangeItemSystem.AddItem`, and
restores any final clipped grids with immediate native readback. The mechanism
it restores is [terrain.md](../../rules/terrain.md); which sources
produce which values is not part of that contract, and this operation restores
whatever the replay recorded rather than deriving it from a type.

### quit_game

Input is an empty object.

Typical output:

```json
{"requested":true,"exit_code":0}
```

The operation invokes Unity application shutdown. It refuses with
`invalid_game_state` unless the game is at the main menu with no current match.
The adapter confirms the request; the session additionally waits for the
Adapter to disconnect.

### record_fight

Input contains one absolute, non-existing destination with a `.mcfr` suffix:

```json
{"output":"/absolute/path/fight.mcfr"}
```

An optional `video_output` enables the logic-frame visual recording, and an optional `speed_up`
selects whether the recording runs on scaled time:

```json
{
  "output":"/absolute/path/fight.mcfr",
  "video_output":"/absolute/path/fight.mov",
  "speed_up":true
}
```

`speed_up` defaults to `true` and applies only without `video_output`. From arming until the
terminal tick the adapter holds Unity's `Time.timeScale` at 50, setting it again on every
`FightController.Update` because the game's `FightSpeedUpController` sets its own play speed while it
fights, and then puts back the value it found when it armed; a failed or stopped recording puts it back
too. The fight advances in logic ticks whatever the frame rate and the capture samples every tick, so
the scale changes how long a recording takes and nothing it records: a Rhino mirror of 210 ticks, a
Crawler against Wasps of 417 and a replay round of 2289 hash the same at 1x and 50x. It is set at
arming, not at the first fighting tick, because the transition into fighting runs on scaled time too
and took two of a recording's seconds unscaled. Above 50 a recording is bound by the game's own work
per tick: the Crawler swarm takes about 4 ms a tick at 50x and at 100x alike. The recording
does not vote for the game's speed-up (`RequestSpeedUp`), which the separate `speed_up` operation
still does. `record_replay_round` has no such field: it fights without a scene, so no frame paces it.

The optional path must be absolute, non-existing, distinct from `output`, and use `.mov`. A game
started with `-nographics` renders no frame, so it refuses a `video_output` with
`invalid_game_state` before arming anything. With no
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
recording temporarily sets `Application.targetFrameRate` to 20 and runs on unscaled time: the render
barrier below paces the logic update. A main-camera post-render hook releases the next `FightController.Update`; pixel readback at
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
is disabled, time is scaled as `speed_up` describes. It calls `mechcore-mcfr::McfrWriter` directly, whose `finish` reads the
packaged file back and checks its hash before publishing it atomically, and returns the
state/transition counts and `hashes.result_hash`.

Projectile release/removal and damage use narrow native hooks so objects created and removed inside
one logic step remain in `E`. The release hook records the native projectile, owner and target; the
removal hook additionally records position and the native `intercepted` argument. During
`DamagePerformer.Perform`, the Adapter scopes the native provider attribution. Each
`FightController.OnActorHitted(HitDamageInfo)` then records one actor target and its positive
`damageReal`; `FightActor.ReduceLife(HitDamageInfo)` scopes that same attribution around synchronous
death processing. Battlefield Shield damage is recorded per actual Shield result. The same native
chain attaches the Shield reference to the subsequent projectile removal as `absorbed_by`. A target
no snapshot has numbered yet, a unit or shield that joined the fight within the tick, such as a
Rhino Assault's rhinos, is held by its native object and resolved once the tick's snapshot has
numbered it; a target that snapshot does not hold fails the recording. A unit that stands alive
again after its death, as a Phoenix does, may die again.

The native snapshot closure directly reads units, projectiles, the alive `FightCrystal` union
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
Unknown shield data-source classes, inconsistent active-list membership and dangling projectile birth
containment references fail the recording. A destroyed shield's address can be handed to a later
object, so a pointer that has left the full list and appears again is a new shield with a new
Shield ID, as a terrain's is. Whether the game pools the objects or the collector reuses the
memory was not read. Shield lifecycle
events are generated from authoritative changes in full-list membership at consecutive native
snapshot boundaries; the removal source proves destruction but not a narrower cause, so
`shield_destroyed.reason` is `unknown`.

Embedded layout contraptions are read from current native inventory, not
`GetFightObjectRecorder().GeRecords()`: full contraption shields (including
inactive reset-next-round objects), `TeamMineManager.GetLandMines()`, and the
live interceptor subset of `TeamInterceptSourceManager` sources. This retains
earlier-round objects without resurrecting consumed missiles or destroyed
interceptors. Unit interception sources are not layout contraptions. Layout
retains native full-list shield order at deployment and is not delayed or
reordered from S(1).

The same shield collection also holds standing Shield Airdrops. Those are
commander-skill objects, so export splits them out of `contraptions` into
standing `shield_airdrop` entries of the side's `battle_skills`. Each standing
Sticky Oil Bomb area, read from `RangeItemSystem` and grouped by its provider,
is exported as a standing entry named by that provider's skill.

Units are numbered afresh by every snapshot before S(1), because units can join as the fight
starts and the initial numbering is S(1)'s scene; the numbering S(1) writes is final, references the
first combat tick cached are carried over to it, and later units append IDs.

Native hooks use temporary Shield IDs before S(1). At the first advancing combat
snapshot, initial IDs are assigned once in `(team_id, active_order)` order, with
inactive shields following each team's active shields. First-tick-removed
shields follow all S(1) rows, grouped by team, keeping persisted IDs contiguous.
Those fallback groups sort by source kind, owner reference, position X/Y/Z,
radius, round policy, maximum energy and current energy; indistinguishable keys
fail closed. Cached references and first-tick traces are remapped before state,
projectile, instrument and event references are finalized. Removed objects
retain identities for E(1). After that boundary, IDs remain pointer-stable even
when active order changes; later new shields append IDs without reuse.
The layout still uses only `type`, `x`, `y` and ordinary placement bounds.

Dynamic terrain is enumerated by the six native `RangeItemType` controllers. A controller or item
list that has not been instantiated contributes an empty collection; each member returned by
`RangeItemController.GetItems()` is active and receives a stable Terrain ID. A pointer that has
left the lists and appears again is a new terrain with a new Terrain ID, since a released item's
address can be handed to a later object. The Adapter reads its
team, type, position, radius, optional grid mask, optional cross-round remainder, optional logic
lifetime, and the controller's direct unit applications. `affectedUnits` is joined to Unit IDs;
`affectedUnitTimes` and positive `effectTimeDuration` provide an optional periodic clock.
Terrain creation and removal events follow authoritative item-list membership changes at consecutive
snapshot boundaries. Removed pointers are tombstoned, and the removal source records
`terrain_removed.reason=unknown`. These snapshot-difference events are synthesized,
not native callback traces: creation events are appended first, then removal
events, with each batch ordered by stable Terrain ID rather than process-local
pointer address. Previously captured combat callbacks retain their original order.

Native capture coverage includes `oil`, `fire`, `acid`, and `fog` battle-skill
layouts. In those recordings all four terrain types were present in `terrains.parquet` and their
application lists referenced the affected enemy units. Oil populated the sparse
`remaining_rounds` field; fire, acid, and fog used null.

### record_replay_round

Input identifies an existing native replay, a one-based combat round, and a new
MCFR destination. The round has no upper bound: reading round `N` out of a
replay is decoding, not staging, so `MAX_STAGED_ROUND` does not apply here. The
replay may be one the game saved or a layout that `mechcore convert --to grbr`
wrote as one.

```json
{
  "grbr": "/absolute/path/match.grbr",
  "round": 6,
  "output": "/absolute/path/round-6.mcfr"
}
```

Both `record_replay_round` and `record_fight` take an optional `instrument`
list, the channels to record into the MCFR beside its tables:

```json
{"instrument": ["target_refs", "skill_attackable_checker", "target_search"]}
```

A channel is a view of the fight's inside that a study asks for. Channels
combine freely, every requested channel is written as the fight is, one member
`instrument/<channel>.parquet` each, and the hash does not read them, so a
recording with channels hashes the same as one without
([mcfr.md](../mcfr/mcfr.md#instrument-channels)). A tick whose captured rows
do not answer exactly the requested channels fails the recording with
`capture_failed`. A channel whose hooks could not be resolved is refused before
arming, naming the channel and why.

`target_refs` records, per unit and tick, the mech's lock and the main skill's
`lockTarget` and `attackTarget`. Beside them, `skill_state` names the class of
the skill's current `SkillStateController` state (`SkillIdleState`,
`SkillPrepareState`, `SkillAttackState`, `SkillCoolingState`, …),
`skill_attack_phase` says which `SkillAttackController` phase is current —
`before`, `attacking`, `after`, or null between blows — and `skill_is_idle`
reads `FightSkillBase.IsIdle`. All three are plain field reads at the snapshot
boundary; they are null for a skill that is not a `FightSkill`.

`group_slots` shows what `target_refs` cannot for a grouped unit, whose main
skill is a `SkillGroup` rather than a `FightSkill`: a Wraith's four slots are
four `FightSkill`s, each with its own lock, attack target and state machine.
For every unit whose main skill is not a `FightSkill`, it records one row per
`FightSkill` in the unit's `GetSkills()`: the skill's index there, its
`lockTarget` and `attackTarget`, and the same `skill_state`,
`skill_attack_phase` and `skill_is_idle` reads. A unit with an ordinary main
skill has no rows.

`skill_attackable_checker` records every call of
`SkillAttackableChecker.Check(bool isAttackingCheck)` made during the update a
row closes, in call order: the skill's owner, the skill's index in the
owner's `GetSkills()` (or its parent's, for a child skill the list does not
hold; null when neither is listed), `is_attacking_check`, the skill's
lock, attack target, state and attack phase read just `before` the call and
just `after` it, with its `attackTime`, `attackInterval` and its attack
controller's `performCount`, and what it returned. The method is hooked the way the
selector method is — its first four arm64 instructions, `sub sp` and three
`stp`, are stack-only and move to a trampoline unchanged — and is forwarded
with its arguments and result untouched; the reads on either side are field
reads, with no managed call. The skill's index is found at the snapshot
boundary, where `GetSkills()` may be called.

`target_search` records every target search of the update, one row each in
the order the searches returned: the unit or construction that searched, the
skill's index in its `GetSkills()` (null when it searched for itself), the
path the build took, how many candidates it scored, what it returned and the
nearest actor. `target_candidate` holds a search's five lowest scores, which
the build calls best, with the candidate and, where they are seen,
`CalculateScore`'s arguments: distance, distance score, angle, angle score
and side, and the search's maximum attack range, source rotation and rotation
window. Rows grow with the number of searches, not with
searches times candidates.

The build scores targets three ways, and the Adapter names a candidate in each:

- `select`: `ScoreRatingTargetSelector.Select` over fewer than 51 actors
  scores on the main thread, calling `CalculateScore` and then
  `Selector.CheckResultTarget(score, candidate)` for each candidate. The terms
  are kept from the first and the candidate taken from the second.
- `select_job`: `Select` over 51 or more actors scores on worker threads in
  `ScoreRatingTargetSelectJob` and calls `CheckResultTarget` on the main
  thread afterwards, so the candidate and its score are seen and the terms are
  null.
- `team`: a main skill without a skill group whose selector is a score
  selector is searched for its whole team at once by
  `TeamScoreRatingTargetSelectJob` on worker threads, which keeps only each
  source's winners. After `TrySelect` reads them back, the Adapter scores the
  source's candidates again from the job's `SourceData` and `TargetData`, with
  the job's own native functions (`DistanceCalculator.Calculate`,
  `FPoint.op_GreaterThanOrEqual` for the minimum range,
  `DistanceScoreCalculator.Calculate`, `FightUtility.CalculateAngle`,
  `AngleScoreCalculator.Calculate`, `CalculateScore`), and the winner the job
  kept must be the lowest score within `FPoint.op_LessThan`'s tolerance, or
  the recording fails.

A search that falls back from `TrySelect` to `Select` is two rows. What a
search returned is the second best when the best is not visible.

The three RVO channels record each `RVOAgentFixed.CalculateVelocity` of the
update, read on either side of the call. A solve sees only its agent's
neighbour list, at most `maxNeighbours` agents (20 in every recording so far),
and makes at most one VO from each, so the channels grow with the number of
agents, not with its square.

- `rvo_solve`, one row per solve: the agent's position, elevation and height,
  current and desired velocity (`sync_desiredVelocity`), desired target and
  speed, maximum speed, radii, size, priority, layers, `sync_group`,
  `sync_ignoreSameGroup`, `sync_team`, `maxNeighbours` and the neighbour count;
  how the solve ended (`locked`, `manual`, `free`, `avoided`); the velocity and
  target `BiasDesiredVelocity` left; the best point and score of each of the two
  `Trace`s of an avoided solve; and the `calculatedTargetPoint` and
  `calculatedSpeed` it wrote.
- `rvo_neighbour`, one row per entry of the neighbour list, nearest first: the
  neighbour, its squared distance, what `GenerateNeighbourAgentVOs` made of it
  (`opponent` for another RVO group, `team_radius`, `same_group`,
  `ignored_same_group` when the neighbour lets its group pass, `other_elevation`
  when the vertical ranges do not overlap), the index, radius and `colliding` of
  the VO it made, the VO's `Gradient` weight at the unbiased desired velocity
  (how far inside it that velocity lay) and, for an avoided solve, its
  `ScaledGradient` weight at the output velocity; the largest of those is the VO
  that bound the solution. A neighbour the recording does not hold, a map
  `FightCrystal` in neither side's building lists (map 1032 has them, map 1021
  none), has a null identity.
- `rvo_vo`, one row per VO of the solve's buffer, every field of `Agent.VO`. It
  is the one to leave off: it is about six times the other two together,
  in a fight of a few dozen units and in a round of a few hundred alike.

The kind of each neighbour is decided the way the build's loop decides it, and
the VOs it counts must equal the buffer's length, or the recording fails. The
weights come from the build's own `VO.Gradient` and `VO.ScaledGradient`, called
on a copy of each VO. A multithreaded simulator solves on worker threads and
joins them at the next update, four ticks later; while an RVO channel is
armed, the Adapter joins them before the tick that started them ends, with the
simulator's own `BlockUntilSimulationStepIsDone`, so every solve belongs to that
tick. The solves read only the buffers the update synchronised, and the physics
hash of every fight recorded with the channels equals the one recorded
without. A solve of an agent the recording does not hold is left out, and
must be a locked one.

The Adapter requires `main_menu`, and the fight never leaves it: the replay is
fought the way the game's own `SimpleSimulator` fights one, in a
`FastSimulationMatch` that builds no scene and runs to its end inside one
main-thread call. `MatchUtility.LoadReplay` reads the file and
`BattleRecord.IsValid` and `BattleRecord.IsAvaliableRound` must accept it and the
round. Every `PlayerRoundRecord` after the round is dropped, which is where the
match reads the round it ends on. The Adapter then builds the `BattleSetting`
that `PlayReplayCommand.StartReplay` builds for a replay (`MatchType.Replay`, the
record's map, both players replayed, the requested start round) and hands it to
`MatchUtility.StartFastBattleSimulation` rather than to `ClientAgent.CreateHost`.
That call sets `ExternalConfig.fastBattleSimulation` and clears
`fastFightSimulation` and never puts them back; while the first is set,
`CreateHost` refuses every match that is not a replay, so the Adapter restores
both before it returns.

A round opens from its `PlayerRoundRecord` snapshot, not from the actions of the
rounds before it. The fight draws only from streams the game derives from the
record's `SystemSeed` and the round, so a round fought this way is the round a
scene replay of the same file fights.

Capture is armed before the match exists. It reaches the match through the
current `FightController`'s `match`, since a `FastSimulationMatch` is not a
`MatchClient`, and it ignores every update until `Match.get_RoundCount()` reads
the requested round. The capture hook reads the embedded layout at entry to the
final player's `PlayerController.FinishDeploy()`, before the native transition
can initialize fighting, but does not persist that pre-update state. `S(1)` is
the first state row. Earlier players are rejected unless every other player has
already completed deployment, so a partially replayed deployment cannot be
published. The existing fighting-to-over edge terminates MCFR recording. The
headless call runs on the main thread from a thread of its own, and the queue is
drained into the writer while it runs, so writing overlaps the fight; the
operation waits for the call to return before it answers or stops the capture.
The call returns once the fight is over. A fight that runs out of time ends
outside `FightController.Update`, so the capture never sees its fighting-to-over
edge; the tick last captured before the call returns is then the terminal one.
Two corpus rounds that ran out of time both end at tick 2360.

Replay formations remain ordered by and export their stable native unit index,
but those indices may contain gaps left by units removed in earlier rounds.
Each formation also exports its integer `MechTeam` experience. Replay
constructions export only `type/x/y`, sorted by that tuple, without native
construction indices. Negative and duplicate unit indices and negative experience fail
closed. Active commander
abilities enter `battle_skills` only when native
`TryGetReleaseCommanderSkillData` supplies positional release data; active
non-release abilities are outside that layout field.

The writer verifies the MCFR before publishing it, and the operation returns at
the main menu it started from. It never quits the game process. Invalid input, an unavailable
round and a capture failure publish nothing. A replay whose match ends before
the requested round's fight, so that the call returns with no tick captured, is
a `capture_failed` refusal as soon as it does, since nothing more can arrive.

### record_watch_replay

Input names bounded scene/match timeouts and may name an absolute corpus
directory:

```json
{
  "wait_for_scene_seconds": 900,
  "match_timeout_seconds": 7200
}
```

With no `output_dir`, `output` is the game-owned file under
`Mechabellum.app/ProjectDatas/Replay` and `published_copy` is `false`. An
explicit different directory receives a create-new copy and reports
`published_copy: true`; explicitly naming the native Replay directory is
equivalent to omitting the field.

A higher claim stops the operation at its next poll, wherever it is, and it
fails with the `evicted` code rather than returning a recording. Abandoning the match is the
price of handing the machine over within seconds rather than hours.

The operation admits only a round-one, normal `VS_1_1` scene from the server
matchmaking watch list. It watches to `Match.get_IsFinished`, waits ten seconds
for the game's own autosave to produce a stable new native GRBR, requests
`MatchProxy.SaveReplay` if none appears, copies the file without overwriting,
and returns only after the native match quit reaches `main_menu`. The recording
itself is the game's, and is published as written.

[`mcscript.md`](../mechcore/mcscript.md#unattended-standard-1v1-corpus-recording)
states the scene admission rules, which a script cannot widen.

Requalifying this path after a game update means running a batch and accepting
it only when one result has `operation.recorded` and
`operation.cleanup.match_exited` true, a final status of `main_menu`, an output
file `mechcore convert --to match` can open carrying the build and a non-negative seat, and
no managed exception in either log. That decode is the reviewer's check on a new
build, not a step the collector performs per match.

### save_replay

Input may name an absolute path the replay is copied to:

```json
{"output":"/tmp/slices/scene-round-4.grbr"}
```

Typical output:

```json
{"saved":true,"native_source":".../ProjectDatas/Replay/2324_20260928--861_[a]VS[b].grbr","output":"/tmp/slices/scene-round-4.grbr"}
```

The operation calls `MatchProxy.SaveReplay` on the match being watched,
finished or not, waits up to 30 seconds for the file it writes under
`Mechabellum.app/ProjectDatas/Replay` to settle, and copies it to `output`
without overwriting. It refuses with `invalid_game_state` when there is no
match or the match is not watched. The game names a replay after its match,
so a second save of one match rewrites `native_source`; `output` is where a
copy that must survive the next save goes.

A replay saved mid-match holds every round the spectator was present for, from
the round it joined in, and an empty round-0 record ahead of them when it
joined after round one. A round's record is complete once the spectator has
entered that round's fight. Joined before round two, the spectator builds the
match from its start and the replay also holds round 0, the opening
specialist choice. `BattleRecord.GetAvaliableStartRound` makes the joined round
the one a replay of it starts from.

### quit_match

Input is an empty object.

Typical output:

```json
{"performed":true}
```

The operation requests that the active Training Ground, replay, or spectated
match exit. It refuses with `invalid_game_state` when there is no active match.
The MCP layer additionally waits for `main_menu`.

### speed_up

Input is an empty object.

Typical output:

```json
{"requested":true}
```

The operation submits the native fight speed-up request. It refuses with
`invalid_game_state` when there is no active match, and again when the match
exposes no action controller. `status` carries no speed field, so the native
call completing normally is the authoritative completion condition.

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

Both `PlayerAgent` objects are configured with `FirstRoundSupply=10000` and
`MaxRoundSupply=10000` before `CreateHost`, with exact getter readback
required. `apply_layout` therefore performs no hidden economy mutation of its
own: native layout actions still run their ordinary affordability checks and
deduct their exact costs.

### status

Input is an empty object.

Main-menu output:

```json
{"status":"main_menu"}
```

Training Ground output:

```json
{"status":"training_ground","round_count":1,"fight_ready":true,"deploying":true,"fighting":false,"match_seed":1787720817}
```

`status` is exactly one of `main_menu`, `training_ground`, `replay`,
`spectating`, or `unknown`. Every active match status additionally reports
`round_count`, `fight_ready`, `deploying`, `fighting`, and the effective
`match_seed` read from the native match random stream; a temporarily
unavailable native detail is `null`. `fight_ready` is whether the match already
owns a `FightController`, so `deploying` and `fighting` are `null` exactly while
it is `false`. `spectating` also reports `finished` from
`Match.get_IsFinished`, `watch_delay` from `BattleInfo.WatchDelay`, and `live`.

`live` is what the fight server said the match was doing when the spectator
joined, `null` until the server has answered the join. A spectator runs about
`watch_delay` seconds behind the match, so its own `round_count`, `deploying`
and `fighting` say where the match was; `live` is copied from the
`MatchStageInfo` answering the join, in `MH_MatchStageInfo.DoProcess`, since
the client keeps none of it:

```json
{"round":7,"state":"deploy","state_elapsed_seconds":61.0,"since_start_seconds":852.0,"deploy_time_seconds":100,"deploy_remaining_seconds":39.0}
```

`state` is one of `zero`, `loading`, `prepare`, `deploy`, `fighting` or
`ending` (`EFightState`). The two elapsed times are measured by the server's
clock, `ServerProxy.GetTimeSpanToCurrentServerTime`, from when the state and the
match began, and are read afresh at each `status`. `deploy_remaining_seconds`
is set in `deploy` only: `BattleInfo.DeployTime` less the time spent, the
latest the fight can begin, since both players may finish sooner. Deployments
have been seen to run a few seconds past it.

`main_menu` is the menu scene, `MainMenu` at build index 0, with no current
match. That scene is the active one from the game's start, at its login window
and at the popup an offline game stops on, so `main_menu` does not say the
game has logged in or shown its own main menu, only that no match is in the
way of the next operation.

`status` refuses nothing. It reports `unknown` rather than failing, so a caller
may use it to decide what to do about any other operation's
`invalid_game_state`.

### toggle_fight

Input is an empty object.

Typical output:

```json
{"performed":true}
```

The operation changes the Training Ground process state from deployment to
fight. The MCP layer additionally observes the fight transition before
returning.

### watch_scene

Input names a scene of the lobby's page:

```json
{"scene_id":201458701}
```

Typical output:

```json
{"started":true,"scene_id":201458701}
```

The operation calls `LobbyProxy.WatchScene` from `main_menu` and returns at
once. The spectator is in once `status` reports `spectating` with `live` set,
which took 2.5 to 8.4 seconds whenever the server answered. Some listed scenes,
taken to have ended, were never answered. It refuses with
`invalid_game_state` in a match or when the lobby is not registered.

### watch_scenes

Input may set `refresh`, `true` by default:

```json
{"refresh":false}
```

Typical output:

```json
{"ready":true,"refreshed":false,"scenes":[{"scene_id":201458701,"map_id":1021,"round":7,"watcher_num":0,"standard":true}]}
```

The operation reads the lobby's cached first page of match-made scenes,
`LobbyProxy.GetRoomFilterDataByType(MatchFirst).GetWatchScenes()`, at most 20
scenes. With `refresh` it first asks the lobby for a new page
(`SwitchToRoomListFilter`), which empties the cache until the server answers
some seconds later, so a caller refreshes once and then reads without
refreshing. `standard` applies the rules `record_watch_replay` admits a scene
by, at any round. `round` is the lobby's, which has differed by one from the
round `live` reported on joining. `ready` is `false` until login has
registered the lobby.

## Errors

A failed response carries a `code` and a human `message`. The code is what a
caller branches on, and it answers one question: is repeating this request
worth anything?

**The request is wrong.** Repeating it unchanged cannot succeed.

| Code | Raised when |
| --- | --- |
| `invalid_arguments` | an argument is missing, malformed, or names a field no argument type declares |
| `invalid_request` | the line is not a request this protocol defines |
| `unsupported` | the operation is real but this build does not implement the path it needs |

**The game is not where the operation needs it.** Repeating the request from
the same game state fails the same way. Something has to move the game first.

| Code | Raised when |
| --- | --- |
| `invalid_game_state` | the operation requires a scene or phase the game is not in |
| `game_rejected_operation` | the game was in the right state and refused the native action anyway |
| `il2cpp_error` | a call into the game failed; a managed exception is named by its class and the managed frames it was thrown through |

**The game was taken.** `evicted` means a higher claim arrived. It is the one
failure that asks the caller to come back: the adapter holds the game at the
main menu for the winner, so a client that reads it must leave the process
alone and reconnect rather than treat the adapter as crashed. An operation in
flight is answered with this code before the connection closes.

**The operation did not finish.** `operation_timeout` and
`main_thread_dispatch_failed` say the work did not complete, not that it did
not happen. A mutation is never retried automatically, and a caller that
retries one is responsible for deciding the game is still where it thinks.

**The output failed.** Nothing partial is published, so a failed recording
leaves nothing to clean up, and retrying is harmless but pointless until the
cause is addressed.

| Code | Raised when |
| --- | --- |
| `capture_failed` | the capture itself broke, including a second recording header |
| `deployment_capture_failed`, `deployment_capture_timeout` | a named round's readback failed or did not reach its opening/finish boundaries |
| `match_capture_failed` | round/source coverage, state continuity or match writing failed, or the whole-match resource budget expired |
| `match_publication_failed` | destination creation, source identity recheck, syncing or no-clobber publication failed |
| `mcfr_error` | the recording could not be written, or did not read back as written |
| `video_error`, `video_verification_failed` | the optional video output could not be written or did not verify |
| `native_replay_directory` | the native replay directory could not be resolved |

**The teardown failed.** `replay_cleanup_failed` and `watch_cleanup_failed`
replace whatever code the operation would otherwise have returned, so the
original cause survives only in the message. They mean the game may not be back
at the main menu, which makes them the one class where the next operation's
precondition is genuinely unknown.

## Unresolved

**Should the error codes be a closed set?** A code is a free-form `String`
constructed at each failure site, and only `evicted` has a named constant in
`mechcore-protocol`. A caller cannot match exhaustively, and a typo at one site
is indistinguishable from a new code. Making them an enum would settle it, at
the cost of a protocol change every time a failure mode is added.

**Should a teardown failure hide the failure it followed?** Today the cleanup
code wins and the original error survives only as text. The alternative is
reporting both, which needs a shape the response format does not currently
have.

**Whose limit is `MAX_STAGED_ROUND`?** This contract refuses a round above it
because staging every earlier round must fit one timeout budget, and says so as
an executor limit rather than a schema rule.
[layout.md](../document/layout.md) leaves the matching question open from the
other side: whether a layout above that round is a valid document nothing can
currently apply, or not a document at all.
