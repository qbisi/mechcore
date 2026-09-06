# Game session acquisition

[TOC]

This document defines how a `mechcore` process acquires the running game, how
it classifies what it finds, and what each classification permits. It is the
shared contract behind `mechcore shell` and `mechcore run`; neither invents its
own launch path.

Acquisition is always **explicitly declared**. There is no implicit fallback
from attach to launch, and no operation silently starts a game.

## Scope and status

| Part | Status |
| --- | --- |
| Adapter socket contract, `busy` greeting | implemented |
| Detection signals and state matrix | specified here; consumed by `session.rs` |
| `mechcore shell` acquisition flags | implemented |
| `mechcore run` `game:` declaration | implemented |

Both frontends were built against this document, and every state in the
matrix below has been exercised against the real game.

## Who owns the socket file

The Adapter owns `/tmp/mechcore-adapter-<uid>.sock` (or `MECHCORE_ADAPTER_SOCKET`)
for its whole lifetime. It creates the endpoint at mode `0600`, admits only a
peer with the same effective UID, and removes the endpoint on exit.

On bind the Adapter refuses to replace a non-socket or a socket owned by
another UID, refuses to replace an endpoint that is still live, and otherwise
unlinks the stale file before binding.

**The endpoint exists exactly as long as the hosting process.** The Adapter
runs on a detached thread, so a Unity shutdown tears the process down without
unwinding it; a process exit handler therefore unlinks the endpoint. This also
covers the player closing the window. Deleting the endpoint earlier, when
`quit_game` is handled, would leave a window in which the game is still alive
with no endpoint, which a client cannot distinguish from state **C**.

An exit handler does not run on `SIGKILL`, `abort()`, or `_exit()`, so a
crashed or force-killed game still leaves state **B** behind. That is a
recoverable state, not a leak: the next bind clears it.

`mechcore` never creates, unlinks, or chmods that path. A stale endpoint is
resolved by the next Adapter bind, not by the client.

## Detection signals

Three independent signals classify the environment. None requires `lsof`,
parsing another process's environment, or any dependency beyond `libc`.

**1. Game process.** Enumerate processes and match the Mechabellum executable
path. This is the only way to distinguish "no game" from "a game the player
started without the Adapter".

**2. Connect result.** `connect()` on the endpoint. `ENOENT` means no file;
`ECONNREFUSED` means a stale file with no listener.

**3. Greeting.** The Adapter writes its greeting immediately on accept. A
client that connects and receives no line has not been accepted by a serving
loop.

Because the Adapter's accept loop is serial, a second client's `connect()`
succeeds while its greeting waits in the backlog. The Adapter therefore
answers an already-occupied endpoint explicitly rather than leaving the caller
to infer occupancy from a timeout:

```json
{"kind":"hello","protocol":"mechcore.adapter.v1","capabilities":["status", "..."]}
{"kind":"busy","protocol":"mechcore.adapter.v1"}
```

A `busy` greeting is peer-verified like any other connection and is followed by
an immediate close. A greeting timeout after a successful connect is therefore
**not** normal occupancy; it indicates an unresponsive Adapter and must be
reported as such.

The two are distinguishable precisely because answering `busy` requires the
greeter to run. State **F** is reproduced by stopping the game process: the
kernel still completes the connection from its listen backlog, but no thread
writes a greeting, and `attach` reports `adapter_unresponsive` at the deadline.
Continuing the process restores `busy`, so **F** is a momentary reading rather
than a latched state.

## State matrix

| Process | Endpoint | Probe | State | `launch` | `attach` |
| --- | --- | --- | --- | --- | --- |
| absent | absent | — | **A** clean | launch, **owned** | fail `no_game` |
| absent | present | connect refused | **B** stale endpoint | launch, **owned** | fail `no_game` |
| present | absent | — | **C** foreign game | fail `foreign_game` | fail `foreign_game` |
| present | present | `hello` | **D** adapter idle | fail `already_running` | attach, **not owned** |
| present | present | `busy` | **E** adapter occupied | fail `adapter_busy` | fail `adapter_busy` |
| present | present | no greeting | **F** adapter unresponsive | fail `adapter_unresponsive` | fail `adapter_unresponsive` |

State **C** is the player's own game. The Adapter cannot be injected into a
live process, and starting a second instance would corrupt both. Both verbs
fail closed and name the running PID.

State **D** fails under `launch` deliberately. `launch` means "I want to own a
fresh process"; degrading it to an attach is exactly the implicit behaviour
this design removes. The error names `attach` as the remedy.

State **B** does not unlink anything. The Adapter clears the stale endpoint
when the newly launched game binds.

## Ownership

Ownership decides shutdown, and it is never inferred:

- `launch` sets **owned**. Normal exit shuts the game down through `quit_game`.
- `attach` sets **not owned**. Exit closes the connection and leaves the game
  running. A not-owned session must never terminate the process, because the
  process belongs to another session or to the user.

Ownership is visible in the shell banner, in `status`, and in the structured
result of every `mechcore run` execution.

## Declaration

### mechcore run

The optional top-level `game:` key declares acquisition. It accepts exactly
`launch` or `attach`. Omitting it means the script is **offline** and touches
no game.

```yaml
game: launch
steps:
  - record_replay_round: {grbr: $grbr, round: 2, output: $out/replay.mcfr}
```

An offline script is the normal case for comparison work:

```yaml
steps:
  - compare: {left: $simulated, right: $native}
    expect: {equal: true}
```

See [mcscript.md](mcscript.md) for the full document shape and the operations
each mode admits.

**Static rule.** A script that omits `game:` and uses any native operation is
rejected before execution, without probing or launching anything. `mechcore run
--check` performs this validation offline, so whether a script needs the game
is answerable without holding it.

### mechcore shell

```sh
mechcore shell            # offline; native commands report how to acquire
mechcore shell --launch
mechcore shell --attach
```

`--launch` and `--attach` are mutually exclusive. The REPL additionally offers
`launch`, `attach`, and `detach`, so a session may start offline, run a
comparison, and acquire the game only when needed. `detach` on an owned session
requires confirmation, or `quit`.

## Error taxonomy

Every acquisition failure carries a stable code, the observed state, and the
concrete next action.

| Code | Meaning | Reported detail |
| --- | --- | --- |
| `no_game` | `attach` with no running game | endpoint path probed |
| `foreign_game` | game running without the Adapter | PID, executable path |
| `already_running` | `launch` with an idle Adapter available | PID; suggests `attach` |
| `adapter_busy` | another `mechcore` holds the endpoint | endpoint path |
| `adapter_unresponsive` | connected, no greeting before deadline | endpoint path, deadline |
| `protocol_mismatch` | greeting protocol or capability set differs | expected and observed |
| `launch_failed` | Adapter dylib or game executable missing | resolved paths tried |

`adapter_busy` and `adapter_unresponsive` are distinct states and must not be
collapsed into one message.

## Resolved paths

| Item | Resolution order |
| --- | --- |
| Adapter dylib | sibling of the running `mechcore` executable |
| Game executable | `MECHCORE_GAME`, then the default Steam location |
| Endpoint | `MECHCORE_ADAPTER_SOCKET`, then `/tmp/mechcore-adapter-<uid>.sock` |

Resolving the dylib as a sibling of the executable keeps a built `target/release`
directory relocatable as a unit. A `launch_failed` error names every path it
tried.

## Diagnostics

Two log channels carry native-side detail, and they are separate. Neither
appears in a frontend's own output, so both are worth naming before a capture
is investigated.

| Log | Written by | Contains |
| --- | --- | --- |
| `/tmp/mechcore-game-<uid>.log` | this tool, per launch | the game process's stdout and stderr, including every Adapter `eprintln!` |
| `~/Library/Logs/GameRiver/Mechabellum/Player.log` | Unity, always | engine startup, IL2CPP, and game-side exceptions |

**These do not overlap.** Unity writes `Player.log` through its own file
handle, not through file descriptor 2, so Adapter diagnostics never reach it;
conversely the game's own engine logging never reaches the launch log. A failed
capture usually needs both.

`Player.log` is truncated on each game start and the previous run is kept as
`Player-prev.log`, which is the copy to read after a failure that has already
been followed by another launch. Its directory is the Unity company and product
name, chosen by the game rather than by this tool, so an update may move it.

The launch log sits beside the endpoint, user-scoped for the same reason and
truncated on each launch. Its path is reported
in the shell banner. An **attached** session has no launch log: whoever started
that game chose where its output went.

A non-default `MECHCORE_ADAPTER_SOCKET` moves the endpoint for both the Adapter
and the client. A game launched with a custom endpoint while the client probes
the default one is indistinguishable from state **C**; pass the same override to
both sides.
