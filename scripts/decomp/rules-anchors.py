#!/usr/bin/env python3
"""Hold every rules document's anchors to the game version it describes.

    python3 scripts/decomp/rules-anchors.py                 every anchor resolves in this version's dump
    python3 scripts/decomp/rules-anchors.py --since OLD     which read claims a move from OLD may have changed

A rules document ends in `## Evidence`, and each item under its `### Read`
names the build members the claim was read from, as `Class.member` in
backticks, `Outer.Inner.member` for a nested class: a method, a field or a
property the class itself declares, not a class nested in it. Those are the
document's anchors. They are what a claim read from the build rests on, and
what can move under it when the game does.
Namespace prefixes and explicit interface member names keep their dots;
generic arity in the dump's filename is not part of the anchor.

Without arguments every anchor has to resolve in `work/decomp/<version>/`, the
version `GAME_VERSION` names. With `--since OLD` each anchor is compared with
the same member in `work/decomp/<OLD>/`: a method whose instructions differ once
addresses and field offsets are normalised, a field or property whose
declaration differs, or a
member that is gone. Each claim resting on one is listed for re-reading. A
claim whose anchors all stand still is left alone: the build does what it did.

`scripts/decomp/decompile.py` generates the installed version's dump locally.
"""

import argparse
import collections
import functools
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
RULES = ROOT / "docs" / "rules"
ANCHOR = re.compile(r"`([A-Z][A-Za-z0-9_]*(?:\.[A-Z][A-Za-z0-9_]*)*)\.([A-Za-z_][A-Za-z0-9_]*)`")
# What moves between two builds without the logic moving: addresses, jump
# targets, field offsets, which shift whenever a class gains a field, and the
# padding (`Nop`) between methods.
ADDRESS = (re.compile(r"0x[0-9A-Fa-f]{4,}"), re.compile(r"\b[0-9A-Fa-f]{6,}h\b"), re.compile(r"\{\d+\}"),
           re.compile(r"(?<=\[)(r\w+|stack)\+\d+(?=[\]+])"))
DECLARATION = re.compile(r"^\s*(?:public|private|protected|internal)\b")


def read_items(path):
    """(claim, [(class, member)]) for every item under the document's `### Read`."""
    lines = path.read_text().splitlines()
    items, section, current = [], None, None
    for line in lines:
        if line.startswith("## "):
            section = line[3:].strip() if line[3:].strip() == "Evidence" else None
            current = None
            continue
        if section != "Evidence":
            continue
        if line.startswith("### "):
            section_part = line[4:].strip()
            current = [] if section_part == "Read" else None
            continue
        if current is None:
            continue
        if line.startswith("- "):
            items.append([line[2:], ANCHOR.findall(line)])
        elif line.startswith("  ") and items:
            items[-1][0] += " " + line.strip()
            items[-1][1] += ANCHOR.findall(line)
    return [(text, anchors) for text, anchors in items]


class Dump:
    """One version's C# stubs and instruction dump, by class name."""

    def __init__(self, version):
        base = ROOT / "work" / "decomp" / version / "cpp2il"
        if not (base / "DiffableCs").is_dir():
            sys.exit(f"rules-anchors: no dump of {version}; run scripts/decomp/decompile.py "
                     "on a machine with that game version installed")
        self.stubs = collections.defaultdict(list)
        for stub in sorted((base / "DiffableCs").rglob("*.cs")):
            # Generic arity is in the filename, but not the C# type declaration.
            outer = re.sub(r"`\d+", "", stub.stem.split("_NestedType_")[0])
            namespace = re.search(r"^namespace ([\w.]+);", stub.read_text(errors="replace"), re.MULTILINE)
            parts = namespace.group(1).split(".") if namespace else []
            # Accept a short type name or a namespace-qualified one. Keep the
            # actual stub path for locating its generic instruction dump.
            for start in range(len(parts) + 1):
                self.stubs[".".join([*parts[start:], outer])].append(stub)
        self.isil = base / "IsilDump"
        self.base = base

    @functools.cache
    def members(self, cls):
        """(instruction path, declaration line, member lines) of each type `cls` names.

        A type's own members are indented one tab deeper than its declaration,
        so a nested type's members are neither its outer type's nor the
        reverse."""
        parts = cls.split(".")
        split = next((i for i in range(len(parts), 0, -1) if ".".join(parts[:i]) in self.stubs), None)
        if split is None:
            return []
        outer, nested = parts[split - 1], parts[split:]
        found = []
        for stub in self.stubs[".".join(parts[:split])]:
            lines, head = stub.read_text(errors="replace").splitlines(), None
            for depth, name in enumerate([outer, *nested]):
                indent = "\t" * depth
                start = next((i for i, line in enumerate(lines) if re.match(
                    rf"{indent}(?!\t).*\b(class|struct|enum|interface) {re.escape(name)}\b", line)), None)
                if start is None:
                    lines = None
                    break
                end = next((i for i in range(start + 1, len(lines)) if lines[i] == indent + "}"), len(lines))
                head, lines = lines[start], lines[start + 1:end]
            if lines is not None:
                own = [line for line in lines if re.match(rf"\t{{{len(nested) + 1}}}(?!\t)", line)]
                dump = (self.isil / stub.relative_to(self.base / "DiffableCs")).with_name(
                    "_NestedType_".join([stub.stem, *nested]) + ".txt")
                found.append((dump, head, own))
        return found

    @functools.cache
    def resolve(self, cls, member):
        """Separate the declaring type from an explicit interface member.

        Dots can belong to a namespace, a nested type, or an explicitly
        implemented interface. Only the dump can decide where the type ends.
        """
        parts = cls.split(".")
        for split in range(len(parts), 0, -1):
            owners = self.members(".".join(parts[:split]))
            if owners:
                return owners, ".".join([*parts[split:], member])
        return [], member

    @functools.cache
    def declarations(self, cls, member):
        found = []
        owners, member = self.resolve(cls, member)
        # Short member anchors also name explicit implementations. When an
        # interface is given, match its complete name rather than a suffix.
        boundary = r"(?<![\w.])" if "." in member else r"\b"
        for _, head, lines in owners:
            # An enum's values are declared bare, as `Name = 0,`.
            enum = re.search(r"\benum\b", head)
            for line in lines:
                code = line.split("//")[0]
                if DECLARATION.match(line) and re.search(rf"{boundary}(get_|set_)?{re.escape(member)}\b\s*[(;{{=]", code) \
                        or enum and re.match(rf"\s*{re.escape(member)}\s*=", code):
                    found.append(" ".join(code.split()))
        return found

    @functools.cache
    def bodies(self, cls, member):
        found = []
        owners, member = self.resolve(cls, member)
        boundary = r"(?<![\w.])" if "." in member else r"\b"
        for dump, _, _ in owners:
            if not dump.exists():
                continue
            for section in dump.read_text(errors="replace").split("\nMethod: ")[1:]:
                head = section.splitlines()[0]
                if not re.search(rf"{boundary}(get_|set_)?{re.escape(member)}\(", head):
                    continue
                body = []
                for line in section.splitlines():
                    matched = re.match(r"\s*\d+ (.*)", line)
                    if matched and "initialize_runtime_metadata" not in line \
                            and matched.group(1).strip() != "Nop":
                        text = matched.group(1)
                        for pattern in ADDRESS:
                            text = pattern.sub("A", text)
                        body.append(text)
                found.append((head, body))
        return found


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--since", help="an older version under work/decomp to compare every anchor with")
    arguments = parser.parse_args()
    version = build_data.build()
    new = Dump(version)
    old = Dump(arguments.since) if arguments.since else None

    unresolved, moved, counted = [], [], 0
    for path in sorted(RULES.glob("*.md")):
        for claim, anchors in read_items(path):
            for cls, member in anchors:
                counted += 1
                declared = new.declarations(cls, member)
                if not declared:
                    unresolved.append(f"{path.relative_to(ROOT)}: `{cls}.{member}` is not declared in {version}")
                    continue
                if old and (old.declarations(cls, member) != declared
                            or old.bodies(cls, member) != new.bodies(cls, member)):
                    moved.append((path.relative_to(ROOT), f"{cls}.{member}", claim))

    for line in unresolved:
        print(f"error: {line}", file=sys.stderr)
    if old:
        for path, anchor, claim in moved:
            print(f"{path}: `{anchor}` moved since {arguments.since}: {claim[:120]}")
        print(f"{counted} anchors; {len(moved)} moved since {arguments.since}")
    else:
        print(f"{counted} anchors resolve in {version}" if not unresolved else f"{len(unresolved)} of {counted} anchors do not resolve")
    return 1 if unresolved else 0


if __name__ == "__main__":
    sys.exit(main())
