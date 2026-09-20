#!/usr/bin/env python3
"""Extract what an officer does to a fight into `config/officer_effects.yaml`.

`config/officers.yaml` carries what an officer does to a ledger — a discount, an
income, a squad it hands out. This carries the other half: the corrections it
writes onto a unit's numbers.

They come from the same export `scripts/extract_prices.py` reads,
`work/inputs/config-data-container-build2259.json`, which is
`ConfigDataContainer` at path id 160 of the build's `level0`. The fields are the
ones `GameRiver.OfficerData` answers `ICommonMechDataChangeDataSource` with —
the interface `OfficerData`, `TechnologyData`, `EquipmentData` and
`EnergyTowerSkillData` all implement, which is why one shape serves all four and
why the other three join this table when their objects are parsed.

    python3 scripts/extract-officer-effects.py

The export is a local research artifact under `work/`, which is not tracked, so
this exits 2 when it is absent rather than failing a run that cannot have it.
"""

import json
import pathlib
import re
import sys

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
EXPORT = REPOSITORY / "work/inputs/config-data-container-build2259.json"
OUTPUT = REPOSITORY / "config/officer_effects.yaml"
UNIT_TECHS = REPOSITORY / "config/unit_techs.yaml"
OFFICERS = REPOSITORY / "docs/rules/officers.md"
BUILD = "1.11.1.3.2259"

# A rate is an `FPoint`, which the export writes as its Q32.32 raw integer. A
# value is a plain `int` in the runtime getter, and the export writes it as one.
RATES = (
    ("damage_rate", "damageChangeRate"),
    ("damage_rate_by_kill_count", "damageChangeRateByKillCount"),
    ("life_rate", "lifeChangeRate"),
    ("life_rate_by_kill_count", "lifeChangeRateByKillCount"),
    ("attack_interval_rate", "attackIntervalChangeRate"),
    ("attack_range_rate", "attackRangeChangeRate"),
    ("projectile_life_rate", "projectileLifeChangeRate"),
    ("tower_life_rate", "towerLifeChangeRate"),
    ("energy_shield_rate", "energyShieldChangeRate"),
    ("land_mine_rate", "landMineChangeRate"),
    ("super_deployment_time_rate", "superDeploymentTimeChangeRate"),
)
# These are FPoint too, but they correct a number in its own units rather than
# by a proportion: ten metres of range, two fifths of a second of interval.
VALUES = (
    ("attack_range_value", "attackRangeChangeValue"),
    ("min_attack_range_value", "minAttackRangeChangeValue"),
    ("attack_interval_value", "attackIntervalChangeValue"),
    ("splash_range_value", "splashRangeChangeValue"),
    ("projectile_speed_value", "projectileSpeedChangeValue"),
)
# Plain integers, by the runtime getter's own return type.
INTEGERS = (
    ("speed_value", "speedChangeValue"),
    ("extra_life", "extraLife"),
    ("exp_rate", "expChangeRate"),
)
ONE = 1 << 32


def unit_names() -> dict[int, str]:
    """Unit IDs to the type names a document writes, from the tracked table."""
    names = {}
    unit_id = None
    for line in UNIT_TECHS.read_text().splitlines():
        if line.startswith("  - type: "):
            unit_id = line.removeprefix("  - type: ").strip()
        elif line.startswith("    unit_id: ") and unit_id is not None:
            names[int(line.removeprefix("    unit_id: "))] = unit_id
            unit_id = None
    return names


def raw(value) -> int:
    """An FPoint's raw integer, or a plain integer as it stands."""
    if isinstance(value, dict):
        return int(value.get("m_rawValue") or 0)
    return int(value or 0)


def percent(value: int) -> str:
    return f"{value / ONE:+.6g}"


def crosscheck(written: str) -> tuple[int, list[str]]:
    """Every percentage the officer index states has to be in the table.

    `docs/rules/officers.md` lists each officer's effect in the game's own
    localized words, read out of build 2227 by another route entirely. A
    misparse here would have to agree with that text to pass, which is the same
    standard `scripts/extract_prices.py` holds its own tables to.
    """
    rows = {}
    for block in written.split("  - id: ")[1:]:
        officer = int(block.split("\n")[0])
        rows[officer] = {
            match.group(1): int(match.group(2))
            for match in re.finditer(r"^    ([a-z_]+): (-?\d+)", block, re.M)
        }
    checked, disagreed = 0, []
    for line in OFFICERS.read_text().splitlines():
        stated = re.match(r"\| `(\d+)` \|.*\|([^|]*)\|\s*$", line)
        if not stated:
            continue
        officer, text = int(stated.group(1)), stated.group(2)
        said = {abs(int(value)) for value in re.findall(r"by (\d+)%", text)}
        if officer not in rows or not said:
            continue
        held = {
            round(abs(value) / ONE * 100)
            for name, value in rows[officer].items()
            if "rate" in name
        }
        held |= {abs(value) for name, value in rows[officer].items() if name == "exp_rate"}
        checked += 1
        if not said <= held:
            disagreed.append(f"officer {officer} reads {sorted(said)}% and the table holds {sorted(held)}")
    return checked, disagreed


def main() -> int:
    if not EXPORT.exists():
        print(
            f"{EXPORT} is missing; it is a local research artifact and is not"
            " tracked",
            file=sys.stderr,
        )
        return 2
    structure = json.loads(EXPORT.read_text())["m_Structure"]
    names = unit_names()

    lines = [
        "schema: mechcore.officer_effects",
        f"game_build: {BUILD}",
        "",
        "# What an officer does to a fight, which is a correction it writes onto",
        "# the units it targets. `config/officers.yaml` carries what it does to a",
        "# ledger instead. `docs/rules/officer_effects.md` states what each field",
        "# means, how a rate is encoded, and what is not established.",
        "#",
        "# A rate is an FPoint Q32.32 raw integer: 1288490188 is +0.3. A value is",
        "# an FPoint in the number's own units: 42949672960 is +10 of range.",
        "",
        "officers:",
    ]
    written = 0
    for row in sorted(structure["officerDatas"], key=lambda row: row["id"]):
        held = []
        for name, field in RATES:
            value = raw(row.get(field))
            if value:
                held.append((name, value, percent(value)))
        for name, field in VALUES:
            value = raw(row.get(field))
            if value:
                held.append((name, value, percent(value)))
        for name, field in INTEGERS:
            value = raw(row.get(field))
            if value:
                held.append((name, value, None))
        if not held:
            continue
        written += 1
        lines.append(f"  - id: {row['id']}")
        lines.append(f"    name: {row.get('name') or ''}")
        lines.append(f"    mech_type: {(row.get('mechType') or [0])[0]}")
        units = [names.get(unit, str(unit)) for unit in row.get("unitID") or []]
        if units:
            lines.append(f"    units: [{', '.join(units)}]")
        for name, value, reading in held:
            comment = f"  # {reading}" if reading else ""
            lines.append(f"    {name}: {value}{comment}")

    table = "\n".join(lines) + "\n"
    checked, disagreed = crosscheck(table)
    for problem in disagreed:
        print(f"error: {problem}", file=sys.stderr)
    if disagreed:
        return 1
    OUTPUT.write_text(table)
    print(
        f"{written} officers with a combat effect -> {OUTPUT.relative_to(REPOSITORY)}"
        f"; {checked} of them agree with the percentages docs/rules/officers.md states"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
