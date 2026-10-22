---
name: Match
about: A whole recorded match fails simulator verification
labels: match
assignees: ""
---

## Recorded match

<!-- Link the complete match at a pinned mechcore-replay commit. One BattleID
has one issue, including every failing round. Do not hand-correct its YAML. -->

Match: <pinned URL>
Simulator master commit: <commit>
CI run and complete verification artifacts: <URL>

## Whole-match verification

```sh
cargo build --release -p mechcore --no-default-features
target/release/mechcore verify --backend simulator match.yaml
```

<!-- Include the first reported failure, and link the complete JSON report. -->

## Resolution

The complete command above must exit 0 with a `kind: match`, `valid: true`
report. Fixing only the first failing round is not sufficient. Re-run
Tournament matches in mechcore-replay, setting `match` to the complete source
path, to verify current master and resolve the existing issue.
