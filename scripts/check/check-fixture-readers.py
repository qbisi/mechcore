#!/usr/bin/env python3
"""Check that no crate's code reads a fixture under the root tests/ directory.

crates/README.md says why: a fixture is the game's evidence, recorded again
whenever the rules move, and verify alone reads it. This finds every string
literal in a Rust source file's code, comments left out, that names a path
reaching the root tests/ from where the code could be resolving it: the file's
directory (include_str!, include_bytes!), the crate's (a path joined to
CARGO_MANIFEST_DIR, or a test's working directory) or the repository's. A
path that only reaches a crate's own tests/ is the crate's and passes; a
comment citing a fixture as evidence reads nothing and passes.

Run from the repository root: python3 scripts/check/check-fixture-readers.py
"""

import os
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / "tests"
LITERAL = re.compile(r'"((?:[^"\\\n]|\\.)*)"')


def code_of(line: str) -> str:
    """The line without a `//` comment, leaving `//` inside a string alone."""
    in_string = False
    escaped = False
    for at, character in enumerate(line):
        if escaped:
            escaped = False
        elif character == "\\":
            escaped = True
        elif character == '"':
            in_string = not in_string
        elif not in_string and line.startswith("//", at):
            return line[:at]
    return line


def reaches_fixtures(literal: str, source: Path, crate: Path) -> bool:
    """Whether the literal, read from any of the places code resolves a path
    from, names something under the root tests/."""
    if "tests" not in literal.split("/"):
        return False
    for base in (source.parent, crate, REPO):
        resolved = Path(os.path.normpath(base / literal))
        if resolved == FIXTURES or FIXTURES in resolved.parents:
            if resolved.exists():
                return True
    return False


def findings(source: Path, text: str) -> list[str]:
    crate = REPO / "crates" / source.relative_to(REPO / "crates").parts[0]
    found = []
    for number, line in enumerate(text.splitlines(), start=1):
        for literal in LITERAL.findall(code_of(line)):
            if reaches_fixtures(literal, source, crate):
                found.append(f"{source.relative_to(REPO)}:{number}: reads {literal!r}")
    return found


def selftest() -> None:
    """The cases the rule turns on, against a source file of the document crate."""
    source = REPO / "crates/document/src/lib.rs"
    passes = [
        '/// `tests/marksman/vs-arclight.yaml` pins it.',
        'let x = 1; // read tests/marksman/vs-arclight.yaml by hand',
        'include_str!("../tests/fixtures/invalid-unit-collision.yaml")',
    ]
    caught = [
        'include_bytes!("../../../tests/marksman/vs-arclight.yaml")',
        'PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/marksman/vs-arclight.yaml")',
        'repository().join("tests/marksman/vs-arclight.yaml")',
        'root.join("tests")',
    ]
    for line in passes:
        assert not findings(source, line), line
    for line in caught:
        assert findings(source, line), line


def main() -> int:
    selftest()
    found = []
    for source in sorted((REPO / "crates").rglob("*.rs")):
        if "target" in source.relative_to(REPO).parts:
            continue
        found += findings(source, source.read_text(encoding="utf-8"))
    for line in found:
        print(line)
    if found:
        print(
            f"\n{len(found)} read(s) of the root tests/ from crate code; crates/README.md "
            "says a test writes the fight it needs instead",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
