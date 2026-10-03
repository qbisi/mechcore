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

The table holds the rows the simulator fights: the buff skills whose buff
moves only what the table carries, a slow, a rate on the damage taken,
invincibility and disabled technology, and every
support skill, which `SupportUnitSystem` summons units for, and every shield
skill, whose landing stands a shield of `AdvancedEnergyShieldSystem`, and every
damage skill, which strikes one circle or scatters several. The script
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
# description, story, audio, prices, the rounds a skill can be dealt in, and
# `scope`, when the card may be used), a
# shield's (`energyShieldDamage`, `isCrossAdvancedShield`: a side carrying a
# shield is refused before a skill is released), and what
# `CommanderSkillData.PreProcess` overwrites for a circle (`subEffectCount`,
# `subEffectRange`, `subEffectIntervalTime`). `subEffectDefaultHeight` is where
# a sub-effect's fall stops and the fall starts that much higher, so it moves
# nothing the fight reads.
INTEGERS = (
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
BUFF_RATES = {
    "speedChangeRate": "move_speed_rate",
    "amplifyDamageRate": "amplify_damage_rate",
    "lifeChangeRate": "life_change_rate",
    "stepTime": "step_time",
}
BUFF_FLAGS = (
    ("isAdditiveMode", "additive"),
    ("debuff", "debuff"),
    ("disableTechnology", "disable_technology"),
    ("invincible", "invincible"),
    ("canAffectConstruction", "can_affect_construction"),
    ("canAffectTower", "can_affect_tower"),
)
BUFF_ZERO = (
    "maxLifeChangeRate", "lifeChangeDisposableValue",
    "currentLifeDisposableChangeRate", "speedChangeValue", "attackDurationChangeRate",
    "extraAttackDurationChangeRate", "damageChangeRate",
    "attackRangeChangeValue", "extraAttackRangeChangeValue", "attackRangeChangeRate",
    "extraAttackRangeChangeRate", "summonUnitID", "freeze", "disableRecover",
    "isAdditiveEffect", "isDeadingLeavingRangeItem", "disableSkill",
)


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def reading(value):
    text = f"{value / ONE:.6f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def carried(buff):
    """Whether the table carries every field the buff moves."""
    return not any(raw(buff.get(field, 0)) for field in BUFF_ZERO)


def buff_lines(buff):
    if not carried(buff):
        moving = [field for field in BUFF_ZERO if raw(buff.get(field, 0))]
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


# A damage skill's row. A circle strikes once, and
# `CommanderSkillData.PreProcess` makes its one sub-effect reach `effectRange`,
# which the row then carries as its count and range; a random circle or a line
# scatters `subEffectCount` sub-effects, `subEffectIntervalTime` apart, each
# striking `subEffectRange` about where it lands; one whose `subEffectBuffID`
# names a buff writes it on what it reaches, which the row then carries.
DAMAGE_INTEGERS = (
    ("effectRangeType", "effect_range_type"),
    ("effectType", "effect_type"),
    ("subEffectDamage", "sub_effect_damage"),
    ("subEffectBuffID", "sub_effect_buff_id"),
)
DAMAGE_FIXED = (
    ("startTime", "start_time"),
    ("effectRange", "effect_range"),
    ("subEffectMoveSpeed", "sub_effect_move_speed"),
    ("subEffectMoveTime", "sub_effect_move_time"),
    ("subEffectDefaultHeight", "sub_effect_default_height"),
    ("subEffectIntervalTime", "sub_effect_interval_time"),
)
# `isDirectHit` only rides on the hit's event, which nothing in the fight
# reads.
DAMAGE_FLAGS = (("isCrossAdvancedShield", "cross_advanced_shield"),)


# A terrain skill's row: a line of `subEffectCount` sub-effects, each leaving
# a `RangeItem` of `subEffectRange` where it lands, of the kind its list
# names. `lifeTime` is how long a fire burns, and `effectDuration` the rounds
# the others stand; a fog's `attackRangeChangeRate` is what it does, and an
# acid's or an oil's `buffID` what they write.
TERRAIN_KINDS = (
    ("fireCommanderSkills", "fire"),
    ("oilCommanderSkills", "oil"),
    ("fogCommanderSkills", "fog"),
    ("acidCommanderSkills", "acid"),
)
TERRAIN_INTEGERS = (
    ("effectRangeType", "effect_range_type"),
    ("effectType", "effect_type"),
    ("subEffectCount", "sub_effect_count"),
    ("effectDuration", "effect_duration"),
)
TERRAIN_FIXED = (
    ("startTime", "start_time"),
    ("subEffectRange", "sub_effect_range"),
    ("subEffectMoveSpeed", "sub_effect_move_speed"),
    ("subEffectMoveTime", "sub_effect_move_time"),
    ("subEffectDefaultHeight", "sub_effect_default_height"),
    ("subEffectIntervalTime", "sub_effect_interval_time"),
    ("lifeTime", "life_time"),
    ("fireLifeTime", "fire_life_time"),
    ("attackRangeChangeRate", "attack_range_change_rate"),
)


def terrain_lines(group, buffs):
    lines = ["", "terrain_skills:"]
    for kind_list, kind in TERRAIN_KINDS:
        for row in group[kind_list]:
            if row["isTestData"]:
                continue
            lines += [f"  - id: {row['id']}", f"    name: {row['name']}", f"    kind: {kind}"]
            for field, name in TERRAIN_INTEGERS:
                lines.append(f"    {name}: {row.get(field, 0)}")
            for field, name in TERRAIN_FIXED:
                value = raw(row.get(field, 0))
                lines.append(f"    {name}: {value}{reading(value)}")
            buff = row.get("buffID", 0)
            if buff:
                lines += buff_lines(buffs[buff])
    return lines


def ground_fire_lines():
    """`Config`'s fire: what a burning terrain deals, and how often."""
    config = build_data.level0("Config")
    interval = raw(config["fireAttackInterval"])
    return [
        "",
        "# `Config.groundFireDamage` and `fireAttackInterval`: what a fire deals",
        "# a unit standing in it, and how often.",
        "ground_fire:",
        f"  damage: {config['groundFireDamage']}",
        f"  interval: {interval}{reading(interval)}",
    ]


def damage_lines(group, buffs):
    lines = ["", "damage_skills:"]
    for row in group["damageCommanderSkills"]:
        if row["isTestData"]:
            continue
        circle = row["effectRangeType"] == 0
        lines += [f"  - id: {row['id']}", f"    name: {row['name']}"]
        for field, name in DAMAGE_INTEGERS:
            lines.append(f"    {name}: {row[field]}")
        lines.append(f"    sub_effect_count: {1 if circle else row['subEffectCount']}")
        reach = raw(row["effectRange"] if circle else row["subEffectRange"])
        lines.append(f"    sub_effect_range: {reach}{reading(reach)}")
        for field, name in DAMAGE_FIXED:
            value = raw(row[field])
            lines.append(f"    {name}: {value}{reading(value)}")
        for field, name in DAMAGE_FLAGS:
            lines.append(f"    {name}: {str(row[field]).lower()}")
        if row["subEffectBuffID"]:
            lines += buff_lines(buffs[row["subEffectBuffID"]])
    return lines


# A waypoint skill's row: `CSRC_WayPoint` selects the units within
# `subEffectRange` of the first position and walks them along the path, a
# segment of that width between each two positions. `startTime` and
# `effectRange` are the release's, which the fight does not read.
WAYPOINT_INTEGERS = (
    ("effectTargetType", "effect_target_type"),
    ("subEffectBuffID", "sub_effect_buff_id"),
)
WAYPOINT_FIXED = (("subEffectRange", "sub_effect_range"),)


def waypoint_lines(group):
    lines = ["", "waypoint_skills:"]
    for row in group["wayPointCommanderSkills"]:
        if row["isTestData"]:
            continue
        lines += [f"  - id: {row['id']}", f"    name: {row['name']}"]
        for field, name in WAYPOINT_INTEGERS:
            lines.append(f"    {name}: {row.get(field, 0)}")
        for field, name in WAYPOINT_FIXED:
            value = raw(row[field])
            lines.append(f"    {name}: {value}{reading(value)}")
    return lines


def other_lines(group, written):
    """Every other row, by the list it comes from, which a refusal names."""
    ids = {int(line.split(": ")[1]) for line in written if line.startswith("  - id: ")}
    lines = [
        "",
        "# The skills of every other kind, and the rows of the lists above this",
        "# build does not release, by the CommanderSkillGroupData list each comes",
        "# from.",
        "other_skills:",
    ]
    rows = []
    for kind, entries in group.items():
        if not isinstance(entries, list):
            continue
        for row in entries:
            if isinstance(row, dict) and "id" in row and not row.get("isTestData") and row["id"] not in ids:
                rows.append((row["id"], row["name"], kind))
    for identifier, name, kind in sorted(rows):
        lines.append(f"  - {{id: {identifier}, name: {name}, kind: {kind}}}")
    return lines


def render(group):
    buffs = {buff["id"]: buff for buff in build_data.container()["buffDatas"]}
    rows = [
        row for row in group["buffCommanderSkills"]
        if not row["isTestData"] and carried(buffs[row["subEffectBuffID"]])
    ]
    if not any(row["subEffectBuffID"] == BUFF for row in rows):
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
    lines += damage_lines(group, buffs)
    lines += waypoint_lines(group)
    lines += terrain_lines(group, buffs)
    lines += ground_fire_lines()
    lines += other_lines(group, lines)
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
