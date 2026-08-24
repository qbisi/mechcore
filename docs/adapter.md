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
    "record_battle",
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
`record_battle` remains active through the complete logic-tick capture and atomic MCFR publication. Other
cross-scene readiness belongs to `mechcore mcp`, which observes the status
stream before returning from lifecycle tools.

## Operations

### apply_layout

Input is the complete layout object defined by [layout.md](layout.md), with a
top-level activation `round` in `1..=15` and `sides.blue` and `sides.red`
fields.

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
fixed at world `(0,500,0)`, rotated to `(90,0,0)`, and made orthographic. The render is fixed at
1280x720. Its orthographic size is `800/3` (about `266.67`), making both image dimensions
1.5 times larger than the original full-field calibration; at 16:9, the visible world range is about
`x=-474.07..474.07`, `z=-266.67..266.67`, so the far ends of the battlefield are cropped without
introducing perspective distortion. A completed render is captured at the following logic-update
boundary, so every screenshot corresponds to the pending MCFR snapshot rather than to the state being
advanced.
The normal screen-space UI is included. The terminal snapshot is not returned until its completed
render has also been captured. Frames are JPEG-encoded and written as a QuickTime Motion JPEG stream
whose sample duration equals `D.logic_step`; frame count must equal MCFR tick count. Screen and camera
controller state are restored after terminal capture or failure, and partial media remains unpublished.
The result reports `view`, `projection`, camera position/rotation, and orthographic size alongside the
media dimensions so a renderer can reconstruct the same world-to-screen calibration.

The operation is valid only after layout completion in Training Ground deployment. It arms native
capture, starts combat, records `S(0)` before the first combat update, and captures every subsequent
`FightController.Update` boundary through the unique fighting-to-over transition. The adapter calls
`mechcore-mcfr::McfrWriter` directly, publishes atomically, reopens the file with
`McfrReader::open_verified`, and returns the state/transition counts and all formal hashes.

Projectile release/removal and damage use narrow native hooks so objects created and removed inside
one logic step remain in `E`. The release hook records the native projectile, owner and target; the
removal hook additionally records position and the native `intercepted` argument; the damage hook
records the positive `DamagePerformer.Perform` return value and its native provider and target.
Events are never synthesized from adjacent snapshots.

The current native snapshot closure directly reads units, projectiles, buildings, personal shields,
and BuffManager statuses. Area shields and dynamic terrain are outside the baseline schema until a
complete direct native capture path is implemented; their presence does not make an otherwise
capturable recording fail. The same rule applies to future instrumentation channels.

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
