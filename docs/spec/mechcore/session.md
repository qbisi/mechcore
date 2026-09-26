# Game session acquisition

[TOC]

## Scope

This contract defines how a `mechcore` process acquires the running game, how
it classifies what it finds, and what each classification permits. It is the
shared contract behind `mechcore shell` and `mechcore run`, and neither invents
its own launch path.

Acquisition is always **explicitly declared**. There is no implicit fallback
from attach to launch, and no operation silently starts a game.

What happens after acquisition is not here. The socket, its operations and
their refusals are [adapter.md](../adapter/adapter.md); the run document a
session executes is [mcscript.md](mcscript.md). This document ends where a
client is greeted.

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

**3. Greeting.** A client claims the game at its level, and the Adapter answers
that claim. A client that connects, claims, and receives no line has not been
accepted by a serving loop.

Because the Adapter's accept loop is serial, a second client's `connect()`
succeeds while its answer waits in the backlog. The Adapter therefore answers
an already-occupied endpoint explicitly rather than leaving the caller to infer
occupancy from a timeout:

```json
{"kind":"hello","protocol":"mechcore.adapter.v5","capabilities":["status", "..."]}
{"kind":"busy","protocol":"mechcore.adapter.v5","holder_level":1,"evicting":false}
```

A `busy` answer is peer-verified like any other connection and is followed by
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
| present | present | `hello` | **D** adapter idle | take over, **not owned** | take over, **not owned** |
| present | present | `busy`, `evicting` | **E1** outranked holder | wait, then take over, **not owned** | same |
| present | present | `busy` | **E2** adapter occupied | fail `adapter_busy` | fail `adapter_busy` |
| present | present | no greeting | **F** adapter unresponsive | fail `adapter_unresponsive` | fail `adapter_unresponsive` |

State **C** is the player's own game. The Adapter cannot be injected into a
live process, and starting a second instance would corrupt both. Both verbs
fail closed and name the running PID.

The two verbs differ only in states **A** and **B**: `launch` starts a game
where there is none, `attach` refuses to. Everywhere else both mean "give me
the game", and what decides is the level, not the verb. A script that takes
over a game it did not start is **not owned**: ownership follows who started
the process, so taking the game over never makes a session responsible for
shutting a stranger's game down.

State **B** does not unlink anything. The Adapter clears the stale endpoint
when the newly launched game binds.

States **E1** and **E2** are the same endpoint seen by two different clients.
Every acquisition carries a level in `0..=4`, declared where the acquisition is
made — a script's `level:` key, `game launch --level` in a prompt, `--level` on
a command — and defaulting to `1`. A claim strictly above the
holder's takes the game; an equal or lower one is refused with the holder's
level named.

Taking the game is the Adapter's work, not the claimant's. It stops the
holder's current operation at its next polling point, closes that connection
with an `evicted` notice, returns the game to the main menu, and only then
admits the next client. The claimant is told `evicting` and connects again
until it is greeted, which is one acquisition from its own side; the wait ends
after two minutes with `adapter_busy` if the hand-over never completes.

Nothing waits for a claim. Every wait inside a long operation abandons itself
at its next polling point, a capture included: the recording in flight is torn
down and published nowhere. Leaving the match and settling at the main menu is
the only part of a hand-over that still takes time.

An evicted client releases nothing, whatever its ownership: the game process it
started is being kept at the main menu for the client that claimed it, so
shutting it down there would destroy someone else's game. Its run ends where it
was interrupted, reports `{"operation":"evicted","completed":false}`, and exits
successfully.

Nothing else evicts. A client that is refused waits, retries or gives up, and
no client can take the game by any means other than outranking its holder.

## Ownership

Ownership decides shutdown, and it is never inferred:

- Starting the process sets **owned**. Normal exit shuts the game down through
  `quit_game`.
- Finding a game already running sets **not owned**, for both verbs. Exit
  closes the connection and leaves the game running. A not-owned session must
  never terminate the process, because the process belongs to another session
  or to the user.
- Being evicted releases nothing at all, owned or not.

`quit_game` is an operation, not a property of ownership: any client may shut
the game down deliberately, which is what makes a rebuilt Adapter loadable. A
running game keeps the Adapter it started with.

Ownership is visible in the shell banner, in `status`, and in the structured
result of every `mechcore run` execution.

## Declaration

### mechcore run

The optional top-level `game:` key declares acquisition. It accepts exactly
`launch` or `attach`. Omitting it means the script is **offline** and touches
no game. The optional `level:` key declares what the run outranks, `0..=4`,
defaulting to `1`, and is rejected without a `game:`. The optional
`headless: true` starts the game without a window (see [Headless](#headless))
and is rejected unless the script declares `game: launch`.

```yaml
game: launch
level: 0
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
mechcore shell
> game launch
> game launch --headless
> game attach --level 3
> game detach
```

A shell opens offline and reports how to acquire when a game operation is
asked for. Acquiring is an operation and not an option: the prompt is a
session, and a session says so in a line rather than in the command that
started it. `--level` rides on the line that claims, defaulting to `1`, so
what a session outranks is stated where it is claimed and nowhere else.

A session may therefore start offline, run a comparison, and take the game
only when it needs one. `game detach` on an owned session requires
confirmation, or `quit`.

### mechcore game

A command is one operation and then an exit, so it joins a game whoever is
keeping it alive and leaves it to them:

```sh
mechcore game status
mechcore game apply_layout layout.yaml --level 3
```

There is nothing else a command could do, so it does not declare it. A launched
game is **owned**, and ownership is what shuts it down, so a command that
launched one would take it down as it exits; a session that outlives one
operation, which is the shell and a run document, is where launching belongs.
`--launch` is refused with that, and `launch`, `attach` and `detach` are
refused as verbs for the same reason.

`--level` is the one acquisition option a command takes, because a command
claims like any other client.

## Headless

A headless launch starts the game with Unity's `-batchmode -nographics`: no
window, and the null graphics device in place of Metal. Nothing about a fight
changes, since the fight is fixed-point code that never reads what is drawn: a
Rhino mirror, a replay round of 1251 ticks and the same Rhino mirror recorded
in the Training Ground hash the same headless as with a window. It saves time
where the game draws, not where it fights: in the runs compared, the main menu
was up in about 11 s rather than 23 s and a layout applied in 3.6 s rather than
7.8 s, while a replay round, which is fought without a scene, took 3 s either
way.

The Adapter reads the same two switches off the game's command line, whoever
started it:

- `-batchmode` keeps the game out of the Dock. A process is filed as a Dock
  application or a background one from its bundle's `Info.plist` when it checks
  in, so the Adapter marks the in-memory copy `LSBackgroundOnly` from its
  initializer, before that happens; the bundle on disk is untouched.
- `-nographics` refuses `record_battle` with a `video_output`, since no frame
  is ever rendered ([adapter.md](../adapter/adapter.md#record_battle)).

Watching the server's matches, `record_watch_replay`, has not been tried
headless.

A launch that finds an idle Adapter joins that game as it is, window or not,
as it would for any other launch (state **D**).

## Error taxonomy

Every acquisition failure carries a stable code, the observed state, and the
concrete next action.

| Code | Meaning | Reported detail |
| --- | --- | --- |
| `no_game` | `attach` with no running game | endpoint path probed |
| `foreign_game` | game running without the Adapter | PID, executable path |
| `adapter_busy` | a client of the same or higher level holds the endpoint | endpoint path, holder level |
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
| `~/Library/Logs/GameRiver/Mechabellum/Player.log` | Unity, unless headless | engine startup, IL2CPP, and game-side exceptions |

A headless game writes Unity's log to its standard output instead of
`Player.log`, so both channels land in the launch log.

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

## Unresolved

**Who shuts down a game whose owner was evicted?** A launching session is
**owned** and shuts the game down on exit. If a higher claim takes that game,
the evicted session releases nothing, and the claimant is **not owned** and so
leaves the game running too. The process then outlives every session that
touched it. Either ownership should transfer with the game, or eviction should
be allowed to end a process the claimant did not start, or the outcome is
correct and a human closing the window is the intended end. Nothing decides
which.

**Should a custom endpoint be discoverable?** A game launched with
`MECHCORE_ADAPTER_SOCKET` set, probed by a client using the default, reads
exactly as state **C**: a foreign game. The current answer is that both sides
must be given the same override. The alternative is for detection to read the
endpoint from the process it already found, which would remove a class of
confusing failure at the cost of a fourth detection signal.

**Whose number is the hand-over deadline?** State **E1** waits two minutes for
a hand-over and then reports `adapter_busy`. The limit is not declarable, and a
script cannot say that it is willing to wait longer for a capture it knows is
long. Whether that belongs in the declaration, in the adapter, or nowhere is
open.

