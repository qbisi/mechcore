#!/usr/bin/env python3
"""Extract build 2259's eleven ordinary EquipmentData rows, without a type tree.

    work/tools/asset-venv/bin/python scripts/extract-equipment-effects.py

Reads level0 path 188 in the serialized field order of GRObject, ConfigData,
ItemData, ReinforceItemData and EquipmentData in GRCore. Subclass arrays are
outside this table. Q32.32 integers are written as raw integers; only the
comment beside one reads it as a decimal.
"""

import argparse
import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("skills", ROOT / "scripts/extract-skills.py")
skills = importlib.util.module_from_spec(spec)
spec.loader.exec_module(skills)

FIELDS = (
    ("life_rate", "i64"), ("damage_rate", "i64"), ("speed_value", "i32"),
    ("min_attack_range_value", "i64"), ("attack_range_value", "i64"),
    ("attack_range_rate", "i64"), ("attack_interval_value", "i64"),
    ("attack_interval_rate", "i64"), ("splash_range_value", "i64"),
    ("projectile_speed_value", "i64"), ("projectile_life_rate", "i64"),
    ("important_unit", "flag"), ("mech_type", "ints"), ("units", "ints"),
    ("exp_rate", "i32"), ("upgrade_supply_rate", "i32"),
    ("upgrade_supply_value", "i32"), ("supply_value", "i32"),
    ("destroy_huge_mech_earnings_value", "i32"), ("grade_upper_limit", "i32"),
)


def row(r):
    d = {"id": r.i32(), "name": r.string()}
    if r.flag():
        raise ValueError("test EquipmentData is outside build 2259's table")
    for _ in range(4):  # ItemData: icon, description, descParams, story
        r.string()
    # ReinforceItemData's private itemLevel and ItemData.paramsArray are not serialized.
    d.update(level=r.i32(), scope=r.i32(), supply=r.i32())
    r.string(), r.string()  # reinforcePicName, advancePicName
    d["reactor_core"] = r.i32()
    d["limited_scene"] = r.ints()
    d.update(earliest_round=r.i32(), latest_round=r.i32(), can_repeated=r.flag())
    d.update(permanent_effect=r.flag(), main_skill_effect=r.flag(), extra_skill_effect=r.flag())
    d.update(round_duration=r.i32(), round_supply=r.i32())
    for name, kind in FIELDS:
        d[name] = getattr(r, kind)()
    return d


def extract(level0):
    env = skills.UnityPy.load(str(level0))
    obj = next(o for o in env.objects if o.path_id == 188)
    r = skills.Reader(obj.get_raw_data())
    r.i32(), r.i64(), r.flag(), r.i32(), r.i64(), r.string()
    folder = r.string()
    if folder != "equipmentDatas":
        raise ValueError(f"path 188 is not EquipmentGroupData: {folder!r}")
    count = r.i32()
    if count != 11:
        raise ValueError(f"expected eleven ordinary equipment rows, got {count}")
    rows = [row(r) for _ in range(count)]
    if [d["id"] for d in rows] != list(range(13030001, 13030012)):
        raise ValueError("EquipmentData IDs moved; check the serialized layout")
    # Independent native stats: the three captured items and their own channels.
    expected = [(0, "attack_range_value", 20 << 32),
                (1, "life_rate", 3221225472), (2, "damage_rate", 2791728742)]
    for i, field, value in expected:
        if rows[i][field] != value:
            raise ValueError(f"row {rows[i]['id']} {field} disagrees with native stats")
    return rows


# What a row writes onto a unit, in the order an officer's table writes it.
# A rate and an FPoint value are Q32.32 raw integers, and a comment reads each
# one; speed and the two unit-effect integers are plain.
RATES = ("life_rate", "damage_rate", "attack_range_rate", "attack_interval_rate",
         "projectile_life_rate")
VALUES = ("attack_range_value", "min_attack_range_value", "attack_interval_value",
          "splash_range_value", "projectile_speed_value")
INTEGERS = ("speed_value", "exp_rate", "grade_upper_limit")
# Which skills and for how long, written only when set.
FLAGS = ("main_skill_effect", "extra_skill_effect", "permanent_effect", "important_unit")


def reading(value):
    text = f"{value / (1 << 32):+.4f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def render(rows):
    lines = [
        "schema: mechcore.equipment_effects",
        "game_build: 1.11.1.3.2259",
        "",
        "# What an ordinary EquipmentData row writes onto the unit that wears it,",
        "# read out of level0 path 188 by scripts/extract-equipment-effects.py.",
        "# docs/rules/equipment_effects.md states what each field means. A field",
        "# is written only when it is set; what an equipment costs is",
        "# config/reinforce_items.yaml's, and its other pool fields are not here.",
        "#",
        "# A rate is an FPoint Q32.32 raw integer: 3221225472 is +0.75. A value is",
        "# an FPoint in the number's own units: 85899345920 is +20 of range.",
        "",
        "equipment:",
    ]
    for d in rows:
        lines.append(f"  - id: {d['id']}")
        lines.append(f"    name: {d['name']}")
        lines.append(f"    mech_type: [{', '.join(map(str, d['mech_type']))}]")
        if d["units"]:
            lines.append(f"    units: [{', '.join(map(str, d['units']))}]")
        for flag in FLAGS:
            if d[flag]:
                lines.append(f"    {flag}: true")
        if d["round_duration"]:
            lines.append(f"    round_duration: {d['round_duration']}")
        for field in RATES + VALUES:
            if d[field]:
                lines.append(f"    {field}: {d[field]}{reading(d[field])}")
        for field in INTEGERS:
            if d[field]:
                lines.append(f"    {field}: {d[field]}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--level0", type=Path, default=skills.LEVEL0)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    output = ROOT / "config/equipment_effects.yaml"
    written = render(extract(args.level0))
    if args.check:
        if not output.exists() or output.read_text() != written:
            raise SystemExit("equipment effect table differs from level0")
        print("eleven EquipmentData rows agree byte for byte")
    else:
        output.write_text(written)
        print(f"eleven EquipmentData rows -> {output}")


if __name__ == "__main__":
    main()
