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
| `status` | yes | current status snapshot |
| `start_test` | yes | `seed`; rarely needed, see `apply_layout` |
| `apply_layout` | yes | the layout object, or `{layout, seed}` |
| `record_battle` | yes | `output`, optional `video_output`, optional `speed_up` |
| `record_replay_round` | yes | `grbr`, `round`, `output`, optional `speed_up` |
| `toggle_fight` | yes | |
| `speed_up` | yes | standalone operation, distinct from the recording field |
| `quit_match` | yes | |
| `quit_game` | yes | |

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
`seed`; `0` still means system-random.

## Variables

`$name` and `$name.field` resolve against `vars` and anything a `let` bound.

A string that is **exactly** one reference keeps the referenced value's type; a
reference embedded in longer text is stringified and spliced:

```yaml
- apply_layout: {layout: $layout, seed: $case.seed}  # stays a number
- record_battle: {output: $out/battle.mcfr}          # becomes a path string
```

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

```yaml
- compare:
    left: $out/replay.mcfr
    right: $out/training.mcfr
  expect:
    equal: true
    content_equal: true
```

## Output

One JSON object per completed step, on stdout:

```json
{"step": 4, "operation": "record_battle", "elapsed_ms": 10787, "result": {...}}
```

`elapsed_ms` measures the operation alone, which is what makes capture cost
attributable per step rather than per run.

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
