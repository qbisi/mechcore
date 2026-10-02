#!/usr/bin/env python3
"""Extract what an EquipmentData row writes onto a unit into `config/equipment_effects.yaml`.

    python3 scripts/extract/extract-equipment-effects.py [--build BUILD] [--check]

The rows are every list of `EquipmentGroupData` in `level0`, as
`scripts/build_data.py` reads them, each with the list it comes from as its
`kind`. Every class is an `EquipmentData`, and `Equipment.AddData` writes its
plain `ICommonMechDataChangeDataSource` correction whatever the class, so
every row carries those fields; what a subclass does beyond them (a buff,
lifesteal, a shield and the rest) is its kind's mechanism. Test rows and rows
limited to Interstellar Expedition are left out. Q32.32 values are written as
raw integers; only the comment beside one reads it as a decimal.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
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
# What a subclass's row answers its own interface with, by the build's field
# and the list of `EquipmentGroupData` whose rows carry it.
SUBCLASS_FIELDS = {
    "lifesteal_multiplier": ("lifestealMultiplier", "lifestealEquipmentDatas"),
    "start_time": ("startTime", "autoRecoveryEquipmentDatas"),
    "recovery_duration": ("recoveryDuration", "autoRecoveryEquipmentDatas"),
    "recovery_life_rate": ("recoveryLifeRate", "autoRecoveryEquipmentDatas"),
    "splash_range": ("range", "splashEquipmentDatas"),
    "shield_life_rate": ("lifeRate", "energyShieldEquipmentDatas"),
    "barrier_radius": ("radius", "advancedEnergyShieldEquipmentDatas"),
    "barrier_energy": ("shieldValue", "advancedEnergyShieldEquipmentDatas"),
}
# The subclass fields that are plain integers rather than FPoint raw values.
SUBCLASS_INTEGERS = {"barrier_radius", "barrier_energy"}


# A buff item's trigger, read by the simulator, and the rest of its
# BuffEquipmentData fields, any of which set is named in `buff_special`. The
# effect's name and whether it follows are what the client shows.
BUFF_TRIGGER = {
    "buff_trigger": "buffTechTrigger", "buff_targets": "effectTargetTypes",
    "probability": "probability",
}
BUFF_ITEM_OTHER = (
    "energyShieldDamageMultiplier", "min", "max", "intervalTime", "delayTime", "targetFlyType",
    "targetDamageDistanceType", "buffTargetUpdateModel", "triggerRangeItemBuffId",
    "triggerRangeItemType", "triggerLifeTime", "triggerRangeItemRange", "triggerRoundDuration",
    "isDistanceCalculateTargetRadius", "isDistanceCalculateSelfRadius",
)
# The fields of the buffDatas row a buff item names that the simulator
# reads; any other set is named in the buff's `special`.
BUFF_READ = {
    "buffDivide": "divide", "isAdditiveMode": "additive", "debuff": "debuff",
    "invincible": "invincible", "disableTechnology": "disable_technology",
    "amplifyDamageRate": "amplify_damage_rate",
    "isClearSelfBuffWhenDisableTech": "clear_when_technologies_disabled",
}
BUFF_DESCRIPTIVE = {"id", "name", "isTestData", "duration", "effectType"}


def set_fields(row, fields):
    return [field for field in fields if raw(row.get(field)) not in (0, False, None, "", [], {})]


def buff_lines(row, buffs):
    """A buff item's trigger and the buffDatas row it adds."""
    lines = [
        f"    buff_trigger: {row['buffTechTrigger']}",
        f"    buff_targets: [{', '.join(map(str, row['effectTargetTypes']))}]",
        f"    probability: {raw(row['probability'])}{reading(raw(row['probability']))}",
    ]
    special = set_fields(row, BUFF_ITEM_OTHER)
    if special:
        lines.append(f"    buff_special: [{', '.join(special)}]")
    buff = buffs[row["buffID"]]
    lines += [
        "    buff:",
        f"      id: {buff['id']}",
        f"      name: {buff['name']}",
        f"      duration: {raw(buff['duration'])}{reading(raw(buff['duration']))}",
    ]
    for field, name in BUFF_READ.items():
        value = raw(buff[field])
        if isinstance(value, bool):
            lines.append(f"      {name}: {str(value).lower()}")
        elif field == "amplifyDamageRate":
            lines.append(f"      {name}: {value}{reading(value) if value else ''}")
        else:
            lines.append(f"      {name}: {value}")
    special = set_fields(buff, [field for field in buff if field not in BUFF_READ and field not in BUFF_DESCRIPTIVE])
    if special:
        lines.append(f"      special: [{', '.join(special)}]")
    return lines


# A production line's SupportUnitEquipmentData: what it makes, how many and
# how often, where around its wearer, and how its makes are corrected.
PRODUCTION = (
    ("support_unit_id", "supportUnitID"), ("unit_level", "unitLevel"),
    ("max_batch", "maxBatch"), ("max_alive", "maxCount"),
    ("create_count_per_time", "createCountPerTime"), ("start_time", "startTime"),
    ("appear_type", "appearType"), ("max_create_count", "maxCreateCount"),
)


def production_lines(row):
    lines = ["    production:"]
    for name, field in PRODUCTION:
        lines.append(f"      {name}: {row[field]}")
    duration = raw(row["createDuration"])
    lines.append(f"      create_duration: {duration}{reading(duration)}")
    rate = raw(row["unitLifeChangerate"])
    lines.append(f"      unit_life_rate: {rate}{reading(rate) if rate else ''}")
    offsets = ", ".join(f"{{x: {raw(p['x'])}, z: {raw(p['y'])}}}" for p in row["positions"])
    lines.append(f"      positions: [{offsets}]")
    return lines


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def extract():
    rows = []
    for kind, entries in build_data.level0("EquipmentGroupData").items():
        if not isinstance(entries, list):
            continue
        for row in entries:
            if row["isTestData"] or not build_data.in_standard(row):
                continue
            entry = {"id": row["id"], "name": row["name"], "kind": kind}
            entry.update({column: raw(row[field]) for column, field in FIELDS.items()})
            entry.update({column: raw(row[field]) for column, (field, owner) in SUBCLASS_FIELDS.items()
                          if kind == owner})
            if kind == "buffEquipmentDatas":
                entry["buff_row"] = row
            if kind == "ignoreBuffEquipmentDatas":
                entry["buff_group"] = row["buffGroup"]
            if kind == "supportUnitEquipmentDatas":
                entry["production"] = row
            rows.append(entry)
    return sorted(rows, key=lambda row: row["id"])


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
    buffs = {buff["id"]: buff for buff in build_data.container()["buffDatas"]}
    groups = {group["id"]: group for group in build_data.container()["buffGroups"]}
    lines = [
        "schema: mechcore.equipment_effects",
        "",
        "# What an EquipmentData row writes onto the unit that wears it, read out",
        "# of EquipmentGroupData by scripts/extract/extract-equipment-effects.py.",
        "# docs/rules/equipment_effects.md states what each field means. A field",
        "# is written only when it is set; what an equipment costs is",
        "# config/reinforce_items.yaml's, and its other pool fields are not here.",
        "#",
        "# A row's `kind` is the list of EquipmentGroupData it comes from:",
        "# `equipmentDatas` for an item that does nothing but correct its unit's",
        "# numbers, and a subclass's list for one that does something more.",
        "# Every class writes the corrections below, whatever else it does, and",
        "# a subclass's row also carries what it answers its own interface with:",
        "# a buff item what triggers it (`buff_trigger`, a BuffTechListener: 1",
        "# is the fight's start), whom it reaches (`buff_targets`, TargetTypes:",
        "# 1 is the unit itself), how likely, and the buffDatas row it adds;",
        "# an anti-interference item the `ignored_buffs` of its buff group;",
        "# a production line the `production` it runs: the unit it makes,",
        "# `max_batch` batches of `create_count_per_time` every `create_duration`",
        "# seconds while fewer than `max_alive` live, each at its `positions`",
        "# offset from its wearer, in FPoint raw metres;",
        "# a shield item its `shield_life_rate`, its shield's share of its",
        "# unit's maximum life; a barrier item the `barrier_radius`, in whole",
        "# metres, and `barrier_energy` of the battlefield shield it carries;",
        "# a lifesteal item its `lifesteal_multiplier`, the share of a hit's",
        "# damage its unit takes back as life, and a repair item the seconds",
        "# hurt before it repairs, the seconds between two repairs and the",
        "# share of maximum life each restores, and a splash item the",
        "# `splash_range` it adds to its unit's main skill.",
        "#",
        "# A rate is an FPoint Q32.32 raw integer: 3221225472 is +0.75. A value is",
        "# an FPoint in the number's own units: 85899345920 is +20 of range.",
        "",
        "equipment:",
    ]
    for d in rows:
        lines.append(f"  - id: {d['id']}")
        lines.append(f"    name: {d['name']}")
        lines.append(f"    kind: {d['kind']}")
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
        if "buff_row" in d:
            lines += buff_lines(d["buff_row"], buffs)
        if "production" in d:
            lines += production_lines(d["production"])
        if "buff_group" in d:
            group = groups[d["buff_group"]]
            lines.append(f"    ignored_buffs: [{', '.join(map(str, group['buffs']))}]  # {group['name']}")
        for field in SUBCLASS_FIELDS:
            if field in d:
                comment = reading(d[field]) if d[field] and field not in SUBCLASS_INTEGERS else ""
                lines.append(f"    {field}: {d[field]}{comment}")
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
