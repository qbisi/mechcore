#!/usr/bin/env python3
"""Extract what a construction is into `config/constructions.yaml`.

A construction is not one object. `GroupConstructionManager.CreateConstruction`
builds a `FightConstruction` per child and hands them back as a list, so the
numbers a fight reads are per child — a Defensive Wall's `maxLife` is one
block's, not the wall's. `docs/rules/constructions.md` says what each field
means and which of them this build has measured.

The source is `ConfigDataContainer.constructionDatas`, the same Unity object
`scripts/extract_prices.py` reads a card's price out of, exported as JSON
because reading it back out of the asset needs a type tree. The export keeps
field names, so nothing here depends on declaration order.

Two checks stand between the export and the table.

* Every construction the layout catalog names must have a grid whose cells are
  ten metres: `gridColumnCount x 10` by `gridRowCount x 10` has to equal the
  footprint `crates/document/src/catalog.rs` pins, and that footprint was read
  off placements the game accepted rather than out of this object.
* Every row must carry a positive `count` and `maxLife`, because both are per
  child and a zero would mean the fields are not the ones they are named.

    python3 scripts/extract-constructions.py
"""

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
CONFIG_JSON = ROOT / "work/inputs/config-data-container-build2259.json"
CATALOG = ROOT / "crates/document/src/catalog.rs"
OUTPUT = ROOT / "config/constructions.yaml"
BUILD = "1.11.1.3.2259"
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
    container = json.loads(CONFIG_JSON.read_text(encoding="utf-8"))
    return container["m_Structure"]["constructionDatas"]


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


def render(entries):
    lines = [
        "schema: mechcore.constructions",
        f"game_build: {BUILD}",
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
    return "\n".join(lines) + "\n"


def main():
    if not CONFIG_JSON.exists():
        print(f"{CONFIG_JSON} is absent; it is a local research artifact", file=sys.stderr)
        return 2
    named = catalog_footprints()
    entries = []
    for row in sorted(rows(), key=lambda row: row["id"]):
        check(row, named)
        entries.append(entry(row, named))
    OUTPUT.write_text(render(entries), encoding="utf-8")
    print(f"{OUTPUT}: {len(entries)} constructions, {len(named)} checked against the catalog")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
