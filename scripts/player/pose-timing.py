"""Read a recording's unit poses for how each unit type acts.

    uv run --with pyarrow python3 scripts/player/pose-timing.py <recording.mcfr>

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
channel; the recording itself stays out of the repository.
"""

import collections
import io
import statistics
import sys
import zipfile
from pathlib import Path

import pyarrow.parquet as pq

ROOT = Path(__file__).resolve().parents[2]
MOTION = ["idle", "moving", "attacking", "stopped", "transitioning"]
# `events.parquet`'s `type` tags, mcfr.md's event table counted from 0.
RELEASED, DAMAGE = 0, 2
UNIT = 0


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
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    archive = zipfile.ZipFile(sys.argv[1])
    if "instrument/unit_pose.parquet" not in archive.namelist():
        sys.exit("the recording holds no unit_pose channel")

    def table(member):
        return pq.read_table(io.BytesIO(archive.read(member))).to_pylist()

    names = unit_names()
    units = table("units.parquet")
    kind = {row["unit_id"]: names.get(row["unit_type_id"], str(row["unit_type_id"])) for row in units}
    motion = {(row["tick"], row["unit_id"]): row["motion_state"] for row in units}
    base = {}
    for pose in table("instrument/unit_pose.parquet"):
        if pose["layer"] == 0:
            base[(pose["tick"], pose["unit"]["id"])] = pose

    def clip_of(pose):
        clips = sorted(pose["clips"], key=lambda clip: -clip["weight"])
        return clips[0]["name"] if clips else "-"

    played = collections.defaultdict(collections.Counter)
    lengths = {}
    for (tick, unit), pose in base.items():
        clip = clip_of(pose)
        state = motion.get((tick, unit))
        played[kind[unit]][(MOTION[state] if isinstance(state, int) else state, clip)] += 1
        lengths[(kind[unit], clip)] = (pose["state_length"], pose["state_speed"])

    landed = collections.defaultdict(list)
    for event in table("events.parquet"):
        source = event.get("source")
        if not source or source["kind"] not in (UNIT, "unit"):
            continue
        melee = event["type"] == DAMAGE and event.get("object") is None
        if event["type"] != RELEASED and not melee:
            continue
        pose = base.get((event["tick"], source["id"]))
        if pose:
            time = pose["normalized_time"]
            landed[(kind[source["id"]], "blow" if melee else "shot", clip_of(pose))].append(
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
