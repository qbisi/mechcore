#!/usr/bin/env python3
"""Extract what an ordinary EquipmentData row writes onto a unit into `config/equipment_effects.yaml`.

    python3 scripts/extract-equipment-effects.py [--build BUILD] [--check]

The rows are `EquipmentGroupData.equipmentDatas` of `level0`, as
`scripts/build_data.py` reads them: the equipment whose effect is the plain
`ICommonMechDataChangeDataSource` correction. The subclass lists beside it
(buff, lifesteal, shield and the rest) are other mechanisms and not here. Test
rows and rows limited to Interstellar Expedition are left out. Q32.32 values
are written as raw integers; only the comment beside one reads it as a decimal.
"""

import sys
from pathlib import Path

import build_data

ROOT = Path(__file__).resolve().parent.parent
# The build's field for each column this table writes.
FIELDS = {
    "life_rate": "lifeChangeRate", "damage_rate": "damageChangeRate",
    "speed_value": "speedChangeValue", "min_attack_range_value": "minAttackRangeChangeValue",
    "attack_range_value": "attackRangeChangeValue", "attack_range_rate": "attackRangeChangeRate",
    "attack_interval_value": "attackIntervalChangeValue", "attack_interval_rate": "attackIntervalChangeRate",
    "splash_range_value": "splashRangeChangeValue", "projectile_speed_value": "projectileSpeedChangeValue",
    "projectile_life_rate": "projectileLifeChangeRate", "important_unit": "importantUnit",
    "mech_type": "mechType", "units": "unitID", "exp_rate": "expChangeRate",
    "grade_upper_limit": "changeGradeUpperLimit", "round_duration": "roundDuration",
    "main_skill_effect": "mainSkillEffect", "extra_skill_effect": "extraSkillEffect",
    "permanent_effect": "permanentEffect",
}


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def extract():
    rows = []
    for row in build_data.level0("EquipmentGroupData")["equipmentDatas"]:
        if row["isTestData"] or not build_data.in_standard(row):
            continue
        entry = {"id": row["id"], "name": row["name"]}
        entry.update({column: raw(row[field]) for column, field in FIELDS.items()})
        rows.append(entry)
    return rows


# What a row writes onto a unit, in the order an officer's table writes it.
# A rate and an FPoint value are Q32.32 raw integers, and a comment reads each
# one; speed and the two unit-effect integers are plain.
RATES = ("life_rate", "damage_rate", "attack_range_rate", "attack_interval_rate",
         "projectile_life_rate")
VALUES = ("attack_range_value", "min_attack_range_value", "attack_interval_value",
          "splash_range_value", "projectile_speed_value")
INTEGERS = ("speed_value", "grade_upper_limit")
# An int percentage on 1.11 builds, an FPoint rate from 2.0 on.
RATES += ("exp_rate",)
# Which skills and for how long, written only when set.
FLAGS = ("main_skill_effect", "extra_skill_effect", "permanent_effect", "important_unit")


def reading(value):
    text = f"{value / (1 << 32):+.4f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def render(rows):
    lines = [
        "schema: mechcore.equipment_effects",
        "",
        "# What an ordinary EquipmentData row writes onto the unit that wears it,",
        "# read out of EquipmentGroupData by scripts/extract-equipment-effects.py.",
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
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    output = ROOT / "config/equipment_effects.yaml"
    rows = extract()
    written = render(rows)
    if arguments.check:
        if not output.exists() or output.read_text() != written:
            sys.exit("config/equipment_effects.yaml differs from the build's export")
        print(f"{len(rows)} EquipmentData rows agree byte for byte")
    else:
        output.write_text(written)
        print(f"{len(rows)} EquipmentData rows -> {output.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
