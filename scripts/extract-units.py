#!/usr/bin/env python3
"""Extract every unit configuration under `config/units/` from the build.

    python3 scripts/extract-units.py [--build BUILD] [--check]

A unit's configuration joins four of the build's tables, as
`scripts/build_data.py` reads them:

* `ConfigDataContainer.mechDatas`: life, damage, collision radius, move and
  rotate speed, whether it flies and whether it has a body, and `mainSkillID`.
* `ConfigDataContainer.cardDatas`, the row whose `mechID` is the unit: how many
  members a formation has, their slot size and the formation's footprint.
* `MechSkillGroupData`, the main skill's row, and the list it is in, which is
  the attack's path: `skillDatas` direct, `projectileSkillDatas` projectile,
  `laserSkillDatas` laser, `controllBeamSkillDatas` control beam.
* The unit's prefab `Mech_Default_<id>` in `sharedassets0`, whose
  `RVOControllerFixed` sizes its avoidance.

Which units are written is which files `config/units/` holds; each file's
`type_name` has to be the snake case of the unit's official English name. A
unit the build fields in a standard match but that has no file is listed, not
written: adding one is a decision, not an extraction. A main skill the
simulator's shape cannot state (a loading skill, an initial cooldown, an
unknown kind of path) stops the script for that unit.
"""

import pathlib
import re
import struct
import sys

import build_data

ROOT = pathlib.Path(__file__).resolve().parents[1]
UNITS = ROOT / "config" / "units"
ONE = 1 << 32
SIZES = {0: "xs", 1: "s", 2: "m", 3: "l", 4: "xl", 5: "xxl"}


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def boolean(value):
    return "true" if value else "false"


def grid(value, scale):
    """An FPoint on a 1/scale grid, written as the shortest decimal."""
    number = round(raw(value) * scale / ONE) / scale
    return str(int(number)) if number == int(number) else f"{number:.6f}".rstrip("0").rstrip(".")


def ratio(value):
    return f"{raw(value) / ONE:.11f}".rstrip("0").rstrip(".")


def readable(value):
    """The shortest decimal whose Q32.32 truncation is the raw value."""
    for digits in range(10):
        text = f"{round(raw(value) / ONE, digits):.{digits}f}"
        if int(float(text) * ONE) == raw(value):
            return text
    raise SystemExit(f"no decimal reads back as {raw(value)}")


def single(value):
    text = f"{value:.8g}"
    if struct.pack("<f", float(text)) != struct.pack("<f", value):
        raise SystemExit(f"{value} does not round-trip as a single")
    return text


def snake(name):
    return re.sub(r"[^a-z0-9]+", "_", name.lower().replace("'", "")).strip("_")


def skill_rows():
    rows = {}
    for kind, table in build_data.level0("MechSkillGroupData").items():
        if isinstance(table, list):
            for row in table:
                rows[row["id"]] = (kind, row)
    return rows


def refuse(unit, reason):
    raise SystemExit(f"unit {unit}: {reason}")


def render(mech, card, kind, skill, rvo, type_name):
    unit = mech["id"]
    if card["specialUnit"] > 0 or card["isTestUnit"]:
        refuse(unit, "is a special or test unit")
    if raw(skill["damageRate"]) != ONE or skill["damage"]:
        refuse(unit, "main skill carries its own damage")
    for field in ("initialCoolDownTime",):
        if raw(skill[field]):
            refuse(unit, f"main skill has {field}")
    for field in ("isLoadingType", "isDiffusion", "useSelfSplash", "useDefaultRotationSearchTarget"):
        if skill[field]:
            refuse(unit, f"main skill sets {field}")
    if mech["moveType"] != 0:
        refuse(unit, "moves other than on the ground grid")
    if any((weapon["defaultAngle"], weapon["rotateAngleLeft"], weapon["rotateAngleRight"]) != (0, -1, -1)
           for weapon in skill["weapons"]):
        refuse(unit, "a weapon has a limited rotation")
    lines = [
        "schema: mechcore.unit",
        f"type_name: {type_name}",
        f"unit_type_id: {unit}",
        "",
        "formation:",
        f"  members: {card['mechCount']}",
        f"  slot_size: {card['slotSize']}",
        f"  footprint: {{width: {grid(card['cardBaseSize']['x'], 1000)}, depth: {grid(card['cardBaseSize']['y'], 1000)}}}",
        "",
        f"domain: {'air' if mech['isFly'] else 'ground'}",
        f"max_life: {mech['life']}",
        f"collision_radius: {grid(mech['radius'], 1000)}",
        f"move_speed: {mech['moveSpeed']}",
        f"rotate_speed: {grid(mech['rotateSpeed'], 1000)}",
        f"has_body: {boolean(mech['isHaveBody'])}",
    ]
    if mech["isHaveBody"]:
        lines.append("independent_aim: false")
    lines += [
        f"rvo: {{outer_radius: {grid(rvo['radiusOuter'], 1000)}, inner_radius: {grid(rvo['radiusInner'], 1000)}, "
        f"size: {SIZES[rvo['size']]}, collider_priority: {rvo['colliderPriority']}, priority: {readable(rvo['priority'])}}}",
        "",
        "attack:",
        f"  base_damage: {mech['damage']}",
        f"  min_range: {grid(skill['minAttackRange'], 1000)}",
        f"  range: {grid(skill['attackRange'], 1000)}",
        f"  attack_half_angle: {grid(mech['attackAngle'], 1000)}",
        f"  targets: {{ground: {boolean(skill['canAttackGround'])}, air: {boolean(skill['canAttackAir'])}}}",
        f"  lock_target: {boolean(skill['isLockTarget'])}",
        f"  quick_switch_target: {boolean(skill['enableQuickSwitchTarget'])}",
        "  timing:",
        f"    interval: {grid(skill['attackDuration'], 2000)}",
        f"    interval_offset: {grid(skill['attackDurationRandomValue'], 2000)}",
        "    initial_cooldown: 0",
        f"    prepare: {grid(skill['prepareTime'], 2000)}",
        f"    attack_point: {grid(skill['attackPoint'], 2000)}",
        f"    backswing: {grid(skill['attackBackswing'], 2000)}",
        f"    cooling: {grid(skill['coolingTime'], 2000)}",
        f"  splash_radius: {grid(skill['splashRange'], 1000)}",
        "  weapons:",
        f"    mode: {'group' if skill['weaponMode'] == 1 else 'normal'}",
        f"    indices: [{', '.join(str(weapon['index']) for weapon in skill['weapons'])}]",
        f"    per_skill: {skill['weaponCountPerSkill']}",
    ]
    if skill["weaponMode"] == 1:
        lines += [
            f"    fusillade: {boolean(skill['isFusillade'])}",
            f"    allow_same_target: {boolean(skill['canAttackSameTarget'])}",
        ]
    if raw(skill["extraWeaponRotateSpeed"]):
        lines.append(f"    rotation_speed: {grid(skill['extraWeaponRotateSpeed'], 1000)}")
    lines += [f"  melee: {boolean(skill['isMeleeAttack'])}", "  path:"]
    if kind == "projectileSkillDatas":
        life = skill["maxLife"] or [0]
        lines += [
            "    type: projectile",
            f"    count: {skill['projectileCount']}",
            f"    release_interval: {grid(skill['projectileDuration'], 2000)}",
            f"    speed: {grid(skill['bulletSpeed'], 1000)}",
            f"    target_offset_radius: {grid(skill['randomTargetRange'], 1000)}",
            f"    evenly_allocate_targets: {boolean(skill['isEvenlyAllocated'])}",
            f"    extra_search_range: {grid(skill['extraSearchRange'], 1000)}",
            f"    pre_flight_height: {grid(skill['preFlyHeight'], 1000)}",
            f"    simulated_motion: {boolean(skill['isSimulateMode'])}",
            f"    interceptible: {boolean(skill['canBeIntercept'])}",
            f"    max_life: {life[0]}",
        ]
    elif kind == "skillDatas":
        lines.append("    type: direct")
    elif kind == "laserSkillDatas":
        lines += [
            "    type: laser",
            f"    damage_multipliers: [{', '.join(ratio(value) for value in skill['damageMultiplier'])}]",
        ]
    elif kind == "controllBeamSkillDatas":
        lines += [
            "    type: control_beam",
            f"    warmup_attack_count: {skill['prepareAttackCount']}",
            f"    warmup_damage_multiplier: {single(skill['prepareAttackDamageMultiplier'])}",
        ]
    else:
        refuse(unit, f"main skill is a {kind} row")
    return "\n".join(lines) + "\n"


def main():
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    structure = build_data.container()
    mechs = {row["id"]: row for row in structure["mechDatas"]}
    cards = {row["mechID"]: row for row in structure["cardDatas"]}
    skills = skill_rows()
    names = build_data.names("MechData")
    rvos = build_data.shared("RVOControllerFixed")

    files = {}
    for path in sorted(UNITS.glob("*.yaml")):
        match = re.search(r"^unit_type_id: (\d+)$", path.read_text(), re.M)
        files[int(match.group(1))] = path
    written, differing = 0, []
    for unit, path in sorted(files.items()):
        type_name = snake(names[unit]["en"])
        if path.stem != type_name:
            raise SystemExit(f"{path.name} holds unit {unit}, whose English name is {names[unit]['en']!r}")
        kind, skill = skills[mechs[unit]["mainSkillID"]]
        text = render(mechs[unit], cards[unit], kind, skill, rvos[f"Mech_Default_{unit}"], type_name)
        if arguments.check:
            if path.read_text() != text:
                differing.append(path.name)
        else:
            path.write_text(text)
            written += 1
    unconfigured = [
        f"{unit} {names.get(unit, {}).get('en', '?')}"
        for unit, card in sorted(cards.items())
        if unit not in files and build_data.in_standard(card) and not card["isTestUnit"]
        and card["specialUnit"] <= 0 and unit in mechs and unit < 1000
    ]
    if unconfigured:
        print(f"units the build fields with no file under config/units: {', '.join(unconfigured)}")
    if arguments.check:
        if differing:
            print(f"differ from build {build_data.build()}: {', '.join(differing)}", file=sys.stderr)
            return 1
        print(f"{len(files)} unit configurations agree with build {build_data.build()}")
        return 0
    print(f"{written} unit configurations from build {build_data.build()} -> config/units/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
