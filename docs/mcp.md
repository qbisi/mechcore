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

The subcommand accepts no additional arguments. `MECHCORE_ADAPTER` is its only
runtime configuration variable and may name an alternative adapter dylib:

```sh
MECHCORE_ADAPTER=/absolute/path/libmechcore_adapter.dylib mechcore mcp
```

When the variable is absent, the default is
`libmechcore_adapter.dylib` beside the running `mechcore` executable. A normal
release build therefore resolves both artifacts inside `target/release`
without embedding the repository path. `start_game` uses the default macOS
Steam installation of Mechabellum.

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

The MCP server exposes exactly eight tools.

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
layout, starts and accelerates battle, waits for round-two deployment from
resource notifications, then returns to the main menu and exits the game.
