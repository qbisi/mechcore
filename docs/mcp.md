# Mechcore MCP

[TOC]

`mechcore mcp` is a standard-input/standard-output MCP server and the sole
client of the in-game Adapter socket. It does not load the Adapter, launch the
game, or own the game process. It serializes all mutations and never retries a
mutation after an uncertain transport failure.

## Invocation

Build both release artifacts:

```sh
cargo build --release
```

Run the MCP server:

```sh
mechcore mcp
```

The subcommand accepts no additional arguments or project-specific environment
variables. The project-level Codex configuration starts the server through
`cargo`, so it does not depend on the checkout path.

The controlling Agent separately builds and loads the Adapter and launches the
game as documented in [Adapter build and launch](adapter.md#build-and-launch).
After launch, call `connect_adapter`; MCP connects to the Adapter's default
user-scoped socket.

## Status stream

The `status` tool returns the latest snapshot. Clients that need continuous
updates should subscribe to the standard MCP resource `mechcore://status` and
read it whenever `notifications/resources/updated` arrives. Its content type is
`application/json`.

Before Adapter connection or after Adapter disconnection, the snapshot is:

```json
{"status":"game_off"}
```

Once connected, Adapter states are passed through unchanged. Because MCP does
not own the process, `game_off` means only that no Adapter connection exists;
it is not an operating-system process observation. One centralized MCP monitor
samples Adapter `status` and publishes only changed snapshots; lifecycle tools
and resource subscribers consume that same stream.

## Capture lifecycle

Every capture is a bounded Training Ground transaction inside a game session.
The controlling Agent must execute calls in this order:

```text
session: launch game with Adapter outside MCP -> connect_adapter

capture 1: start_test -> apply_layout -> record_battle -> main_menu
capture 2: start_test -> apply_layout -> record_battle -> main_menu
...

native replay capture: record_replay_round -> main_menu

session end: quit_game -> game_off
```

`record_battle` owns the per-capture cleanup boundary. After publishing and
verifying the requested artifacts, it leaves the completed Training Ground and
returns success only after `main_menu` is observed. Its structured result marks
`cleanup.match_exited=true`, reports the final status, and identifies
`start_test` and `quit_game` as the legal next session actions. It never quits
the game. Keep the process only when another layout capture is already queued;
that capture begins again with `start_test`. When the capture queue is empty or
paused, call `quit_game` from `main_menu` instead of leaving the game idle.

If recording or cleanup fails, the error result reports whether recording was
confirmed, the observed status, whether external process resolution is needed,
and any required `quit_match` recovery. Mutations with unknown outcomes are not
retry-safe. `quit_match` remains public for manual test/replay cleanup and for a
reported recovery obligation; it is not part of the successful capture path.

## Tools

The MCP server exposes exactly ten tools.

### apply_layout

Input is the layout object from [layout.md](layout.md), not a path and not a
nested `layout` wrapper. Its required top-level `round` selects the activation
round and must be in `1..=15`. The call requires first-round deployment and
returns only after all setup rounds have been skipped, the adapter's staged
native actions/readbacks succeed, and that activation round reports
`deploying=true` and `fighting=false`.

The shared layout schema accepts build-2259 retained Sticky Oil Bomb terrain in
`sides.<side>.terrains`. During activation the Adapter expands each entry's two
control points with the same native fixed-point primitives as the replay routine, restores its active indexed
points through `RangeItemSystem.AddItem`, and verifies any declared clipped
grid by immediate native readback. The Simulator remains fail-closed for
non-empty terrain layouts; this operation is native replay/Training Ground
restoration, not simulator closure.

### connect_adapter

Input is empty. The Agent must first launch Mechabellum with the Adapter by
following [Adapter build and launch](adapter.md#build-and-launch). This call
retries the default `/tmp/mechcore-adapter-<uid>.sock` for up to 60 seconds,
validates the Adapter protocol and exact capability list, and returns the first
native status snapshot. It neither starts nor terminates a process.

### quit_game

Input is empty. The call requires `main_menu`, requests native application
shutdown, and returns after the Adapter disconnects and the streamed state
becomes `game_off`. MCP does not independently verify the operating-system exit
code.

### record_battle

Input is `{"output":"/absolute/path/battle.mcfr"}`. The path must be absolute, must use the
`.mcfr` suffix, and must not already exist. An optional
`"video_output":"/absolute/path/battle.mov"` enables a QuickTime Motion JPEG sidecar. It is
disabled by default; its path must be absolute, new, distinct from `output`, and use `.mov`.
The call requires completed Training Ground deployment.
It owns the complete recording transaction: the adapter starts combat, requests the wall-clock-only
speed-up vote for MCFR-only capture, and returns only after the adapter captures the fighting-to-over
boundary, publishes the MCFR, and reopens its structure. Video capture disables speed-up, temporarily
sets the Unity target frame rate to 20 fps, and uses a main-camera post-render barrier. When video is
enabled, `calibration_topdown` suspends the
native camera input/Cinemachine controllers and fixes the main camera at world `(0,1070,-1070)`,
45-degree rotation `(45,0,0)`, perspective field of view `20` degrees, and 1920x1080 output. The equal
Y/Z offsets aim at the battlefield origin. The vertical field of view is unchanged from the earlier
2560x1600 render, so vertical coverage matches while horizontal coverage is wider; frames from the
two resolutions are not pixel-comparable. Perspective rendering avoids the opaque black tower-shadow quads produced by the native decal
shader under an orthographic camera, while the disabled controllers prevent external mouse drift. One
completed screen frame, including the normal game UI, is captured for each MCFR snapshot before the
next logic update may advance; this begins at `S(1)` and includes the rendered terminal snapshot. The MOV uses the MCFR
logic step as its sample duration, and publication fails unless its frame count exactly matches the
MCFR tick count.
Callers do not issue `toggle_fight`, `speed_up`, or other Training Ground state controls while this
tool is running. After artifact publication, MCP invokes the Adapter's match-exit operation and
returns success only after `main_menu`; it never exits the game process. The result includes the
cleanup outcome and legal next session actions. A partial failure is not retry-safe and identifies
whether `quit_match` recovery or external process resolution remains required.
The returned video metadata identifies `view: "calibration_topdown"` and includes the fixed projection
and camera parameters needed to reproduce this calibration in another renderer.

### record_replay_round

Input is
`{"grbr":"/absolute/path/battle.grbr","round":6,"output":"/absolute/path/round-6.mcfr"}`.
The source must be an existing absolute `.grbr` file, `round` is one-based and
must be in `1..=15`, and `output` must be a new absolute `.mcfr` path. The call
starts only from `main_menu` and needs no preceding `start_test` or
`apply_layout`.

The Adapter jumps directly to the requested native replay round, enables
zero-delay deployment playback, arms MCFR capture at completed deployment,
requests native combat speed-up after the fighting boundary, records through
the over boundary, verifies the file, and exits the replay. MCP returns success
only after a fresh `main_menu` status readback. The game process remains alive
for another capture.

Replay capture also accepts the same optional `instrumentation` object as
`record_battle`. For local RVO research, use profile `target_refs_rvo_v1` with
`rvo_scope: {start_tick: 8, end_tick: 14, unit_ids: [124, 282, 363, 246]}`
and a new absolute `.h5` output. Scope allows 1–8 unique positive MCFR unit IDs
and at most 64 inclusive positive MCFR ticks; these are not formation indices or
wall-clock frames. Build 2259's native counter advances by 100 per MCFR tick.
The filter selects update-start ticks and retains their
later publication. Scoped sidecar rows are sparse and use actual MCFR tick
numbers. The formal `.mcfr` remains full-round. See `docs/adapter.md` for the
diagnostic fields and identity limitations.

### quit_match

Input is empty. The call leaves an active Training Ground or replay match and
returns only after `main_menu` is observed.

### speed_up

Input is empty. The call requires an active Training Ground fight and returns
after the adapter confirms the native speed-up request. There is no native
speed field in `status`, so successful execution is its completion condition.

### start_test

Input optionally contains `{"seed":1787720817}`. A nonzero signed 32-bit value
requests that native match seed; zero or omission lets the game generate one.
The call creates the fixed layout-test Training Ground and returns only after
round 1 reports `deploying=true`, `fighting=false`, and the effective
`match_seed` in its status snapshot.

### status

Input is empty. The call returns the current status-resource snapshot.

### toggle_fight

Input is empty. The call starts the current Training Ground battle and returns
after fighting is observed. If an exceptionally short battle advances the
round between samples, the round transition also proves that the operation
completed.

## Layout smoke client

After a release build, run:

```sh
scripts/smoke_mcp_layout.py tests/layouts/shield-missile-battle.yaml
```

The script launches the game with the Adapter outside MCP, initializes MCP,
verifies the exact tool and resource surfaces, connects to the Adapter,
subscribes to `mechcore://status`, starts the test, applies the YAML
layout, records and accelerates the battle through `record_battle`, verifies that
the requested MCFR path was published, then returns to the main menu and exits the game.
