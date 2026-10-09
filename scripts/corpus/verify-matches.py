#!/usr/bin/env python3
"""Verify every tracked match's transitions and summarize the coverage.

Runs ``mechcore verify`` over each match YAML in one batch. Each match's
report holds the opening and reinforcement checks and the transition coverage
that ``docs/spec/document/match.md`` defines: every leaf of each next opening
is equal, unequal, unimplemented or decided by the fight. Each report also fights
every round of the match, each from the position the match states for it, and
says whether the simulator refused it or whether its result is the next round's
opening. This script adds the reports up by field group, lists every unequal
leaf, says for each match how many rounds it fought as the match says before
the first that is not and where that was, and lists every round the simulator
fights whose result differs from the match, the divergences left to find, with
whether `tests/corpus/fights/` pins the round already.
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
    python3 scripts/corpus/verify-matches.py --json-out coverage.json
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
        "--json-out",
        type=Path,
        help="also write the summary as JSON to this file, beside the tables",
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


def match_id(path: Path) -> str:
    """The replay's match number, the digits after `--` in the document's name."""
    return path.stem.split("--", 1)[-1].split("_", 1)[0]


def pinned(path: Path, round_number: int) -> bool:
    """Whether `tests/corpus/fights/` pins the round, under the name its readme gives."""
    root = Path(__file__).resolve().parents[2]
    return (root / "tests/corpus/fights" / f"{match_id(path)}-r{round_number}.yaml").is_file()


def summarize(matches: list[Path], reports: list[dict[str, Any]]) -> dict[str, Any]:
    total: dict[str, int] = {}
    fields: dict[str, dict[str, int]] = {}
    files = []
    unequal = []
    equal = 0
    differing = []
    for path, report in zip(matches, reports):
        coverage = report.get("coverage") or {}
        counts = coverage.get("total") or {}
        add(total, counts)
        for group, group_counts in (coverage.get("fields") or {}).items():
            add(fields.setdefault(group, {}), group_counts)
        for difference in coverage.get("unequal") or []:
            unequal.append({"match": path.name, **difference})
        rounds = (report.get("fights") or {}).get("rounds") or []
        stopped = next((fought for fought in rounds if fought["result"] != "equal"), None)
        leading = next(
            (at for at, fought in enumerate(rounds) if fought["result"] != "equal"), len(rounds)
        )
        for fought in rounds:
            if fought["result"] == "equal":
                equal += 1
            elif fought["result"] == "differs":
                differing.append(
                    {
                        "match": path.name,
                        "id": match_id(path),
                        "round": fought["round"],
                        "pinned": pinned(path, fought["round"]),
                        "differences": [
                            f"{difference['side']} {difference['path']}"
                            for difference in fought["differences"]
                        ],
                    }
                )
        files.append(
            {
                "match": path.name,
                "valid": bool(report.get("valid")),
                "error": report.get("error"),
                **{name: int(counts.get(name, 0)) for name in CLASSES},
                "fought": leading,
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
        "equal": equal,
        "differing": differing,
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

    differing = summary["differing"]
    fought = summary["equal"] + len(differing)
    print("rounds the simulator fights whose result differs from the match")
    if differing:
        rows = [["match", "round", "pinned", "differences"]]
        for found in differing:
            leaves = found["differences"]
            shown = ", ".join(leaves[:4]) + (f", +{len(leaves) - 4} more" if len(leaves) > 4 else "")
            rows.append([found["id"], str(found["round"]), "yes" if found["pinned"] else "no", shown])
        print(table(rows, {1}))
    print()

    stops = ", ".join(f"{count} {result}" for result, count in summary["stops"].items())
    print(f"{summary['fought']} rounds fought as the match says before the first that is not"
          + (f"; stopped: {stops}" if stops else ""))
    print(
        f"{summary['equal']} of {fought} rounds the simulator fights come out as the match says;"
        f" {len(differing)} differ"
    )
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
    if args.json_out:
        args.json_out.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    if args.json:
        print(json.dumps(summary, ensure_ascii=False, indent=2))
    else:
        print_summary(summary, args.limit)
    return 0 if summary["valid"] == summary["matches"] else 1


if __name__ == "__main__":
    sys.exit(main())
