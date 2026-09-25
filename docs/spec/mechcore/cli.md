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

A match played here is a match of standard 1v1 as the documents describe it,
played by the rules this binary carries and fought by the simulator it carries.
The game is driven, never asked to decide: if a running game ever gains the
operations to take a player's decisions, that is another contract and not a
second engine behind this one.

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

A caller that holds a session fills in the fields that session already knows,
so the object reaching an operation is the same whoever wrote it. A
[`shell`](#shell) with a match open supplies its document and its side, and an
[`arena`](#arena) supplies them to a player that could not name them; the field
is absent from what they write and present in what the operation reads.

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

A match is played under one build, which its document states as `game_build`
and a binary carrying another build refuses to read at all. A match therefore
cannot be carried on under rules it was not played under, and a document that
reads is a document this binary can finish.

Two processes reach one match by naming one document, and the turn file carries
one advisory lock that every operation takes for the read and the write it
does. A fight is resolved inside that lock, so a round is fought once however
many processes find it due, and a process that dies during a fight leaves
nothing behind but a lock the next one takes.

A side is given, not claimed. `match new` hands the first caller blue and the
second red, records both in the turn file, and refuses a third, so two players
agree on a path and on nothing else. Every later operation names the side it
was given, which says who is calling rather than asking for a side: a process
that lives for one operation carries nothing between operations, and the turn
file holds no process identity for it to be recognised by. A caller that lives
longer names it once instead — a [`shell`](#shell) binds a side when it opens a
match, and a player under an [`arena`](#arena) never names one at all.

A match is in one of four phases:

| Phase | What it waits for |
| --- | --- |
| `opening` | each side's `choose_advance_team` |
| `deploy` | each side's decisions, and its commit |
| `fight` | a fight both sides are in and no backend has resolved |
| `over` | nothing; a side's reactor core has reached zero, a side conceded, a side ran out of deployment time, or the match reached its last round |

Deployment is simultaneous. A side's decisions are taken from the position the
round opened with and reach no other side, so the two sides may be played in
any order, and a match is the same match whichever order they take.

A round is fought when both sides have committed. A side that has not committed
when the header's deployment time runs out has lost the match, which ends there
and is not fought; the document states that side's loss as its concession,
which is the one way a battle says that a side lost a round nobody fought. Any
operation that finds a round due, either way, settles it before it answers, so
nothing has to be watching for the clock.

The clock runs on the rounds that are fought, so the opening has none. A match
nobody has opened is a file rather than a stalled round: no round has been
played, nothing is waiting on the other side, and a player that never joins
costs the other nothing it could have won.

**Running out of time loses, and that is this contract's simplification.** The
tracked replays never show a round running out: every side of every deployment
round ended it deliberately, so what the game does with a side that stops
answering is not established, and [the visibility
index](../../rules/visibility.md) is where a reading of the game would go.
A platform whose players are programs needs a bound that a stalled one cannot
outlive, and playing a round on a program's behalf would be deciding for it. So
the clock ends the match rather than the round, for a program and a person
alike, and a match that wants a longer one says so in `match new
--deploy-time`.

### `match new`

Deals a match, or joins one already dealt.

Operands: the match document. Options: `--seed <i32>`, `--map <i32>`,
`--loadout <file>` for that side's technology loadout, and `--deploy-time
<seconds>`.

Every option may be left out. A seed nobody chose is drawn, a map nobody chose
is drawn from the maps this build has an opening initialization for, and a
deployment time nobody chose is the 100 seconds a standard match deploys in.
All three are written into the header, so a match nobody configured is as
reproducible as one somebody did. A side that names no loadout carries every
technology this build gives its units, which is the most a side could have
chosen and makes a dealt match the build's rather than an absent account's.

Whichever caller names the document first deals the match and writes that
header; the second joins it. A join that names a seed, map or deployment time
the document does not carry is refused rather than silently adopting the
document's, and so is a document that is not a battle.

Answers the side the caller was given, the header, and that side's opening
offers and initial constructions. Each side is also given a seed of its own,
drawn as the match seed is, which the header keeps and no answer shows.

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
`unlocked_units`, and in round 2 its Fangs arrive beside whatever else it deployed, with no
warning that they could. That is the difference between a decision whose
consequence stands on the board and one whose consequence is only an option:

| Hidden | Why |
| --- | --- |
| the other side's `supply` | what a player can afford is what a player plans, and no board shows it |
| the other side's `unlocked_units` | a unit type unlocked and never bought leaves nothing to see |
| its `unlock_unit` decisions, in every round | the same fact, said as a decision |
| the header's `seed` | it deals this match: every opening and every round's offers follow from it |
| each side's own header `seed`, the caller's included | it decides what an officer that draws hands out in every later round |
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

Nothing enumerates the decisions a side may take. A decision that names a
position has as many spellings as the board has places, so a list of them would
be a shape of its own to define and to keep true; asking about one decision is
exact, and it is the same code path that would take it.

A decision is settled by the rules and then the position it leaves is put on
the board: what it costs and what it holds are the transition's, and where a
formation may stand is the layout compiler's, which is the same check the
layout a fight runs over passes. So a purchase that the supply does not cover,
a unit type the shop has not unlocked, a move off the board's grid and a
formation placed on top of another are all refused here, each naming which
rule refused it.

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

A decision the rules refuse is refused and may be replaced by another: nothing
about a refusal ends a round or a match. What ends a match beside its own
course is the clock, and it ends it against the side that did not commit in
time.

### The fight

A fight answers the five fields [battle.md](../document/battle.md) says it
decides: the damage each reactor core takes, the experience each unit gains,
and which contraptions, terrains and airdrop shields remain. Everything else in
the next position is the transition's, and is predicted rather than fought.

The simulator fights it, over the layout the deployment-end position projects
onto. That layout carries the match's seed and the round, which is everything
the fight is drawn from, so a fight is the same fight whenever it is run again
and nothing has to be derived for it. What the fight decided is read back with
[`fight outcome`](#fight), so the reading is one piece of work rather than one
per backend. Nothing else answers a fight: a caller cannot hand a match an
outcome it did not fight, because a document that reads like a played match has
to be one.

A fight nothing can resolve is answered rather than refused: the match stays in
the `fight` phase with both sides' decisions standing, and the answer's
`unresolved` names what is missing. It is an answer because the commits that
reached it stand — a commit cannot be taken back — and because every later
operation takes the lock and tries the fight again, so a match stopped by a gap
carries on by itself once the gap closes. Nothing approximates a fight it
cannot resolve. No recording is kept: a round's fight
is run again from the match itself, which `doc project` writes the layout for.

## `arena`

`arena run` plays matches between players, each a command it starts.

**A player is a client and the arena is its match.** A player writes one JSON
request per line on its standard output and reads one JSON result per line on
its standard input, and the requests are `match`'s own operations: `match.show`,
`match.act` with its decision and its `dry_run`, and `match.commit`. A request
names neither the document nor a side, because the arena knows both and a
player cannot ask about a side it was not given. A player that speaks this to
`shell --json` and a player that speaks it to an arena are the same program.

That is also what makes the arena the only place a side's own view is enforced
rather than agreed. A player run by an arena never opens the match document or
its turn file, so what it knows is what `match show` gave it;
[what a side sees of the other](#what-a-side-sees-of-the-other) is the rule, and
here nothing but the pipe can be read around.

Operands: the match document, or the directory a batch writes into. Options:
`--blue <command>`, `--red <command>`, `--matches <n>` for how many to play,
`--seed`, `--map` and `--deploy-time` as `match new` takes them, and
`--request-timeout <seconds>`.

The arena deals each match rather than joining one: it runs `match new`, hands
one player blue and the other red, and varies the seed across a batch, which is
what makes a batch a comparison rather than one match played twice.

A player that exits, crashes or stops answering is not answered for. It simply
stops committing, and the deployment clock ends the match against it, which is
[the rule a round runs out by](#match). `--request-timeout` bounds one request
rather than the round: a player that has not answered by then is killed, and
the clock does the rest. A player's own standard error is kept beside the match
rather than read as protocol, so a program may log where it likes.

Answers one match's outcome, or one per line and a summary for a batch: the
last round, the phase it ended in, each side's reactor core, and the document
it was written to.

One match is one document. A series, a rating and a tournament are things a
caller builds out of matches, and this namespace plays them rather than
ranking them.

## `game`

The `game` namespace carries the operations of [adapter.md](../adapter/adapter.md)
under the names that protocol gives them: `status`, `start_test`,
`apply_layout`, `record_battle`, `record_replay_round`, `record_watch_replay`,
`toggle_fight`, `speed_up`, `quit_match` and `quit_game`. Each takes the
argument object that protocol defines and answers what it answers.

Three more belong to the session rather than the game:
[session.md](session.md) defines `launch`, `attach` and `detach`, their
`--level` and what each refuses. **Acquiring the game is an operation, not an
option.** A caller that holds a session takes the game by naming one of those
three, and a caller that holds none — a command, which is one operation and
then an exit — joins a game somebody else is keeping alive as it runs, because
there is nothing else it could do: a launched game is owned, and a command that
launched one would shut it down as it exits. So a command takes `--level
<0-4>`, which orders it against other clients, and nothing else; one that
reaches no game refuses with `unavailable` rather than starting one, and
`--launch` names where launching belongs.

That is why `launch`, `attach` and `detach` are verbs of this namespace and not
of a session's own: the object is the game either way, and what differs is
whether the caller lives long enough to hold it.

This namespace is the one place the running game is driven. Everything that
reaches the game reaches it here, rather than around it.

## `fight`

| Verb | What it does |
| --- | --- |
| `fight run <layout.yaml>` | simulates one fight from a layout, optionally writing a recording |
| `fight outcome <recording.mcfr>` | answers what a recorded fight decided |
| `fight stats <recording.mcfr>` | answers a unit's numbers and the corrections behind them |
| `fight buildings <recording.mcfr>` | answers what is standing: the map's towers, and what each construction became |
| `fight compare <left.mcfr> <right.mcfr>` | compares two recordings and names the first tick they differ at |
| `fight verify <recording.mcfr>...` | simulates each recording's own layout again and compares the result with the recording |

`fight run` takes `--seed <i32>` and `--output <recording.mcfr>`, and answers
the simulation result: the terminal structure of the
fight, its hashes and its profiling. `fight compare` and `fight verify` answer
the verdict and the divergence, and exit 1 when the verdict is no.

`fight compare` answers more than whether two recordings agree. The physics
verdict and its first divergent tick come from the stored tick hashes, as
`equal` and `first_divergence`. Then every tick both recordings hold is
compared **field by field**: each object is flattened into its leaves, and a
leaf's field group is its path with the object and any list position removed,
so `unit 2`'s `weapon_aims[0].attack_target` counts under
`units.weapon_aims.attack_target`. An object only one side holds differs in
`<collection>.present`, and a tick's event list in `events`. `fields` holds
every group that differs, nested by its dotted path, with the first and last
tick it differs on and how many; `fields_equal` says none does.

`at` explains one tick: the first divergence of the selected groups, or the
tick `--tick <n>` names. It lists each differing leaf with both sides' values,
each side's events on that tick and the one before, and every object a
difference names as each side has it then — its life, or the tick it died or
was destroyed at. `--fields <group>,...` restricts all of this to the named
groups and everything under them, and makes their agreement the verdict, so
`--fields units.motion_state` exits 0 on recordings whose physics differs
elsewhere. `--format text` prints the same report for a person.

`fight outcome` reads a recording for [the five fields a fight
decides](#the-fight): which formations came out of it, under the indices the
document knows them by, and what remains of the collections a fight thins out.
What no rule and no recording answers is named in `unresolved` and never
approximated, and the verdict is no while anything is — the fight was read, and
the answer is that it does not settle a round. It is the one reader both
backends feed, because a fight the simulator ran and a fight the game played
are the same recording.

`fight stats` reads the same recording for a unit's numbers at one tick, in
both halves: the corrections **written onto** it, in the three channels the
recording keeps apart — the unit's own overlay, its skills', and the buff
aggregate — and the numbers the build then **computed** from them, which
`derived` carries. Neither is something the fight decided, which is why neither
belongs in the outcome, and they are one verb because a capture reads them
together: a rate of `+0.6` beside a damage of 1.6 times the description is one
fact seen twice. [officer_effects.md](../../rules/officer_effects.md) is what
reads them that way.

Every formation answers, whether or not it carries a correction: a unit with
nothing written onto it still has numbers, and that is what a control is read
for. A formation whose technologies are switched off says so with
`technologies_disabled`, which is the state a correction's absence is
explained by rather than a correction of its own.

`--tick <n>` picks the tick to read; the default is the first, where a
correction applied as the fight is built has landed and nothing the fight does
has moved it yet. A mechanism that writes during the fight is read at the tick
it is expected at. A formation answers whether or not it survives, because the
side that spends a correction attacking is commonly the side that loses the
unit carrying it.

`fight buildings` reads the same recording for the objects standing in it, and
takes the same `--tick <n>`. A side answers its `towers` — the buildings the
map gives it, which no layout places — and its `constructions`, one entry per
layout placement in index order.

**A construction is not one object.** `FightConstructionSystem.Create` answers
a list of them, so a placement owns as many buildings as its description says
and each of them is its own row with its own life; `parts` is those rows. A
recording records a building's `BuildingType` and not the construction that
released it, so the rows are matched back to the layout the recording embeds by
the one thing the two share, where a thing stands. A building that belongs to
no single placement is refused rather than assigned. An empty `parts` is a
reading: every object that placement owned is gone.

Positions, bounds and every other length are the recording's own fixed point,
`1 << 32` to the metre, as `fight stats` reports a derived number in.
[constructions.md](../../rules/constructions.md) is what reads them that way.

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
Options: `--json`, and a match document to open with the `--side` to play it
as. The game is not among them: a prompt is a session, and a session acquires
by saying so, with `game launch --level 3` or `game attach`. A shell opens
offline and refuses the game's operations until it holds one.

A shell that opens a match holds both the document and the side. `mechcore
shell m.yaml --side blue` binds them, and so does the first line that opens a
match: `match new m.yaml` binds the side it was handed, and `match show m.yaml
--side red` binds the side it names. That match's verbs are then written
without their namespace, without the document and without the side:
`act {type: buy_unit, name: marksman}`, `show`, `commit`. Every other line
keeps its namespace.

Naming a side once is the session's version of naming it every time. A process
that lives for one operation has nowhere to carry a side, so each command
names the side it was given; a shell is one process for a whole match, so it
is told once and nothing after that repeats what it already holds.

`--json` makes the prompt a request stream: one JSON request per line in, one
JSON result per line out, which is the protocol `arena` speaks to a player.
Those requests name neither the document nor the side, exactly as an arena's do
not, so a program written against one runs under the other unchanged. The shell
holds a session between lines, so a game acquired by one line is still
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

The build's tables have no surface of their own. They are compiled in, and a
reader that wants a price or a pool reads the index that explains it, which is
what the manual answers with. A table answered as data would be a second
spelling of what the binary already computes with, and nothing here asks for
one.

## Unresolved

None.
