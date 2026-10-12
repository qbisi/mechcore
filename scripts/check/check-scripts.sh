#!/bin/sh
# Every pinned fight verifies.
#
# This is what `.github/workflows/ci.yml` runs, given to a checkout, so a
# branch is held to the same pins before it is pushed. The pins are the fight
# documents under `tests/*/`, each fought again by the simulator and
# compared with what the game recorded; `verify` reads them from standard
# input, one path a line, and answers every fixture. One `verify` fights
# its inputs one after another, so the fights are handed out one per process
# across every processor, or `JOBS` processes when it is set. The checker
# fails after every fixture finishes if any failed. Each process writes to
# its own files, so long reports cannot interleave. After every process finishes, all reports are
# printed in fixture-path order and kept with stderr and exit codes.
#
#     scripts/check/check-scripts.sh                 against target/release/mechcore
#     MECHCORE=target/debug/mechcore scripts/check/check-scripts.sh
#     JOBS=1 scripts/check/check-scripts.sh          one fight at a time
#     FIXTURE_REPORT_DIR=reports/fixtures scripts/check/check-scripts.sh
set -eu
cd "$(dirname "$0")/../.."
bin=${MECHCORE:-target/release/mechcore}
jobs=${JOBS:-$(getconf _NPROCESSORS_ONLN)}
case "$jobs" in
    ''|*[!0-9]*) echo 'JOBS must be a positive integer' >&2; exit 2 ;;
esac
if [ "$jobs" -le 0 ]; then
    echo 'JOBS must be a positive integer' >&2
    exit 2
fi
report_root=${FIXTURE_REPORT_DIR:-work/check/fixtures}
mkdir -p "$report_root"
report_root=$(cd "$report_root" && pwd)
fixture_reports=$(mktemp -d "$report_root/run.XXXXXX")
fixture_bin=$bin
export fixture_bin fixture_reports
git ls-files -z 'tests/*/*.yaml' > "$fixture_reports/fixtures.paths"
tr '\000' '\n' < "$fixture_reports/fixtures.paths" > "$fixture_reports/fixtures.txt"
if [ ! -s "$fixture_reports/fixtures.txt" ]; then
    echo 'No tracked fight fixtures were found' >&2
    exit 2
fi

# A verify exit of 255 must not make xargs abandon the remaining fixtures.
# The worker records its status and succeeds; the parent fails after reading
# every status. Worker failures themselves leave an incomplete report.
workers_status=0
xargs -0 -P "$jobs" -n 1 sh -c '
    set -eu
    fixture=$1
    report="$fixture_reports/$fixture"
    mkdir -p "$(dirname "$report")"
    status=0
    "$fixture_bin" verify "$fixture" > "$report.json" 2> "$report.stderr" || status=$?
    printf "%s\n" "$status" > "$report.status"
' fixture-worker < "$fixture_reports/fixtures.paths" || workers_status=$?

passed=0
failed=0
incomplete=0
: > "$fixture_reports/results.jsonl"
: > "$fixture_reports/failures.jsonl"
: > "$fixture_reports/stderr.log"
: > "$fixture_reports/status.tsv"
while IFS= read -r fixture; do
    report="$fixture_reports/$fixture"
    status=missing
    if [ -f "$report.status" ]; then
        status=$(cat "$report.status")
    fi
    printf '%s\t%s\n' "$fixture" "$status" >> "$fixture_reports/status.tsv"
    if [ -f "$report.json" ]; then
        cat "$report.json" >> "$fixture_reports/results.jsonl"
    fi
    case "$status" in
        0) passed=$((passed + 1)) ;;
        missing) incomplete=$((incomplete + 1)) ;;
        *)
            failed=$((failed + 1))
            cat "$report.json" >> "$fixture_reports/failures.jsonl"
            ;;
    esac
    if [ "$status" != 0 ] || [ -s "$report.stderr" ]; then
        printf '\n%s (exit %s)\n' "$fixture" "$status" >> "$fixture_reports/stderr.log"
        if [ -f "$report.stderr" ]; then
            cat "$report.stderr" >> "$fixture_reports/stderr.log"
        fi
    fi
done < "$fixture_reports/fixtures.txt"

# Emit complete JSON reports, including every expected/actual difference.
# The artifact is authoritative if a CI log viewer limits what it displays.
cat "$fixture_reports/results.jsonl"
cat "$fixture_reports/stderr.log" >&2
printf 'Fixtures: %s passed, %s failed, %s incomplete\nReports: %s\n' \
    "$passed" "$failed" "$incomplete" "$fixture_reports" > "$fixture_reports/summary.txt"
cat "$fixture_reports/summary.txt" >&2
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf 'Fixtures: %s passed, %s failed, %s incomplete. Complete reports are in the `fixture-verify` artifact.\n' \
        "$passed" "$failed" "$incomplete" >> "$GITHUB_STEP_SUMMARY"
fi
if [ "$workers_status" -ne 0 ] || [ "$incomplete" -ne 0 ]; then
    exit 2
fi
if [ "$failed" -ne 0 ]; then
    exit 1
fi
