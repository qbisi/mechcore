# Profiles

Solver profile manifests (`mechcore.profile/v1`) passed to `mechcore solve --profile <file>`.

Each YAML file composes a solver runtime from swappable plugins and a recursive
pipeline AST:

- `services`: capability services mounted onto the shared `SolveContext`:
  - `opponent`: resolves the weighted opponent belief set (`opponent.replay_truth`).
  - `space`: computes movement masks and collision checks (`space.strict_grid`).
  - `evaluator`: scores candidate states against opponent beliefs (`eval.sim_exact`).
- `proposers`: batch-first placement proposal plugins aggregated into one pool
  (`proposer.stay`, `proposer.shift_stub`).
- `guards` can stop a loop on stagnation or cap the total number of simulated
  fights with `guard.max_simulations`. The cap includes the baseline and every
  pipeline stage. Within a batch, distinct uncached states get budget in input
  order. Each state needs one fight per opponent hypothesis. Cached scores
  remain available after the budget runs out.
- `filters`: `waterfall` proposal filters (`filter.dedup_poses`,
  `filter.scope_bounds`).
- `observers`: `emit` observers (`observer.trace`).
- `operators`: named reusable stages invoked via `call: <name>`.
- `pipeline`: recursive sequence of `use`, `call`, `seq`, `branch`, `repeat`,
  and `scope` stages.

`default.yaml` runs a minimal iterative coordinate-sweep demo: it replays the
match's recorded opponent actions, warm-starts our side with all movable
formations unpinned, sweeps movable formations using `space.strict_grid`,
`proposer.shift_stub`, and `eval.sim_exact` under simulation and stagnation
guards, and returns the top-3 legal action sequences maximizing `core_delta`.
