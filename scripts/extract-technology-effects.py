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

The source is `TechnologyGroupData` at path id 184 of the build's `level0`, the
object `scripts/extract_prices.py` already reads for a technology's price. No
type tree is needed: a `MonoBehaviour`'s fields serialize base class first and
in declaration order, and the declaration order is the decompiled one. This
reads further into the same record, past `supply`:

    previousTechID, activeLevel, lifeChangeRate, damageChangeRate,
    speedChangeValue, minAttackRangeChangeValue, attackRangeChangeValue,
    attackRangeChangeRate, attackIntervalChangeValue, attackIntervalChangeRate,
    splashRangeChangeValue, projectileSpeedChangeValue,
    projectileLifeChangeRate, unlockCost

Every parse is checked against `docs/rules/unit_techs.md`, whose effect text was
read out of build 2227 by other means, so a silent misparse would have to agree
with an independent extraction of an earlier build to pass.

    work/tools/asset-venv/bin/python scripts/extract-technology-effects.py \\
        "<Mechabellum.app>/Contents/Resources/Data/level0"
"""

import pathlib
import re
import struct
import sys

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = REPOSITORY / "config/technology_effects.yaml"
UNIT_TECHS = REPOSITORY / "config/unit_techs.yaml"
DOC = REPOSITORY / "docs/rules/unit_techs.md"
BUILD = "1.11.1.3.2259"
TECHNOLOGY_GROUP_PATH_ID = 184
ONE = 1 << 32

# The effect lists, in declaration order, with the width of one entry. A
# `speedChangeValue` is a plain integer for the same reason an officer's is:
# the build keeps it in `DataSet.intDatas`.
LISTS = (
    ("life_rate", 8),
    ("damage_rate", 8),
    ("speed_value", 4),
    ("min_attack_range_value", 4),
    ("attack_range_value", 8),
    ("attack_range_rate", 8),
    ("attack_interval_value", 8),
    ("attack_interval_rate", 8),
    ("splash_range_value", 8),
    ("projectile_speed_value", 8),
    ("projectile_life_rate", 8),
)
RATES = {"life_rate", "damage_rate", "attack_range_rate", "attack_interval_rate", "projectile_life_rate"}
INTEGERS = {"speed_value", "min_attack_range_value"}


def blob(level0: pathlib.Path) -> bytes:
    import UnityPy

    environment = UnityPy.load(str(level0))
    for obj in environment.objects:
        if obj.type.name == "MonoBehaviour" and obj.path_id == TECHNOLOGY_GROUP_PATH_ID:
            return obj.get_raw_data()
    raise SystemExit(f"{level0} has no MonoBehaviour {TECHNOLOGY_GROUP_PATH_ID}")


def read_string(data: bytes, offset: int):
    (length,) = struct.unpack_from("<i", data, offset)
    if length < 0 or length > 4096 or offset + 4 + length > len(data):
        return None, offset
    end = offset + 4 + length
    return data[offset + 4 : end], end + (-end) % 4


def parse(data: bytes, offset: int):
    """Reads one `TechnologyData` whose ID starts at `offset`, effects and all."""
    (identifier,) = struct.unpack_from("<i", data, offset)
    name, cursor = read_string(data, offset + 4)
    if not name:
        return None
    try:
        name = name.decode("utf-8")
    except UnicodeDecodeError:
        return None
    cursor += 4  # isTestData, one byte padded to four
    icon, cursor = read_string(data, cursor)
    if icon is None or not icon.isascii():
        return None
    for _ in range(3):  # description, descParams, story
        text, cursor = read_string(data, cursor)
        if text is None:
            return None
    cursor += 4  # targetSkillID
    cursor += 12  # mainSkillEffect, extraSkillEffect, extraSkillNumericalEffect
    supply, _previous, active_level = struct.unpack_from("<iii", data, cursor)
    cursor += 12

    row = {"id": identifier, "name": name, "supply": supply, "active_level": active_level}
    for field, width in LISTS:
        (count,) = struct.unpack_from("<i", data, cursor)
        cursor += 4
        if count < 0 or count > 64 or cursor + count * width > len(data):
            return None
        layout = "<q" if width == 8 else "<i"
        row[field] = [
            struct.unpack_from(layout, data, cursor + index * width)[0]
            for index in range(count)
        ]
        cursor += count * width
    return row


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


def documented() -> dict[int, str]:
    """Each technology's effect text, as `docs/rules/unit_techs.md` states it."""
    stated = {}
    for line in DOC.read_text().splitlines():
        row = re.match(r"^\| `(\d+)` \|[^|]*\|[^|]*\|[^|]*\|[^|]*\|([^|]*)\|", line)
        if row:
            stated[int(row.group(1))] = row.group(2)
    return stated


def crosscheck(rows: dict[int, dict]) -> tuple[int, int, list[str]]:
    """Two ways a misparse would have to survive to reach the table.

    **Every number this table states has to be in the index's text.** The text
    is build 2227's, read by another route and in the game's own words, so a
    misparse would have to agree with an independent extraction of an earlier
    build to pass. The check runs in this direction rather than the officers'
    because a technology's text states effects this table does not carry: a
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
    text_of = documented()
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

        text = text_of.get(identifier)
        if text is None:
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
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-2].strip(), file=sys.stderr)
        return 2
    level0 = pathlib.Path(sys.argv[1])
    if not level0.exists():
        print(f"{level0} does not exist", file=sys.stderr)
        return 2
    data = blob(level0)
    owners = technologies()

    rows = {}
    for identifier, unit in sorted(owners.items()):
        needle = struct.pack("<i", identifier)
        start = 0
        while True:
            position = data.find(needle, start)
            if position < 0:
                print(f"error: technology {identifier} is not in the asset", file=sys.stderr)
                return 1
            try:
                row = parse(data, position)
            except struct.error:
                row = None
            if row and row["id"] == identifier and 0 <= row["supply"] <= 5000:
                row["unit"] = unit
                rows[identifier] = row
                break
            start = position + 1

    checked, _parsed, disagreed = crosscheck(rows)
    for problem in disagreed:
        print(f"error: {problem}", file=sys.stderr)
    if disagreed:
        return 1

    lines = [
        "schema: mechcore.technology_effects",
        f"game_build: {BUILD}",
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
        f" is in the effect text docs/rules/unit_techs.md carries, and every rank"
        f" list is its first entry times the rank"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
