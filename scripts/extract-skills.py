#!/usr/bin/env python3
"""Print skill rows of the build's `MechSkillGroupData`, one JSON object each.

    python3 scripts/extract-skills.py [--build BUILD] 3003001 3002001
    python3 scripts/extract-skills.py [--build BUILD] --all

The skills a unit or a construction fires are the rows of `MechSkillGroupData`
in `level0`, one list per kind of skill (`skillDatas`, `projectileSkillDatas`,
`laserSkillDatas` and the rest), as `scripts/build_data.py` reads them. Each
row printed carries `kind`, the list it came from, and every FPoint in units,
already divided by 2^32. `damage` is empty for a unit's skill, because a
unit's and a construction's damage sit on their own rows in
`ConfigDataContainer`.
"""

import json
import sys

import build_data

ONE = 1 << 32


def units(value):
    if isinstance(value, dict) and set(value) == {"m_rawValue"}:
        return value["m_rawValue"] / ONE
    if isinstance(value, dict):
        return {key: units(item) for key, item in value.items()}
    if isinstance(value, list):
        return [units(item) for item in value]
    return value


def rows():
    every = []
    for kind, table in build_data.level0("MechSkillGroupData").items():
        if isinstance(table, list):
            every += [dict(units(row), kind=kind) for row in table]
    return every


def main():
    def extra(parser):
        parser.add_argument("ids", nargs="*", type=int, help="skill ids")
        parser.add_argument("--all", action="store_true", help="every row")

    arguments = build_data.arguments(__doc__, extra)
    if not arguments.all and not arguments.ids:
        sys.exit("name skill ids, or --all")
    wanted = None if arguments.all else set(arguments.ids)
    for row in rows():
        if wanted is None or row["id"] in wanted:
            print(json.dumps(row, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
