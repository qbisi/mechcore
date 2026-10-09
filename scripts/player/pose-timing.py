"""Read a recording's unit poses for how each unit type acts.

    python3 scripts/player/pose-timing.py <recording.mcfr> [--mechcore <binary>]

A recording made with the `unit_pose` instrument channel holds, for every
unit and tick, the clip its model's base layer plays and how far through it.
This reads it against the recording's own units and events and prints, per
unit type:

- the clips its model plays while it idles, moves and attacks, by tick count;
- each clip's length and playback speed;
- where in its clip each shot leaves (`projectile_released`) and each blow
  lands (a `damage` its source deals with no projectile): the clip and its
  normalized time, median and range.

These are the numbers `crates/player/web/player.js` animates a unit's attack
with (its `SWING` and `ACTIONS`) when a recording holds no poses of its own.
`scripts/player/record-poses.mcscript` records the demo scene with the
channel; the recording itself stays out of the repository. The recording is
read through `mechcore query`.
"""

import argparse
import collections
import json
import statistics
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def query(mechcore, recording, sql):
    """The rows `mechcore query` answers, each a dict by column."""
    answer = subprocess.run(
        [str(mechcore), "query", str(recording), "--sql", sql],
        check=True,
        capture_output=True,
        text=True,
    )
    result = json.loads(answer.stdout)
    return [dict(zip(result["columns"], row)) for row in result["rows"]]


def unit_names():
    """Unit type id to the name config/units/ files it under."""
    names = {}
    for path in (ROOT / "config/units").glob("*.yaml"):
        fields = dict(
            line.split(":", 1) for line in path.read_text().splitlines()[:4] if ":" in line
        )
        names[int(fields["unit_type_id"])] = fields["type_name"].strip()
    return names


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("recording", type=Path)
    parser.add_argument("--mechcore", type=Path, default=ROOT / "target/release/mechcore")
    arguments = parser.parse_args()
    read = lambda sql: query(arguments.mechcore, arguments.recording, sql)
    if not read("SELECT name FROM sqlite_master WHERE name = 'instrument_unit_pose'"):
        sys.exit("the recording holds no unit_pose channel")

    names = unit_names()
    units = read("SELECT tick, unit_id, unit_type_id, motion_state FROM units")
    kind = {row["unit_id"]: names.get(row["unit_type_id"], str(row["unit_type_id"])) for row in units}
    motion = {(row["tick"], row["unit_id"]): row["motion_state"] for row in units}
    # A pose's clip is the one it weighs most, the first of equals.
    base = {
        (pose["tick"], pose["unit__id"]): pose
        for pose in read(
            "SELECT p.tick, p.unit__id, p.normalized_time, p.state_length, p.state_speed, "
            "(SELECT c.name FROM instrument_unit_pose__clips c WHERE c.row = p.row "
            "ORDER BY c.weight DESC, c.ordinal LIMIT 1) AS clip "
            "FROM instrument_unit_pose p WHERE p.layer = 0"
        )
    }

    def clip_of(pose):
        return pose["clip"] or "-"

    played = collections.defaultdict(collections.Counter)
    lengths = {}
    for (tick, unit), pose in base.items():
        clip = clip_of(pose)
        played[kind[unit]][(motion.get((tick, unit)), clip)] += 1
        lengths[(kind[unit], clip)] = (pose["state_length"], pose["state_speed"])

    # A shot leaves with `projectile_released`; a blow lands as a `damage`
    # its source deals with no projectile.
    landed = collections.defaultdict(list)
    for event in read(
        "SELECT tick, type, source__id, has_object FROM events "
        "WHERE source__kind = 'unit' AND (type = 'projectile_released' "
        "OR (type = 'damage' AND has_object = 0))"
    ):
        melee = event["type"] == "damage"
        pose = base.get((event["tick"], event["source__id"]))
        if pose:
            time = pose["normalized_time"]
            landed[(kind[event["source__id"]], "blow" if melee else "shot", clip_of(pose))].append(
                time - int(time)
            )

    for unit_kind in sorted(played):
        print(f"{unit_kind}")
        for (state, clip), ticks in sorted(played[unit_kind].items(), key=lambda item: (str(item[0][0]), -item[1])):
            length, speed = lengths[(unit_kind, clip)]
            print(f"  {state:>10}  {clip:<32} {ticks:5d} ticks   {length:.3f} s at {speed:.3f}x")
        for (owner, what, clip), times in sorted(landed.items()):
            if owner == unit_kind:
                print(
                    f"  {what} in {clip}: {len(times)}, at {statistics.median(times):.3f}"
                    f" ({min(times):.3f} to {max(times):.3f})"
                )


if __name__ == "__main__":
    main()
