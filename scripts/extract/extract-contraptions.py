#!/usr/bin/env python3
"""Extract what an interceptor is into `config/contraptions.yaml`.

    python3 scripts/extract/extract-contraptions.py [--build BUILD] [--check]

Read through `scripts/build_data.py` from the `ContraptionGroupData` object of
`level0`, whose `interceptContraptionDatas` hold the interceptor a layout
places. `InterceptSystem.DoCreateFightInterceptor` makes it a building of its
side, and `InterceptEffectBase` reads the rest: its reach, the attack it deals
a projectile, and how that attack falls with every hit and comes back while it
is idle. `docs/rules/contraptions.md` states what each field does.

One check stands between the export and the table: the row's `slotSize` has
to be the footprint `crates/document/src/catalog.rs` gives an interceptor,
which was read off placements the game accepted.

A time, a distance and a rate are the FPoint Q32.32 raw integers the build
stores, with a comment reading each one; `attackNum` and `maxLife` are plain
integers.
"""

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "config/contraptions.yaml"
CATALOG = ROOT / "crates/document/src/catalog.rs"
ONE = 1 << 32

# (field in the row, name here). What is left out is the client's: icon,
# description, story and audio.
INTEGERS = (
    ("maxLife", "max_life"),
    ("exp", "exp"),
    ("slotSize", "slot_size"),
    ("pathRadius", "path_radius"),
    ("pathfindingColliderPriority", "collider_priority"),
    ("attackNum", "attack"),
    ("effectType", "effect_type"),
    ("effectRangeType", "effect_range_type"),
)
FIXED = (
    ("radiusRangeMax", "range_max"),
    ("radiusRangeMin", "range_min"),
    ("prepareTime", "prepare_time"),
    ("interval", "interval"),
    ("coolingTime", "cooling_time"),
    ("riseInterval", "rise_interval"),
    ("decline", "decline"),
    ("lowerLimit", "lower_limit"),
    ("rise", "rise"),
    ("judgmentProbability", "judgment_probability"),
)


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def reading(value):
    text = f"{value / ONE:.6f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def catalog_footprint():
    text = CATALOG.read_text(encoding="utf-8")
    found = re.search(
        r'b"interceptor" => Some\(formation_spec\(\s*NativeFormation::Contraption\((\d+)\),\s*'
        r"Some\(\((\d+), (\d+)\)\),?\s*\)\)",
        text,
    )
    if not found:
        raise SystemExit(f"{CATALOG} gives the interceptor no footprint")
    return int(found.group(1)), int(found.group(2)), int(found.group(3))


def render(group):
    rows = [row for row in group["interceptContraptionDatas"] if not row["isTestData"]]
    identity, width, height = catalog_footprint()
    if [row["id"] for row in rows] != [identity]:
        raise SystemExit(f"interceptContraptionDatas holds {[row['id'] for row in rows]}, not {identity}")
    row = rows[0]
    if (row["slotSize"], row["slotSize"]) != (width, height):
        raise SystemExit(f"slotSize {row['slotSize']} is not the catalog's {width} x {height}")
    if raw(row["range"]) != raw(row["radiusRangeMax"]):
        raise SystemExit("the interceptor's range and radiusRangeMax differ")
    lines = [
        "schema: mechcore.contraptions",
        "",
        "# What an interceptor is, read out of `ContraptionGroupData`. Generated",
        "# by scripts/extract/extract-contraptions.py; docs/rules/contraptions.md",
        "# states what each field does. A time, a distance or a rate is an",
        "# FPoint Q32.32 raw integer, read in the comment beside it.",
        "",
        "interceptors:",
        f"  - id: {row['id']}",
        f"    name: {row['name']}",
        "    layout_name: interceptor",
    ]
    for field, name in INTEGERS:
        lines.append(f"    {name}: {row[field]}")
    for field, name in FIXED:
        value = raw(row[field])
        lines.append(f"    {name}: {value}{reading(value)}")
    return "\n".join(lines) + "\n"


def main():
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    written = render(build_data.level0("ContraptionGroupData"))
    if arguments.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != written:
            print("config/contraptions.yaml differs from the build's export", file=sys.stderr)
            return 1
        print("config/contraptions.yaml agrees with the build's export byte for byte")
        return 0
    OUTPUT.write_text(written)
    print(f"contraptions of build {build_data.build()} -> {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
