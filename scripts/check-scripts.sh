#!/bin/sh
# Every tracked .mcscript parses, and every one that needs no game runs.
#
# This is the loop `.github/workflows/ci.yml` runs, given to a checkout, so a
# branch is held to the same pins before it is pushed: the native regression
# manifest, each topic's `regressions.mcscript`, and every other offline
# assertion. A script that declares `game:` is only parsed, because a machine
# without the game cannot run it; `run --check` is what says which it is.
#
#     scripts/check-scripts.sh                 against target/release/mechcore
#     MECHCORE=target/debug/mechcore scripts/check-scripts.sh
set -eu
cd "$(dirname "$0")/.."
bin=${MECHCORE:-target/release/mechcore}
for script in $(git ls-files '*.mcscript'); do
    checked=$("$bin" run "$script" --check)
    echo "$checked"
    needs_game=$(printf '%s' "$checked" |
        python3 -c 'import json,sys; print(json.load(sys.stdin)["game"] or "")')
    if [ -z "$needs_game" ]; then
        "$bin" run "$script" >/dev/null
    fi
done
