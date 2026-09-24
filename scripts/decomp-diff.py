#!/usr/bin/env python3
"""What changed between two decompiled builds, declaration by declaration.

    scripts/decomp-diff.py OLD NEW [--assembly NAME ...] [--config]

OLD and NEW are builds under work/decomp (as `scripts/decompile.py` or
`scripts/decomp.py sync` leave them). The C# stubs are compared with every
attribute line dropped, so what remains is declarations: types, fields with
their offsets, and method signatures. A type file present in one build only is
reported whole; a type in both lists the declarations each side lacks.

`--config` compares the two `config-data-container.json` instead: which tables
appeared or went, and for each table in both, which rows (by `id`) appeared or
went and which fields of a surviving row changed.

It reads, never writes. The default assemblies are the three the fight and the
match live in.
"""

import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
DECOMP = ROOT / "work" / "decomp"
DEFAULT_ASSEMBLIES = ("GRFight", "GRCore", "GRUtility")


def declarations(path):
    lines = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("[") or stripped.startswith("//"):
            continue
        if stripped in ("{", "}"):
            continue
        lines.append(stripped)
    return lines


def compare_stubs(old, new, assemblies):
    for assembly in assemblies:
        old_root = DECOMP / old / "cpp2il/DiffableCs" / assembly
        new_root = DECOMP / new / "cpp2il/DiffableCs" / assembly
        old_files = {p.relative_to(old_root) for p in old_root.rglob("*.cs")}
        new_files = {p.relative_to(new_root) for p in new_root.rglob("*.cs")}
        print(f"# {assembly}")
        for relative in sorted(new_files - old_files):
            print(f"\n+ {relative}")
            for line in declarations(new_root / relative):
                print(f"    {line}")
        for relative in sorted(old_files - new_files):
            print(f"\n- {relative}")
        for relative in sorted(old_files & new_files):
            before = declarations(old_root / relative)
            after = declarations(new_root / relative)
            if before == after:
                continue
            removed = [line for line in before if line not in set(after)]
            added = [line for line in after if line not in set(before)]
            if not removed and not added:
                continue
            print(f"\n~ {relative}")
            for line in removed:
                print(f"    - {line}")
            for line in added:
                print(f"    + {line}")
        print()


def rows(table):
    if isinstance(table, list) and all(isinstance(row, dict) and "id" in row for row in table):
        return {row["id"]: row for row in table}
    return None


def compare_config(old, new):
    before = json.loads((DECOMP / old / "config-data-container.json").read_text())["m_Structure"]
    after = json.loads((DECOMP / new / "config-data-container.json").read_text())["m_Structure"]
    for name in sorted(set(after) - set(before)):
        size = len(after[name]) if isinstance(after[name], list) else "-"
        print(f"+ table {name} ({size} rows)")
    for name in sorted(set(before) - set(after)):
        print(f"- table {name}")
    for name in sorted(set(before) & set(after)):
        if before[name] == after[name]:
            continue
        old_rows, new_rows = rows(before[name]), rows(after[name])
        if old_rows is None or new_rows is None:
            print(f"~ table {name} (not keyed by id; changed)")
            continue
        added = sorted(set(new_rows) - set(old_rows))
        removed = sorted(set(old_rows) - set(new_rows))
        changed = [key for key in sorted(set(old_rows) & set(new_rows)) if old_rows[key] != new_rows[key]]
        print(f"~ table {name}: {len(added)} rows added, {len(removed)} removed, {len(changed)} changed")
        for key in added:
            print(f"    + {key} {new_rows[key].get('name', '')}")
        for key in removed:
            print(f"    - {key} {old_rows[key].get('name', '')}")
        for key in changed:
            fields = sorted(
                field for field in set(old_rows[key]) | set(new_rows[key])
                if old_rows[key].get(field) != new_rows[key].get(field)
            )
            print(f"    ~ {key} {new_rows[key].get('name', '')}: {', '.join(fields)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("old")
    parser.add_argument("new")
    parser.add_argument("--assembly", action="append")
    parser.add_argument("--config", action="store_true")
    arguments = parser.parse_args()
    for build in (arguments.old, arguments.new):
        if not (DECOMP / build).is_dir():
            print(f"decomp-diff: no build {build} under {DECOMP}", file=sys.stderr)
            return 2
    if arguments.config:
        compare_config(arguments.old, arguments.new)
    else:
        compare_stubs(arguments.old, arguments.new, arguments.assembly or DEFAULT_ASSEMBLIES)
    return 0


if __name__ == "__main__":
    sys.exit(main())
