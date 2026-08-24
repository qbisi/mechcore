# Mechcore MCP

[TOC]

`mechcore mcp` is a standard-input/standard-output MCP server and the sole
client of the in-game adapter socket. It owns the game process that it starts,
serializes all mutations, and never retries a mutation after an uncertain
transport failure.

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
variables. It loads `libmechcore_adapter.dylib` beside the running `mechcore`
executable, so a normal release build resolves both artifacts inside
`target/release` without embedding the repository path. The project-level Codex
configuration starts the server through `direnv` and `cargo`, so it also does
not depend on the checkout path. `start_game` resolves the default macOS Steam
installation of Mechabellum beneath the current user's home directory.

## Status stream

The `status` tool returns the latest snapshot. Clients that need continuous
updates should subscribe to the standard MCP resource `mechcore://status` and
read it whenever `notifications/resources/updated` arrives. Its content type is
`application/json`.

Before launch or after a confirmed process exit, the snapshot is:

```json
{"status":"game_off"}
```

During launch it may be:

```json
{"status":"starting_game"}
```

Once connected, adapter states are passed through unchanged. A running game
whose adapter connection is lost is `unknown`. Because the adapter protocol is
request/response only, one centralized MCP monitor samples adapter `status`
and publishes only changed snapshots; lifecycle tools and resource subscribers
consume that same stream.

## Tools

The MCP server exposes exactly nine tools.

### apply_layout

Input is the layout object from [layout.md](layout.md), not a path and not a
nested `layout` wrapper. Its required top-level `round` selects the activation
round and must be in `1..=15`. The call requires first-round deployment and
returns only after all setup rounds have been skipped, the adapter's staged
native actions/readbacks succeed, and that activation round reports
`deploying=true` and `fighting=false`.

### quit_game

Input is empty. The call requires `main_menu`, requests native application
shutdown, and returns only after the MCP-owned process exits with code 0 and
the streamed state becomes `game_off`.

### record_battle

Input is `{"output":"/absolute/path/battle.mcfr"}`. The path must be absolute, must use the
`.mcfr` suffix, and must not already exist. An optional
`"video_output":"/absolute/path/battle.mov"` enables a QuickTime Motion JPEG sidecar. It is
disabled by default; its path must be absolute, new, distinct from `output`, and use `.mov`.
The call requires completed Training Ground deployment.
It owns the complete recording transaction: the adapter starts combat, MCP requests the wall-clock-only
speed-up vote, and the call returns only after the adapter captures the fighting-to-over boundary,
publishes the MCFR, and verifies its hashes. When video is enabled, `calibration_topdown` suspends the
native camera input/Cinemachine controllers and fixes the main camera at world `(0,1070,-1070)`,
45-degree rotation `(45,0,0)`, perspective field of view `20` degrees, and 1280x720 output. The equal
Y/Z offsets aim at the battlefield origin and preserve approximately the previous 1.5x center-plane
scale. Perspective rendering avoids the opaque black tower-shadow quads produced by the native decal
shader under an orthographic camera, while the disabled controllers prevent external mouse drift. One
rendered screen frame, including the normal game UI, is captured for each MCFR snapshot at the next
logic-update boundary; this includes `S(0)` and the rendered terminal snapshot. The MOV uses the MCFR
logic step as its sample duration, and publication fails unless its frame count exactly matches the
MCFR tick count.
Callers do not issue `toggle_fight`, `speed_up`, or other Training Ground state controls while this
tool is running. `quit_match` remains the separate owner of leaving the test after recording.
The returned video metadata identifies `view: "calibration_topdown"` and includes the fixed projection
and camera parameters needed to reproduce this calibration in another renderer.

### quit_match

Input is empty. The call leaves an active Training Ground or replay match and
returns only after `main_menu` is observed.

### speed_up

Input is empty. The call requires an active Training Ground fight and returns
after the adapter confirms the native speed-up request. There is no native
speed field in `status`, so successful execution is its completion condition.

### start_game

Input is empty. The call launches the default macOS game with the selected
adapter, validates the adapter protocol and exact capability list, and returns
only after `main_menu` is observed. Socket readiness uses bounded connection
attempts; there is no fixed initialization sleep.

`start_game` resolves the sibling adapter dylib on every game launch. Rebuilding or replacing that
dylib therefore requires only exiting and starting the game again; the long-running MCP process does
not need to restart as long as the adapter protocol capability surface is unchanged.

### start_test

Input is empty. The call creates the fixed layout-test Training Ground and
returns only after round 1 reports `deploying=true` and `fighting=false`.

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

The script initializes MCP, verifies the exact tool and resource surfaces,
subscribes to `mechcore://status`, starts the game and test, applies the YAML
layout, records and accelerates the battle through `record_battle`, verifies that
the requested MCFR path was published, then returns to the main menu and exits the game.
