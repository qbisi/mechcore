#!/usr/bin/env python3
"""Extract what a released battle skill does into `config/commander_skill_effects.yaml`.

    python3 scripts/extract/extract-commander-skill-effects.py [--build BUILD] [--check]

Read through `scripts/build_data.py` from the `CommanderSkillGroupData` object
of `level0`. A released skill is `CommanderSkillReleaseState` over its row:
`CSRS_Prepare` waits `startTime - subEffectMoveTime`, `CSRS_Perform` activates
its sub-effect, which falls from `subEffectMoveSpeed x min(startTime,
subEffectMoveTime)` above the ground at that speed, and its landing asks
`CommanderSkillSubEffectController` for what it does. A row of
`buffCommanderSkills` writes the `buffDatas` row its `subEffectBuffID` names on
every unit within `effectRange`, of either side. `docs/rules/battle_skill.md`
states what each field does.

The table holds the rows the simulator fights: the buff skills whose buff is
the Electromagnetic Impact's, a slow that disables technology, and every
support skill, which `SupportUnitSystem` summons units for, and every shield
skill, whose landing stands a shield of `AdvancedEnergyShieldSystem`. The script
refuses a buff row when its buff moves anything else, and a support row that
places its summons at set offsets.

A time, a distance, a speed and a rate are the FPoint Q32.32 raw integers the
build stores, with a comment reading each one; the rest are plain integers.
"""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "config/commander_skill_effects.yaml"
ONE = 1 << 32
# The Electromagnetic Impact's buff, which every row here writes.
BUFF = 200001

# (field in the row, name here). What is left out is the panel's (icon,
# description, story, audio, prices, the rounds a skill can be dealt in), a
# shield's (`energyShieldDamage`, `isCrossAdvancedShield`: a side carrying a
# shield is refused before a skill is released), and what
# `CommanderSkillData.PreProcess` overwrites for a circle (`subEffectCount`,
# `subEffectRange`, `subEffectIntervalTime`). `subEffectDefaultHeight` is where
# a sub-effect's fall stops and the fall starts that much higher, so it moves
# nothing the fight reads.
INTEGERS = (
    ("scope", "scope"),
    ("effectRangeType", "effect_range_type"),
    ("effectType", "effect_type"),
    ("subEffectDamage", "sub_effect_damage"),
)
FIXED = (
    ("startTime", "start_time"),
    ("effectRange", "effect_range"),
    ("subEffectMoveSpeed", "sub_effect_move_speed"),
    ("subEffectMoveTime", "sub_effect_move_time"),
)
# The buff's fields the fight reads; every other rate has to be zero, and the
# script refuses the row when one is not. `isClearSelfBuffWhenDisableTech`
# is about the buff a unit wrote on itself, never one a skill writes.
BUFF_RATES = {"speedChangeRate": "move_speed_rate"}
BUFF_FLAGS = (
    ("isAdditiveMode", "additive"),
    ("disableTechnology", "disable_technology"),
    ("canAffectConstruction", "can_affect_construction"),
    ("canAffectTower", "can_affect_tower"),
)
BUFF_ZERO = (
    "stepTime", "lifeChangeRate", "maxLifeChangeRate", "lifeChangeDisposableValue",
    "currentLifeDisposableChangeRate", "speedChangeValue", "attackDurationChangeRate",
    "extraAttackDurationChangeRate", "damageChangeRate", "amplifyDamageRate",
    "attackRangeChangeValue", "extraAttackRangeChangeValue", "attackRangeChangeRate",
    "extraAttackRangeChangeRate", "summonUnitID", "invincible", "freeze", "disableRecover",
    "isAdditiveEffect", "isDeadingLeavingRangeItem", "disableSkill",
)


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def reading(value):
    text = f"{value / ONE:.6f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def buff_lines(buff):
    moving = [field for field in BUFF_ZERO if raw(buff.get(field, 0))]
    if moving:
        raise SystemExit(f"buff {buff['id']} moves {moving}, which this table does not carry")
    lines = [
        "    buff:",
        f"      id: {buff['id']}",
        f"      name: {buff['name']}",
        f"      divide: {buff.get('buffDivide', 0)}",
        f"      duration: {raw(buff['duration'])}{reading(raw(buff['duration']))}",
    ]
    for field, name in BUFF_FLAGS:
        lines.append(f"      {name}: {str(buff.get(field, False)).lower()}")
    for field, name in BUFF_RATES.items():
        value = raw(buff.get(field, 0))
        lines.append(f"      {name}: {value}{reading(value)}")
    return lines


# A support skill's row: what it summons, how many and how often, and how
# the summons appear. `positions` has to be empty: a row that places its
# summons at set offsets takes another path the fight does not read.
SUPPORT_INTEGERS = (
    ("scope", "scope"),
    ("effectRangeType", "effect_range_type"),
    ("effectType", "effect_type"),
    ("unitID", "unit_type_id"),
    ("maxCount", "max_count"),
    ("createCountPerTime", "create_count_per_time"),
    ("maxBatch", "max_batch"),
    ("appearType", "appear_type"),
)
SUPPORT_FIXED = (
    ("startTime", "start_time"),
    ("effectRange", "effect_range"),
    ("subEffectMoveSpeed", "sub_effect_move_speed"),
    ("subEffectMoveTime", "sub_effect_move_time"),
    ("createInterval", "create_interval"),
)


def support_lines(group):
    lines = ["", "support_skills:"]
    for row in group["supportUnitCommanderSkills"]:
        if row["isTestData"]:
            continue
        if row["positions"]:
            raise SystemExit(f"support skill {row['id']} places its summons at {row['positions']}")
        lines += [f"  - id: {row['id']}", f"    name: {row['name']}"]
        for field, name in SUPPORT_INTEGERS:
            lines.append(f"    {name}: {row[field]}")
        for field, name in SUPPORT_FIXED:
            value = raw(row[field])
            lines.append(f"    {name}: {value}{reading(value)}")
    return lines


# A shield skill's row: when its shield lands and what the shield holds. Its
# `subEffectDefaultHeight` is where the fall stops, which moves nothing the
# fight reads, and `isCrossAdvancedShield` is never read: `CS_EnergyShield`
# crosses shields whatever its row says.
SHIELD_INTEGERS = (
    ("scope", "scope"),
    ("effectRangeType", "effect_range_type"),
    ("effectType", "effect_type"),
    ("energy", "energy"),
)
SHIELD_FIXED = (
    ("startTime", "start_time"),
    ("effectRange", "effect_range"),
    ("subEffectMoveSpeed", "sub_effect_move_speed"),
    ("subEffectMoveTime", "sub_effect_move_time"),
)


def shield_lines(group):
    lines = ["", "shield_skills:"]
    for row in group["energyShieldCommanderSkills"]:
        if row["isTestData"]:
            continue
        lines += [f"  - id: {row['id']}", f"    name: {row['name']}"]
        for field, name in SHIELD_INTEGERS:
            lines.append(f"    {name}: {row[field]}")
        for field, name in SHIELD_FIXED:
            value = raw(row[field])
            lines.append(f"    {name}: {value}{reading(value)}")
    return lines


def render(group):
    buffs = {buff["id"]: buff for buff in build_data.container()["buffDatas"]}
    rows = [
        row for row in group["buffCommanderSkills"]
        if not row["isTestData"] and row["subEffectBuffID"] == BUFF
    ]
    if not rows:
        raise SystemExit(f"no buff commander skill writes buff {BUFF}")
    lines = [
        "schema: mechcore.commander_skill_effects",
        "",
        "# What a released battle skill does, read out of `CommanderSkillGroupData`.",
        "# Generated by scripts/extract/extract-commander-skill-effects.py;",
        "# docs/rules/battle_skill.md states what each field does. A time, a",
        "# distance, a speed or a rate is an FPoint Q32.32 raw integer, read in the",
        "# comment beside it.",
        "",
        "buff_skills:",
    ]
    for row in rows:
        lines += [f"  - id: {row['id']}", f"    name: {row['name']}"]
        for field, name in INTEGERS:
            lines.append(f"    {name}: {row.get(field, 0)}")
        for field, name in FIXED:
            value = raw(row[field])
            lines.append(f"    {name}: {value}{reading(value)}")
        lines += buff_lines(buffs[row["subEffectBuffID"]])
    lines += support_lines(group)
    lines += shield_lines(group)
    return "\n".join(lines) + "\n"


def main():
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    written = render(build_data.level0("CommanderSkillGroupData"))
    if arguments.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != written:
            print("config/commander_skill_effects.yaml differs from the build's export", file=sys.stderr)
            return 1
        print("config/commander_skill_effects.yaml agrees with the build's export byte for byte")
        return 0
    OUTPUT.write_text(written)
    print(f"commander skill effects of build {build_data.build()} -> {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
