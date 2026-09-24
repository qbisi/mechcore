#!/usr/bin/env python3
"""Extract what an officer does to a fight into `config/officer_effects.yaml`.

    python3 scripts/extract-officer-effects.py [--build BUILD]

`config/officers.yaml` carries what an officer does to a ledger: a discount, an
income, a squad it hands out. This carries the other half, the corrections it
writes onto a unit's numbers, from `ConfigDataContainer.officerDatas` as
`scripts/build_data.py` reads it. The fields are the ones `GameRiver.OfficerData`
answers `ICommonMechDataChangeDataSource` with, the interface `OfficerData`,
`TechnologyData`, `EquipmentData` and `EnergyTowerSkillData` all implement,
which is why one shape serves all four. Officers limited to Interstellar
Expedition are left out.

Every percentage an officer's own English description states ("by 17%") has to
be one of the rates the table holds for it; a misparse would have to agree with
the game's text to pass.
"""

import pathlib
import re
import sys

import build_data

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = REPOSITORY / "config/officer_effects.yaml"
UNIT_TECHS = REPOSITORY / "config/unit_techs.yaml"

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
    # An int percentage on 1.11 builds, an FPoint rate from 2.0 on.
    ("exp_rate", "expChangeRate"),
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


def crosscheck(written: str, officers: list[dict]) -> tuple[int, list[str]]:
    """Every percentage an officer's description states has to be in the table."""
    rows = {}
    for block in written.split("  - id: ")[1:]:
        officer = int(block.split("\n")[0])
        rows[officer] = {
            match.group(1): int(match.group(2))
            for match in re.finditer(r"^    ([a-z_]+): (-?\d+)", block, re.M)
        }
    checked, disagreed = 0, []
    for row in officers:
        text = build_data.description("OfficerData", row)
        said = {abs(int(value)) for value in re.findall(r"by (\d+)%", text)}
        if row["id"] not in rows or not said:
            continue
        held = {
            round(abs(value) / ONE * 100)
            for name, value in rows[row["id"]].items()
            if "rate" in name
        }
        checked += 1
        if not said <= held:
            disagreed.append(f"officer {row['id']} reads {sorted(said)}% and the table holds {sorted(held)}")
    return checked, disagreed


def main() -> int:
    build_data.arguments(__doc__)
    officers = [row for row in build_data.container()["officerDatas"] if build_data.in_standard(row)]
    names = unit_names()

    lines = [
        "schema: mechcore.officer_effects",
        f"game_build: {build_data.build()}",
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
    for row in sorted(officers, key=lambda row: row["id"]):
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
    checked, disagreed = crosscheck(table, officers)
    for problem in disagreed:
        print(f"error: {problem}", file=sys.stderr)
    if disagreed:
        return 1
    OUTPUT.write_text(table)
    print(
        f"{written} officers with a combat effect -> {OUTPUT.relative_to(REPOSITORY)}"
        f"; {checked} of them agree with the percentages their descriptions state"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
