# mcscript

[TOC]

A `.mcscript` describes one bounded run: an optional game acquisition, named
variables, and an ordered list of steps. It replaces the hand-written Python
drivers that each re-implemented the session lifecycle around a client.

```sh
mechcore run <script.mcscript> [--check]
```

`--check` validates the document and exits without probing or launching
anything.

## Document shape

Four top-level keys, all others rejected:

```yaml
game: launch        # optional: launch | attach; omitted means offline
level: 1            # optional: 0..4, needs a game; higher takes it from lower
vars:               # optional
  grbr: tests/grbr/example.grbr
  out: work/research/example
steps:              # required, at least one
  - record_replay_round:
      grbr: $grbr
      round: 2
      output: $out/replay.mcfr
```

Each step is a mapping with exactly one operation key, plus an optional
`expect`. Relative paths resolve against the working directory, matching every
other subcommand.

## Acquisition

`game:` accepts exactly `launch` or `attach`, with the ownership and failure
semantics defined in [session.md](session.md). Omitting it makes the script
**offline**.

**A script that omits `game:` and uses a native operation is rejected before
execution.** Nothing is probed and no game is started, so `--check` answers
"does this need the game?" without holding it. This is what lets simulator and
recording-comparison work share the same runner as native capture.

An acquired game is released on every exit path. A game this run started is
shut down through `quit_game`; a game it found already running is left running.

`level:` is what this run outranks, in `0..=4`, defaulting to `1`. A script
whose level is strictly above the level of the client currently holding the
game takes the game from it; an equal or lower one is refused with
`adapter_busy`. Declaring a level without a `game:` is rejected, because there
is nothing to order.

Being taken over is not a failure, and nothing is waited for. The Adapter
abandons the operation in flight, a recording included, returns the game to the
main menu, and closes the connection; the run reports
`{"operation":"evicted","completed":false}` and exits successfully, leaving the
game to whoever claimed it. That is the one exit path where a launched game is
not shut down: the Adapter is holding it for the next client.

The acquisition states, their failure codes and what evicts what are in
[session.md](session.md).

## Operations

| Operation | Needs a game | Notes |
| --- | --- | --- |
| `let` | no | binds names; see built-ins below |
| `compare` | no | `left`, `right`, optional `verbose`; the verdict, and the divergent tick states only with `verbose` |
| `sim` | no | `layout`, optional `seed`, `output`, `config`; same report as `mechcore sim` |
| `status` | yes | current status snapshot |
| `start_test` | yes | optional `seed`, `map_id`; rarely needed, see `apply_layout` |
| `apply_layout` | yes | the layout object, or `{layout, seed}` |
| `record_battle` | yes | `output`, optional `video_output`, `speed_up`, `instrumentation` |
| `record_replay_round` | yes | `grbr`, `round`, `output`, optional `speed_up`, `instrumentation` |
| `record_watch_replay` | yes | optional `output_dir`, `wait_for_scene_seconds`, `match_timeout_seconds`; records one live standard 1v1 |
| `toggle_fight` | yes | |
| `speed_up` | yes | standalone operation, distinct from the recording field |
| `quit_match` | yes | |
| `quit_game` | yes | |

A recording refuses to overwrite its destination, and a script does not declare
otherwise. Whether to replace an existing recording is a property of the run,
not of the script: the same document is run once to produce its outputs and
again to replace them. `mechcore run --force` answers yes for the whole run.
Without it, an existing destination is asked about once, naming every file at
stake, and a run with no terminal to ask on refuses as before.

The deletion happens in the client either way. The Adapter still refuses to
write over anything; the caller removes the file before asking, so the
fail-closed rule keeps protecting a recording in flight.

Both recording operations accept a research-only HDF5 sidecar request:

```yaml
instrumentation:
  output: $out/rvo.h5
  profile: target_refs_rvo_v1
  rvo_scope: {start_tick: 4, end_tick: 12, unit_ids: [72, 117, 257, 405]}
```

The sidecar path resolves like the recording output and must be new (even with
`force: true`). RVO scope selects 1–8 unique positive MCFR unit IDs and at most
64 ticks of update starts; delayed publications can appear after `end_tick`.
This instrumentation is separate from MCFR and does not participate in its hash.

`compare` returns the verdict, the two recording summaries, and the first
divergent tick. It omits the divergent tick states unless `verbose: true`,
because those are whole world snapshots and a script that only wanted to know
whether two recordings match should not carry megabytes of units through its
log. `mechcore mcfr compare` always prints them.

`sim` runs the deterministic simulator on a layout and returns the same result
object `mechcore sim` prints, so `expect` can assert `seed_source`, `steps`, or
a dotted path like `hashes.physics_result_hash`. It needs no game, which is
what lets `scripts/simulate-regressions.mcscript` drive the whole regression
manifest offline. Omit `output` unless the run should also publish an MCFR.

`apply_layout` owns the whole transaction from the main menu: it creates the
Training Ground itself and brings it to the layout's activation round. A layout
already carries both the seed and the round, so nothing needs threading through
a separate `start_test`, and calling `start_test` first is refused.

It takes a layout object. Read one from disk with `read_yaml`, or take the
authoritative one out of a replay recording with `embedded_layout`. To record
one layout under several seeds, use the wrapper form:

```yaml
- apply_layout:
    layout: $layout
    seed: 1787720817
```

A layout's top-level keys are closed to `seed`, `round` and `sides`, so the
wrapper is never mistaken for a layout. The override wins over the layout's own
`seed`. A layout cannot carry `0`, which the schema rejects; omit `seed` on both
to let the game generate one.

## Variables

`$name` and `$name.field` resolve against `vars` and anything a `let` bound.

A string that is **exactly** one reference keeps the referenced value's type; a
reference embedded in longer text is stringified and spliced:

```yaml
- apply_layout: {layout: $layout, seed: $case.seed}  # stays a number
- record_battle: {output: $out/battle.mcfr}          # becomes a path string
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

`embedded_layout` is how a replay round becomes a Training Ground layout
without a separate conversion step:

```yaml
- record_replay_round: {grbr: $grbr, round: 2, output: $out/replay.mcfr}
- let:
    layout: embedded_layout($out/replay.mcfr)
- start_test: {seed: $layout.seed}
- apply_layout: $layout
```

## Expectations

`expect` asserts on the step's result. Only the declared fields are compared;
extra result fields are ignored. A mismatch fails the run and names both
values.

A key may be a dotted path, because the values worth asserting are nested: a
recording reports `operation.tick_count`, not `tick_count`.

```yaml
- compare:
    left: $out/replay.mcfr
    right: $out/training.mcfr
  expect:
    equal: true
    content_equal: true
```

## Loops

`foreach` repeats a body once per item of a list, binding the item to a name:

```yaml
- foreach: {case: $cases}
  where: {smoke: true}
  steps:
    - apply_layout: {layout: $layout, seed: ${case.seed}}
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
hidden inside a loop to evade an offline script's `game:` requirement.

### Unattended standard 1v1 corpus recording

`record_watch_replay` is one long, atomic native transaction. It refreshes the
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
      - record_watch_replay:
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
selected scene metadata should travel with the corpus. See
[grbr-corpus.md](grbr-corpus.md) for the native call path, evidence boundary and
live qualification result. A ready-to-check batch lives at
`scripts/record-standard-1v1-grbr.mcscript`.

## Output

One JSON object per completed step, on stdout:

```json
{"step": 4, "operation": "record_battle", "elapsed_ms": 10787, "result": {...}}
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

## Worked example

Replay one GRBR round, rebuild it in Training Ground from the layout the replay
itself carries, and require the two recordings to agree:

```yaml
game: launch

vars:
  grbr: tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr
  out: work/research/tuff-replay-vs-training

steps:
  - record_replay_round:
      grbr: $grbr
      round: 2
      output: $out/replay.mcfr
  - let:
      layout: embedded_layout($out/replay.mcfr)
  - apply_layout: $layout
  - record_battle:
      output: $out/training.mcfr
  - compare:
      left: $out/replay.mcfr
      right: $out/training.mcfr
    expect:
      equal: true
      content_equal: true
```

## Regression re-recording

`tests/mcfr-regressions.yaml` stays a data table, read by `crates/simulation/tests/battle.rs`
for the offline simulator regression. A script is a second reader of that same
table, not a copy of it:

```yaml
game: launch

vars:
  out: work/research/regression-refresh

steps:
  - let:
      cases: read_yaml(tests/mcfr-regressions.yaml)
  - foreach: {case: $cases}
    where: {smoke: true}
    steps:
      - let:
          layout: read_yaml(${case.layout})
      - apply_layout:
          layout: $layout
          seed: ${case.seed}
      - record_battle:
          output: $out/${case.name}.mcfr
        expect:
          operation.tick_count: ${case.tick_count}
          operation.hashes.physics_result_hash: ${case.physics_result_hash}
```

The two uses of this shape differ only by the `expect` block, and confusing
them wastes a capture run:

- **Verify.** With `expect`, the run fails on the first case that no longer
  matches the manifest. This is the check that nothing drifted.
- **Refresh.** Without `expect`, the run records unconditionally and reports
  each result, which is what produces the new values to write back into the
  manifest. Nothing is written back automatically; the manifest is edited
  deliberately, so a refresh is a reviewable diff rather than a side effect.

Because `apply_layout` creates the Training Ground itself, cases run one after
another in a single game process. Only the first pays the cold start.
