#!/usr/bin/env python3
"""Write the name tables of docs/rules/ from config/localization.yaml.

    python3 scripts/name-tables.py            rewrite every table
    python3 scripts/name-tables.py --check    exit 1 when one is stale

A rules document that names units, officers, commander skills, equipment or
technologies carries a table of their official English and Simplified Chinese
names, so a reader can find each one in the game. The table sits between two
markers and is generated, never edited:

    <!-- names: officers -->
    ...
    <!-- /names -->

The kinds are `units`, `officers`, `commander_skills`, `equipment` and
`technologies`. Each row gives the ID, the Chinese and English names, and the
name a document writes, from config/names.yaml (a unit's is the catalog's,
the snake case of its English name). Only the standard library is used, so
scripts/check-docs.py can run it wherever it runs.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RULES = ROOT / "docs" / "rules"
BLOCK = re.compile(r"(<!-- names: (\w+) -->\n)(.*?)(<!-- /names -->)", re.S)
KINDS = ("units", "officers", "commander_skills", "equipment", "technologies")


def localization():
    sections, section = {}, None
    for line in (ROOT / "config" / "localization.yaml").read_text().splitlines():
        header = re.fullmatch(r"(\w+):", line)
        if header:
            section = sections.setdefault(header.group(1), {})
            continue
        row = re.fullmatch(r'  (\d+): \{en: "((?:[^"\\]|\\.)*)", zh: "((?:[^"\\]|\\.)*)"\}', line)
        if row and section is not None:
            section[int(row.group(1))] = (row.group(2), row.group(3))
    return sections


def document_names():
    """config/names.yaml's snake-case names, technologies keyed by ID with their unit."""
    names, section, unit = {}, None, None
    for line in (ROOT / "config" / "names.yaml").read_text().splitlines():
        header = re.fullmatch(r"(\w+):", line)
        if header:
            section, unit = header.group(1), None
            names.setdefault(section, {})
            continue
        owner = re.fullmatch(r"  (\w+):", line)
        if owner and section == "technologies":
            unit = owner.group(1)
            continue
        row = re.fullmatch(r"\s+(\d+): (\S+)", line)
        if row and section:
            names[section][int(row.group(1))] = (row.group(2), unit)
    return names


def snake(text):
    return re.sub(r"[^a-z0-9]+", "_", text.lower().replace("'", "")).strip("_")


def cell(text):
    return text.replace("|", "\\|")


def table(kind, localized, named):
    rows = localized.get(kind, {})
    if kind == "technologies":
        lines = ["| Unit | ID | 中文 | English | Document name |", "| --- | ---: | --- | --- | --- |"]
        ordered = sorted(rows, key=lambda row: (named["technologies"].get(row, ("", ""))[1] or "", row))
        for row in ordered:
            document, unit = named["technologies"].get(row, ("", ""))
            en, zh = rows[row]
            lines.append(f"| `{unit}` | {row} | {cell(zh)} | {cell(en)} | `{document}` |")
        return lines
    lines = ["| ID | 中文 | English | Document name |", "| ---: | --- | --- | --- |"]
    for row in sorted(rows):
        en, zh = rows[row]
        document = snake(en) if kind == "units" else named.get(kind, {}).get(row, ("", ""))[0]
        lines.append(f"| {row} | {cell(zh)} | {cell(en)} | `{document}` |")
    return lines


def render(text, localized, named):
    def replace(match):
        kind = match.group(2)
        if kind not in KINDS:
            raise SystemExit(f"unknown name table kind {kind!r}; the kinds are {', '.join(KINDS)}")
        return match.group(1) + "\n".join(table(kind, localized, named)) + "\n" + match.group(4)

    return BLOCK.sub(replace, text)


def stale():
    """Every document whose tables differ from what config/localization.yaml gives, with the new text."""
    localized, named = localization(), document_names()
    changed = {}
    for path in sorted(RULES.glob("*.md")):
        text = path.read_text()
        updated = render(text, localized, named)
        if updated != text:
            changed[path] = updated
    return changed


def main():
    changed = stale()
    if "--check" in sys.argv[1:]:
        for path in changed:
            print(f"error: {path.relative_to(ROOT)}: a name table is stale; run scripts/name-tables.py")
        return 1 if changed else 0
    for path, text in changed.items():
        path.write_text(text)
        print(f"wrote {path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
