#!/usr/bin/env python3
"""Extract what an interceptor, a missile and a shield are into `config/contraptions.yaml`.

    python3 scripts/extract/extract-contraptions.py [--build BUILD] [--check]

Read through `scripts/build_data.py` from the `ContraptionGroupData` object of
`level0`. Its `interceptContraptionDatas` hold the interceptor a layout places:
`InterceptSystem.DoCreateFightInterceptor` makes it a building of its side, and
`InterceptEffectBase` reads the rest, its reach, the attack it deals a
projectile, and how that attack falls with every hit and comes back while it
is idle. Its `landMineContraptionDatas` hold the missile: `MineSystem` fires it
once at the nearest enemy in its trigger range, a projectile with the row's
speed, life, damage and splash, and the `buffDatas` row its `buffID` names is
written on what the projectile hits. `docs/rules/contraptions.md` states what
each field does.

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


MISSILE_INTEGERS = (
    ("count", "count"),
    ("damage", "damage"),
    ("maxLife", "max_life"),
    ("effectType", "effect_type"),
    ("effectRangeType", "effect_range_type"),
)
MISSILE_FIXED = (
    ("range", "trigger_range"),
    ("damageRange", "splash_radius"),
    ("moveSpeed", "speed"),
)
MISSILE_FLAGS = (
    ("canBeIntercept", "interceptible"),
    ("canAttackConstruction", "can_attack_construction"),
)
# The buff's fields the fight reads; every other rate has to be zero, and the
# script refuses the row when one is not.
BUFF_RATES = {"speedChangeRate": "move_speed_rate"}
BUFF_ZERO = (
    "lifeChangeRate", "maxLifeChangeRate", "currentLifeDisposableChangeRate",
    "attackDurationChangeRate", "extraAttackDurationChangeRate", "damageChangeRate",
    "amplifyDamageRate", "attackRangeChangeRate", "extraAttackRangeChangeRate", "stepTime",
)


def missile_lines(group):
    rows = [row for row in group["landMineContraptionDatas"] if not row["isTestData"]]
    if [row["id"] for row in rows] != [20001]:
        raise SystemExit(f"landMineContraptionDatas holds {[row['id'] for row in rows]}, not 20001")
    row = rows[0]
    buffs = {buff["id"]: buff for buff in build_data.container()["buffDatas"]}
    buff = buffs.get(row["buffID"])
    if buff is None:
        raise SystemExit(f"the missile names buff {row['buffID']}, which buffDatas does not hold")
    moving = [field for field in BUFF_ZERO if raw(buff.get(field, 0))]
    if moving:
        raise SystemExit(f"buff {buff['id']} moves {moving}, which this table does not carry")
    lines = [
        "",
        "missiles:",
        f"  - id: {row['id']}",
        *build_data.name_lines(row, "    "),
        "    layout_name: missile",
    ]
    for field, name in MISSILE_INTEGERS:
        lines.append(f"    {name}: {row[field]}")
    for field, name in MISSILE_FIXED:
        value = raw(row[field])
        lines.append(f"    {name}: {value}{reading(value)}")
    for field, name in MISSILE_FLAGS:
        lines.append(f"    {name}: {str(row[field]).lower()}")
    lines += [
        "    buff:",
        f"      id: {buff['id']}",
        *build_data.name_lines(buff, "      "),
        f"      divide: {buff.get('buffDivide', 0)}",
        f"      additive: {str(buff.get('isAdditiveMode', False)).lower()}",
        f"      debuff: {str(buff.get('debuff', False)).lower()}",
        f"      duration: {raw(buff['duration'])}{reading(raw(buff['duration']))}",
        f"      can_affect_construction: {str(buff.get('canAffectConstruction', False)).lower()}",
    ]
    for field, name in BUFF_RATES.items():
        value = raw(buff.get(field, 0))
        lines.append(f"      {name}: {value}{reading(value)}")
    return lines


def shield_lines(group):
    rows = [row for row in group["energyShieldContraptionDatas"] if not row["isTestData"]]
    if [row["id"] for row in rows] != [10001]:
        raise SystemExit(f"energyShieldContraptionDatas holds {[row['id'] for row in rows]}, not 10001")
    row = rows[0]
    return [
        "",
        "shields:",
        f"  - id: {row['id']}",
        *build_data.name_lines(row, "    "),
        "    layout_name: shield",
        f"    energy: {row['energy']}",
        f"    effect_type: {row['effectType']}",
        f"    effect_range_type: {row['effectRangeType']}",
        f"    radius: {raw(row['range'])}{reading(raw(row['range']))}",
    ]


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
        "# What an interceptor, a missile and a shield are, read out of `ContraptionGroupData`. Generated",
        "# by scripts/extract/extract-contraptions.py; docs/rules/contraptions.md",
        "# states what each field does. A time, a distance or a rate is an",
        "# FPoint Q32.32 raw integer, read in the comment beside it.",
        "",
        "interceptors:",
        f"  - id: {row['id']}",
        *build_data.name_lines(row, "    "),
        "    layout_name: interceptor",
    ]
    for field, name in INTEGERS:
        lines.append(f"    {name}: {row[field]}")
    for field, name in FIXED:
        value = raw(row[field])
        lines.append(f"    {name}: {value}{reading(value)}")
    lines += missile_lines(group)
    lines += shield_lines(group)
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
