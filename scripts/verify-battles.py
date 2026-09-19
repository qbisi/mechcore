#!/usr/bin/env python3
"""Verify every tracked battle's transitions and summarize the coverage.

Runs ``mechcore verify`` over each battle YAML in one batch. Each battle's
report holds the opening and reinforcement checks and the transition coverage
that ``docs/spec/document/battle.md`` defines: every leaf of each next opening
is equal, unequal, unimplemented or decided by the fight. This script adds the
reports up by field group and lists every unequal leaf.

The exit status is 0 only when every battle verifies, which needs no unequal
and no unimplemented leaf anywhere. Until the opening rules land it fails, so
CI does not run it yet.

Run from anywhere inside the checkout, after a release build:

    cargo build --release -p mechcore
    python3 scripts/verify-battles.py
    python3 scripts/verify-battles.py --json > coverage.json
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from typing import Any
import unicodedata


CLASSES = ("equal", "unequal", "unimplemented", "fight")


def parse_arguments(root: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=root / "target/release/mechcore")
    parser.add_argument("--battle-dir", type=Path, default=root / "tests/battle")
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


def verify(executable: Path, battles: list[Path]) -> list[dict[str, Any]]:
    """One report per battle, in the order named."""
    result = subprocess.run(
        [str(executable), "verify", *map(str, battles)],
        text=True,
        capture_output=True,
        check=False,
    )
    reports = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    if len(reports) != len(battles):
        raise RuntimeError(
            f"mechcore verify printed {len(reports)} reports for {len(battles)} battles:\n"
            f"{result.stderr}"
        )
    return reports


def add(total: dict[str, int], counts: dict[str, Any]) -> None:
    for name in CLASSES:
        total[name] = total.get(name, 0) + int(counts.get(name, 0))


def summarize(battles: list[Path], reports: list[dict[str, Any]]) -> dict[str, Any]:
    total: dict[str, int] = {}
    fields: dict[str, dict[str, int]] = {}
    files = []
    unequal = []
    for path, report in zip(battles, reports):
        coverage = report.get("coverage") or {}
        counts = coverage.get("total") or {}
        add(total, counts)
        for group, group_counts in (coverage.get("fields") or {}).items():
            add(fields.setdefault(group, {}), group_counts)
        for difference in coverage.get("unequal") or []:
            unequal.append({"battle": path.name, **difference})
        files.append(
            {
                "battle": path.name,
                "valid": bool(report.get("valid")),
                "error": report.get("error"),
                **{name: int(counts.get(name, 0)) for name in CLASSES},
            }
        )
    return {
        "battles": len(battles),
        "valid": sum(file["valid"] for file in files),
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

    rows = [["battle", *CLASSES, "error"]]
    for file in summary["files"]:
        rows.append([*counted(file["battle"], file), "" if file["valid"] else file["error"] or ""])
    print(table(rows, numbers))
    print()

    unequal = summary["unequal"]
    if unequal:
        shown = unequal if limit == 0 else unequal[:limit]
        rows = [["battle", "round", "side", "path", "predicted", "recorded"]]
        for difference in shown:
            rows.append(
                [
                    difference["battle"],
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

    print(f"{summary['valid']}/{summary['battles']} battles verify")


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    args = parse_arguments(root)
    executable = args.mechcore.resolve()
    if not executable.is_file():
        print(
            f"mechcore executable does not exist: {executable}\n"
            "build it with: cargo build --release -p mechcore",
            file=sys.stderr,
        )
        return 2
    battles = sorted(args.battle_dir.resolve().glob("*.yaml"))
    if not battles:
        print(f"no battle YAML in {args.battle_dir}", file=sys.stderr)
        return 2

    summary = summarize(battles, verify(executable, battles))
    if args.json:
        print(json.dumps(summary, ensure_ascii=False, indent=2))
    else:
        print_summary(summary, args.limit)
    return 0 if summary["valid"] == summary["battles"] else 1


if __name__ == "__main__":
    sys.exit(main())
