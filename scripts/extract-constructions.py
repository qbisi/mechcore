#!/usr/bin/env python3
"""Extract what a construction is into `config/constructions.yaml`.

A construction is not one object. `GroupConstructionManager.CreateConstruction`
builds a `FightConstruction` per child and hands them back as a list, so the
numbers a fight reads are per child — a Defensive Wall's `maxLife` is one
block's, not the wall's. `docs/rules/constructions.md` says what each field
means and which of them this build has measured.

The source is `ConfigDataContainer.constructionDatas`, as `scripts/build_data.py`
reads the build's typed export, so nothing here depends on declaration order.

Two checks stand between the export and the table.

* Every construction the layout catalog names must have a grid whose cells are
  ten metres: `gridColumnCount x 10` by `gridRowCount x 10` has to equal the
  footprint `crates/document/src/catalog.rs` pins, and that footprint was read
  off placements the game accepted rather than out of this object.
* Every row must carry a positive `count` and `maxLife`, because both are per
  child and a zero would mean the fields are not the ones they are named.

A construction a layout can place that deals damage fires a skill, and its
`ProjectileSkillData` row, from `MechSkillGroupData` as `scripts/extract-skills.py`
reads it, is written under `skills` in the shape a unit's `attack` has; the
construction's own `damage` and `attackAngle` are written as its `base_damage`
and `attack_half_angle`, which the loader checks back.

    python3 scripts/extract-constructions.py [--build BUILD]
"""

import importlib.util
import pathlib
import re
import sys

import build_data

ROOT = pathlib.Path(__file__).resolve().parents[1]
CATALOG = ROOT / "crates/document/src/catalog.rs"
OUTPUT = ROOT / "config/constructions.yaml"
ONE = 1 << 32
# The side of one deployment grid cell, in metres. `layout.md` states it as the
# grid a placement is checked against, and it is what turns a row's grid counts
# into the footprint the catalog carries.
CELL = 10

# The fields carried through, in the order they are written. What is left out
# is the client's: icon, portrait, wireframe, prefab, description and story.
INTEGERS = (
    "count",
    "maxLife",
    "damage",
    "durability",
    "blockWidth",
    "space",
    "gridColumnCount",
    "gridRowCount",
    "realRowCount",
    "rotateSpeed",
    "skillID",
    "exp",
    "sellSupply",
    "pathfindingColliderPriority",
    "defaultActorVisibility",
)
FIXED = ("radius", "pathRadius", "attackAngle", "specialDamageReduceRate")
FLAGS = (
    "hasDurability",
    "canBeBought",
    "canBeSold",
    "canBeEffectedByTowerBuff",
    "enableSearchTarget",
    "quickSearchTarget",
)
SNAKE = re.compile(r"(?<!^)(?=[A-Z])")


def snake(name):
    return SNAKE.sub("_", name).lower().replace("i_d", "id")


def catalog_footprints():
    """What the layout catalog names a construction, and the box it reserves.

    The catalog was written from placements the game accepted, so it is an
    independent reading of the same four constructions.
    """
    text = CATALOG.read_text(encoding="utf-8")
    body = text.split("pub(crate) const fn resolve_construction_type", 1)[1].split("}", 1)[0]
    found = {}
    for name, id_, width, height in re.findall(
        r'b"(\w+)" => Some\(construction_spec\((\d+), (\d+), (\d+)\)\)', body
    ):
        found[int(id_)] = (name, int(width), int(height))
    if not found:
        raise SystemExit(f"{CATALOG} lists no construction types")
    return found


def rows():
    return build_data.container()["constructionDatas"]


def skill_reader():
    """`scripts/extract-skills.py`, whose file name is not a module name."""
    spec = importlib.util.spec_from_file_location("extract_skills", ROOT / "scripts/extract-skills.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def skill_rows(entries):
    """The skill each placeable construction that deals damage fires."""
    firing = [body for body in entries if "layout_name" in body and body["damage"] != 0]
    if not firing:
        return []
    reader = skill_reader()
    every = {row["id"]: row for row in reader.rows()}
    skills = {}
    for body in firing:
        row = every.get(body["skill_id"])
        if row is None or row["kind"] != "projectileSkillDatas":
            raise SystemExit(
                f"construction {body['id']} ({body['layout_name']}) fires skill "
                f"{body['skill_id']}, which is not a ProjectileSkillData row"
            )
        skills[row["id"]] = attack(row, body)
    return [skills[id_] for id_ in sorted(skills)]


# `GameRiver.Fight.WeaponMode`.
WEAPON_MODES = {0: "normal", 1: "group", 2: "standalone"}


def number(value):
    """An FPoint as the decimal it was authored as: 0.2999999998 is 0.3."""
    rounded = round(value, 6)
    return int(rounded) if rounded == int(rounded) else rounded


def attack(row, body):
    return {
        "id": row["id"],
        "name": row["name"],
        "attack": {
            "base_damage": body["damage"],
            "min_range": number(row["minAttackRange"]),
            "range": number(row["attackRange"]),
            "attack_half_angle": number(body["attack_angle"] / ONE),
            "targets": {"ground": row["canAttackGround"], "air": row["canAttackAir"]},
            "lock_target": row["isLockTarget"],
            "quick_switch_target": row["enableQuickSwitchTarget"],
            "timing": {
                "interval": number(row["attackDuration"]),
                "interval_offset": number(row["attackDurationRandomValue"]),
                "initial_cooldown": number(row["initialCoolDownTime"]),
                "prepare": number(row["prepareTime"]),
                "attack_point": number(row["attackPoint"]),
                "backswing": number(row["attackBackswing"]),
                "cooling": number(row["coolingTime"]),
            },
            "splash_radius": number(row["splashRange"]),
            "weapons": {
                "mode": WEAPON_MODES[row["weaponMode"]],
                "count": len(row["weapons"]),
                "per_skill": row["weaponCountPerSkill"],
            },
            "melee": row["isMeleeAttack"],
            "path": {
                "type": "projectile",
                "count": row["projectileCount"],
                "release_interval": number(row["projectileDuration"]),
                "speed": number(row["bulletSpeed"]),
                "target_offset_radius": number(row["randomTargetRange"]),
                "evenly_allocate_targets": row["isEvenlyAllocated"],
                "extra_search_range": number(row["extraSearchRange"]),
                "pre_flight_height": number(row["preFlyHeight"]),
                "simulated_motion": row["isSimulateMode"],
                "interceptible": row["canBeIntercept"],
                "max_life": (row["maxLife"] or [0])[0],
            },
            **(
                {"magazine": {"capacity": row["loadingCapacity"], "reload": number(row["reloadingTime"])}}
                if row["isLoadingType"]
                else {}
            ),
        },
    }


def render_mapping(value, indent):
    """A nested mapping as block YAML, with two small mappings kept inline."""
    lines = []
    for key, item in value.items():
        if isinstance(item, dict) and key in ("targets", "magazine"):
            inline = ", ".join(f"{k}: {scalar(v)}" for k, v in item.items())
            lines.append(f"{indent}{key}: {{{inline}}}")
        elif isinstance(item, dict):
            lines.append(f"{indent}{key}:")
            lines.extend(render_mapping(item, indent + "  "))
        else:
            lines.append(f"{indent}{key}: {scalar(item)}")
    return lines


def check(row, named):
    """Refuse a row this script has not understood, naming the construction."""
    id_ = row["id"]
    if row["count"] <= 0 or row["maxLife"] <= 0:
        raise SystemExit(
            f"construction {id_} ({row['name']}) carries count {row['count']} and "
            f"maxLife {row['maxLife']}; both are per child and must be positive"
        )
    if id_ not in named:
        return
    layout_name, width, height = named[id_]
    grid = (row["gridColumnCount"] * CELL, row["gridRowCount"] * CELL)
    if grid != (width, height):
        raise SystemExit(
            f"construction {id_} ({layout_name}) has a {grid[0]}x{grid[1]} grid and "
            f"the catalog reserves {width}x{height}"
        )


def entry(row, named):
    body = {"id": row["id"], "name": row["name"]}
    if row["id"] in named:
        body["layout_name"] = named[row["id"]][0]
    for field in INTEGERS:
        body[snake(field)] = row[field]
    for field in FIXED:
        body[snake(field)] = row[field]["m_rawValue"]
    for field in FLAGS:
        body[snake(field)] = row[field]
    return body


def scalar(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str) and (value == "" or re.search(r"[:#]|^\s|\s$", value)):
        return '"' + value.replace('"', '\\"') + '"'
    return str(value)


def render(entries, skills):
    lines = [
        "schema: mechcore.constructions",
        "",
        "# What a construction is, read out of `ConfigDataContainer.constructionDatas`.",
        "# `docs/rules/constructions.md` states what each field means and which of",
        "# them this build has measured against the game.",
        "#",
        "# **Every number here is one child's.** A construction is built as `count`",
        "# `FightConstruction` objects and a recording holds each of them as its own",
        "# building, so `max_life` is one block's life and a Defensive Wall's five",
        "# blocks hold 5560 between them.",
        "#",
        "# `layout_name` is present on the four a layout can place. The rest are the",
        "# build's own rows, carried because leaving them out would make the table",
        "# look like the whole of what the build holds.",
        "#",
        "# A fixed-point field is an FPoint Q32.32 raw integer; `radius` is half the",
        "# bounding box a recording reports.",
        "",
        "constructions:",
    ]
    for body in entries:
        first = True
        for key, value in body.items():
            prefix = "  - " if first else "    "
            comment = ""
            if key in ("radius", "path_radius") and value % ONE == 0:
                comment = f"  # {value // ONE} m"
            lines.append(f"{prefix}{key}: {scalar(value)}{comment}")
            first = False
    if skills:
        lines += [
            "",
            "# What a construction's skill does, for the constructions that fire one.",
            "#",
            "# Each is the `ProjectileSkillData` row the construction's `skill_id` names,",
            "# read out of `MechSkillGroupData` by `scripts/extract-skills.py`, in the",
            "# shape `config/units/*.yaml` gives a",
            "# unit's `attack`. Two numbers are the construction row's rather than the",
            "# skill's, and the loader refuses a table where they disagree: `base_damage`",
            "# is the row's `damage` (a skill row carries none), and `attack_half_angle`",
            "# is the row's `attack_angle`. `magazine` is the row's `loadingCapacity` and",
            "# `reloadingTime`; `docs/rules/turrets.md` says what each was measured",
            "# against.",
            "skills:",
        ]
        for skill in skills:
            lines.append(f"  - id: {skill['id']}")
            lines.append(f"    name: {scalar(skill['name'])}")
            lines.append("    attack:")
            lines.extend(render_mapping(skill["attack"], "      "))
    return "\n".join(lines) + "\n"


def main():
    build_data.arguments(__doc__)
    named = catalog_footprints()
    entries = []
    for row in sorted(rows(), key=lambda row: row["id"]):
        check(row, named)
        entries.append(entry(row, named))
    skills = skill_rows(entries)
    OUTPUT.write_text(render(entries, skills), encoding="utf-8")
    print(
        f"{OUTPUT}: {len(entries)} constructions, {len(named)} checked against the catalog, "
        f"{len(skills)} skills"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
