#!/usr/bin/env python3
"""Extract what an Energy Tower skill writes onto a unit into `config/energy_tower_skill_effects.yaml`.

    python3 scripts/extract/extract-energy-tower-skill-effects.py [--build BUILD] [--check]

The rows are `energyTowerSkillDatas` of the config container, as
`scripts/build_data.py` reads it. A skill reaches a fight only when
`EnergyTowerSkill.IsCommonEffect` holds, which reads two fields and no other:
its speed value and its attack range value. So a skill is written only when
one of them is set, with its targeting and those two fields; what it does to a
ledger is `config/economy.yaml`'s. Q32.32 values are written as raw integers;
only the comment beside one reads it as a decimal.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def extract():
    rows = []
    for row in sorted(build_data.container()["energyTowerSkillDatas"], key=lambda row: row["id"]):
        speed = row.get("speedChangeValue", 0)
        attack_range = raw(row.get("attackRangeChangeValue", 0))
        if not speed and not attack_range:
            continue
        rows.append({
            "id": row["id"],
            "name": row.get("name"),
            "mech_type": row["mechType"],
            "units": row.get("unitID") or [],
            "attack_range_value": attack_range,
            "speed_value": speed,
        })
    return rows


def reading(value):
    text = f"{value / (1 << 32):+.4f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def render(rows):
    lines = [
        "schema: mechcore.energy_tower_skill_effects",
        "",
        "# What an Energy Tower skill writes onto the units of the side that",
        "# activated it, read out of energyTowerSkillDatas by",
        "# scripts/extract/extract-energy-tower-skill-effects.py.",
        "# docs/rules/energy_tower_skills.md states what each field means. Only",
        "# a skill that IsCommonEffect admits is here; what a skill costs and",
        "# pays is config/economy.yaml's.",
        "#",
        "# A value is an FPoint in the number's own units: 64424509440 is +15 of",
        "# range. A speed value is a plain integer.",
        "",
        "skills:",
    ]
    for d in rows:
        lines.append(f"  - id: {d['id']}")
        lines += build_data.name_lines(d, "    ")
        lines.append(f"    mech_type: [{', '.join(map(str, d['mech_type']))}]")
        if d["units"]:
            lines.append(f"    units: [{', '.join(map(str, d['units']))}]")
        if d["attack_range_value"]:
            value = d["attack_range_value"]
            lines.append(f"    attack_range_value: {value}{reading(value)}")
        if d["speed_value"]:
            lines.append(f"    speed_value: {d['speed_value']}")
    return "\n".join(lines) + "\n"


def main():
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    output = ROOT / "config/energy_tower_skill_effects.yaml"
    rows = extract()
    written = render(rows)
    if arguments.check:
        if not output.exists() or output.read_text() != written:
            sys.exit("config/energy_tower_skill_effects.yaml differs from the build's export")
        print(f"{len(rows)} energy tower skills agree byte for byte")
    else:
        output.write_text(written)
        print(f"{len(rows)} energy tower skills -> {output.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
