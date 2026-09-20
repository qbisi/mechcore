#!/usr/bin/env python3
"""The build's fight architecture, read out of the local decompilation index.

`docs/spec/simulation/architecture.md` mirrors the build's module structure,
object model and derived-value layer. Every list and table in it comes from
here, so a claim about the build's shape can be rerun rather than believed.

    python3 scripts/fight-structure.py [--index work/unity-index/<build>/index.sqlite]

The index holds types, methods and call edges and **no method bodies**, so this
reports membership and call edges only: which module exists, what a property
reads, which systems a mechanism touches. It cannot report arithmetic, branch
conditions, or the order of calls inside a body, and neither can the document.

The index is a local research artifact under `work/`, which is not tracked, so
this exits 2 when it is absent rather than failing the run.
"""

import argparse
import pathlib
import sqlite3
import sys
import textwrap

REPOSITORY = pathlib.Path(__file__).resolve().parent.parent
NAMESPACE = "GameRiver.Fight"
NOISE = ("0x", "il2cpp", "0")


def default_index() -> pathlib.Path | None:
    for candidate in sorted((REPOSITORY / "work" / "unity-index").glob("*/index.sqlite")):
        return candidate
    return None


def names(database: sqlite3.Connection, suffix: str) -> list[str]:
    rows = database.execute(
        "select distinct type_name from symbols"
        " where namespace = ? and kind = 'type'"
        " and type_name like ? and type_name not like '%+%'"
        " order by type_name",
        (NAMESPACE, f"%{suffix}"),
    )
    return [row[0] for row in rows]


def reads(database: sqlite3.Connection, type_name: str, method: str) -> list[str]:
    rows = database.execute(
        "select distinct c.target_label from calls c"
        " join symbols s on c.caller_id = s.id"
        " where s.type_name = ? and s.name like ?",
        (type_name, f"%{method}%"),
    )
    return sorted(
        {
            target
            for (target,) in rows
            if not target.startswith(NOISE[0]) and NOISE[1] not in target and target != NOISE[2]
        }
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", default=None)
    arguments = parser.parse_args()
    index = pathlib.Path(arguments.index) if arguments.index else default_index()
    if index is None or not index.exists():
        print(
            "no decompilation index under work/unity-index; this is a local"
            " research artifact and is not tracked",
            file=sys.stderr,
        )
        return 2
    database = sqlite3.connect(f"file:{index}?mode=ro", uri=True)

    counts = dict(
        database.execute(
            "select kind, count(*) from symbols group by kind"
        ).fetchall()
    )
    print(f"index {index}")
    print(
        "  "
        + ", ".join(f"{kind}s {count}" for kind, count in sorted(counts.items()))
        + f", call edges {database.execute('select count(*) from calls').fetchone()[0]}"
    )

    systems = names(database, "System")
    print(f"\nmodules ({len(systems)})")
    print(textwrap.fill("  ".join(systems), width=78, initial_indent="  ",
                        subsequent_indent="  "))

    print("\nmodule lifecycle, from FightModule and what a system overrides")
    for (signature,) in database.execute(
        "select distinct signature from symbols"
        " where (type_name = 'FightModule'"
        "        or (type_name = 'FightCoreSystem' and name = 'Init'))"
        " and low_level = 0 and kind = 'method'"
        " order by name"
    ):
        print(f"  {signature.strip()}")

    print("\nwhat each derived value reads")
    for property_name in names(database, "Property"):
        if property_name.startswith("FightProperty") or "Dispatcher" in property_name:
            continue
        dependencies = reads(database, property_name, "Refresh")
        if not dependencies:
            continue
        print(f"  {property_name}")
        for dependency in dependencies:
            if dependency.startswith("FightProperty."):
                continue
            print(f"      {dependency}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
