#!/usr/bin/env python3
"""Extract every unit configuration under `config/units/` from the build.

    python3 scripts/extract/extract-units.py [--build BUILD] [--check]

A unit's configuration joins four of the build's tables, as
`scripts/build_data.py` reads them:

* `ConfigDataContainer.mechDatas`: life, damage, collision radius, move and
  rotate speed, whether it flies and whether it has a body, `mainSkillID`, and
  for a unit that moves underground the four numbers its move ability reads.
  `isFreeMove` is not read from the table, where it is false for every unit:
  `MechData.PreProcess` sets it on load for the ids in `FREE_MOVE_IDS`.
* `ConfigDataContainer.cardDatas`, the row whose `mechID` is the unit: how many
  members a formation has, their slot size and the formation's footprint.
* `MechSkillGroupData`, the main skill's row, and the list it is in, which is
  the attack's path: `skillDatas` direct, `projectileSkillDatas` projectile,
  `laserSkillDatas` laser, `controllBeamSkillDatas` control beam.
* The unit's prefab, `MechData.prefabName` in `sharedassets0`, whose
  `RVOControllerFixed` sizes its avoidance: `Mech_Default_<id>` for every unit
  but the Mountain, whose is `Mech_Default_51`.

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

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402
import yaml  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
UNITS = ROOT / "config" / "units"
UNIT_TECHS = ROOT / "config" / "unit_techs.yaml"
ONE = 1 << 32
SIZES = {0: "xs", 1: "s", 2: "m", 3: "l", 4: "xl", 5: "xxl"}
# `MechData.PreProcess` sets `isFreeMove` for an id of at most 54 whose bit is
# set in the literal 0x40000000040010: the Melting Point, the Wraith and the
# id 54 unit. The table's own column is false for every unit.
FREE_MOVE_MASK = 0x40000000040010



# `MechData.mechType`, a `UnitType`: what `UnitUtility.IsEffectTarget` reads
# for a row targeting small, medium or huge units.
UNIT_SIZES = {0: "small", 1: "medium", 2: "huge"}

def free_move(unit):
    return unit <= 54 and bool(FREE_MOVE_MASK >> unit & 1)


# `MechMoveType`: `MoveAbility.Create` makes an `UndergroundMoveAbility` for
# `Underground` and a `CloakMoveAbility` for `Cloak`, which no unit's row sets.
MOVE_NORMAL, MOVE_UNDERGROUND = 0, 1


# `FightWeapon`'s constructor gives each weapon of the unit whose data is 27,
# the Raiden, a transform of its own of `RotateType.Fixed`, parented to the
# unit's own. No table column says so.
FIXED_TO_BODY_UNIT = 27
WEAPON_MODES = {0: "normal", 1: "group", 2: "standalone"}
# `WeaponMountNode`, which the skill's `weaponMountNode` names: what a weapon
# that turns within an arc turns about. `Default` is written as nothing.
WEAPON_MOUNTS = {1: "mech", 2: "mech_body"}


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


def render(mech, card, kind, skill, rvo, type_name, extra_weapons):
    unit = mech["id"]
    if card["specialUnit"] > 0 or card["isTestUnit"]:
        refuse(unit, "is a special or test unit")
    if raw(skill["damageRate"]) != ONE or skill["damage"]:
        refuse(unit, "main skill carries its own damage")
    for field in ("initialCoolDownTime",):
        if raw(skill[field]):
            refuse(unit, f"main skill has {field}")
    for field in ("isLoadingType", "isDiffusion", "useSelfSplash"):
        if skill[field]:
            refuse(unit, f"main skill sets {field}")
    if skill["weaponMode"] not in WEAPON_MODES:
        refuse(unit, f"main skill has weapon mode {skill['weaponMode']}")
    if mech["moveType"] not in (MOVE_NORMAL, MOVE_UNDERGROUND):
        refuse(unit, "moves cloaked")
    lines = [
        "schema: mechcore.unit",
        f"type_name: {type_name}",
        f"unit_type_id: {unit}",
        f"main_skill: {skill['id']}",
        "",
        "formation:",
        f"  members: {card['mechCount']}",
        f"  slot_size: {card['slotSize']}",
        f"  footprint: {{width: {grid(card['cardBaseSize']['x'], 1000)}, depth: {grid(card['cardBaseSize']['y'], 1000)}}}",
        "",
        f"domain: {'air' if mech['isFly'] else 'ground'}",
        f"size: {UNIT_SIZES[mech['mechType']]}",
        f"max_life: {mech['life']}",
        f"collision_radius: {grid(mech['radius'], 1000)}",
        f"move_speed: {mech['moveSpeed']}",
        f"rotate_speed: {grid(mech['rotateSpeed'], 1000)}",
        f"has_body: {boolean(mech['isHaveBody'])}",
    ]
    if free_move(unit):
        lines.append("free_move: true")
    if mech["isHaveBody"]:
        lines.append("independent_aim: false")
    if mech["isEnableMechSearchTarget"]:
        lines.append("mech_search: true")
    lines += [
        f"rvo: {{outer_radius: {grid(rvo['radiusOuter'], 1000)}, inner_radius: {grid(rvo['radiusInner'], 1000)}, "
        f"size: {SIZES[rvo['size']]}, collider_priority: {rvo['colliderPriority']}, priority: {readable(rvo['priority'])}}}",
    ]
    if mech["moveType"] == MOVE_UNDERGROUND:
        lines += [
            "",
            "underground:",
            f"  enter: {grid(mech['moveAbilityEnterTime'], 2000)}",
            f"  exit: {grid(mech['moveAbilityExitTime'], 2000)}",
            f"  exit_keep: {grid(mech['moveAbilityExitKeepEffectTime'], 2000)}",
            f"  attack_range: {grid(mech['underGroundExitRange'], 1000)}",
        ]
    lines += ["", "attack:"]
    lines += attack_lines(unit, kind, skill, f"base_damage: {mech['damage']}", mech["attackAngle"], "  ")
    extras = []
    for technology, (extra_kind, extra_skill, row) in extra_weapons.items():
        written = extra_weapon_lines(mech, technology, extra_kind, extra_skill, row)
        if written:
            extras += written
    if extras:
        lines += ["", "extra_weapons:"] + extras
    return "\n".join(lines) + "\n"


def extra_weapon_lines(mech, technology, kind, skill, row):
    """An extra weapon technology's skill, when the simulator's shape can state it.

    `ExtraWeaponTech` adds the row's `skillID` beside the unit's main skill
    (`ExtraSkillSystem.AddMech`). A row whose hit leaves a fire states how long
    the fire burns (`fireLifeTime`); a row that leaves another terrain, writes
    a buff, changes a shield's damage or reduces every weapon's damage is left
    out, and the simulator refuses the technology by name.
    """
    # `rangeItemType` -1 leaves nothing, 0 a fire; `energyShieldDamage` -1
    # leaves a shield's damage as it is.
    fire = row.get("rangeItemType", -1) == 0
    if (row.get("rangeItemType", -1) not in (-1, 0) or row.get("buffID")
            or row.get("energyShieldDamage", -1) != -1
            or (not fire and any(raw(value) for value in row.get("fireLifeTime") or []))
            or raw(row.get("fogAttackRangeChangeRate")) or raw(row.get("allWeaponReduceDamageRate"))):
        return []
    # A skill with no damage rate deals its own damage, one entry a level, or
    # none when its row lists none; one whose damage is the unit's times its
    # rate is not stated here yet.
    if raw(skill["damageRate"]):
        return []
    if raw(skill["initialCoolDownTime"]) or any(
            skill[field] for field in ("isLoadingType", "isDiffusion", "useSelfSplash")):
        return []
    try:
        attack = attack_lines(mech["id"], kind, skill, f"base_damage: {(skill['damage'] or [0])[0]}",
                              skill["canAttackAngle"], "      ", angle_absent=360 * ONE)
    except SystemExit:
        return []
    lines = [
        f"  - technology: {technology}",
        f"    skill: {skill['id']}",
        f"    use_main_skill_range: {boolean(row.get('useMainSkillRange', False))}",
        f"    damage_by_level: [{', '.join(str(value) for value in skill['damage'])}]",
    ]
    if fire:
        life = ", ".join(str(grid(value, 2000)) for value in row["fireLifeTime"])
        lines.append(f"    fire: {{life_time: [{life}]}}")
    return lines + ["    attack:"] + attack


def attack_lines(unit, kind, skill, damage_line, attack_angle, indent, angle_absent=None):
    """A skill's attack, as `config/units/` states it, each line indented.

    `FightSkill.Init` takes a skill's attack angle from its own row where the
    row sets one, and otherwise an extra skill's is the full circle and a main
    skill's is its owner's: `attack_angle` is the owner's for a main skill,
    and `angle_absent` what an extra skill without one of its own reads.
    """
    if angle_absent is not None and raw(attack_angle) <= 0:
        attack_angle = {"m_rawValue": angle_absent}
    lines = [
        f"  {damage_line}",
        f"  min_range: {grid(skill['minAttackRange'], 1000)}",
        f"  range: {grid(skill['attackRange'], 1000)}",
        f"  attack_half_angle: {grid(attack_angle, 1000)}",
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
        f"    mode: {WEAPON_MODES[skill['weaponMode']]}",
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
    if unit == FIXED_TO_BODY_UNIT:
        lines.append("    fixed_to_body: true")
    # A weapon whose arc is no wider than its rest straight ahead points as
    # its unit does, as one that turns freely from it does when nothing else
    # turns it: `FightWeapon` gives it no transform of its own.
    fixed = [(weapon["defaultAngle"], weapon["rotateAngleLeft"], weapon["rotateAngleRight"]) == (0, 0, 0)
             for weapon in skill["weapons"]]
    if any(fixed) and not all(fixed):
        refuse(unit, "fixes some weapons straight ahead and not others")
    if skill["weaponMountNode"]:
        lines.append(f"    mount: {WEAPON_MOUNTS[skill['weaponMountNode']]}")
    if not all(fixed) and any((weapon["defaultAngle"], weapon["rotateAngleLeft"], weapon["rotateAngleRight"])
                              != (0, -1, -1) for weapon in skill["weapons"]):
        lines.append("    arcs:")
        for weapon in skill["weapons"]:
            arc = [f"default: {weapon['defaultAngle']}"]
            for side, field in (("left", "rotateAngleLeft"), ("right", "rotateAngleRight")):
                if weapon[field] >= 0:
                    arc.append(f"{side}: {weapon[field]}")
            lines.append(f"      - {{{', '.join(arc)}}}")
    lines += [
        f"  melee: {boolean(skill['isMeleeAttack'])}",
        f"  crosses_shields: {boolean(skill['canCrossAdvancedShield'])}",
    ]
    if skill["useDefaultRotationSearchTarget"]:
        lines.append("  default_rotation_search: true")
    lines.append("  path:")
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
    elif kind == "sweepSkillDatas":
        if len(skill["unitRadiusList"]) != len(skill["maxDamageTimesList"]):
            refuse(unit, "sweep lists a hit cap for other than each unit radius")
        lines += [
            "    type: sweep",
            f"    perpendicular: {boolean(skill['isPerpendicular'])}",
            f"    length: {skill['sweepLength']}",
            f"    width: {skill['sweepWidth']}",
            f"    sweeps: {skill['sweepTimes']}",
            f"    damage_times: {skill['damageTimes']}",
            f"    damage_interval: {readable(skill['damageInteval'])}",
            f"    damage_delay: {readable(skill['damageDelay'])}",
            "    hit_caps:",
        ]
        for radius, hits in zip(skill["unitRadiusList"], skill["maxDamageTimesList"]):
            lines.append(f"      - {{radius: {readable(radius)}, hits: {hits}}}")
    else:
        refuse(unit, f"main skill is a {kind} row")
    return [indent + line[2:] for line in lines]


def main():
    arguments = build_data.arguments(__doc__, lambda parser: parser.add_argument("--check", action="store_true"))
    structure = build_data.container()
    mechs = {row["id"]: row for row in structure["mechDatas"]}
    cards = {row["mechID"]: row for row in structure["cardDatas"]}
    skills = skill_rows()
    names = build_data.names("MechData")
    extra_rows = {row["id"]: row for row in build_data.level0("TechnologyGroupData")["extraWeaponTechnologies"]
                  if not row.get("isTestData") and build_data.in_standard(row)}
    researched = {}
    for entry in yaml.safe_load(UNIT_TECHS.read_text())["units"]:
        researched[entry["unit_id"]] = [tech["id"] for tech in entry["technologies"]]
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
        extra_weapons = {
            technology: (*skills[extra_rows[technology]["skillID"]], extra_rows[technology])
            for technology in researched.get(unit, []) if technology in extra_rows
        }
        text = render(mechs[unit], cards[unit], kind, skill, rvos[mechs[unit]["prefabName"]], type_name,
                      extra_weapons)
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
