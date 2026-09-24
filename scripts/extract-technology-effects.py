#!/usr/bin/env python3
"""Extract what a technology does to a fight into `config/technology_effects.yaml`.

`config/unit_techs.yaml` carries which technologies a unit may research and what
each costs. This carries the other half: the corrections one writes onto its
unit's numbers.

The fields are the ones `GameRiver.TechnologyData` answers
`ICommonMechDataChangeDataSource` with — the interface `OfficerData`,
`TechnologyData`, `EquipmentData` and `EnergyTowerSkillData` all implement,
which is why `config/officer_effects.yaml` has the same shape and why
`docs/rules/officer_effects.md`'s composition rule is this table's too.

**A technology's effect is a list indexed by rank.** `lifeChangeRate` and its
neighbours are `List<FPoint>` rather than one value, and Elite Marksman's
`+5` of range and `+0.25` of damage per rank arrive as nine ascending entries.
A technology whose effect does not grow carries a single entry.

The source is every list of `TechnologyGroupData` in `level0`, as
`scripts/build_data.py` reads the build's typed export; a subclass's row
(`BuffTechnologyData`, `SplashTechnologyData` and the rest) carries the same
fields. The technologies read are the ones `config/unit_techs.yaml` lists.

Every number the table states has to be in the technology's own English
description, as the build localizes it with its placeholders filled; and a
list that grows with rank has to be its first entry times the rank.

    python3 scripts/extract-technology-effects.py [--build BUILD]
"""

import pathlib
import re
import sys

import build_data

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = REPOSITORY / "config/technology_effects.yaml"
UNIT_TECHS = REPOSITORY / "config/unit_techs.yaml"
ONE = 1 << 32

# The effect lists this table carries, by the build's field. A
# `speedChangeValue` is a plain integer for the same reason an officer's is:
# the build keeps it in `DataSet.intDatas`.
LISTS = (
    ("life_rate", "lifeChangeRate"),
    ("damage_rate", "damageChangeRate"),
    ("speed_value", "speedChangeValue"),
    ("min_attack_range_value", "minAttackRangeChangeValue"),
    ("attack_range_value", "attackRangeChangeValue"),
    ("attack_range_rate", "attackRangeChangeRate"),
    ("attack_interval_value", "attackIntervalChangeValue"),
    ("attack_interval_rate", "attackIntervalChangeRate"),
    ("splash_range_value", "splashRangeChangeValue"),
    ("projectile_speed_value", "projectileSpeedChangeValue"),
    ("projectile_life_rate", "projectileLifeChangeRate"),
)
RATES = {"life_rate", "damage_rate", "attack_range_rate", "attack_interval_rate", "projectile_life_rate"}
INTEGERS = {"speed_value", "min_attack_range_value"}


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def rows_by_id() -> dict[int, dict]:
    """Every technology row of every list, by id."""
    rows = {}
    for table in build_data.level0("TechnologyGroupData").values():
        if isinstance(table, list):
            for row in table:
                effect = {"id": row["id"], "name": row["name"], "row": row}
                for field, source in LISTS:
                    effect[field] = [raw(value) for value in row.get(source) or []]
                rows[row["id"]] = effect
    return rows


def described(row: dict) -> str | None:
    """A technology's English description, from whichever technology table localizes it."""
    for term in build_data._terms():
        match = re.fullmatch(r"ConfigData/(\w+)/description_(\d+)", term)
        if match and int(match.group(2)) == row["id"] and (
                "Tech" in match.group(1) or match.group(1) in ("SearchTargetSpecificData", "BurrowData")):
            return build_data.description(match.group(1), row)
    return None


def technologies() -> dict[int, str]:
    """Every technology the tracked table knows, with the unit that owns it."""
    owned = {}
    unit = None
    for line in UNIT_TECHS.read_text().splitlines():
        if line.startswith("  - type: "):
            unit = line.removeprefix("  - type: ").strip()
        else:
            entry = re.match(r"\s+- \{id: (\d+), supply: (\d+)\}", line)
            if entry and unit:
                owned[int(entry.group(1))] = unit
    return owned


def reading(field: str, value: int) -> str:
    if field in INTEGERS:
        return str(value)
    if field in RATES:
        return f"{value / ONE:+.6g}"
    return f"{value / ONE:+.6g}"


def crosscheck(rows: dict[int, dict]) -> tuple[int, int, list[str]]:
    """Two ways a misparse would have to survive to reach the table.

    **Every number this table states has to be in the technology's description.**
    The check runs in this direction rather than the officers' because a
    technology's text states effects this table does not carry: a
    damage rate against aerial units, damage received, a bombardment it
    summons. Those are its skill's, not its unit's, and they live in the skill
    the technology points at with `targetSkillID`.

    **Every rank list has to be its first entry times the rank.** A technology
    that grows stores one value per rank rather than a multiplier, and the
    build rounds each of them on its own, so this allows two raw units of
    drift and nothing more. Garbage read off the wrong offset is not
    arithmetic.
    """
    stated_by_text = {
        "life_rate",
        "damage_rate",
        "attack_range_value",
        "attack_range_rate",
        "attack_interval_value",
        "attack_interval_rate",
        "speed_value",
        "splash_range_value",
    }

    def claim(field: str, raw: int) -> str:
        if field in INTEGERS:
            return f"{abs(raw)}"
        value = abs(raw) / ONE
        return f"{round(value * 100)}" if field in RATES else f"{value:g}"

    checked, disagreed = 0, []
    for identifier, row in sorted(rows.items()):
        for field, _ in LISTS:
            values = row[field]
            if len(values) < 2 or not any(values):
                continue
            for rank, value in enumerate(values):
                grown = round(values[0] / ONE * (rank + 1) * ONE)
                if abs(value - grown) > 2:
                    disagreed.append(
                        f"technology {identifier} ({row['name']}) {field} rank"
                        f" {rank + 1} is {value}, and its first entry times the"
                        f" rank is {grown}"
                    )
                    break

        text = described(row["row"])
        if not text:
            continue
        numbers = set(re.findall(r"\d+(?:\.\d+)?", text))
        claims = {
            claim(field, next(value for value in row[field] if value))
            for field, _ in LISTS
            if field in stated_by_text and any(row[field])
        }
        if not claims:
            continue
        checked += 1
        if not claims <= numbers:
            disagreed.append(
                f"technology {identifier} ({row['name']}) states"
                f" {sorted(claims - numbers)}, which its effect text does not"
            )
    return checked, len(rows), disagreed


def main() -> int:
    build_data.arguments(__doc__)
    owners = technologies()
    every = rows_by_id()
    rows = {}
    for identifier, unit in sorted(owners.items()):
        if identifier not in every:
            print(f"error: technology {identifier} is not in the build", file=sys.stderr)
            return 1
        rows[identifier] = dict(every[identifier], unit=unit)

    checked, _parsed, disagreed = crosscheck(rows)
    for problem in disagreed:
        print(f"error: {problem}", file=sys.stderr)
    if disagreed:
        return 1

    lines = [
        "schema: mechcore.technology_effects",
        f"game_build: {build_data.build()}",
        "",
        "# What a technology does to a fight, which is a correction it writes onto",
        "# the unit that researched it. `config/unit_techs.yaml` carries which",
        "# technologies a unit may research and what each costs.",
        "# `docs/rules/technology_effects.md` states what each field means and what",
        "# is not established.",
        "#",
        "# Every effect is a list indexed by the unit's rank: one entry for a",
        "# technology whose effect is flat, and one per rank for a technology that",
        "# grows with it. A rate is an FPoint Q32.32 raw integer, a value is an",
        "# FPoint in the number's own units, and speed is a plain integer.",
        "",
        "technologies:",
    ]
    written = 0
    for identifier, row in sorted(rows.items()):
        held = [
            (field, row[field])
            for field, _ in LISTS
            if any(value != 0 for value in row.get(field, []))
        ]
        if not held:
            continue
        written += 1
        lines.append(f"  - id: {identifier}")
        lines.append(f"    name: {row['name']}")
        lines.append(f"    unit: {row['unit']}")
        for field, values in held:
            raw = ", ".join(str(value) for value in values)
            if field in INTEGERS:
                lines.append(f"    {field}: [{raw}]")
                continue
            readings = ", ".join(reading(field, value) for value in values)
            lines.append(f"    {field}: [{raw}]  # {readings}")

    OUTPUT.write_text("\n".join(lines) + "\n")
    print(
        f"{written} of {len(rows)} technologies write onto a unit's numbers ->"
        f" {OUTPUT.relative_to(REPOSITORY)}; every number {checked} of them state"
        f" is in their own description, and every rank list is its first entry"
        f" times the rank"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
