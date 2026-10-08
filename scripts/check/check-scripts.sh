#!/bin/sh
# Every pinned fight verifies, and every tracked .mcscript parses.
#
# This is what `.github/workflows/ci.yml` runs, given to a checkout, so a
# branch is held to the same pins before it is pushed. The pins are the fight
# documents under `tests/*/fights/`, each fought again by the simulator and
# compared with what the game recorded; `verify` reads them from standard
# input, one path a line, and answers every invalid one. One `verify` fights
# its inputs one after another, so the fights are handed out one per process
# across every processor, or `JOBS` processes when it is set, and `xargs`
# fails when any of them does. A run script is only parsed: none that is
# tracked needs no game, and a machine without the game cannot run the others;
# `run --check` is what says which it is.
#
#     scripts/check/check-scripts.sh                 against target/release/mechcore
#     MECHCORE=target/debug/mechcore scripts/check/check-scripts.sh
#     JOBS=1 scripts/check/check-scripts.sh          one fight at a time
set -eu
cd "$(dirname "$0")/../.."
bin=${MECHCORE:-target/release/mechcore}
jobs=${JOBS:-$(getconf _NPROCESSORS_ONLN)}
git ls-files 'tests/*/fights/*.yaml' | xargs -P "$jobs" -n 1 "$bin" verify >/dev/null
for script in $(git ls-files '*.mcscript'); do
    checked=$("$bin" run "$script" --check)
    echo "$checked"
    needs_game=$(printf '%s' "$checked" |
        python3 -c 'import json,sys; print(json.load(sys.stdin)["game"] or "")')
    if [ -z "$needs_game" ]; then
        "$bin" run "$script" >/dev/null
    fi
done
