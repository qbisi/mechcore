#!/usr/bin/env python3
"""The corpus distance of one commit as Markdown, beside the commit before it.

Reads the reports `fight-coverage.py` and `verify-matches.py` print, saved as
`fight-coverage.txt` and `verify-matches.txt` in one directory, and writes a
table of how far the simulator is from the corpus: the rounds the simulator
accepts, the rounds fought as the match says, where the matches stop, the
refusals that hold the most rounds, and the unit technologies the game has
that the simulator fights, the rest by unit with their ids, and again by a
name another id shares. With `--before`, a directory holding the
same two reports of an earlier commit, each number carries its change.

    python3 scripts/corpus/distance-report.py <after> [--before <dir>] [--title TEXT]

`.github/workflows/corpus.yml` keeps it in one comment on each pull request,
compared with the master commit the pull request is based on.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

ACCEPTED = re.compile(r"^(\d+) rounds projected, (\d+) of them the simulator accepts$", re.M)
REFUSAL = re.compile(r"^\s+(\d+) rounds \(\s*\d+%\)\s+(\d+) alone  (.+)$", re.M)
FOUGHT = re.compile(r"^(\d+) rounds fought as the match says", re.M)
STOPPED = re.compile(r"(\d+) (differs|unsupported)")
VERIFY = re.compile(r"^(\d+)/(\d+) matches verify$", re.M)
EVERY = re.compile(
    r"^(\d+) of (\d+) rounds the simulator fights come out as the match says; (\d+) differ$", re.M
)
DIFFERING = "rounds the simulator fights whose result differs from the match"
TECHNOLOGIES = re.compile(
    r"^(\d+) unit technologies the game lets a unit research, (\d+) of them the simulator accepts$",
    re.M,
)
BY_UNIT = "unit technologies the simulator refuses, by unit"
BY_NAME = "unit technologies the simulator refuses whose name another id shares, by name"
UNIT_LINE = re.compile(r"^  +(\d+)  (\w+)$")
MEMBER_LINE = re.compile(r"^ {10}(\d+) (\w+) \(")
NAME_LINE = re.compile(r"^  +(\d+)  (.+?): (.+)$")
SHOWN = 12


def read(directory: pathlib.Path) -> dict:
    coverage = (directory / "fight-coverage.txt").read_text()
    matches = (directory / "verify-matches.txt").read_text()
    accepted = ACCEPTED.search(coverage)
    fought = FOUGHT.search(matches)
    verified = VERIFY.search(matches)
    if not (accepted and fought and verified):
        sys.exit(f"{directory} does not hold both reports whole")
    every = EVERY.search(matches)
    technologies = TECHNOLOGIES.search(coverage)
    summary_line = matches[fought.start():].splitlines()[0]
    stopped = {kind: int(count) for count, kind in STOPPED.findall(summary_line)}
    return {
        "rounds": int(accepted.group(1)),
        "accepted": int(accepted.group(2)),
        "fought": int(fought.group(1)),
        "differs": stopped.get("differs", 0),
        "unsupported": stopped.get("unsupported", 0),
        "verified": int(verified.group(1)),
        "matches": int(verified.group(2)),
        "refusals": [
            (name, int(rounds), int(alone)) for rounds, alone, name in REFUSAL.findall(coverage)
        ],
        # A report from before the rounds after a stop were fought has
        # neither; the numbers it cannot give are left out of the comparison.
        "equal": int(every.group(1)) if every else None,
        "differ": int(every.group(3)) if every else None,
        "differing": differing(matches),
        # A report from before the technologies were counted has neither.
        "technologies": int(technologies.group(1)) if technologies else None,
        "technologies_accepted": int(technologies.group(2)) if technologies else None,
        "technology_units": technology_units(coverage),
        "technology_names": technology_names(coverage),
        "coverage": coverage,
        "verify": matches,
    }


def section(coverage: str, heading: str) -> list[str] | None:
    """The lines of one of the report's blocks, up to the first blank line;
    None for a report from before the block was printed."""
    lines = coverage.splitlines()
    if heading not in lines:
        return None
    block = []
    for line in lines[lines.index(heading) + 1:]:
        if not line.strip():
            break
        block.append(line)
    return block


def technology_units(coverage: str) -> dict[str, list[str]] | None:
    """The refused technologies by unit, each as its id and layout name."""
    block = section(coverage, BY_UNIT)
    if block is None:
        return None
    units: dict[str, list[str]] = {}
    unit = None
    for line in block:
        if found := UNIT_LINE.match(line):
            unit = found.group(2)
            units[unit] = []
        elif (found := MEMBER_LINE.match(line)) and unit:
            units[unit].append(f"{found.group(1)} {found.group(2)}")
    return units


def technology_names(coverage: str) -> dict[str, tuple[int, str]] | None:
    """The refused technologies whose name another id shares: by name, how
    many and the line naming them."""
    block = section(coverage, BY_NAME)
    if block is None:
        return None
    return {
        found.group(2): (int(found.group(1)), found.group(3))
        for line in block
        if (found := NAME_LINE.match(line))
    }


def differing(matches: str) -> list[tuple[str, str, str, str]] | None:
    """The rows of the differing rounds' table: match, round, pinned, leaves."""
    lines = matches.splitlines()
    if DIFFERING not in lines:
        return None
    # The heading's block runs to the first blank line. A table, when a round
    # differs, opens with its column names and rule; with none, the block is
    # empty.
    block = []
    for line in lines[lines.index(DIFFERING) + 1:]:
        if not line.strip():
            break
        block.append(line)
    rows = []
    for line in block[2:]:
        match, round_number, pinned, leaves = re.split(r"\s{2,}", line.strip(), maxsplit=3)
        rows.append((match, round_number, pinned, leaves))
    return rows


def change(after: int, before: int | None) -> str:
    if before is None or after == before:
        return f"{after}"
    return f"{after} ({after - before:+d})"


def report(after: dict, before: dict | None, title: str) -> str:
    def was(key: str) -> int | None:
        return before[key] if before else None

    lines = [f"### {title}", ""]
    lines += [
        "| | " + ("before | after |" if before else "now |"),
        "| --- | " + ("---: | ---: |" if before else "---: |"),
    ]
    rows = [
        ("rounds the simulator accepts", "accepted", f" of {after['rounds']}"),
        ("rounds fought as the match says, every one", "equal", ""),
        ("rounds fought that differ from the match", "differ", ""),
        ("rounds fought as the match says before a match stops", "fought", ""),
        ("matches stopped where a round differs", "differs", ""),
        ("matches stopped at a round it refuses", "unsupported", ""),
        ("matches that verify", "verified", f" of {after['matches']}"),
        (
            "unit technologies the simulator accepts",
            "technologies_accepted",
            f" of {after['technologies']}",
        ),
    ]
    for label, key, suffix in rows:
        if after[key] is None:
            continue
        cells = [f"{change(after[key], was(key))}{suffix}"]
        if before:
            cells.insert(0, "" if before[key] is None else f"{before[key]}")
        lines.append(f"| {label} | " + " | ".join(cells) + " |")

    lines += differing_table(after["differing"] or [], before["differing"] if before else None)

    held_before = {name: rounds for name, rounds, _ in before["refusals"]} if before else {}
    alone_before = {name: alone for name, _, alone in before["refusals"]} if before else {}
    lines += [
        "",
        "The refusals that hold the most rounds, and the rounds each holds alone,",
        "which is what supplying it would open:",
        "",
        "| refusal | rounds | alone |",
        "| --- | ---: | ---: |",
    ]
    for name, rounds, alone in after["refusals"][:SHOWN]:
        shown = name if len(name) <= 90 else name[:87] + "..."
        lines.append(
            f"| {shown} | {change(rounds, held_before.get(name) if before else None)} "
            f"| {change(alone, alone_before.get(name) if before else None)} |"
        )
    lines += technology_table(after, before)
    for heading, key in (("fight-coverage.py", "coverage"), ("verify-matches.py", "verify")):
        lines += [
            "",
            f"<details><summary><code>{heading}</code></summary>",
            "",
            "```",
            after[key].rstrip(),
            "```",
            "</details>",
        ]
    return "\n".join(lines) + "\n"


def technology_table(after: dict, before: dict | None) -> list[str]:
    """The unit technologies the simulator refuses, by the unit that
    researches them, each by id; and again those whose name another id
    shares, which one mechanism usually clears together. Each is named, with
    its kind and cause, in `fight-coverage.py`'s report."""
    units = after["technology_units"]
    if after["technologies"] is None or units is None:
        return []
    was = before["technology_units"] if before else None
    lines = [
        "",
        "The unit technologies the game has and the simulator refuses, by unit; "
        "each is named, with its kind and cause, in `fight-coverage.py`'s report below:",
        "",
    ]
    if units:
        lines += ["| unit | refused | technologies |", "| --- | ---: | --- |"]
        for unit, members in units.items():
            count = change(len(members), len(was.get(unit, [])) if was is not None else None)
            lines.append(f"| {unit} | {count} | {', '.join(members)} |")
    else:
        lines.append("None.")
    if was is not None:
        now = {member for members in units.values() for member in members}
        gone = sorted(
            {member for members in was.values() for member in members} - now,
            key=lambda member: int(member.split()[0]),
        )
        if gone:
            lines += ["", "No longer refused: " + ", ".join(gone)]
    names = after["technology_names"] or {}
    lines += [
        "",
        "The refused ones whose name a technology of another id shares, by name:",
        "",
    ]
    if names:
        lines += ["| name | refused | technologies |", "| --- | ---: | --- |"]
        for name, (count, members) in names.items():
            lines.append(f"| {name} | {count} | {members} |")
    else:
        lines.append("None.")
    return lines

def differing_table(
    after: list[tuple[str, str, str, str]], before: list[tuple[str, str, str, str]] | None
) -> list[str]:
    """The rounds the simulator fights and gets wrong, the divergences left to
    find, each marked when the change brings it, and the ones it fixes."""
    lines = [
        "",
        "The rounds the simulator fights and gets wrong, each a divergence to find"
        " and, once fixed, a round to pin under `tests/corpus/`:",
        "",
    ]
    was = {(match, round_number) for match, round_number, _, _ in before} if before is not None else None
    now = {(match, round_number) for match, round_number, _, _ in after}
    if after:
        lines += ["| match | round | pinned | differences |", "| --- | ---: | --- | --- |"]
        for match, round_number, pinned, leaves in after:
            new = " (new)" if was is not None and (match, round_number) not in was else ""
            lines.append(f"| {match} | {round_number}{new} | {pinned} | {leaves} |")
    else:
        lines.append("None.")
    if was is not None:
        gone = sorted(was - now)
        if gone:
            lines += ["", "No longer wrong: " + ", ".join(f"{match} r{number}" for match, number in gone)]
    return lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("after", type=pathlib.Path)
    parser.add_argument("--before", type=pathlib.Path)
    parser.add_argument("--title", default="Corpus distance")
    arguments = parser.parse_args()
    before = read(arguments.before) if arguments.before else None
    sys.stdout.write(report(read(arguments.after), before, arguments.title))
    return 0


if __name__ == "__main__":
    sys.exit(main())
