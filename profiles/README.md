# Profiles

Solver profile manifests (`mechcore.profile/v1`) passed to `mechcore solve --profile <file>`.

Each YAML file composes a solver runtime from swappable plugins and a recursive
pipeline AST:

- `services`: capability services mounted onto the shared `SolveContext`:
  - `templates`: optional macro layout generator (`templates.r1_macro`) backed by
    YAML tables in [`data/`](data/). Paths are resolved relative to the profile
    manifest directory.
  - `opponent`: resolves the weighted opponent belief set
    (`opponent.replay_truth`, `opponent.replay_unpinned_templates`).
  - `space`: computes movement masks and collision checks (`space.strict_grid`).
  - `evaluator`: scores candidate states against opponent beliefs
    (`eval.sim_exact`).
- `proposers`: batch-first placement proposal plugins aggregated into one pool
  (`proposer.stay`, `proposer.shift_stub`).
- `guards` can stop a loop on stagnation (`guard.stagnation`) or cap the total
  number of simulated fights with `guard.max_simulations`. The cap includes the
  baseline and every pipeline stage. Within a batch, distinct uncached states get
  budget in input order. Each state needs one fight per opponent hypothesis.
  Cached scores remain available after the budget runs out.
- `filters`: `waterfall` proposal filters (`filter.dedup_poses`,
  `filter.scope_bounds`).
- `observers`: `emit` observers (`observer.trace`).
- `operators`: named reusable stages invoked via `call: <name>`.
- `pipeline`: recursive sequence of `use`, `call`, `seq`, `branch`, `repeat`,
  and `scope` stages, including `warm_start.own_actions`,
  `seed.macro_templates`, `placement.coordinate_sweep`,
  `placement.predict_unpinned`, `flow.dedup`, `eval.score`, and `eval.top_k`.

`data/` holds solver prior and skeleton tables (`unit_profiles.yaml` and
`r1_opening.yaml`) separate from `config/`.

`default.yaml` runs a minimal iterative coordinate-sweep demo: it replays the
match's recorded opponent actions, warm-starts our side with all movable
formations unpinned, sweeps movable formations using `space.strict_grid`,
`proposer.shift_stub`, and `eval.sim_exact` under simulation and stagnation
guards, and returns the top-3 legal action sequences maximizing `core_delta`.

`r1_blind.yaml` runs Round 1 macro-template search under uncertain opponent
positioning: both sides replay non-`concede` Round 1 actions and unpin all
movable formations, `opponent.replay_unpinned_templates` builds a 6-hypothesis
belief set (`asym_front`, `asym_mid`, `sym_mid`, `sym_deep`) with random
whole-board shifts, and `seed.macro_templates` evaluates 16 own macro variants
with `risk_lambda: 0.2`. A template skips a variant if its zone capacities or
per-unit rotation permissions leave a formation without a legal placement.
