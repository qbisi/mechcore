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

Three top-level keys, all others rejected:

```yaml
game: launch        # optional: launch | attach; omitted means offline
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

An acquired game is released on every exit path. A launched game is shut down
through `quit_game`; an attached game is left running.

## Operations

| Operation | Needs a game | Notes |
| --- | --- | --- |
| `let` | no | binds names; see built-ins below |
| `compare` | no | `left`, `right`; same report as `mechcore mcfr compare` |
| `sim` | no | `layout`, optional `seed`, `output`, `config`; same report as `mechcore sim` |
| `status` | yes | current status snapshot |
| `start_test` | yes | `seed`; rarely needed, see `apply_layout` |
| `apply_layout` | yes | the layout object, or `{layout, seed}` |
| `record_battle` | yes | `output`, optional `video_output`, optional `speed_up` |
| `record_replay_round` | yes | `grbr`, `round`, `output`, optional `speed_up` |
| `toggle_fight` | yes | |
| `speed_up` | yes | standalone operation, distinct from the recording field |
| `quit_match` | yes | |
| `quit_game` | yes | |

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

Nested loops are rejected, as is `steps` or `where` on a plain operation, and
`expect` on the loop itself; assert inside the body instead.

The static rule reaches into loop bodies, so a native operation cannot be
hidden inside a loop to evade an offline script's `game:` requirement.

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
