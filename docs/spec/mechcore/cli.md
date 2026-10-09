# mechcore command line

[TOC]

## Scope

This contract defines the surface of the `mechcore` binary: the verbs it holds
over files, the namespaces it holds for what outlives one command, what each
operation takes and answers, how a result is written, and what an exit code
means. It is the shared contract behind the four ways an operation is called,
so a caller that learns one way can use the others.

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
mechcore <verb> <file>... [--options]
mechcore <namespace> <verb> [operands] [--options]
```

Four rules decide which of the two a command is and what it means.

**A namespace exists only for something whose state outlives a command.**
Three do:

| Namespace | Its object |
| --- | --- |
| `match` | one match being played, whose turn file carries a round between commands |
| `arena` | players, and the matches it runs them through |
| `game` | the running game process, and the session that holds it |

**An operation on files is a top-level verb, and a verb has one meaning across
every kind it takes.** The kind is read from the content, never from a flag or
an extension, and a verb given a kind it does not take is refused, naming the
kind. [Kinds and verbs](#kinds-and-verbs) says how each kind is recognised and
which verbs take it.

**`convert <in> --to <kind> [<out>]` derives a file of another kind.** `--to`
is required. Each pair is either a **rewrite**, the same content in another
form, lossless or refused, or a **computation**, which derives facts the input
does not hold by running the fight. Without `<out>` a conversion to a
document, a layout or a fight, answers the document on standard output, and a
computation to a recording answers its result and writes nothing. Every other
rewrite requires `<out>`, because what it answers is a report of the file it
wrote. A computation into a recording is fought by the simulator this binary
carries, or with `--backend game` by the game.

**`man <kind>` lists the verbs a kind takes**, from the same table the verbs
are refused by, ahead of the document that describes the kind.

Three commands stand outside both, because their object is neither a file nor
a state: `shell` opens a prompt, `run` executes a run document, and `man`
answers with the manual the binary carries.

## Kinds and verbs

| Kind | Recognised by |
| --- | --- |
| `layout` | a YAML document whose first document states `kind: layout` |
| `fight` | a YAML document stating `kind: fight` |
| `match` | a YAML stream whose first document states `kind: match` |
| `state` | a YAML document stating `kind: state` |
| `action` | a YAML document stating `kind: action` |
| `mcfr` | the ZIP local file header an [MCFR](../mcfr/mcfr.md) opens with |
| `grbr` | the serialization header .NET's `BinaryFormatter` writes, which a native replay opens with |

A YAML document that names no kind, or one this binary does not read, is
refused rather than guessed at.

| Kind | `verify` | `convert --to` | `diff` | `show --view` | `query` | `play` | `format` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `layout` | yes | `grbr` (rewrite), `mcfr` (computation), `fight` (computation) | yes | — | — | yes (computation) | yes |
| `fight` | yes | `mcfr` (computation) | yes | — | — | yes (computation) | yes |
| `match` | yes | `grbr` (rewrite), `layout` (rewrite) | — | — | — | — | — |
| `state` | — | — | — | — | — | — | — |
| `action` | — | — | — | — | — | — | — |
| `mcfr` | yes | `fight` (rewrite) | yes | `outcome`, `stats`, `buildings` | yes | yes | — |
| `grbr` | — | `match` (rewrite), `mcfr` and `fight` (computation, the game alone) | — | — | — | — | — |

A state and an action are read inside a match, which is what verifies them.
`schema` names a
kind rather than a file, and answers the shape of `layout`, `fight`, `match`,
`state` and `action`.

## One operation, four callers

An operation is a name, an argument object, and a result object. A top-level
verb is named by the verb alone, `verify` or `convert`, and a namespace's verb
as `<namespace>.<verb>`, `match.act` or `game.record`. Four callers name the
same operation and pass the same object:

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
| 1 | — | The operation happened and the answer is no: a file does not verify, two files differ | the caller's inputs |
| 2 | `usage` | The command is not one this contract defines | the caller's command line |
| 3 | `refused` | The rules do not allow what was asked: an illegal decision, a layout that does not compile, a kind the verb does not take, a fight outside what the chosen backend resolves | the caller's request |
| 4 | `unavailable` | The game is absent, busy or unresponsive, as [session.md](session.md) classifies it | the environment |
| 5 | `failed` | The operation could not be carried out: unreadable input, unwritable output, a broken file | the environment |

Exit 1 is an answer, not an error, and carries a result rather than an error
object. The four error kinds are distinguished so that an agent can tell a
decision it should not repeat (3) from a platform it should wait for (4).

A refusal never half-applies: a refused decision leaves the match exactly as it
was, and a refused operation writes no file.

## `verify`

`verify [--backend game [--update]] <file>...` checks each file against the contract its kind defines. It
takes its paths as operands, or one per line on standard input when it has
none, and answers one report per file, in order. A file that does not verify is
an answer, not an error: the reports are written and the command exits 1. A
file that cannot be read, names no kind, or is of a kind `verify` does not take
is a report like any other, whose `kind` is `unreadable`, so the rest of a
batch still runs.

- A **layout** is compiled as `game apply_layout` would compile it, and the
  report carries the counts of what it places.
- A **match** is checked against its seed, and each round's next opening is
  predicted from the one before. Every round is then fought from the layout its
  decisions deploy, as `convert --to layout` projects it: the simulator's result
  is written onto the deployment and held to the position the next round opens
  with, on the leaves the fight decides (the reactor core, each formation's
  `exp`, the contraptions and the standing objects). Each round starts from the
  position the match states for it, so one that is refused or differs does not
  keep the rounds after it from being fought. `fights.rounds` carries each
  round's `result`: `equal`; `differs`, with each leaf that does; `unsupported`,
  with the simulator's reason; or `not_projected`, with why its deployment is
  no layout. A match verifies only when every leaf outside the fight is
  predicted and agrees, every round projects onto a layout the compiler takes,
  and every fight is fought and agrees; its `error` names the first round that
  is not.
- A **recording** is checked by simulating the layout it embeds again and
  comparing the result with what it holds. The report's `comparison` carries
  both timelines, the first tick they part at, and that tick explained.
- A **fight** is fought again as [`convert --to fight`](#convert) fights a
  layout: its projection, with its seed, through the simulator and the one
  reader. It verifies when the simulator's document states what it states, as
  [fight.md](../document/fight.md#root-fields) says a fight is checked: every
  result field, and `ticks` and `hash`.
  `compared` names which of the two were, and `differences` lists each path
  they part on, as `diff` spells one, with the document's value as `expected`
  and the simulator's as `actual`. A document holds no tick of its fight, so a
  hash that differs names no tick; `verify` over the recording it was read
  from does. A fight the simulator does not fight, or whose recording its
  reader does not answer, does not verify, and the refusal is the reason.

`--backend game` has the game fight instead of the simulator, in a game
somebody started, attached once for the whole batch, as
[`convert --backend game`](#convert) joins one. A fight is recorded from its
projection with its seed, and a recording from the layout it embeds with its
seed and the instrument channels it holds; each recording is read back as a
fight and compared as above, the game's fight as `actual`. A layout or a
match holds no fight and is checked as without it. No game answering is
`unavailable`, and stops the batch.

`--update`, only with `--backend game`, writes the game's fight back to each
file it differs from: a fight document becomes what `convert --to fight`
reads from the new recording, with its leading comment kept and no
`game_build` where it stated none, and a recording is replaced by the new one.
A recording that no longer reads as a fight, one of an older MCFR format, is
recorded again from its layout all the same. The report says `updated`, still
lists the `differences` it wrote over, and is valid. The simulator never
updates a file: a pinned result comes from the game.

## `convert`

`convert <in> --to <kind> [<out>]` derives a file of another kind, with
`--force` to replace an existing `<out>`. Without `--force` an existing
destination is refused, because a conversion that silently replaced a file
would make a document's provenance unrecoverable; the input is read, never
written. A pair [the table](#kinds-and-verbs) does not name is refused, naming
both kinds.

**`grbr` to `match`, a rewrite.** Reads a native replay and writes the match
document it records to `<out>`. It answers what it wrote and how much of each
transition the rules predict. A replay this converter does not read is refused
rather than partly converted, and the refusal names which of the replay's
properties it stands on.

**`match` to `grbr`, a rewrite.** Writes a match back to `<out>` as the replay
it converts from, which converts to the same match again byte for byte, and
whose rounds the game fights ([match-replay.md](../document/match-replay.md)
says where such a fight parts from the match's). A match it cannot write is
refused, naming why. It answers the replay's path, map, seed and rounds.

**`layout` to `grbr`, a rewrite.** Writes a layout to `<out>` as a replay the
game fights: one deployment round, the layout's, opened from a snapshot that
holds the layout and carries no action. `--seed` overrides the layout's seed,
and one of the two has to name it. The layout is compiled first, so a layout
`game apply_layout` would refuse is refused here too, and so is one a replay
cannot open or play: [layout-replay.md](../document/layout-replay.md) says what
it states and what it refuses. It answers the replay's path, map, seed and
round.

**`match` to `layout`, a rewrite of one round.** `--round <n>` names the round,
and the layout is the one that round's fight starts from: the round's
decisions applied to the position it opened with, and that position projected.
It holds what the match states about that fight and nothing the match does
not, and a round whose decisions the rules do not settle is refused. It is how
a fight is run again without a recording being kept of it. Without `<out>` it
answers the layout on standard output; with one it writes the layout there and
answers its path and round.

**`layout` to `mcfr`, a computation.** Simulates one fight from a layout, with
`--seed <i32>` overriding the layout's own. It answers the simulation result:
the terminal structure of the fight, its hashes and its profiling. With
`<out>` it also writes the recording there, which [mcfr.md](../mcfr/mcfr.md)
defines.

The profiling is what the fight cost to compute, so the simulator's speed can
be followed from one change to the next. It gives the whole generation's
duration and its rate against the fight's own time, and splits the duration
into phases that add up to it: `prepare`, building the scene; `step`, the
rules advancing each tick; `snapshot`, reading each tick's state out; `record`,
hashing each tick and storing it when a recording is kept; and `finish`,
closing the recording and reading a written one's tick hashes back to check
its result hash. Beside them it states the fight's size as `unit_ticks`, the
live units summed over every tick, with `peak_live_units`, and the step's cost
per tick and per unit-tick, with the tick whose step took longest and how many
units it had.

**`--profile <svg>` samples where the time goes.** The phases say how a
conversion's duration splits; `--profile` says which functions inside them it
is spent in. The call stacks of the conversion this binary makes are sampled a
thousand times a second while it runs and drawn as a flame graph to `<svg>`,
which the report then names as `flamegraph`. An existing `<svg>` is refused
without `--force`, and the game's conversion, with `--backend game`, is not
sampled. A release build strips its symbols, so its graph names addresses;
`cargo build --profile profiling` is the release build with them kept, and
the binary to profile with.

**`fight` to `mcfr`, a computation.** Fights a fight document's projection
with the seed the document states, which is why it takes no `--seed`: the
fight whose result the document is.

**`grbr` to `mcfr` or `fight`, a computation the game alone makes.** Fights
one round of a replay, `--round <n>`. The simulator does not open a replay, so
without `--backend game` it is refused.

**Who fights a computation into a recording.** `--backend simulator`, the
default, fights with the simulator this binary carries. `--backend game`
fights in the game, headless, through the Adapter, and writes what the game
recorded, or with `--to fight` the fight document that recording states, read
as [`mcfr` to `fight`](#convert) reads one, the recording itself not kept; it
requires `<out>`, takes `--instrument a,b`, the instrument
channels to record into the MCFR by name
([mcfr.md](../mcfr/mcfr.md#instrument-channels)), and, as a command, joins a
game somebody started, with the `--level` the [`game`](#game) namespace's
commands take. A run step names it `backend: game`, and the run declares a
game for it as it does for `game.*`. A client that finds the game served
waits a moment for the holder to go before it is refused, so a pipeline of
commands records one fight after another.

A layout fought in the game is written as a replay, as `convert --to grbr`
writes it, and that replay's round is recorded as a replay's round is, so it
is fought and recorded without a Training Ground: the game fights it without a
scene, from the main menu and back to it. It answers what a replay's round
answers, with the layout it was given as `layout_input`. The game can refuse a
decision the replay records and fight on without it, so the recording is held
to the layout the game read back as the fight began: when the two differ in
any field, once both are in normal form, the recording is removed and the
refusal names the fields.

**`mcfr` to `fight`, a rewrite.** Reads a recording for what its fight decided
and writes it onto the layout the recording embeds, as the
[fight](../document/fight.md) document it records: `source` is `game` for a
recording the game made and `simulator` for one the simulator wrote, as the
recording's `producer` says, and `ticks` and `hash` are the recording's.
Every result is read from the recording and none is computed; a recording that
does not answer one is refused, naming all it does not answer, and no document
is written. Without `<out>` it answers the document; with one it writes it
there and answers its path, round and source.

The recording holds the fight's objects under its own identities, so each
layout entry is found by what the two share. A formation is paired with its
placement as [`show --view outcome`](#show) pairs it, and its `after` is the
formation's experience at the last tick, cut to a whole number as the fight's
end cuts it; the formation has to open the fight holding the layout's
`before`, and its bar has to be the table's. `core_damage` is what
[reactor_damage.md](../../rules/reactor_damage.md) states, from the units
standing at the last tick. A shield contraption, a standing Shield Airdrop and
an interceptor are the shield, or the building, of the side standing exactly
at the entry's position when the fight opens, and each is retained when the
recording still holds it at the last tick. A missile is retained unless a
projectile nobody owns, of its side, is first recorded beside it, which is how
a missile fires ([contraptions.md](../../rules/contraptions.md)). A Shield
Airdrop released this round is retained when a commander-skill shield of the
side stands at its position at the last tick. A Sticky Oil Bomb released this
round leaves each of its seven points where an oil area of the side the fight
created still stands at the last tick with a round left to run, as its grid
when it holds one. Two recorded objects that could each be one entry, or one
that could be two, are refused rather than chosen between.

**`layout` to `fight`, a computation.** Simulates one fight from a layout, as
`convert --to mcfr` does and with the same `--seed`, and reads the recording it
makes as `mcfr` to `fight` does. Its `source` is `simulator`.

## `diff`

`diff <left> <right>` reports what two files of one kind differ in, and exits 1
when they differ. Two files of different kinds are refused.

Two **layouts**, or two **fights**, are normalized and every field they differ
in is reported, as a JSON pointer with both sides' values. A unit, construction or contraption is
matched by its `index` rather than its place in the list, so an inserted entry
reads as one addition rather than a change to every entry after it.

Two **recordings** are compared for more than whether they agree. The verdict
and its first divergent tick come from the stored tick hashes, which cover
every field, as `equal` and `first_divergence`; `left` and `right` carry each
recording's `result_hash` and tick count. Then every tick both recordings hold is
compared **field by field**: each object is flattened into its leaves, and a
leaf's field group is its path with the object and any list position removed,
so `unit 2`'s `skills[0].enabled.attack_target` counts under
`units.skills.enabled.attack_target`. An object only one side holds differs in
`<collection>.present`, and a tick's event list in `events`. `fields` holds
every group that differs, nested by its dotted path, with the first and last
tick it differs on and how many; `fields_equal` says none does.

`at` explains one tick: the first divergence of the selected groups, or the
tick `--tick <n>` names. It lists each differing leaf with both sides' values,
each side's events on that tick and the one before, and every object a
difference names as each side has it then — its life, or the tick it died or
was destroyed at. `--fields <group>,...` restricts all of this to the named
groups and everything under them, and makes their agreement the verdict, so
`--fields units.motion_state` exits 0 on recordings that differ elsewhere. `--format text` prints the same report for a person.

[mcfr.md](../mcfr/mcfr.md) defines what a recording holds and what makes two of
them equal.

## `show`

`show <file> --view <view>` answers one view of what a file holds. A recording
has three, and `--view` names one of them.

**`outcome`** reads a recording as [`convert --to fight`](#convert) reads it,
and answers what a fight document leaves out: the formations that came out of
the fight, under the indices the document knows them by, with how many
members each took in and brought out and the life they have left, which a
fight document does not carry because a unit comes back whole next round; and
`unresolved`, everything the fight decided that the recording does not answer,
which is what keeps the recording from converting. A unit counts as deployed
from its side's formations when it opened the fight in a formation a placement
takes and the fight did not create it; a formation no placement takes, such as
one an officer hands a side as the fight is built, is no placement's and is not
among the survivors. A unit that died in the fight and stands at its end was
reborn, and scores as one. What the recording does not answer is never
approximated, and the verdict is no while anything is — the fight was read, and
the answer is that it converts to no fight document and settles no round.
What the fight decided is the fight document itself; the view stays because a
recording's survivors are what a capture of a mechanism is commonly read for,
and a fight document has no place for them.

**`stats`** reads the same recording for a unit's numbers at one tick: the
`buffs` it holds, each by its `buff_id` (or the `technology_id` of a
technology that serves as its own buff data) and its `stacks` when it stacks,
and the numbers the build **computed** after every correction on it: the
unit's `move_speed`, and each skill's `attack_range`, `attack_damage` and
`current_attack_interval`, by its slot. None is something the fight decided,
which is why none belongs in the outcome. A recording keeps no correction a
technology, an officer or an equipment writes, so a capture reads a correction
off the number it comes to: a damage of 1.6 times the description is a rate
of `+0.6`. [officer_effects.md](../../rules/officer_effects.md) is what reads
them that way.

Every formation answers, whether or not anything corrects it: a unit with
nothing on it still has numbers, and that is what a control is read for. It
answers one reading per distinct state its standing members are in, each
naming its members by unit id: a buff the build hands a single member, one it
takes on being hit, splits that member off.

`--tick <n>` picks the tick to read; the default is the first, where a
correction applied as the fight is built has landed and nothing the fight does
has moved it yet. A mechanism that writes during the fight is read at the tick
it is expected at. A formation answers whether or not it survives, because the
side that spends a correction attacking is commonly the side that loses the
unit carrying it.

**`buildings`** reads the same recording for the objects standing in it, and
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
`1 << 32` to the metre, as `stats` reports a derived number in.
[constructions.md](../../rules/constructions.md) is what reads them that way.

`match show` shares the verb because it is the same question asked of a match
in play: what one side is shown of it.

## `query`

`query <file>... --sql <sql>` answers one SQL statement over the tables of one
recording or several, as SQLite runs it in a database of the binary's own, so
a question a recording can answer needs no program beside the binary. `--query <name>`
runs a statement the binary carries instead, and `--schema` answers what can be
asked: every table with its key and columns, and every named statement with
what it answers and the parameters it takes. Exactly one of the three is
given. A statement's `:name` parameters are bound by `--param name=value`, once
each, as an integer or a number where the value reads as one and as text
otherwise; a parameter the statement does not take, or one it takes and is
not given, is a usage failure.

**Several recordings.** One recording's tables go unprefixed. Two paths are
`left` and `right`, as `diff` takes them, and `name=path` names each of any
number, a name of lowercase letters, digits and `_` other than `main` and
`temp`; a statement then reads each recording's tables under its name, as
`left.units` and `right.units`, the same tables one recording has. A name given
twice is a usage failure. `meta` holds each recording's `path` beside its file
metadata, and `fight.producer` says which made it, whatever its name.

The answer carries `columns`, the statement's column names, and `rows`, each
an array of values in the order the statement answered them. `--format text`
prints them as aligned columns under a header.

**Tables.** Each member of the recording is a table under its own name,
`instrument/<channel>` as `instrument_<channel>`; a per-tick table the
recording leaves out is a table with no rows. A member's columns are laid out
by one rule read off its schema:

- a scalar field is a column under its own name;
- a struct's fields are columns of the row that holds it, named
  `<struct>__<field>`, so a position is `position__x`, `position__y` and
  `position__z` and an event's source `source__kind` and `source__id`; `__`
  separates a path's steps in a column's name and a table's, and no field the
  format names holds it, so no laid-out name is another's. A struct that may
  be null adds `has_<struct>`, 1 where it is present, so a null struct is told
  from one whose fields are null;
- a list is a table of its own, `<table>__<list>`, one row per element: the
  key of the row that holds it, `ordinal`, the element's place in the list
  from 0, then the element's columns, or `value` for an element that is a
  scalar. A deeper list's table names its parent's `ordinal` after the parent's
  list, so `units__skills__enabled__weapons` is keyed by `tick`, `unit_id`,
  `skills_ordinal` and `ordinal`;
- an enum is its tag's name as [mcfr.md](../mcfr/mcfr.md#common-types-and-enum-tags)
  names it, `ObjectRef`'s `kind` included, and `events.reason`, whose enum
  depends on the event's type, its integer tag, which each kind's view names;
- a fixed-point value, a `tick_hash` and every other value is the stored one,
  an unsigned 64-bit integer keeping its bits in SQLite's signed one, and a
  binary value is lowercase hex.

A member's own table is keyed by `tick` and the identity the format orders its
rows by: `unit_id`, `projectile_id`, `building_id`, `shield_id`,
`terrain_id`, `formation_id`, the recorder for `statistics` and `ordinal` for
`events`. An instrument channel has no identity and is keyed by `row`, its
row's place in the member. `meta` holds `ticks.parquet`'s file metadata as
`key` and `value`, with the embedded layout under `layout.yaml`, and `fight`
is one row of it: `producer`, which says whether the game or the simulator
made the recording, `game_build`, `format`, `result_hash`, `tick_count`,
`terminal_tick` and `combat_round`. `--schema` names each table's recording under
`database` when several are read, says of each table whether the content hash
reads it, and gives each column its place in the member and each
enum column its tags.

**Event kinds.** Each event kind has a view of `events`, `ev_<kind>` such as
`ev_damage` or `ev_buff_removed`, holding that kind's rows with the event's
`tick` and `ordinal`, its references, and the payload columns the kind may
set and no others. Its `reason`, where it carries one, is the tag's name as
the kind's own enum names it.

**The layout.** The embedded layout is laid out by its own shape, each row
naming its `side` and the `team_id` the recording records that side under:
`layout`, one row of `game_build`, `map_id`, `seed` and `round`;
`layout_sides`, each side's `legacy_index`; `layout_units`, one row per
formation placement keyed by `side` and `placement`, the placement's `index`,
with its `name`, `x` and `y` as the document writes them, `level`, `exp__current`
and `exp__maximum`, `rotated` and `travelling` where the document states them;
`layout_units__equipment`, what each formation wears in order;
`layout_constructions` and `layout_contraptions`, keyed the same way; and
`layout_choices`, every list of names or numbers a side states — its officers,
blueprints, energy tower skills, tower strengthen levels and, under `unit`, its
technologies by unit type — as `list`, `unit`, `ordinal` and `value`.
`layout_units.formation_id` is the recorded formation the placement is, matched
as [`show --view outcome`](#show) matches them: by where the formation's
members stand at the first tick. A placement no formation is matched to has
none, and where the matching refuses every one is null and `meta` holds the
refusal as `layout_formations_refused`.

Beside SQLite's own functions, `q32(x)` reads a fixed-point value as a real
number, and `sqrt(x)` and `hypot(x, y)` are the square root and the length of a
vector.

A table is decoded only once a statement reads it, so a statement pays for the
members it names and no others.

## `play`

`play <file> [<page>] [--seed <i32>] [--no-open]` writes the page that plays a fight back:
one HTML file, carrying its script and the whole fight, that a browser opens
offline and plays the battlefield from above at any speed. Without `<page>` it
writes beside the file, under the file's name with `.html`, replacing a page
already there, and it answers the page's path, the kind of the file, who
fought the fight (`producer`), its ticks and, for a fight fought here, the seed
it was fought with and where that seed came from.

Once the page is written the command asks the system to open it in its
default browser (`open` on macOS, `xdg-open` elsewhere, `start` on Windows),
and answers `opened` for whether the system took the request. A page the
system cannot open is still written: the command does not fail for it, and
says so on standard error. `--no-open` only writes the page, and so does a
run script's `play` step, which never opens one.

A recording is played as it holds the fight, and one made with the
`unit_pose` instrument channel ([mcfr.md](../mcfr/mcfr.md#instrument-channels))
is drawn in the poses it records: each unit takes its stance and the phase of
its attack from the clip its model played. A fight with no poses, which every
fight the simulator makes is, is drawn from its units' motion and its events.
A layout or a fight document is
fought by the simulator first, as [`convert --to mcfr`](#convert) fights it,
`--seed` overriding a layout's own and a fight document taking its own seed and
no `--seed`; the fight is kept in memory and laid out for the page directly,
so no recording is written. One that is wanted is `convert --to mcfr`, and
playing it plays the same fight. A seed given to a recording or a fight
document is a usage error, and a fight the simulator does not fight is refused
as `convert` refuses it.

## `format` and `schema`

`format <document>` writes the document in its normal form, in place with
`--write`. A layout and a fight are the kinds it takes.

`schema <kind>...` takes the kinds a document declares in its own `kind` field:
`layout`, `fight`, `state`, `match` and `action`. It answers the shape of the document,
which is what a reader validates against and what a writer generates from; it
says nothing about what the fields mean, which is the document's own spec. A
kind this contract does not name is refused.

## `match`

A match is a [match](../document/match.md) document and a turn file beside
it. `<match>.yaml` holds the rounds that have been played, and it is a valid
match document between operations, so `verify` reads it at any point.
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
which is the one way a match says that a side lost a round nobody fought. Any
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
document's, and so is a document that is not a match.

Answers the side the caller was given, the header, and that side's opening
offers and initial constructions. Each side is also given a seed of its own,
drawn as the match seed is, which the header keeps and no answer shows.

The deal follows from the seed, which [match.md](../document/match.md)
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
| the other side's opening `offers` | the four combinations are dealt to a side privately, as [match.md](../document/match.md) says |

Everything else a committed round reached is visible: the units with their
levels, experience, equipment and facing, the reactor core, the towers, the
constructions and contraptions, the officers, the
technologies, the blueprints, the Energy Tower skills, the commander skill
panel with what its earlier releases left standing, the equipment a side holds unfitted, and the allocator in `next_index`,
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

The decisions are collapsed into the normal form a match is written in and
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

A fight answers the four fields [match.md](../document/match.md) says it
decides: the damage each reactor core takes, the experience each unit gains,
which contraptions survive, and which objects earlier releases left standing
remain. Everything else in
the next position is the transition's, and is predicted rather than fought.

The simulator fights it, over the layout the deployment-end position projects
onto. That layout carries the match's seed and the round, which is everything
the fight is drawn from, so a fight is the same fight whenever it is run again
and nothing has to be derived for it. What the fight decided is read back as
the fight document [`convert --to fight`](#convert) writes, so the reading is
one piece of work rather than one per backend, and the document's result is
written onto the position each side deployed before the next round opens on
it. Nothing else answers a fight: a caller cannot hand a match an outcome it did
not fight, because a document that reads like a played match has to be one.

A fight nothing can resolve is answered rather than refused: the match stays in
the `fight` phase with both sides' decisions standing, and the answer's
`unresolved` names what is missing. It is an answer because the commits that
reached it stand — a commit cannot be taken back — and because every later
operation takes the lock and tries the fight again, so a match stopped by a gap
carries on by itself once the gap closes. Nothing approximates a fight it
cannot resolve. No recording is kept: a round's fight
is run again from the match itself, which `convert --to layout` writes the
layout for.

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
`apply_layout`, `toggle_fight`, `speed_up`, `quit_match` and `quit_game`. Each
takes the argument object that protocol defines and answers what it answers.

`game record` records what is not a file: the scene or a match the server
makes. A layout, a fight or a replay's round is a file, and it is fought into a
recording by [`convert --to mcfr --backend game`](#convert); `game record`
given one refuses and says so.

| Command | What it records | Protocol operation |
| --- | --- | --- |
| `game record <out.mcfr>` | the fight staged in the current scene | `record_fight` |
| `game record --watch` | a live standard 1v1 the server makes | `record_watch_replay` |

The scene takes `--video <file>`, `--no-speed-up`, `--instrument a,b` and
`--force`; a watch takes `--output-dir`, `--wait-for-scene-seconds` and
`--match-timeout-seconds`. `apply_layout` and a recording of the scene remain
the way to fight a layout a replay cannot state and to record a video.

A [fight](../document/fight.md) document is recorded as its projection is,
with the seed it states, so one file both records a fight and, once its result
is read from that recording, is what [`verify`](#verify) checks the simulator
against. Its `layout_input` is the projection.

Three more belong to the session rather than the game:
[session.md](session.md) defines `launch`, `attach` and `detach`, their
`--level` and what each refuses. **Acquiring the game is an operation, not an
option.** A caller that holds a session takes the game by naming one of those
three, and a caller that holds none — a command, which is one operation and
then an exit — joins a game somebody else started, including one a run or a
shell launched and left to linger. Launching is declared by the two frontends
that hold a session, and nowhere else. So a command takes `--level
<0-4>`, which orders it against other clients, and nothing else; one that
reaches no game refuses with `unavailable` rather than starting one, and
`--launch` names where launching belongs.

That is why `launch`, `attach` and `detach` are verbs of this namespace and not
of a session's own: the object is the game either way, and what differs is
whether the caller lives long enough to hold it.

This namespace is the one place the running game is driven. Everything that
reaches the game reaches it here, rather than around it.

## `shell`

`shell` opens a prompt whose every line is a command with the program name
dropped, so a line in the shell and a command in a script are the same text.
Options: `--json`, and a match document to open with the `--side` to play it
as. The game is not among them: a prompt is a session, and a session acquires
by saying so, with `game launch --level 3` or `game attach`. A shell opens
without a game and refuses the game's operations until it holds one.

A shell that opens a match holds both the document and the side. `mechcore
shell m.yaml --side blue` binds them, and so does the first line that opens a
match: `match new m.yaml` binds the side it was handed, and `match show m.yaml
--side red` binds the side it names. That match's verbs are then written
without their namespace, without the document and without the side:
`act {type: buy_unit, name: marksman}`, `show`, `commit`. Every other line
keeps its own spelling. A `show` that names no file is the bound match's, and
`show <file> --view <view>` stays the file verb, since the operand says which
is meant.

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
`rules/reinforcements`, `spec/document/match`, `spec/mechcore/cli`, which is
this contract. A name no other topic shares
may be written on its own, as `reinforcements`. A document's links to its
siblings resolve to topic names the same way, so a reader without the
repository can still follow them. A link to something the binary does not
carry, a configuration table among them, is left as the document wrote it
rather than rewritten into a topic that does not exist.

A kind is named by itself, `mechcore man layout`, and its page opens with the
verbs a file of that kind takes and the kinds `convert` reaches from it, as
[kinds and verbs](#kinds-and-verbs) states them, before the document that
describes the kind. A kind no document describes, `grbr`, answers its verbs
alone. `mechcore man` lists the kinds after the topics.

`--format json` answers `{topic, title, game_build, text}` rather than the text
alone, for a caller that stores what it reads, with `verbs` beside them for a
kind. `--lang <code>` answers a
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
