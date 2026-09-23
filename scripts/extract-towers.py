#!/usr/bin/env python3
"""Extract what strengthening a tower and losing one do into `config/towers.yaml`.

    python3 scripts/extract-towers.py            write the table
    python3 scripts/extract-towers.py --check    compare it with the tracked one

The source is `ConfigDataContainer.m_Structure`, exported as JSON to
`work/inputs/config-data-container-build2259.json` (a local research artifact,
not tracked). Two of its lists are read:

* `towerStrengthenDatas`: one row per strengthen level, 1 to 4, each adding
  life to the tower and naming the buff its loss writes.
* `buffDatas` ids 1 to 5, all named 能量塔摧毁. A level-0 tower's loss writes
  id 1, which no strengthen row names; the fights recorded for the tower-loss
  question read its 180 ticks. The five rows are the same buff but for
  `duration`, and the script refuses them if they are not.

Rates and durations are written as the FPoint raw integers the build stores,
with a comment reading each one. `docs/rules/towers.md` states what they mean.
"""

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
EXPORT = ROOT / "work/inputs/config-data-container-build2259.json"
OUTPUT = ROOT / "config/towers.yaml"
BUILD = "1.11.1.3.2259"
ONE = 1 << 32
LEVEL_ZERO_BUFF = 1
# The fields that may differ between the five rows. Everything else is the
# one buff, and has to agree.
PER_ROW = {"id", "duration"}


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def reading(value):
    text = f"{value / ONE:+.4f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def render(structure):
    buffs = {row["id"]: row for row in structure["buffDatas"]}
    levels = sorted(structure["towerStrengthenDatas"], key=lambda row: row["level"])
    if [row["level"] for row in levels] != [1, 2, 3, 4]:
        raise SystemExit("towerStrengthenDatas does not hold levels 1 to 4")
    named = [LEVEL_ZERO_BUFF] + [row["buffID"] for row in levels]
    rows = [buffs[buff] for buff in named]
    first = rows[0]
    for row in rows[1:]:
        differs = {
            key for key in set(first) | set(row)
            if key not in PER_ROW and raw(first.get(key)) != raw(row.get(key))
        }
        if differs:
            raise SystemExit(f"buff {row['id']} differs from buff {first['id']} in {sorted(differs)}")
    for row in rows:
        if raw(row["duration"]) % ONE:
            raise SystemExit(f"buff {row['id']} lasts a fraction of a second")

    lines = [
        "schema: mechcore.towers",
        f"game_build: {BUILD}",
        "",
        "# What strengthening a tower and losing one do, read out of",
        "# ConfigDataContainer by scripts/extract-towers.py. docs/rules/towers.md",
        "# states what each field means. A rate is an FPoint Q32.32 raw integer:",
        "# -3865470566 is -0.9.",
        "",
        "# The buff a tower's loss writes on its side: buffDatas ids 1 to 5, one",
        "# buff that differs only in how long it lasts.",
        "destroyed_buff:",
        f"  name: {first['name']}",
        f"  buff_divide: {first['buffDivide']}",
        f"  additive: {str(first['isAdditiveMode']).lower()}",
        f"  max_additive_stack: {first['maxAdditiveStack']}",
        f"  can_affect_construction: {str(first['canAffectConstruction']).lower()}",
        f"  clear_when_technologies_disabled: {str(first['isClearSelfBuffWhenDisableTech']).lower()}",
        f"  move_speed_rate: {raw(first['speedChangeRate'])}{reading(raw(first['speedChangeRate']))}",
        f"  damage_rate: {raw(first['damageChangeRate'])}{reading(raw(first['damageChangeRate']))}",
        f"  amplify_damage_rate: {raw(first['amplifyDamageRate'])}{reading(raw(first['amplifyDamageRate']))}",
        "",
        "# One row per strengthen level. `life` is what the level adds to the",
        "# tower, on top of the levels below it; `buff` is the buffDatas row its",
        "# loss writes, and `duration` that row's, in seconds.",
        "levels:",
    ]
    lives = [0] + [row["life"] for row in levels]
    for level, (life, row) in enumerate(zip(lives, rows)):
        lines.append(
            f"  - {{level: {level}, life: {life}, buff: {row['id']}, duration: {raw(row['duration']) // ONE}}}"
        )
    return "\n".join(lines) + "\n"


def main():
    if not EXPORT.exists():
        print(f"{EXPORT} is missing; it is a local research artifact and is not tracked", file=sys.stderr)
        return 2
    written = render(json.loads(EXPORT.read_text())["m_Structure"])
    if "--check" in sys.argv[1:]:
        if not OUTPUT.exists() or OUTPUT.read_text() != written:
            print("config/towers.yaml differs from the export", file=sys.stderr)
            return 1
        print("config/towers.yaml agrees with the export byte for byte")
        return 0
    OUTPUT.write_text(written)
    print(f"five tower levels -> {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
