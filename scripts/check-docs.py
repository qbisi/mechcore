#!/usr/bin/env python3
"""Check what a machine can check about the documents.

Relative links resolve, section anchors exist, readmes are spelled README.md,
every spec follows the convention in docs/README.md, and the name tables of
docs/rules/ are the ones config/localization.yaml gives. Nothing here judges
whether a sentence is true; that still needs a reader.

Run from the repository root: python3 scripts/check-docs.py
"""

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]

# Kinds from docs/README.md. A spec must appear in exactly one set, so adding a
# spec without classifying it fails rather than being silently unchecked.
DOCUMENT_FORMAT = {
    "docs/spec/document/layout.md",
    "docs/spec/mechcore/turn.md",
    "docs/spec/document/state.md",
    "docs/spec/document/battle.md",
    "docs/spec/document/action.md",
    "docs/spec/mcfr/mcfr.md",
    "docs/spec/simulation/unit-rules.md",
}
INTERFACE_CONTRACT = {
    "docs/spec/adapter/adapter.md",
    "docs/spec/mechcore/cli.md",
    "docs/spec/mechcore/mcscript.md",
    "docs/spec/mechcore/session.md",
}
ALGORITHM_CONTRACT = {
    "docs/spec/simulation/architecture.md",
    "docs/spec/simulation/rvo.md",
    "docs/spec/simulation/quadtree.md",
}

# Specs that predate the convention. Structure checks are skipped for these and
# the list is reported, so converting one means deleting a line here. An entry
# naming a file that already conforms is itself an error. It is empty: every
# spec is on the convention, and a new one is checked from its first commit.
PENDING: set[str] = set()

LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
FENCE = re.compile(r"^\s*```")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*$")


def tracked_markdown():
    out = subprocess.run(
        ["git", "ls-files", "*.md"], cwd=REPO, capture_output=True, text=True, check=True
    )
    return [Path(line) for line in out.stdout.split() if line]


def outside_fences(text):
    """Yield (line number, line) for lines that are not inside a code fence."""
    fenced = False
    for number, line in enumerate(text.split("\n"), 1):
        if FENCE.match(line):
            fenced = not fenced
            continue
        if not fenced:
            yield number, line


def slug(title):
    """GitHub's heading anchor: lowercased, punctuation dropped, spaces hyphened."""
    text = title.replace("`", "")
    text = re.sub(r"[^\w\s-]", "", text, flags=re.UNICODE)
    return re.sub(r"\s+", "-", text.strip()).lower()


def headings(path):
    text = (REPO / path).read_text()
    return [(len(m.group(1)), m.group(2)) for _, line in outside_fences(text)
            for m in [HEADING.match(line)] if m]


def sections(path):
    """The `## ` section titles of a document, in order."""
    return [title for level, title in headings(path) if level == 2]


def check_links(paths, fail):
    anchors = {str(p): {slug(t) for _, t in headings(p)} for p in paths}
    for path in paths:
        text = (REPO / path).read_text()
        for number, line in outside_fences(text):
            for match in LINK.finditer(line):
                target = match.group(1).strip()
                if target.startswith(("http://", "https://", "mailto:")):
                    continue
                file_part, _, anchor = target.partition("#")
                where = f"{path}:{number}"
                if not file_part:
                    if anchor and anchor not in anchors[str(path)]:
                        fail(f"{where}: no heading for anchor #{anchor}")
                    continue
                resolved = ((REPO / path).parent / file_part).resolve()
                if not resolved.exists():
                    fail(f"{where}: broken link {target}")
                    continue
                key = str(resolved.relative_to(REPO))
                if anchor and key in anchors and anchor not in anchors[key]:
                    fail(f"{where}: {file_part} has no heading for #{anchor}")


def check_readme_naming(fail):
    for path in REPO.rglob("readme*.md"):
        if any(part in {"target", ".git", ".venv", "work"} for part in path.parts):
            continue
        fail(f"{path.relative_to(REPO)}: readmes are spelled README.md")


def check_repeated_paragraphs(paths, fail):
    """Refuse a document that says the same paragraph twice.

    A document is prose written once. The same paragraph appearing again is a
    paste gone wrong, or a script's replacement that matched more than it
    meant to: an empty search string replaced into every gap of a file once
    turned a 101-line readme into 55,000 lines of its own paragraphs, and the
    link check had nothing to say about it. Short lines and fenced code are
    left alone, because a table row or a snippet can honestly repeat.
    """
    for path in paths:
        text = path.read_text(encoding="utf-8")
        seen = set()
        fenced = False
        paragraph = []
        blocks = []
        for line in text.splitlines() + [""]:
            if line.startswith("```"):
                fenced = not fenced
                paragraph = []
                continue
            if fenced:
                continue
            if line.strip():
                paragraph.append(line.strip())
            elif paragraph:
                blocks.append(" ".join(paragraph))
                paragraph = []
        for block in blocks:
            if len(block) < 80 or block.startswith("|"):
                continue
            if block in seen:
                fail(f"{path}: a paragraph appears twice: {block[:60]}...")
                break
            seen.add(block)


def check_spec_classification(paths, fail):
    classified = DOCUMENT_FORMAT | INTERFACE_CONTRACT | ALGORITHM_CONTRACT
    present = {str(p) for p in paths
               if str(p).startswith("docs/spec/") and not str(p).endswith(".zh.md")}
    for path in sorted(present - classified):
        fail(f"{path}: not classified in scripts/check-docs.py; "
             f"add it to a kind and to docs/README.md")
    for path in sorted(classified - present):
        fail(f"{path}: classified but missing from the tree")
    for path in sorted(PENDING - present):
        fail(f"{path}: listed as pending but missing from the tree")


def check_spec_structure(fail):
    converted = (DOCUMENT_FORMAT | INTERFACE_CONTRACT | ALGORITHM_CONTRACT) - PENDING
    for path in sorted(converted):
        names = sections(Path(path))
        if not names:
            fail(f"{path}: no `## ` sections")
            continue
        if names[0] != "Scope":
            fail(f"{path}: first section is `{names[0]}`, must be `Scope`")
        if names[-1] != "Unresolved":
            fail(f"{path}: last section is `{names[-1]}`, must be `Unresolved`")
        for name in names:
            if name.lower().startswith("current") or name.lower() == "status":
                fail(f"{path}: section `{name}` names a moment, not a contract")
    for path in sorted(PENDING & DOCUMENT_FORMAT | PENDING & INTERFACE_CONTRACT
                       | PENDING & ALGORITHM_CONTRACT):
        names = sections(Path(path))
        if names[:1] == ["Scope"] and names[-1:] == ["Unresolved"]:
            fail(f"{path}: listed as pending but already conforms; "
                 f"remove it from PENDING in scripts/check-docs.py")


# What a rules document may not cite, by docs/README.md's "Evidence a rule may
# cite": an untracked path, an asset path id or an ISIL line, all of which
# move between builds or machines.
UNREPRODUCIBLE = (
    (re.compile(r"(?<![\w/.-])work/"), "a path under work/, which is not tracked"),
    (re.compile(r"\bpath[ _]id\b", re.I), "an asset path id, which moves between builds"),
    (re.compile(r"\bISIL (line )?\d+|@isil:\d+", re.I), "an ISIL line, which moves between builds"),
)


def check_rules_evidence(fail):
    for path in sorted((REPO / "docs" / "rules").glob("*.md")):
        for number, line in outside_fences(path.read_text()):
            for pattern, why in UNREPRODUCIBLE:
                if pattern.search(line):
                    fail(f"{path.relative_to(REPO)}:{number}: cites {why}")


# A rules document ends in `## Evidence`: what a recording pinned under tests/
# shows, what was read from the build and the members it rests on, and what is
# not established. docs/README.md says why. The documents not yet written that
# way are listed here, and leave the list as they are.
RULES_PENDING = {
    "battle_skill.md", "combat.md", "constructions.md",
    "officer_effects.md", "officers.md", "opening.md", "reinforce_items.md",
    "reinforcements.md", "technology_effects.md", "terrain.md", "towers.md",
    "unit_experience.md", "unit_techs.md",
}
EVIDENCE_PARTS = ("Recorded", "Replayed", "Read", "Not established")
ANCHOR = re.compile(r"`[A-Z][A-Za-z0-9_]*\.[A-Za-z_][A-Za-z0-9_]*`")
TESTS_PATH = re.compile(r"`(tests/[^`]+)`")


def evidence_items(text):
    """{part: [item text]} for the `## Evidence` section, or None without one."""
    parts, section, part = {}, None, None
    for line in text.splitlines():
        if line.startswith("## "):
            section = line[3:].strip()
            part = None
        elif section == "Evidence" and line.startswith("### "):
            part = line[4:].strip()
            parts.setdefault(part, [])
        elif section == "Evidence" and part and line.startswith("- "):
            parts[part].append(line[2:])
        elif section == "Evidence" and part and line.startswith("  ") and parts[part]:
            parts[part][-1] += " " + line.strip()
    return parts if any(line.strip() == "## Evidence" for line in text.splitlines()) else None


def check_rules_evidence_sections(fail):
    for path in sorted((REPO / "docs" / "rules").glob("*.md")):
        if path.name in RULES_PENDING:
            continue
        name = path.relative_to(REPO)
        parts = evidence_items(path.read_text())
        order = [part for part in EVIDENCE_PARTS if part in (parts or {})]
        if not parts or list(parts) != order or any(not items for items in parts.values()):
            fail(f"{name}: ends in ## Evidence, its parts among ### {', ### '.join(EVIDENCE_PARTS)}, "
                 "in that order, none of them empty")
            continue
        for item in parts.get("Replayed", []):
            if "`scripts/verify-battles.py`" not in item:
                fail(f"{name}: a replayed claim cites scripts/verify-battles.py: {item[:80]}")
        for item in parts.get("Recorded", []):
            cited = TESTS_PATH.findall(item)
            if not cited:
                fail(f"{name}: a recorded claim cites no pin under tests/: {item[:80]}")
            for cite in cited:
                target = REPO / cite
                if not target.exists():
                    fail(f"{name}: cites {cite}, which does not exist")
                elif target.suffix == ".mcscript" and re.search(r"^game:", target.read_text(), re.M):
                    fail(f"{name}: cites {cite}, which needs the game; a recorded claim cites what CI replays")
        for item in parts.get("Read", []):
            if not ANCHOR.search(item):
                fail(f"{name}: a read claim names no `Class.member` it rests on: {item[:80]}")


# The game version is written once, in GAME_VERSION; everything else reads it.
# A five-part version string, or a build named by number, anywhere else is a
# second pin that nothing keeps in step. plan.md is the migration's own record.
# A rules document still pending its evidence section is exempt, as it may
# still say which version its evidence came from.
VERSION_PIN = re.compile(r"\b[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+\b|\b[Bb]uild[- ][0-9]{3,}\b")
VERSION_WRITERS = {"GAME_VERSION", "plan.md"}
VERSION_PENDING = tuple(f"docs/rules/{name}" for name in RULES_PENDING)


def check_version_pins(fail):
    listed = subprocess.run(["git", "ls-files", "-z"], cwd=REPO, check=True,
                            capture_output=True).stdout.decode().split("\0")
    for name in filter(None, listed):
        if name in VERSION_WRITERS or name.startswith(VERSION_PENDING):
            continue
        try:
            text = (REPO / name).read_text()
        except (UnicodeDecodeError, FileNotFoundError, IsADirectoryError):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            match = VERSION_PIN.search(line)
            if match and not match.group(0).startswith("0.0.0.0."):
                fail(f"{name}:{number}: names a game version ({match.group(0)}); GAME_VERSION is the only place")


def check_name_tables(fail):
    """The name tables of docs/rules/ are what config/localization.yaml gives."""
    import importlib.util

    spec = importlib.util.spec_from_file_location("name_tables", REPO / "scripts" / "name-tables.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for path in module.stale():
        fail(f"{path.relative_to(REPO)}: a name table is stale; run scripts/name-tables.py")


def main():
    problems = []
    paths = tracked_markdown()
    check_links(paths, problems.append)
    check_readme_naming(problems.append)
    check_spec_classification(paths, problems.append)
    check_spec_structure(problems.append)
    check_repeated_paragraphs(paths, problems.append)
    check_name_tables(problems.append)
    check_rules_evidence(problems.append)
    check_version_pins(problems.append)
    check_rules_evidence_sections(problems.append)

    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    converted = len(DOCUMENT_FORMAT | INTERFACE_CONTRACT | ALGORITHM_CONTRACT) - len(PENDING)
    total = len(DOCUMENT_FORMAT | INTERFACE_CONTRACT | ALGORITHM_CONTRACT)
    print(f"{len(paths)} markdown files, {converted}/{total} specs on the convention, "
          f"{len(PENDING)} pending")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
