# mechcore command line

[TOC]

## Scope

This contract defines the surface of the `mechcore` binary: the namespaces it
holds, the operations in each, what each operation takes and answers, how a
result is written, and what an exit code means. It is the shared contract
behind the four ways an operation is called, so a caller that learns one way
can use the others.

What an operation means is not here. A decision's effect on a position is the
[action](../document/action.md) and [state](../document/state.md) specs, a
layout's shape is [layout.md](../document/layout.md), the wire protocol behind
the `game` namespace is [adapter.md](../adapter/adapter.md), how a process
acquires the game is [session.md](session.md), and the shape of a run document
is [mcscript.md](mcscript.md). What the game itself decides is
[`docs/rules/`](../../rules), beside the tables in
[`config/`](../../../config) that the binary carries and computes with. Those
documents and these contracts are what `man` answers with, so a reader needs
the binary and nothing else. This document says how each is invoked and what
comes back.

A match played here is a match of standard 1v1 as the documents describe it.
The binary carries no matchmaking, no accounts and no network transport: two
players reach one match by opening the same match document, and a match is a
file rather than a service.

## The shape of a command

```sh
mechcore <namespace> <verb> [operands] [--options]
```

Six namespaces hold the operations:

| Namespace | Its object |
| --- | --- |
| `match` | one match being played |
| `arena` | players, and the matches they are put through |
| `game` | the running game process |
| `fight` | one fight: simulating it, and comparing recordings of it |
| `replay` | a native replay file |
| `doc` | a document on disk |

Three commands stand outside them, because their object is not one of those:
`shell` opens a prompt, `run` executes a run document, and `man` answers with
the manual the binary carries.

## One operation, four callers

An operation is a name, `<namespace>.<verb>`, an argument object, and a result
object. Four callers name the same operation and pass the same object:

| Caller | How it names the operation |
| --- | --- |
| A command | `mechcore match act m.yaml --side blue '{type: buy_unit, name: marksman}'` |
| A shell line | `match act m.yaml --side blue '{type: buy_unit, name: marksman}'` |
| A request | `{"op": "match.act", "match": "m.yaml", "side": "blue", "action": {...}}` |
| A run step | `- match.act: {match: m.yaml, side: blue, action: {...}}` |

A command's operands and options are the argument object written on a command
line. Each operation below names its operands in order; `--<field> <value>`
sets that field, and `--<field>` alone sets a boolean field to true. A caller
that would rather pass the object whole writes `--args '<yaml>'`, which no
other option may accompany.

A shell line is a command with the program name dropped. A request is one JSON
object on a line, answered by one JSON object on a line. A run step is a
one-key mapping from the operation's name to its argument object.

## Results

A result is one JSON object on standard output, carrying the `schema` of its
kind:

```json
{"schema": "mechcore.match.v1", "match": "m.yaml", "round": 3, "phase": "deploy", ...}
```

An operation that answers per input writes one object per line, one per input,
in the order the inputs were given. `--format yaml` writes the same object as
YAML and `--format text` as a short human rendering; `json` is the default, and
an operation that has no text rendering refuses `--format text` rather than
inventing one.

Standard output carries results and nothing else. Progress, warnings and
prompts go to standard error, so a caller may read a result from a pipe while a
person watches the same run.

## Error taxonomy

A failure is one JSON object on standard error and an exit code:

```json
{"schema": "mechcore.error.v1", "kind": "refused", "operation": "match.act",
 "reason": "moving a unit fixed in place"}
```

| Exit | Kind | What it means | Who fixes it |
| --- | --- | --- | --- |
| 0 | — | The operation happened | — |
| 1 | — | The operation happened and the answer is no: a document does not verify, two recordings differ | the caller's inputs |
| 2 | `usage` | The command is not one this contract defines | the caller's command line |
| 3 | `refused` | The rules do not allow what was asked: an illegal decision, a layout that does not compile, a fight outside what the chosen backend resolves | the caller's request |
| 4 | `unavailable` | The game is absent, busy or unresponsive, as [session.md](session.md) classifies it | the environment |
| 5 | `failed` | The operation could not be carried out: unreadable input, unwritable output, a broken file | the environment |

Exit 1 is an answer, not an error, and carries a result rather than an error
object. The four error kinds are distinguished so that an agent can tell a
decision it should not repeat (3) from a platform it should wait for (4).

A refusal never half-applies: a refused decision leaves the match exactly as it
was, and a refused operation writes no file.

## `match`

A match is two files. `<match>.yaml` is a [battle](../document/battle.md)
document holding the rounds that are over, and it is a valid battle document
between operations, so `doc verify` reads it at any point. `<match>.journal` is
a JSONL record of the round in progress: every decision as it was taken, and
each side's commit. Resolving a fight folds the journal into the document, in
the normal form a battle is written in, and empties it.

A match is in one of four phases:

| Phase | What it waits for |
| --- | --- |
| `opening` | each side's `choose_advance_team` |
| `deploy` | each side's decisions, and its commit |
| `fight` | a fight both sides have committed to and no backend has resolved |
| `over` | nothing; a side's reactor core has reached zero, a side conceded, or the match reached its last round |

Deployment is simultaneous. A side's decisions are taken from the position the
round opened with and reach no other side, so the two sides may be played by
two processes in any order, and a match is the same match whichever order they
take.

Three verbs are the whole of playing: `show` to see, `act` to decide, `commit`
to end the round. A player loops over them, and `commit` answers with the next
round, so nothing polls.

### `match new`

Deals a match and writes its header.

Operands: the match document to create. Options: `--seed <i32>`, `--map <i32>`,
`--loadout <side>=<file>` for each side's technology loadout, `--fight
sim|game|external` for the backend its fights are resolved by, and `--force` to
replace an existing document.

Answers the header it wrote: the seed, the map, and each side's opening offers
and initial constructions. Refuses a seed or map the build has no opening
initialization for, and an existing document without `--force`.

The deal follows from the seed, which [battle.md](../document/battle.md)
defines. What else that seed decides, the streams and pools a match is dealt
from, is the checker's and not a player's, and no operation here answers with
it.

### `match show`

Answers one side's view of the match.

Operands: the match document. Options: `--side blue|red`, and `--omniscient`
for both sides in full, which is what a spectator and a test read.

The view carries the phase, the round, that side's own position in full, the
round's reinforcement offers, whether the other side has committed, and the
other side's board as the round opened. Which of the other side's fields a
player may see is stated in [Unresolved](#unresolved).

### `match act`

Takes one decision for one side.

Operands: the match document, and the decision as the [action](../document/action.md)
spec writes it. Options: `--side blue|red`, and `--dry-run` to answer without
writing anything.

Answers the side's position after the decision, and the events the decision
produced. An event is a one-key mapping naming what happened, such as
`{created: {index: 7, name: marksman, position: {x: -40, y: -160}}}`, so that a
caller learns the index a purchase created and where the board put it without
diffing two positions.

`--dry-run` is how a player asks whether a decision is one it may take: the
answer is the same answer, and the refusal is the same refusal, but the match
is untouched either way. A decision is legal exactly when the rules settle it
from the position it is taken in, so asking is running it.

Refuses a decision the rules do not settle, naming the reason the transition
gives; refuses a side that has already committed to the round, and a decision
in a phase that does not hold one.

### `match commit`

Ends one side's deployment, which is the native `FinishDeploy`, and answers
with the next round.

Operands: the match document. Options: `--side blue|red`, `--timeout
<seconds>`, `--no-wait`, `--fight <backend>` to override the match's own, and
`--outcome <file>` for the `external` backend.

A commit waits for the other side. When both sides have committed, the fight is
resolved and the round after it is opened, and the commit answers with that
round's view of the committing side, or with the match's outcome when the match
is over. A commit that reaches its timeout answers with the phase it is still
in, which is an answer rather than an error; `--no-wait` answers that way
immediately.

The fight is resolved once. Whichever process sees both commits resolves it and
writes the result; a process waiting in its own commit answers from what that
one wrote, so two players never fight the same round twice.

A side that has already committed may commit again. It waits as before, and
when the match is in the `fight` phase it resolves the fight again, which is
how a fight a backend refused is retried after the backend can carry it.

A fight's seed is derived from the match's seed and the round, so a match
replays identically.

A backend answers the five fields a fight decides, which
[battle.md](../document/battle.md) names: the damage each reactor core takes,
the experience each unit gains, and which contraptions, terrains and airdrop
shields remain. Everything else in the next position is the transition's, and
is predicted rather than fought.

| Backend | How it resolves |
| --- | --- |
| `sim` | the deterministic simulator, over the layout the position projects onto |
| `game` | the game, through `game apply_layout` and `game record_battle` |
| `external` | an outcome document the caller supplies |

A backend that cannot resolve the fight refuses it, naming what it does not
carry, and the match stays in the `fight` phase with both commits standing.
Nothing approximates a fight it cannot resolve.

An outcome document is `mechcore.fight-outcome.v1` and carries exactly those
five fields per side. It is what `external` reads and what every backend
answers, so an outcome may be recorded, replayed and compared.

## `arena`

`arena run` plays one match between two players, each a command the arena
starts. It speaks the request protocol to each player's standard input and
reads its decisions from that player's standard output, so a player is any
program that answers requests, and the human front end is `shell`.

Operands: the match document. Options: `--blue <command>`, `--red <command>`,
`--fight <backend>`, and `--rounds <n>` to stop early. A player sees only the
view `match show --side` gives it.

Answers the match's outcome: the last round, the phase it ended in, and each
side's reactor core.

## `game`

The `game` namespace carries the operations of [adapter.md](../adapter/adapter.md)
under the names that protocol gives them: `status`, `start_test`,
`apply_layout`, `record_battle`, `record_replay_round`, `record_watch_replay`,
`toggle_fight`, `speed_up`, `quit_match` and `quit_game`. Each takes the
argument object that protocol defines and answers what it answers.

Three more belong to the session rather than the game:
[session.md](session.md) defines `launch`, `attach` and `detach`, their
`--level` and what each refuses. Acquisition is declared, never inferred, so a
command in this namespace declares `--attach`, and one that reaches no game
refuses with `unavailable` rather than starting one. `--launch` is a session's
and not a command's: a launched game is owned, and a command that launched one
would shut it down as it exits, so launching belongs to the shell and to a run
document.

This namespace is the one place the running game is driven, and `match --fight
game` reaches the game through it rather than around it.

## `fight`

| Verb | What it does |
| --- | --- |
| `fight run <layout.yaml>` | simulates one fight from a layout, optionally writing a recording |
| `fight compare <left.mcfr> <right.mcfr>` | compares two recordings and names the first tick they differ at |
| `fight verify <recording.mcfr>...` | simulates each recording's own layout again and compares the result with the recording |

`fight run` takes `--seed <i32>` and `--output <recording.mcfr>`, and answers
the simulation result: the terminal structure of the
fight, its hashes and its profiling. `fight compare` and `fight verify` answer
the verdict and the divergence, and exit 1 when the verdict is no.

[mcfr.md](../mcfr/mcfr.md) defines what a recording holds and what makes two of
them equal.

## `replay`

`replay convert <replay.grbr> <battle.yaml>` reads a native replay and writes
the battle document it records, with `--force` to replace an existing document.
It answers what it wrote and how much of each transition the rules predict.

A replay this converter does not read is refused rather than partly converted,
and the refusal names which of the replay's properties it stands on.

## `doc`

| Verb | What it does |
| --- | --- |
| `doc verify <document>...` | checks each document against the contract its `kind` names |
| `doc format <document.yaml>` | writes the document in its normal form, in place with `--write` |
| `doc diff <left.yaml> <right.yaml>` | normalizes both and reports every field they differ in |
| `doc schema <kind>...` | answers the JSON Schema of a document kind |

`doc verify` takes its paths as operands, or one per line on standard input
when it has none, and answers one report per document. A document that does not
verify is an answer, not an error: the reports are written and the command
exits 1.

`doc schema` takes the kinds a document declares in its own `kind` field:
`layout`, `state`, `battle` and `action`. It answers the shape of the document,
which is what a reader validates against and what a writer generates from; it
says nothing about what the fields mean, which is the document's own spec. A
kind this contract does not name is refused.

## `shell`

`shell` opens a prompt whose every line is a command with the program name
dropped, so a line in the shell and a command in a script are the same text.
Options: `--launch`, `--attach` and `--level <0-4>` as
[session.md](session.md) defines them, `--json`, and a match document to open.

With a match open, that match's verbs are written without their namespace and
without the document: `act blue {type: buy_unit, name: marksman}`, `show blue`,
`finish blue`. Every other line keeps its namespace.

`--json` makes the prompt a request stream: one JSON request per line in, one
JSON result per line out, which is the protocol `arena` speaks to a player. The
shell holds a session between lines, so a game acquired by one line is still
acquired for the next.

## `run`

`run <script.mcscript>` executes a run document, whose steps name the
operations of this contract. `--check` validates a script without performing
its steps, and `--force` answers yes to every prompt a step would raise.
[mcscript.md](mcscript.md) defines the document; this contract defines the
operations its steps name.

## `man`

`man` answers with the manual the binary carries: the game's rules, as
[`docs/rules/`](../../rules) states them, and the contracts its own documents
and commands are held to, as [`docs/spec/`](..) states them. A binary is
distributed on its own, so what it knows travels with it, and a caller that has
the binary needs nothing else to read the same rules it computes with or the
same contracts it is written against.

```sh
mechcore man                 # every topic, with the line each one opens with
mechcore man <topic>         # that topic, as text
```

A topic is named by its document's path under `docs/`, without the extension:
`rules/reinforcements`, `spec/document/battle`, `spec/mechcore/cli`, which is
this contract. A name no other topic shares
may be written on its own, as `reinforcements`. A document's links to its
siblings resolve to topic names the same way, so a reader without the
repository can still follow them. A link to something the binary does not
carry, a configuration table among them, is left as the document wrote it
rather than rewritten into a topic that does not exist.

`--format json` answers `{topic, title, game_build, text}` rather than the text
alone, for a caller that stores what it reads. `--lang <code>` answers a
translation, and a topic with no translation in that language is refused rather
than answered in another one.

The manual is the game's rules and not this binary's usage: what a command does
is `--help`, and what it contracts to do is this document.

## Unresolved

- Which of the other side's fields a player may see. The board as the round
  opened is visible, and supply is not; officers, technologies, the skill panel
  and the shop are not decided, and the answer is the game's rather than this
  contract's.
- Whether a match document records which backend resolved each round's fight,
  and with which seed, or whether that belongs to the journal alone.
- Whether the journal stays a second file once a battle document can carry a
  round that is still being played.
- Whether `arena` holds more than one match: a series, a rating, a tournament.
- Whether `game` gains the decision operations of a live match, which would make
  the game a second engine for `match act` rather than a fight backend alone.
- Whether a player that fails to answer, or answers something illegal, forfeits
  the match or is asked again, and how many times.
- Whether the build's tables need a surface of their own, answered as data
  rather than read out of the manual's prose, and if so whether a name in a
  table is asked for the way a document writes it.
- Whether a player needs the decisions it may take enumerated, beyond asking
  about one with `match act --dry-run`.
- What becomes of a fight whose resolving process dies partway: how a match
  tells that from a fight still being resolved, and how long it waits before
  another process may resolve it.
