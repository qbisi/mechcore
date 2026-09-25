#!/usr/bin/env python3
"""How far the simulator is from fighting the rounds the corpus recorded.

Two numbers, and they measure different things.

*What the simulator accepts* is the progress bar: every tracked round projected
onto the layout its fight starts from and handed to `fight run`, counting the
ones it does not refuse. It is zero today and is meant only to go up.

*What the corpus asks for* is the work order: the same refusals, read for the
fields they name. A refusal names every field both sides carry that no
implemented module understands, and the module that owes each one, so this is a
histogram of what the simulator's own registry says it is missing rather than a
field list written down a second time.

    python3 scripts/fight-coverage.py [--binary target/release/mechcore]

A round the projection itself refuses is reported rather than skipped silently.
"""

import argparse
import collections
import json
import pathlib
import re
import subprocess
import sys
import tempfile

import build_data

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent

# `side blue needs modules this build has not implemented: officers (Modifier),
# constructions (FightConstructionSystem)`, one clause per side.
ASKED = re.compile(r"([a-z][a-z ]+) \(([A-Za-z]+)\)")

# A refusal that names no field at all: a module claims the field and still
# refuses what the round put in it, as `Modifier` does for an officer whose
# effect this build cannot compose. Such a round is blocked by something no
# module landing can clear, so it is counted apart rather than dropped — a
# refusal nobody attributes would otherwise read as a round already inside the
# closure.
UNATTRIBUTED = ("«refused without naming a field»", "—")


def asked_for(refusal: str) -> set[tuple[str, str]]:
    """Which (field, module) pairs a refusal names, across both sides."""
    return set(ASKED.findall(refusal))


def rounds_of(battle: pathlib.Path) -> int:
    return sum(1 for line in battle.read_text().splitlines() if line == "kind: state")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/debug/mechcore")
    parser.add_argument("--battles", help="the battle documents (default: work/battle/<version>)")
    arguments = parser.parse_args()
    binary = REPOSITORY / arguments.binary
    if not binary.exists():
        print(f"{binary} is not built; cargo build first", file=sys.stderr)
        return 1

    asked: collections.Counter[tuple[str, str]] = collections.Counter()
    blockers: list[set[tuple[str, str]]] = []
    refused_projection = []
    unattributed: list[tuple[str, int, str]] = []
    accepted = 0
    with tempfile.TemporaryDirectory() as room:
        layout = pathlib.Path(room) / "deployment.yaml"
        battles = REPOSITORY / (arguments.battles or f"work/battle/{build_data.build()}")
        for battle in sorted(battles.glob("*.yaml")):
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
                fought = subprocess.run(
                    [binary, "fight", "run", layout], capture_output=True
                )
                if fought.returncode == 0:
                    accepted += 1
                    blockers.append(set())
                    continue
                reason = json.loads(fought.stderr.decode())["reason"]
                held = asked_for(reason)
                if not held:
                    unattributed.append((battle.name, round_number, reason))
                    held = {UNATTRIBUTED}
                blockers.append(held)
                for field in held:
                    asked[field] += 1

    total = len(blockers)
    print(f"{total} rounds projected, {accepted} of them the simulator accepts")
    for name, round_number, reason in refused_projection:
        print(f"  projection refused {name} round {round_number}: {reason}")
    if unattributed:
        print(f"  {len(unattributed)} refused without naming a field, for example:")
        for name, round_number, reason in unattributed[:3]:
            print(f"    {name} round {round_number}: {reason}")
    if not total:
        return 1

    print("\nwhat the corpus asks for")
    for (field, module), count in asked.most_common():
        print(f"  {field:24} {module:26} {count:4} rounds ({100 * count // total}%)")
    sizes = collections.Counter(len(held) for held in blockers)
    print(
        "  fields per round: "
        + ", ".join(f"{size}->{count}" for size, count in sorted(sizes.items()))
    )

    # A module lands whole, so the order that opens the corpus fastest is over
    # modules and not over fields. It is not the same as which field blocks the
    # most rounds: no round is one module away from playable.
    owed = [{module for _, module in held} for held in blockers]
    print("\nrounds inside the closure as modules land, greedily ordered")
    done: set[str] = set()
    while True:
        # The marker is not a module and never lands, so a round owing it
        # stays outside the closure however many modules arrive.
        remaining = (set().union(*owed) - done - {UNATTRIBUTED[1]}) if owed else set()
        if not remaining:
            break
        best = max(
            remaining,
            key=lambda module: sum(1 for held in owed if not held - done - {module}),
        )
        done.add(best)
        inside = sum(1 for held in owed if not held - done)
        print(f"  + {best:28} {inside:4}/{total}")
    print("  modules per round: " + ", ".join(
        f"{size}->{count}"
        for size, count in sorted(collections.Counter(len(h) for h in owed).items())
    ))
    return 0


if __name__ == "__main__":
    sys.exit(main())
