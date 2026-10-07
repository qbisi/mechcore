#!/usr/bin/env python3
"""How far the simulator is from fighting the rounds the corpus recorded.

Two numbers, and they measure different things.

*What the simulator accepts* is the progress bar: every tracked round projected
onto the layout its fight starts from and handed to `convert --to mcfr`, counting the
ones it does not refuse. It is meant only to go up.

*What the corpus asks for* is the work order: the same refusals, read for what
they name. A refusal names everything the layout is refused for at once, one
clause each: a field no implemented module understands, with the module that
owes it, or one member of a field the registry lets through — a unit with no
behaviour, an officer whose effect this build cannot compose. So this is a
histogram of what the simulator itself says it is missing rather than a list
written down a second time.

    python3 scripts/corpus/fight-coverage.py [--binary target/release/mechcore]

A round the projection itself refuses is reported rather than skipped silently.
A round in which a side concedes is not counted: the match ends with it, unfought.

Two more views group the same refusals. *By system* owes a member refusal to
the build list it comes from, since one mechanism clears the whole list, and a
registry clause to its module. *By layout field* says which field of the
layout each refusal is about. Each is printed as the rounds a group touches
and alone holds, and the order that opens the most rounds as groups land.
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
ASKED = re.compile(r"([a-z][a-z_ ]+) \(([A-Za-z]+)\)")

# What a member refusal says about a side is not what blocks it: the same
# officer blocks a blue side and a red one alike.
SIDE = re.compile(r"^side (?:blue|red)(?:: | )")


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
            member = SIDE.sub("", clause)
            held.add((member, member))
    return held


# The system a refusal is owed to: a whole build list for a member that names
# the list it comes from, a module for a registry clause, and otherwise the
# kind of thing it says it cannot do. Implementing a system clears every member
# of it at once, which is why the work is ordered over systems.
LISTED = re.compile(r"comes from (\w+)'s (\w+) list")
FIELDS_WRITTEN = re.compile(r"^(technology|officer) \d+ \(.*?\) (?:sets|writes) (\w+)")
UNIT = re.compile(r'^unit(?: type)? "(\w+)"')


def system_of(what: str, owner: str) -> str:
    """The system a refusal belongs to."""
    if what != owner:
        return owner
    if match := LISTED.search(owner):
        return f"{match.group(1)}.{match.group(2)}"
    if match := FIELDS_WRITTEN.search(owner):
        return f"{match.group(1)} field {match.group(2)}"
    if match := UNIT.search(owner):
        return f"unit {match.group(1)}"
    if "grows with the unit's rank" in owner:
        return "technology by rank"
    return owner.split(",")[0]


def field_of(what: str, owner: str) -> str:
    """The layout field a refusal is about."""
    if what != owner:
        return what
    # Named as the module registry names the field it claims, so a registry
    # clause and a member refusal about one field count together.
    for prefix, field in (
        ("technology", "unit technologies"),
        ("officer", "officers"),
        ("equipment", "unit equipment"),
        ("battle skill", "battle skills"),
        ("unit", "units"),
    ):
        if owner.startswith(prefix):
            return field
    if "missile" in owner:
        return "missile contraptions"
    return "other"


def view(heading: str, blockers: list, key) -> None:
    """How many rounds each group touches and alone holds, then the order
    that clears the most rounds as groups land whole."""
    total = len(blockers)
    owed = [{key(what, owner) for what, owner in held} for held in blockers]
    touched = collections.Counter(group for held in owed for group in held)
    alone = collections.Counter(next(iter(held)) for held in owed if len(held) == 1)
    print(f"\n{heading}: rounds it touches, rounds it alone holds")
    for group, count in touched.most_common():
        print(f"  {count:4} touched  {alone[group]:3} alone  {group}")
    print(f"\n{heading}, greedily ordered")
    done: set[str] = set()
    while remaining := set().union(*owed) - done:
        best = max(
            sorted(remaining),
            key=lambda group: sum(1 for held in owed if not held - done - {group}),
        )
        done.add(best)
        inside = sum(1 for held in owed if not held - done)
        print(f"  {inside:4}/{total}  + {best}")


def named(what: str, owner: str) -> str:
    """A blocker as a line: a field with its module, or a member up to its why."""
    return f"{what} ({owner})" if what != owner else what.split(", and ")[0]


def rounds_of(match_doc: pathlib.Path) -> list[int]:
    """The rounds a match states, less the one a side concedes: a concession
    ends the match with no fight."""
    text = match_doc.read_text()
    stated = sum(1 for line in text.splitlines() if line == "kind: state")
    conceded = set()
    for segment in re.split(r"^---$", text, flags=re.MULTILINE):
        found = re.match(r"\s*kind: action\nround: (\d+)$", segment, flags=re.MULTILINE)
        if found and re.search(r"^- \{type: concede\}$", segment, flags=re.MULTILINE):
            conceded.add(int(found.group(1)))
    return [number for number in range(1, stated + 1) if number not in conceded]


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
            for round_number in rounds_of(match_doc):
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

    view("by system", blockers, system_of)
    view("by layout field", blockers, field_of)
    return 0


if __name__ == "__main__":
    sys.exit(main())
