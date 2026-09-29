#!/usr/bin/env python3
"""How far the simulator is from fighting the rounds the corpus recorded.

Two numbers, and they measure different things.

*What the simulator accepts* is the progress bar: every tracked round projected
onto the layout its fight starts from and handed to `convert --to mcfr`, counting the
ones it does not refuse. It is zero today and is meant only to go up.

*What the corpus asks for* is the work order: the same refusals, read for what
they name. A refusal names everything the layout is refused for at once, one
clause each: a field no implemented module understands, with the module that
owes it, or one member of a field the registry lets through — a unit with no
behaviour, an officer whose effect this build cannot compose. So this is a
histogram of what the simulator itself says it is missing rather than a list
written down a second time.

    python3 scripts/corpus/fight-coverage.py [--binary target/release/mechcore]

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

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]

# `cannot simulate layout <path>: ` before the refusal proper.
WHERE = re.compile(r"^cannot simulate layout \S+: ")

# `side blue needs modules this build has not implemented: officers (Modifier),
# constructions (FightConstructionSystem)`, one clause per side.
REGISTRY = re.compile(r"^side (?:blue|red) needs modules this build has not implemented: ")
ASKED = re.compile(r"([a-z][a-z ]+) \(([A-Za-z]+)\)")

# What a member refusal says about a side or a round is not what blocks it: the
# same officer blocks a blue side and a red one, and equipment worn past round
# one blocks every later round alike.
SIDE = re.compile(r"^side (?:blue|red)(?:: | )")
WORN = re.compile(r"is worn in round \d+")


def blockers_of(reason: str) -> set[tuple[str, str]]:
    """What a refusal names, as (what, owner) pairs across both sides.

    A registry clause is owed by its module and lands with it; a member refusal
    is its own owner, cleared by nothing but its own mechanism.
    """
    held = set()
    for clause in WHERE.sub("", reason).split("; "):
        if REGISTRY.match(clause):
            held |= set(ASKED.findall(REGISTRY.sub("", clause)))
        else:
            member = WORN.sub("is worn in a later round", SIDE.sub("", clause))
            held.add((member, member))
    return held


def named(what: str, owner: str) -> str:
    """A blocker as a line: a field with its module, or a member up to its why."""
    return f"{what} ({owner})" if what != owner else what.split(", and ")[0]


def rounds_of(match_doc: pathlib.Path) -> int:
    return sum(1 for line in match_doc.read_text().splitlines() if line == "kind: state")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/mechcore")
    parser.add_argument("--matches", help="the match documents (default: work/match/<version>)")
    arguments = parser.parse_args()
    binary = REPOSITORY / arguments.binary
    if not binary.exists():
        print(f"{binary} is not built; cargo build --release first", file=sys.stderr)
        return 1

    asked: collections.Counter[tuple[str, str]] = collections.Counter()
    blockers: list[set[tuple[str, str]]] = []
    refused_projection = []
    accepted = 0
    with tempfile.TemporaryDirectory() as room:
        layout = pathlib.Path(room) / "deployment.yaml"
        matches = REPOSITORY / (arguments.matches or f"work/match/{build_data.configured_build()}")
        for match_doc in sorted(matches.glob("*.yaml")):
            for round_number in range(1, rounds_of(match_doc) + 1):
                projected = subprocess.run(
                    [binary, "convert", match_doc, "--to", "layout", "--round", str(round_number),
                     layout, "--force"],
                    capture_output=True,
                )
                if projected.returncode != 0:
                    refused_projection.append(
                        (match_doc.name, round_number, projected.stderr.decode().strip())
                    )
                    continue
                fought = subprocess.run(
                    [binary, "convert", layout, "--to", "mcfr"], capture_output=True
                )
                if fought.returncode == 0:
                    accepted += 1
                    blockers.append(set())
                    continue
                held = blockers_of(json.loads(fought.stderr.decode())["reason"])
                blockers.append(held)
                for blocker in held:
                    asked[blocker] += 1

    total = len(blockers)
    print(f"{total} rounds projected, {accepted} of them the simulator accepts")
    for name, round_number, reason in refused_projection:
        print(f"  projection refused {name} round {round_number}: {reason}")
    if not total:
        return 1

    # How many rounds each refusal is all that stands between: the ones its
    # owner alone would open. Most rounds are held by several at once, so this
    # and not the count of rounds a refusal appears in orders the work.
    owed = [{owner for _, owner in held} for held in blockers]
    alone = collections.Counter(next(iter(held)) for held in owed if len(held) == 1)
    print("\nwhat the corpus asks for, and the rounds it alone holds")
    for (what, owner), count in asked.most_common():
        print(
            f"  {count:4} rounds ({100 * count // total:2}%)  {alone[owner]:3} alone  "
            f"{named(what, owner)}"
        )
    sizes = collections.Counter(len(held) for held in blockers)
    print(
        "  refusals per round: "
        + ", ".join(f"{size}->{count}" for size, count in sorted(sizes.items()))
    )

    # A module lands whole, so the order that opens the corpus fastest is over
    # owners and not over fields. It is not the same as which refusal blocks
    # the most rounds: a round opens only when everything it names is cleared.
    print("\nrounds inside the closure as owners clear, greedily ordered")
    done: set[str] = set()
    while remaining := set().union(*owed) - done:
        best = max(
            sorted(remaining),
            key=lambda owner: sum(1 for held in owed if not held - done - {owner}),
        )
        done.add(best)
        inside = sum(1 for held in owed if not held - done)
        print(f"  {inside:4}/{total}  + {named(best, best)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
