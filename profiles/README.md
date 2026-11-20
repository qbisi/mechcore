# Profiles

Solver profile manifests (`mechcore.profile/v1`) passed to `mechcore solve --profile <file>`.

Each YAML file declares:

- `opponent`: the opponent belief plugin used inside evaluation (for example,
  `opponent.replay_truth` replays the opponent's recorded actions from the match).
- `pipeline`: an ordered list of beam-transforming operator plugins executed from
  the round opening to produce candidate solutions and their decision traces.

`default.yaml` is the minimal placement-search demo profile: it replays the
match's recorded opponent actions, warm-starts our side from our recorded round
actions with all movable formations unpinned, predicts each unpinned formation's
placement sequentially through the placement predictor seam (`shift_stub`),
deduplicates equivalent states, and returns the top-`k` solutions maximizing
`core_delta` (`opponent_core_damage - own_core_damage`).
