# mcscript

[TOC]

## Scope

A `.mcscript` describes one bounded run: an optional game acquisition, named
variables, and an ordered list of steps. It replaces the hand-written Python
drivers that each re-implemented the session lifecycle around a client.

```sh
mechcore run <script.mcscript> [--check]
```

`--check` validates the document and exits without probing or launching
anything, which is the whole reason the static rules below are static.

This contract defines the document and what running it means. How the game is
acquired and what the acquisition failures are called is
[session.md](session.md); what each native operation does once the socket is
open is [adapter.md](../adapter/adapter.md). A layout a step applies is
[layout.md](../document/layout.md) and a recording it produces is
[mcfr.md](../mcfr/mcfr.md).

## Document shape

Six top-level keys, all others rejected:

```yaml
game: launch        # optional: launch | attach; omitted means gameless
level: 1            # optional: 0..4, needs a game; higher takes it from lower
headless: true      # optional: needs game: launch; no window, no graphics device
offline: true       # optional: needs game: launch; no network, Steam's included
vars:               # optional
  grbr: work/replay/replays/<version>/example.grbr
  out: /tmp/mechcore/example
steps:              # required, at least one
  - convert:
      input: $grbr
      to: mcfr
      backend: game
      round: 2
      output: $out/replay.mcfr
```

Each step is a mapping with exactly one operation key, plus an optional
`expect`. Relative paths resolve against the working directory, matching every
other subcommand.

What a script records goes under `/tmp/mechcore/<topic>/<script>`, the
directory and file the script itself is named by, and never into the
checkout: a recording is regenerable evidence, and what the repository keeps
is the script that regenerates it and the readings it asserts.

## Acquisition

`game:` accepts exactly `launch` or `attach`, with the ownership and failure
semantics defined in [session.md](session.md). Omitting it makes the script
**gameless**.

**A script that omits `game:` and uses a native operation is rejected before
execution.** Nothing is probed and no game is started, so `--check` answers
"does this need the game?" without holding it. This is what lets simulator and
recording-comparison work share the same runner as native capture.

An acquired game is left on every exit path and never shut down on the way out.
A game a launch started lingers for 30 s and quits itself if no other run
claims it, so consecutive runs that launch share one game
([session.md](session.md#leaving-the-game)).

`level:` is what this run outranks, in `0..=4`, defaulting to `1`. A script
whose level is strictly above the level of the client currently holding the
game takes the game from it; an equal or lower one is refused with
`adapter_busy`. Declaring a level without a `game:` is rejected, because there
is nothing to order.

Being taken over is not a failure, and nothing is waited for. The Adapter
abandons the operation in flight, a recording included, returns the game to the
main menu, and closes the connection; the run reports
`{"operation":"evicted","completed":false}` and exits successfully, leaving the
game to whoever claimed it.

`headless: true` launches the game with no window and no graphics device, and
is rejected unless the script declares `game: launch`: a game that is attached
to runs as whoever started it chose. A recording does not depend on it, since
the fight never reads what is drawn, and the ones compared hash the same; only
a video, which needs rendered frames, is refused. [session.md](session.md#headless) says what
changes.

`offline: true` launches the game with no network, Steam's included, under the
same rule; watching the server's matches is refused, and nothing else changes
([session.md](session.md#offline)).

The acquisition states, their failure codes and what evicts what are in
[session.md](session.md).

## Operations

A step names an operation of [cli.md](cli.md) as the command line does: a verb
over files by itself, `convert` or `diff`, and a namespace's verb as
`<namespace>.<verb>`, `game.record`. A step and a command say the same thing,
and a step's fields are the command's operands and options by name. `let` is
the exception: it binds names and belongs to this document alone.

This table says which operations need a game and what the script layer adds to
each. A game operation's own arguments, result and refusals are
[adapter.md](../adapter/adapter.md)'s, and a step passes them through rather
than redefining them.

| Operation | Needs a game | Notes |
| --- | --- | --- |
| `let` | no | binds names; see built-ins below |
| `verify` | no | `input`, a file or a list of files; every report `mechcore verify` writes, in one result |
| `convert` | no, yes with `backend: game` | `input`, `to`, optional `output`, `seed`, `round`, `backend`, `instrument`; the same result as `mechcore convert` |
| `diff` | no | `left`, `right`, optional `fields` (a group or a list of groups) and `tick`; the same report as `mechcore diff` |
| `show` | no | `input`, `view`, optional `tick`; the same answer as `mechcore show` |
| `game.status` | yes | current status snapshot |
| `game.start_test` | yes | optional `seed`, `map_id`; rarely needed, see `game.apply_layout` |
| `game.apply_layout` | yes | the layout object, or `{layout, seed}` |
| `game.record` | yes | the scene or a watched match, as `game record` records them; see below |
| `game.toggle_fight` | yes | |
| `game.speed_up` | yes | standalone operation, distinct from the recording field |
| `game.quit_match` | yes | |
| `game.watch_scenes` | yes | optional `refresh`, `true` by default; the lobby's page of match-made scenes, waiting for a refreshed page |
| `game.watch_scene` | yes | `scene_id`; joins as a spectator and waits for the server's `live` stage, or leaves again and reports `joined: false` |
| `game.save_replay` | yes | optional `output`; saves the match being watched as it stands |
| `game.quit_game` | yes | |

`game.record` takes the fields of the form of [`game record`](cli.md#game) it
reaches:

| Fields | What it records |
| --- | --- |
| `output`, optional `video_output`, `speed_up`, `instrument` | the fight staged in the current scene |
| `watch: true`, optional `output_dir`, `wait_for_scene_seconds`, `match_timeout_seconds` | one live standard 1v1 |

A layout, a fight or a replay's round is a file, and a `convert` step with
`to: mcfr` and `backend: game` fights it in the run's game, as
[`convert --backend game`](cli.md#convert) does; `game.record` given an
`input` refuses and says so.

A step that writes a file refuses to overwrite it, and a script does not
declare otherwise. The destinations are every path `game.record` and `convert`
publish: `output` and `video_output`. Whether to replace
an existing one is a property of the run,
not of the script: the same document is run once to produce its outputs and
again to replace them. `mechcore run --force` answers yes for the whole run.
Without it, a step's existing destinations are asked about once, naming every
file at stake, and a run with no terminal to ask on refuses.

The deletion happens in the client either way, and only after every
destination of the step has been checked, so a step refused for one file
leaves the others as they were. The Adapter and the MCFR writer still refuse
to write over anything; the caller removes the file before asking, so the
fail-closed rule keeps protecting a recording in flight.

`force` is not a script field at all, and a step that carries one is rejected.

A recording other than a watch takes the instrument channels to record into
the MCFR, by name, in any combination:

```yaml
instrument: [target_refs, skill_attackable_checker, target_search]
```

A channel lives inside the recording, outside both of its hashes
([mcfr.md](../mcfr/mcfr.md#instrument-channels)); what each channel holds is
[adapter.md](../adapter/adapter.md#record_replay_round).

`diff` over two recordings returns the report `mechcore diff` prints: the hash
verdict, the field groups that differ with the ticks they differ on, and one
tick explained. `fields` names the groups a step is about, and makes
`fields_equal` their verdict, so a script can hold a unit's lock or motion
state to a recording while other fields still differ, which the hash cannot:

```yaml
- diff:
    left: $out/game.mcfr
    right: $out/simulated.mcfr
    fields: [units.mech_lock_target, units.motion_state, units.weapon_aims]
  expect:
    fields_equal: true
```

A group that differs is reached by its dotted path, as
`fields.units.motion_state.first_divergence`.

`verify` checks each file as `mechcore verify` does and answers every report
in one result: `reports` holds them in the order of `input`, `valid` says
whether all of them verified, and `invalid` lists each that did not as
`{path, error}`, with the path as the step wrote it. That a file does not
verify is an answer, as a difference is `diff`'s, so a step that holds files
to their contracts says so with `expect`; `invalid: []` is the expectation
that fails naming every file that does not verify rather than the first:

```yaml
- let:
    fights: glob(tests/units/fights/*.yaml)
- verify: {input: $fights}
  expect:
    invalid: []
```

Naming the files is the script's, as it is a command line's: `verify` takes
files and refuses a directory, and `glob` lists one. An empty `input` is
refused, so a pattern that matched nothing fails the step instead of
verifying nothing.

`show` with `view: outcome` and `view: stats` reads a recording for the two
halves a capture is taken for: what the fight left of its units, and what was
written onto them before it moved them. Both answer the same object the command
prints, so `expect` asserts a measurement directly — `sides.red.survivors.0.life` for
the one, `sides.blue.0.skill.0.modifiers.damage_rate.add` for the other. That
is what turns a capture script from a probe of the build into a regression
against it; `tests/modifier/composition.mcscript` is the worked example.

`convert` with `to: mcfr` runs the deterministic simulator on a layout and
returns the same result object `mechcore convert --to mcfr` prints, so `expect`
can assert `seed_source`, `steps`, or a dotted path like `hashes.result_hash`.
It needs no game unless the step names `backend: game`. Omit `output` unless the run should also publish
an MCFR; an existing one is replaced as a recording's is. A rewrite to a layout
without `output` answers the layout it would have written.

`game.apply_layout` owns the whole transaction from the main menu: it creates the
Training Ground itself and brings it to the layout's activation round. A layout
already carries both the seed and the round, so nothing needs threading through
a separate `game.start_test`, and calling `game.start_test` first is refused.

It takes a layout object. Read one from disk with `read_yaml`, or take the
authoritative one out of a replay recording with `embedded_layout`. To record
one layout under several seeds, use the wrapper form:

```yaml
- game.apply_layout:
    layout: $layout
    seed: 1787720817
```

A layout's top-level keys are closed to `seed`, `round`, `blue` and `red`, so the
wrapper is never mistaken for a layout. The override wins over the layout's own
`seed`. A layout cannot carry `0`, which the schema rejects; omit `seed` on both
to let the game generate one.

## Variables

`$name` and `$name.field` resolve against `vars` and anything a `let` bound.

A string that is **exactly** one reference keeps the referenced value's type; a
reference embedded in longer text is stringified and spliced:

```yaml
- game.apply_layout: {layout: $layout, seed: $case.seed}  # stays a number
- game.record: {output: $out/fight.mcfr}                # becomes a path string
```

`${name.field}` delimits the reference explicitly. A bare reference runs to
the first character that cannot be part of a path, so `$out/$case.name.mcfr`
would look for a field called `mcfr`; write `$out/${case.name}.mcfr` instead.

An undefined reference fails the step by name before the operation runs.

### Built-in functions

Only inside a `let` value.

| Function | Result |
| --- | --- |
| `read_yaml(<path>)` | the parsed YAML document |
| `embedded_layout(<path.mcfr>)` | the layout embedded in that recording |
| `range(<count>)` | integers from `0` through `count - 1`; `count` is at most 10,000 |
| `glob(<pattern>)` | the paths of the files in one directory whose names match, sorted |

`glob` matches `*`, any run of characters, and `?`, any one, in the last
component alone, and spells each path as the pattern spells its directory:
`glob(tests/units/fights/*.yaml)` binds `[tests/units/fights/a.yaml, ...]`. A
wildcard in a directory is refused, and a directory holding no match binds an
empty list.

`embedded_layout` is how a replay round becomes a Training Ground layout
without a separate conversion step:

```yaml
- convert: {input: $grbr, to: mcfr, backend: game, round: 2, output: $out/replay.mcfr}
- let:
    layout: embedded_layout($out/replay.mcfr)
- game.start_test: {seed: $layout.seed}
- game.apply_layout: $layout
```

## Expectations

`expect` asserts on the step's result. Only the declared fields are compared;
extra result fields are ignored. A mismatch fails the run and names both
values.

A key may be a dotted path, because the values worth asserting are nested: a
recording reports `operation.tick_count`, not `tick_count`.

```yaml
- diff:
    left: $out/replay.mcfr
    right: $out/training.mcfr
  expect:
    equal: true
```

## Loops

`foreach` repeats a body once per item of a list, binding the item to a name:

```yaml
- foreach: {case: $cases}
  where: {smoke: true}
  steps:
    - game.apply_layout: {layout: $layout, seed: ${case.seed}}
```

The source is any list value, so a loop consumes a data file rather than
copying it into the script. `where` filters items by the same subset-equality
rule as `expect`: every declared field must match, and other fields are
ignored.

Each iteration gets its own bindings. The loop variable, and anything the body
binds with `let`, are discarded at the end of the iteration, so one case cannot
see the previous one's values and nothing leaks past the loop.

A failing step ends the run. The iteration is the unit that fails: a step whose
predecessor failed cannot produce a meaningful result, so nothing after it in
that body runs, and no later iteration starts.

A loop inside a loop is rejected, as is `steps` or `where` on a plain
operation, and `expect` on the loop itself; assert inside the body instead. A
loop needs no stopping condition of its own: a higher claim ends the whole run
wherever it is.

The static rule reaches into loop bodies, so a native operation cannot be
hidden inside a loop to evade a gameless script's `game:` requirement.

### Unattended standard 1v1 corpus recording

`game.record` with `watch: true` is one long, atomic native transaction. It refreshes the
server matchmaking watch list, selects an eligible scene at round one, watches
through the result, and returns to the main menu. By default the result is the
file already saved in the game's own `ProjectDatas/Replay` directory. Setting
`output_dir` additionally publishes a create-new corpus copy there. The next
loop iteration cannot start until all of those steps have completed.

The admissible scene policy is fixed rather than script-configurable:

- server-provided watch list (`ERoomListFilter.MatchFirst`) without competition
  `matchInfo`;
- exactly two players, subtype `Mod1V1`, no custom rule deltas, normal game
  mode, `VS_1_1` map mode, and round one;
- fewer than the native 300-watcher limit.

After the game finishes, the Adapter accepts only a new or changed file in the
game's native `ProjectDatas/Replay` directory. It waits for the file to become
stable, uses `MatchProxy.SaveReplay` if autosave produced nothing, and copies
with create-new semantics. The file is the game's own recording of a match it
admitted at round one, and is published unread. There is no `force` field: a
basename collision aborts the run instead of replacing corpus data.

A collector declares the lowest level, so anything else takes the machine from
it, and `range` gives the batch a bounded source:

```yaml
game: launch
level: 0

steps:
  - let:
      captures: range(10000)
  - foreach: {capture: $captures}
    steps:
      - game.record:
          watch: true
          wait_for_scene_seconds: 900
          match_timeout_seconds: 7200
```

Running any ordinary script ends it: the default level of `1` outranks it, the
watch in progress is abandoned at its next poll, the game returns to the main
menu, and the collector exits leaving that game to its claimant. Every match
already collected is untouched, and the abandoned one produces no recording.

The loop fails closed on the first unsuccessful match and emits one JSON result
line per recording. Add `output_dir` only when a separate corpus copy is wanted.
Redirect stdout to a JSONL file when the per-file path, publication mode and
selected scene metadata should travel with the corpus. A ready-to-run batch
lives at `replay/record-standard-1v1.mcscript`.

The native replay is never deleted, and neither is a copy that reached the
corpus directory. A published copy survives even when only the match-exit check
fails, and the failed result line is what keeps it out of an accepted manifest
until someone looks at it.

## Output

One JSON object per completed step, on stdout:

```json
{"step": 4, "operation": "game.record", "elapsed_ms": 10787, "result": {...}}
```

`elapsed_ms` measures the operation alone, which is what makes capture cost
attributable per step rather than per run.

Inside a loop each line carries `iteration`, and `step` counts within the body.
The loop emits its own line on completion:

```json
{"step": 2, "operation": "foreach", "elapsed_ms": 17005,
 "result": {"iterations": 1, "considered": 1}}
```

`considered` counts every item and `iterations` only those that passed `where`,
so a filter that matched nothing is visible rather than silent.

A failed step aborts the run, reports `step <n> (<operation>)` with the
underlying error, and still releases the game.

## Failure

Every failure lands in one of five classes, and which one it is decides what it
cost.

**Rejected before anything runs.** A document that does not parse, an unknown
top-level key, a step without exactly one operation key, a `level:` with no
`game:`, a native operation in a gameless script, a loop inside a loop, `steps`
or `where` on a plain operation, `expect` on a loop itself, or a `force` field
on a step. `--check` finds all of these, nothing is probed, and no game starts.
The static rules reach into loop bodies, so a native operation cannot hide in
one to evade a gameless script's `game:` requirement.

**Acquisition failed.** The codes are [session.md](session.md)'s: `no_game`,
`foreign_game`, `adapter_busy`, `adapter_unresponsive`, `protocol_mismatch`,
`launch_failed`. No step has run.

**A step failed.** Either the operation returned an error, whose code is
[adapter.md](../adapter/adapter.md)'s, or an `expect` did not match. The run
reports `step <n> (<operation>)` with the underlying error, stops, and still
releases the game. Inside a loop the iteration is the unit that fails: nothing
later in that body runs and no further iteration starts. Whatever earlier steps
published stays on disk.

**A destination already exists.** A step refuses to overwrite. With
`--force` the client removes the file first; without it the run asks once,
naming every file at stake, and a run with no terminal to ask on refuses. The
adapter itself never overwrites either way, so a recording in flight stays
protected.

**Taken over.** Not a failure. The run reports
`{"operation":"evicted","completed":false}` and exits successfully, leaving the
game to the claimant and shutting nothing down.

Only the third and fourth classes can cost a capture. The first two cost
nothing, which is what makes `--check` worth running before a long script.

## Worked example

Replay one GRBR round, rebuild it in Training Ground from the layout the replay
itself carries, and require the two recordings to agree:

```yaml
game: launch

vars:
  grbr: work/replay/replays/<version>/<replay>.grbr
  out: /tmp/mechcore/tuff-replay-vs-training

steps:
  - convert:
      input: $grbr
      to: mcfr
      backend: game
      round: 2
      output: $out/replay.mcfr
  - let:
      layout: embedded_layout($out/replay.mcfr)
  - game.apply_layout: $layout
  - game.record:
      output: $out/training.mcfr
  - diff:
      left: $out/replay.mcfr
      right: $out/training.mcfr
    expect:
      equal: true
```

## Regression re-recording

A pinned fight is a [fight](../document/fight.md) document under
`tests/<topic>/fights/`, and the directory is the table: no other file lists
the fights. CI hands every one to `verify`, and `crates/simulation/tests/fight.rs`
reads three of `tests/regression/fights/` to name a few fields — a unit's lock
and motion state — so that a failure says which one moved. Re-recording is one
more reader of the same files, not a copy of them, and a run expresses it as a
loop over a directory:

```yaml
game: launch
headless: true

vars:
  out: /tmp/mechcore/regression/refresh

steps:
  - let: {fights: glob(tests/regression/fights/*.yaml)}
  - foreach: {fight: $fights}
    steps:
      - convert:
          input: $fight
          to: mcfr
          backend: game
          output: $out/${fight}.mcfr
      - convert:
          input: $out/${fight}.mcfr
          to: fight
          output: $out/$fight
      - diff:
          left: $fight
          right: $out/$fight
        expect:
          equal: true
```

A fight states its own seed, so the recording `convert` takes none, and the
recording read back by `convert --to fight` is a document of the same kind as
the fixture, which `diff` compares field by field. `glob` answers paths a
script cannot take apart, so each output lands under `out` at its fixture's own
path. The repository's own re-recording, `scripts/record-fights.py`, is the
same loop as commands, one per fight.

## Unresolved

**Should a failing iteration end the whole run?** Today it does: a loop over a
directory of fights stops at the first one that fails, and the fights after it
are never recorded. For a verify run that is right, because the first drift is the answer.
For a refresh run over a long corpus it throws away the rest of an expensive
session to report something already known. Either the loop grows a way to say
which it is, or the two uses stay distinguished only by the presence of
`expect`, as they are now.

**Should a loop be allowed inside a loop?** Rejecting it keeps the output shape
flat, since a line carries one `iteration` and a `step` within one body. Nesting
would need a shape for that, and no case has yet needed one.
