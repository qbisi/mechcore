#!/usr/bin/env python3
"""Extract the build's official English names into `config/names.yaml`.

A battle document names what a side holds rather than numbering it: an
officer, a unit's technologies, a blueprint, an energy tower skill, a
commander skill and an equipment item. The
names are the game's own English localization, the `I2LanguagesForConfigData`
language source in `resources.assets` as `scripts/build_data.py` reads it,
where each configuration row has a term `ConfigData/<Table>/name_<id>`. A name is spelled in snake case: lower case, apostrophes
dropped, and every run of other characters one underscore, so "Vulcan's
Descent" is `vulcans_descent` as a layout already spells it.

Which rows are named is what a standard 1v1 battle can hold: the officers a
reinforcement card grants (`config/reinforce_items.yaml`) and the opening
specialists (`config/advance_teams.yaml`); the commander skills a card, one of
those officers or a blueprint grants, and the equipment a card or one of those
officers hands out; the technologies `config/unit_techs.yaml` lists per unit;
and the blueprints and energy tower skills `config/economy.yaml` lists.

Names within one kind have to be distinct, a technology's within its unit, and
a reinforcement card's across the officers, commander skills and equipment it
can grant, since a card is named by what it grants. One clash is resolved by
rule: a skill a card grants that shares its name with a skill a blueprint
grants takes a `_card` suffix, which makes the card's Mobile Beacon
`mobile_beacon_card`. Any other clash stops the extraction rather than
inventing a spelling.

    uv run --with pyyaml python3 scripts/extract_names.py [--build BUILD]
"""

import collections
from pathlib import Path
import re
import sys

import yaml

import build_data

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "config/names.yaml"
# Technologies are spread over every table whose rows are technologies; these
# two carry some without saying so in their names.
TECHNOLOGY_TABLES = ("SearchTargetSpecificData", "BurrowData")


def language_terms() -> dict[str, list[str]]:
    """Every configuration term, English first."""
    return {term: [english, chinese] for term, (english, chinese) in build_data._terms().items()}


def snake(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", text.lower().replace("'", "")).strip("_")


def named(terms, table: str, row: int) -> str | None:
    value = terms.get(f"ConfigData/{table}/name_{row}")
    return snake(value[0]) if value and value[0] else None


def distinct(rows: dict[int, str], kind: str) -> dict[int, str]:
    """Refuses two rows of one kind that share a name."""
    clash = sorted(name for name, count in collections.Counter(rows.values()).items() if count > 1)
    if clash:
        raise SystemExit(f"{kind} names collide: {clash}")
    return rows


def main():
    build_data.arguments(__doc__)
    terms = language_terms()
    items = yaml.safe_load((ROOT / "config/reinforce_items.yaml").read_text())["items"]
    held = {row["id"] for row in items if row["kind"] == "officer"} | {
        row["id"]
        for row in yaml.safe_load((ROOT / "config/advance_teams.yaml").read_text())["teams"]
        if row["kind"] == "officer"
    }
    officers = {}
    for row in sorted(held):
        name = named(terms, "OfficerData", row)
        if name is None:
            raise SystemExit(f"officer {row} has no English name")
        officers[row] = name
    officers = distinct(officers, "officer")

    economy = yaml.safe_load((ROOT / "config/economy.yaml").read_text())
    delivered = [
        row
        for row in yaml.safe_load((ROOT / "config/officers.yaml").read_text())["officers"]
        if row["id"] in held
    ]
    by_blueprint = {row["grants_skill"] for row in economy["blueprints"] if "grants_skill" in row}
    by_card = {row["id"] for row in items if row["kind"] == "commander_skill"}
    skill_ids = by_blueprint | by_card | {
        skill for row in delivered for skill in row.get("commander_skills", [])
    }
    skill_tables = sorted({term.split("/")[1] for term in terms if term.startswith("ConfigData/CSD_")})
    skills_named = {}
    for skill in sorted(skill_ids):
        found = {named(terms, table, skill) for table in skill_tables} - {None}
        if len(found) != 1:
            raise SystemExit(f"commander skill {skill} has names {found}")
        skills_named[skill] = found.pop()
    blueprint_names = {skills_named[skill] for skill in by_blueprint}
    for skill in sorted(skill_ids - by_blueprint):
        if skill in by_card and skills_named[skill] in blueprint_names:
            skills_named[skill] += "_card"
    commander_skills = distinct(skills_named, "commander skill")

    equipment_ids = {row["id"] for row in items if row["kind"] == "equipment"} | {
        item for row in delivered for item in row.get("equipment", [])
    }
    equipment_tables = sorted(
        {term.split("/")[1] for term in terms if re.match(r"ConfigData/\w*EquipmentData/", term)}
    )
    equipment = {}
    for item in sorted(equipment_ids):
        found = {named(terms, table, item) for table in equipment_tables} - {None}
        if len(found) != 1:
            raise SystemExit(f"equipment {item} has names {found}")
        equipment[item] = found.pop()
    equipment = distinct(equipment, "equipment")
    distinct({**officers, **commander_skills, **equipment}, "reinforcement card")

    tables = [
        table.split("/")[1]
        for table in {"/".join(term.split("/")[:2]) for term in terms}
        if "Tech" in table or table.endswith(TECHNOLOGY_TABLES)
    ]
    technologies = {}
    for unit in yaml.safe_load((ROOT / "config/unit_techs.yaml").read_text())["units"]:
        rows = {}
        for technology in unit["technologies"]:
            found = {named(terms, table, technology["id"]) for table in tables} - {None}
            if len(found) != 1:
                raise SystemExit(f"technology {technology['id']} has names {found}")
            rows[technology["id"]] = found.pop()
        technologies[unit["type"]] = distinct(rows, f"{unit['type']} technology")

    blueprints = {
        row["id"]: named(terms, "BlueprintData", row["id"]) for row in economy["blueprints"]
    }
    skills = {
        row["id"]: named(terms, "EnergyTowerSkillData", row["id"])
        for row in economy["energy_tower_skills"]
    }
    for kind, rows in (("blueprint", blueprints), ("energy tower skill", skills)):
        if None in rows.values():
            raise SystemExit(f"a {kind} has no English name: {rows}")
        distinct(rows, kind)

    lines = [
        "schema: mechcore.names",
        f"game_build: {build_data.build()}",
        "",
        "# The game's English names, in snake case, for what a battle document",
        "# names rather than numbers. Generated by scripts/extract_names.py from",
        "# the I2LanguagesForConfigData language source. Only what a standard",
        "# 1v1 battle can hold is named: the officers a reinforcement card",
        "# grants and the opening specialists, among them. A card's Mobile",
        "# Beacon is mobile_beacon_card beside the blueprint's mobile_beacon.",
        "",
        "officers:",
        *(f"  {row}: {name}" for row, name in sorted(officers.items())),
        "",
        "technologies:",
    ]
    for unit, rows in technologies.items():
        lines.append(f"  {unit}:")
        lines.extend(f"    {row}: {name}" for row, name in sorted(rows.items()))
    lines += ["", "blueprints:"]
    lines += [f"  {row}: {name}" for row, name in sorted(blueprints.items())]
    lines += ["", "energy_tower_skills:"]
    lines += [f"  {row}: {name}" for row, name in sorted(skills.items())]
    lines += ["", "commander_skills:"]
    lines += [f"  {row}: {name}" for row, name in sorted(commander_skills.items())]
    lines += ["", "equipment:"]
    lines += [f"  {row}: {name}" for row, name in sorted(equipment.items())]
    OUTPUT.write_text("\n".join(lines) + "\n")
    print(
        f"{len(officers)} officers, {sum(map(len, technologies.values()))} technologies, "
        f"{len(blueprints)} blueprints, {len(skills)} energy tower skills, "
        f"{len(commander_skills)} commander skills, {len(equipment)} equipment"
    )


if __name__ == "__main__":
    main()
