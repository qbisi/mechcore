# Mechcore adapter

[TOC]

`mechcore-adapter` is an in-process Rust `cdylib`. On macOS the release
artifact is `target/release/libmechcore_adapter.dylib`; the artifact name is
not tied to a game version.

The adapter creates a Unix domain socket and waits for one client. The MCP
process assigns its private endpoint through `MECHCORE_ADAPTER_SOCKET` when it
launches the game. When this internal variable is absent, the fallback is
`/tmp/mechcore-adapter-<uid>.sock`. A configured path must be absolute and no
longer than 100 bytes. The adapter refuses to replace a non-socket or a socket
owned by another user, creates the endpoint with mode `0600`, and admits only a
peer with the same effective UID.

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
also waits until the requested activation-round deployment is stable. Other
cross-scene readiness belongs to `mechcore mcp`, which observes the status
stream before returning from lifecycle tools.

## Operations

### apply_layout

Input is the complete layout object defined by [layout.md](layout.md), with a
top-level activation `round` and `sides.blue` and `sides.red` fields.

Typical output:

```json
{"applied":true,"round":3,"formation_count":12,"skipped_rounds":[1,2],"stages":[{"stage":"prepare"},{"stage":"pre_activation"},{"stage":"activation"}]}
```

The operation begins only during first-round Training Ground deployment. It
compiles and validates the complete layout before mutation, clears both sides,
and lets earlier empty rounds end naturally. Non-travelling ambush units are
placed in the round immediately before activation; only that round uses the
private Training Ground finish-fight action if battle begins. Every remaining
formation and modifier is applied in the activation round. The operation
returns only after authoritative readback and stable activation-round
deployment status.

### quit_game

Input is an empty object.

Typical output:

```json
{"requested":true,"exit_code":0}
```

The operation invokes Unity application shutdown and is valid only at the main
menu. The adapter confirms the request; the MCP layer additionally waits for
the owned process to exit.

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

Input is an empty object.

Typical output:

```json
{"created":true,"initial_supply":10000}
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
{"status":"training_ground","round_count":1,"deploying":true,"fighting":false}
```

`status` is exactly one of `main_menu`, `training_ground`, `replay`, or
`unknown`. Training Ground additionally reports `round_count`, `deploying`, and
`fighting`; a temporarily unavailable native detail is `null`.

### toggle_fight

Input is an empty object.

Typical output:

```json
{"performed":true}
```

The operation changes the Training Ground process state from deployment to
battle. The MCP layer additionally observes the battle transition before
returning.

## Build

```sh
cargo build -p mechcore-adapter --release
```
