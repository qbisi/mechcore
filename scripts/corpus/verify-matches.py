#!/usr/bin/env python3
"""Verify every tracked match's transitions and summarize the coverage.

Runs ``mechcore verify`` over each match YAML in one batch. Each match's
report holds the opening and reinforcement checks and the transition coverage
that ``docs/spec/document/match.md`` defines: every leaf of each next opening
is equal, unequal, unimplemented or decided by the fight. Each report also fights
the match's rounds in order until the first the simulator refuses or whose
result is not the next round's opening. This script adds the reports up by
field group, lists every unequal leaf, and says for each match how many rounds
it fought as the match says and where it stopped.
The documents are the ones `scripts/corpus/export-replay-corpus.py` converts from the
corpus's replays of this checkout's version.

The exit status is 0 only when every match verifies, which needs no unequal
and no unimplemented leaf anywhere, and every round fought as the match says.

Run from anywhere inside the checkout, after a release build and a corpus
fetch:

    cargo build --release -p mechcore
    python3 scripts/corpus/replay.py sync
    python3 scripts/corpus/export-replay-corpus.py
    python3 scripts/corpus/verify-matches.py
    python3 scripts/corpus/verify-matches.py --json > coverage.json
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from typing import Any
import unicodedata

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402


CLASSES = ("equal", "unequal", "unimplemented", "fight")


def parse_arguments(root: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=root / "target/release/mechcore")
    parser.add_argument(
        "--match-dir",
        type=Path,
        help="the match documents (default: work/match/<version>, as "
        "scripts/corpus/export-replay-corpus.py writes them)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="print the summary as one JSON object instead of tables",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=50,
        help="unequal leaves to print, 0 for all (default: 50)",
    )
    return parser.parse_args()


def verify(executable: Path, matches: list[Path]) -> list[dict[str, Any]]:
    """One report per match, in the order named."""
    result = subprocess.run(
        [str(executable), "verify", *map(str, matches)],
        text=True,
        capture_output=True,
        check=False,
    )
    reports = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    if len(reports) != len(matches):
        raise RuntimeError(
            f"mechcore verify printed {len(reports)} reports for {len(matches)} matches:\n"
            f"{result.stderr}"
        )
    return reports


def add(total: dict[str, int], counts: dict[str, Any]) -> None:
    for name in CLASSES:
        total[name] = total.get(name, 0) + int(counts.get(name, 0))


def summarize(matches: list[Path], reports: list[dict[str, Any]]) -> dict[str, Any]:
    total: dict[str, int] = {}
    fields: dict[str, dict[str, int]] = {}
    files = []
    unequal = []
    for path, report in zip(matches, reports):
        coverage = report.get("coverage") or {}
        counts = coverage.get("total") or {}
        add(total, counts)
        for group, group_counts in (coverage.get("fields") or {}).items():
            add(fields.setdefault(group, {}), group_counts)
        for difference in coverage.get("unequal") or []:
            unequal.append({"match": path.name, **difference})
        fights = report.get("fights") or {}
        stopped = fights.get("stopped")
        files.append(
            {
                "match": path.name,
                "valid": bool(report.get("valid")),
                "error": report.get("error"),
                **{name: int(counts.get(name, 0)) for name in CLASSES},
                "fought": len(fights.get("equal") or []),
                "stopped": f"{stopped['result']} r{stopped['round']}" if stopped else "",
            }
        )
    stops: dict[str, int] = {}
    for file in files:
        if file["stopped"]:
            result = file["stopped"].split()[0]
            stops[result] = stops.get(result, 0) + 1
    return {
        "matches": len(matches),
        "valid": sum(file["valid"] for file in files),
        "fought": sum(file["fought"] for file in files),
        "stops": dict(sorted(stops.items())),
        "total": {name: total.get(name, 0) for name in CLASSES},
        "fields": dict(sorted(fields.items())),
        "files": files,
        "unequal": unequal,
    }


def width(text: str) -> int:
    """Terminal columns: a wide character, such as CJK, takes two."""
    return sum(2 if unicodedata.east_asian_width(char) in "WF" else 1 for char in text)


def pad(text: str, columns: int, right: bool) -> str:
    fill = " " * (columns - width(text))
    return fill + text if right else text + fill


def table(rows: list[list[str]], right: set[int]) -> str:
    widths = [max(width(row[at]) for row in rows) for at in range(len(rows[0]))]
    lines = []
    for number, row in enumerate(rows):
        cells = [pad(cell, widths[at], at in right) for at, cell in enumerate(row)]
        lines.append("  ".join(cells).rstrip())
        if number == 0:
            lines.append("  ".join("-" * columns for columns in widths))
    return "\n".join(lines)


def counted(name: str, counts: dict[str, int]) -> list[str]:
    return [name, *(str(counts[key]) for key in CLASSES)]


def print_summary(summary: dict[str, Any], limit: int) -> None:
    numbers = set(range(1, len(CLASSES) + 1))
    header = ["field", *CLASSES]
    rows = [header]
    rows += [counted(group, counts) for group, counts in summary["fields"].items()]
    rows.append(counted("total", summary["total"]))
    print(table(rows, numbers))
    print()

    rows = [["match", *CLASSES, "fought", "stopped", "error"]]
    for file in summary["files"]:
        rows.append([
            *counted(file["match"], file),
            str(file["fought"]),
            file["stopped"],
            "" if file["valid"] else file["error"] or "",
        ])
    print(table(rows, numbers | {len(CLASSES) + 1}))
    print()

    unequal = summary["unequal"]
    if unequal:
        shown = unequal if limit == 0 else unequal[:limit]
        rows = [["match", "round", "side", "path", "predicted", "recorded"]]
        for difference in shown:
            rows.append(
                [
                    difference["match"],
                    str(difference["round"]),
                    difference["side"],
                    difference["path"],
                    str(difference["predicted"]),
                    str(difference["recorded"]),
                ]
            )
        print(table(rows, {1}))
        if len(shown) < len(unequal):
            print(f"... {len(unequal) - len(shown)} more unequal leaves; --limit 0 lists all")
        print()

    stops = ", ".join(f"{count} {result}" for result, count in summary["stops"].items())
    print(f"{summary['fought']} rounds fought as the match says before the first that is not"
          + (f"; stopped: {stops}" if stops else ""))
    print(f"{summary['valid']}/{summary['matches']} matches verify")


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    args = parse_arguments(root)
    executable = args.mechcore.resolve()
    if not executable.is_file():
        print(
            f"mechcore executable does not exist: {executable}\n"
            "build it with: cargo build --release -p mechcore",
            file=sys.stderr,
        )
        return 2
    match_dir = args.match_dir or root / "work" / "match" / build_data.configured_build()
    matches = sorted(match_dir.resolve().glob("*.yaml"))
    if not matches:
        print(f"no match YAML in {match_dir}; run scripts/corpus/export-replay-corpus.py", file=sys.stderr)
        return 2

    summary = summarize(matches, verify(executable, matches))
    if args.json:
        print(json.dumps(summary, ensure_ascii=False, indent=2))
    else:
        print_summary(summary, args.limit)
    return 0 if summary["valid"] == summary["matches"] else 1


if __name__ == "__main__":
    sys.exit(main())
