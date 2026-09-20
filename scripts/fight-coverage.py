#!/usr/bin/env python3
"""How far the simulator is from fighting the rounds the corpus recorded.

Two numbers, and they measure different things.

*What the corpus asks for* is a fact about the battles: every tracked round is
projected onto the layout its fight starts from, and each layout is read for the
fields a fight has to understand. That histogram says which mechanism is worth
building next, and it does not move when the simulator does.

*What the simulator accepts* is the progress bar: the same layouts handed to
`fight run`, counting the ones it does not refuse. It is zero today and is meant
only to go up.

    python3 scripts/fight-coverage.py [--binary target/release/mechcore]

Both halves need `doc project`, so the binary is built first. A round the
projection itself refuses is reported rather than skipped silently.
"""

import argparse
import collections
import json
import pathlib
import re
import subprocess
import sys
import tempfile

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent

# The layout fields a fight has to understand, in the document's own names.
# `travelling`, `level` and `equipment` sit on a unit rather than on the side,
# so they are read out of the unit lines.
SIDE_FIELDS = [
    "officers",
    "techs",
    "blueprints",
    "energy_tower_skills",
    "battle_skills",
    "constructions",
    "contraptions",
    "airdrop_shields",
    "terrains",
]


def asked_for(layout: str) -> set[str]:
    """Which fields this layout carries that a bare fight does not cover."""
    held = {
        field
        for field in SIDE_FIELDS
        if re.search(rf"^  {field}:\s*$", layout, re.M)
        or re.search(rf"^  {field}: \[.+\]$", layout, re.M)
    }
    if re.search(r"^  tower_strengthen_levels: \[(?!0, 0\])", layout, re.M):
        held.add("tower_strengthen_levels")
    if re.search(r"level: [2-9]", layout):
        held.add("unit level")
    if "equipment:" in layout:
        held.add("unit equipment")
    if "travelling:" in layout:
        held.add("travelling")
    return held


def rounds_of(battle: pathlib.Path) -> int:
    return sum(
        1 for line in battle.read_text().splitlines() if line == "kind: state"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/debug/mechcore")
    parser.add_argument("--battles", default="tests/battle")
    arguments = parser.parse_args()
    binary = REPOSITORY / arguments.binary
    if not binary.exists():
        print(f"{binary} is not built; cargo build first", file=sys.stderr)
        return 1

    asked = collections.Counter()
    blockers = []
    refused_projection = []
    accepted = 0
    with tempfile.TemporaryDirectory() as room:
        layout = pathlib.Path(room) / "deployment.yaml"
        for battle in sorted((REPOSITORY / arguments.battles).glob("*.yaml")):
            for round_number in range(1, rounds_of(battle) + 1):
                projected = subprocess.run(
                    [binary, "doc", "project", battle, "--round", str(round_number),
                     "--output", layout],
                    capture_output=True,
                )
                if projected.returncode != 0:
                    refused_projection.append(
                        (battle.name, round_number, projected.stderr.decode().strip())
                    )
                    continue
                held = asked_for(layout.read_text())
                blockers.append(held)
                for field in held:
                    asked[field] += 1
                fought = subprocess.run(
                    [binary, "fight", "run", layout], capture_output=True
                )
                accepted += int(fought.returncode == 0)

    total = len(blockers)
    print(f"{total} rounds projected, {accepted} of them the simulator accepts")
    for name, round_number, reason in refused_projection:
        print(f"  projection refused {name} round {round_number}: {reason}")
    if not total:
        return 1

    print("\nwhat the corpus asks for")
    for field, count in asked.most_common():
        print(f"  {field:24} {count:4} rounds ({100 * count // total}%)")
    sizes = collections.Counter(len(held) for held in blockers)
    print(
        "  fields per round: "
        + ", ".join(f"{size}->{count}" for size, count in sorted(sizes.items()))
    )

    # Which order opens the corpus fastest, which is not the same as which
    # field blocks the most rounds: no round is one field away from playable.
    print("\nrounds inside the closure as mechanisms land, greedily ordered")
    done: set[str] = set()
    while True:
        remaining = set().union(*blockers) - done if blockers else set()
        if not remaining:
            break
        best = max(
            remaining,
            key=lambda field: sum(
                1 for held in blockers if not held - done - {field}
            ),
        )
        done.add(best)
        inside = sum(1 for held in blockers if not held - done)
        print(f"  + {best:24} {inside:4}/{total}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
