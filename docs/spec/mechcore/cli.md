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

A match is a [battle](../document/battle.md) document and a turn file beside
it. `<match>.yaml` holds the rounds that have been played, and it is a valid
battle document between operations, so `doc verify` reads it at any point.
`<match>.turn` holds what the round in progress has not settled yet: which side
each player was given, each side's decisions before they are committed, when
the round opened, and whether each side has committed. It is the match's
coordination and not its record, it is gone when the match is over, and
[turn.md](turn.md) defines it.

**A commit is a write.** A side's decisions reach the document when that side
commits and not before, and what is written is written: a match has no undo.
A side's own decisions are its own until then, which is what lets two sides
deploy at once without seeing each other.

Two processes reach one match by naming one document, and the turn file carries
one advisory lock that every operation takes for the read and the write it
does. A fight is resolved inside that lock, so a round is fought once however
many processes find it due, and a process that dies during a fight leaves
nothing behind but a lock the next one takes.

A side is given, not claimed. `match new` hands the first caller blue and the
second red, records both in the turn file, and refuses a third, so two players
agree on a path and on nothing else. Every later operation names the side it
was given, which says who is calling rather than asking for a side.

A match is in one of four phases:

| Phase | What it waits for |
| --- | --- |
| `opening` | each side's `choose_advance_team` |
| `deploy` | each side's decisions, and its commit |
| `fight` | a fight both sides are in and no backend has resolved |
| `over` | nothing; a side's reactor core has reached zero, a side conceded, or the match reached its last round |

Deployment is simultaneous. A side's decisions are taken from the position the
round opened with and reach no other side, so the two sides may be played in
any order, and a match is the same match whichever order they take.

A round is fought when both sides have committed, or when the deployment time
the header states has run out, whichever comes first. Whatever a side has not
committed by then was not played: a side that never committed plays that round
with no decisions at all. Any operation that finds a round due resolves it
before it answers, so nothing has to be watching for the clock.

### `match new`

Deals a match, or joins one already dealt.

Operands: the match document. Options: `--seed <i32>`, `--map <i32>`,
`--loadout <file>` for that side's technology loadout, and `--deploy-time
<seconds>`.

Every option may be left out. A seed nobody chose is drawn, a map nobody chose
is drawn from the maps this build has an opening initialization for, and a
deployment time nobody chose is the 100 seconds a standard match deploys in.
All three are written into the header, so a match nobody configured is as
reproducible as one somebody did.

Whichever caller names the document first deals the match and writes that
header; the second joins it. A join that names a seed, map or deployment time
the document does not carry is refused rather than silently adopting the
document's, and so is a document that is not a battle.

Answers the side the caller was given, the header, and that side's opening
offers and initial constructions.

The deal follows from the seed, which [battle.md](../document/battle.md)
defines. What else that seed decides, the streams and pools a match is dealt
from, is the checker's and not a player's, and no operation here answers with
it.

### `match show`

Answers one side's view of the match.

Operands: the match document. Options: `--side blue|red`, `--wait
[<seconds>]`, and `--omniscient` for both sides in full, which is what a
spectator and a test read.

The view carries the phase, the round, that side's own position with its
uncommitted decisions applied, the round's reinforcement offers, whether the
other side has committed, and what the next section leaves of the other side.

### What a side sees of the other

**The other side's past is visible and its round in progress is not.** The
round it is deploying now is hidden whole, because deployment is simultaneous
and that is what a round decides. Of the rounds it has committed, a player sees
the position they reached and the decisions that reached it, less the five
things below.

Those five stay hidden for good, not until the next round, because none of them
reaches a board. A side that unlocks Fang in round 1 and buys none has shown
nothing: the unlock is not in its decisions, the unlocked type is not in its
shop, and in round 2 its Fangs arrive beside whatever else it deployed, with no
warning that they could. That is the difference between a decision whose
consequence stands on the board and one whose consequence is only an option:

| Hidden | Why |
| --- | --- |
| the other side's `supply` | what a player can afford is what a player plans, and no board shows it |
| the other side's `shop` | a unit type unlocked and never bought leaves nothing to see |
| its `unlock_unit` decisions, in every round | the same fact, said as a decision |
| the header's `seed` | it deals this match: every opening and every round's offers follow from it |
| the other side's opening `offers` | the four combinations are dealt to a side privately, as [battle.md](../document/battle.md) says |

Everything else a committed round reached is visible: the units with their
levels, experience, equipment and facing, the reactor core, the towers, the
constructions, contraptions, terrains and airdrop shields, the officers, the
technologies, the blueprints, the Energy Tower skills, the commander skill
panel, the equipment a side holds unfitted, and the allocator in `next_index`,
which the units and the reinforcements a side took already account for. The
round's `reinforce_offers` are the same array for both sides and stay whole.

`tech_loadout` is shown for the unit types that have stood on that side's
board, and for no others. A loadout is chosen before the match and it bounds
what a side may research; the rows a player has seen fielded are the rows that
player has been shown the consequences of.

`--omniscient` answers both sides in full, which is what a spectator and a test
read. It is not a view any player is given.

What a human is shown by the game is a different question, and
[the visibility index](../../rules/visibility.md) holds what is known about it:
the client carries both sides in full, so hiding is the interface's work; what
the interface draws is not established, and this contract decides rather than
reproduces it.

`--wait` blocks until the match is waiting for that side again: a new round has
opened, or the match is over. It is how a side that has committed learns what
the fight did, and a wait that reaches its own timeout answers with the phase
it is still in, which is an answer rather than an error.

### `match act`

Takes one decision for one side.

Operands: the match document, and the decision as the [action](../document/action.md)
spec writes it. Options: `--side blue|red`, and `--dry-run` to answer without
keeping the decision.

The decision joins that side's uncommitted decisions in the turn file, and the
answer is that side's position after it, with the events the decision produced.
An event is a one-key mapping naming what happened, such as `{created: {index:
7, name: marksman, position: {x: -40, y: -160}}}`, so that a caller learns the
index a purchase created and where the board put it without diffing two
positions.

`--dry-run` is how a player asks whether a decision is one it may take: the
answer is the same answer, and the refusal is the same refusal, but nothing is
kept. A decision is legal exactly when the rules settle it from the position it
is taken in, so asking is running it.

Refuses a decision the rules do not settle, naming the reason the transition
gives; refuses a side that has already committed the round, and a decision in a
phase that does not hold one.

### `match commit`

Writes that side's decisions into the match, which is what playing them means.

Operands: the match document. Options: `--side blue|red`.

The decisions are collapsed into the normal form a battle is written in and
written to the round's actions, the turn file records that this side has
committed, and the round is fought when the other side has committed too.
Answers the phase the match is now in; a side that wants the next round waits
for it with `show --wait`.

A commit cannot be taken back, and a side commits a round once.

### The fight

A fight answers the five fields [battle.md](../document/battle.md) says it
decides: the damage each reactor core takes, the experience each unit gains,
and which contraptions, terrains and airdrop shields remain. Everything else in
the next position is the transition's, and is predicted rather than fought.

The simulator fights it, over the layout the deployment-end position projects
onto, with a seed derived from the match's seed and the round, so a fight is
the same fight whenever it is run again. Nothing else answers a fight: a caller
cannot hand a match an outcome it did not fight, because a document that reads
like a played match has to be one.

A fight nothing can resolve is refused, naming what is missing, and the match
stays in the `fight` phase with both sides' decisions standing. Nothing
approximates a fight it cannot resolve. No recording is kept: a round's fight
is run again from the match itself, which `doc project` writes the layout for.

## `arena`

`arena run` plays one match between two players, each a command the arena
starts. It speaks the request protocol to each player's standard input and
reads its decisions from that player's standard output, so a player is any
program that answers requests, and the human front end is `shell`.

Operands: the match document. Options: `--blue <command>`, `--red <command>`,
and `--rounds <n>` to stop early. A player sees only the view
`match show --side` gives it.

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

This namespace is the one place the running game is driven. Everything that
reaches the game reaches it here, rather than around it.

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
| `doc project <battle.yaml>` | writes the layout a round's fight starts from |
| `doc schema <kind>...` | answers the JSON Schema of a document kind |

`doc verify` takes its paths as operands, or one per line on standard input
when it has none, and answers one report per document. A document that does not
verify is an answer, not an error: the reports are written and the command
exits 1.

`doc project` takes `--round <n>` and writes the layout that round's fight
starts from: the round's decisions applied to the position it opened with, and
that position projected. It is how a fight is run again without a recording
being kept of it, and `--output <layout.yaml>` writes the layout rather than
answering with it.

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

- Whether a match records the build that fought it, so that a match replayed
  under later rules is told apart from one replayed under the rules it was
  played under.
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
