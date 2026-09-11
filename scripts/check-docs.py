#!/usr/bin/env python3
"""Check what a machine can check about the documents.

Relative links resolve, section anchors exist, readmes are spelled README.md,
and every spec follows the convention in docs/README.md. Nothing here judges
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
    "docs/spec/document/state.md",
    "docs/spec/document/turn.md",
    "docs/spec/document/battle.md",
    "docs/spec/document/action.md",
    "docs/spec/mcfr/mcfr.md",
    "docs/spec/simulation/unit-rules.md",
}
INTERFACE_CONTRACT = {
    "docs/spec/adapter/adapter.md",
    "docs/spec/mechcore/mcscript.md",
    "docs/spec/mechcore/session.md",
}
ALGORITHM_CONTRACT = {
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


def main():
    problems = []
    paths = tracked_markdown()
    check_links(paths, problems.append)
    check_readme_naming(problems.append)
    check_spec_classification(paths, problems.append)
    check_spec_structure(problems.append)

    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    converted = len(DOCUMENT_FORMAT | INTERFACE_CONTRACT | ALGORITHM_CONTRACT) - len(PENDING)
    total = len(DOCUMENT_FORMAT | INTERFACE_CONTRACT | ALGORITHM_CONTRACT)
    print(f"{len(paths)} markdown files, {converted}/{total} specs on the convention, "
          f"{len(PENDING)} pending")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
