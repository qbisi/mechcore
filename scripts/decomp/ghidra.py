#!/usr/bin/env python3
"""Decompile the installed game's methods to C with Ghidra.

    scripts/decomp/ghidra.py prepare [--game APP] [--force]
    scripts/decomp/ghidra.py decompile [--game APP] [--out DIR] [--force] METHOD...

`decompile.py`'s ISIL is the build instruction by instruction; this is the
decompiler's C for the methods asked for, with names. `prepare` makes the
Ghidra project once per build:

    work/decomp/<build>/il2cppdumper/   Il2CppDumper's dump.cs, script.json and il2cpp.h
    work/decomp/<build>/ghidra/         the arm64 slice and the Ghidra project it is in

Il2CppDumper reads the IL2CPP metadata against the arm64 slice of
`GameAssembly.dylib`, the one the game runs, so an address in the C is one in
the running process. `ghidra-analyzeHeadless` imports the slice without
auto-analysis, which takes hours on it, and `ghidra/ApplyIl2Cpp.java` makes a
function at every method entry, named `Namespace.Class$$Method`, and a label
at every metadata usage (`..._TypeInfo`, `Method$...`). Then
`ghidra/ApplyIl2CppTypes.java` parses `il2cpp.h`, rewritten for Ghidra's C
parser, into the program, and types every method with its C signature and
every metadata label with its class: the decompiler reads a field by name,
`this->fields.moveRange`, where it would read an offset.

`decompile` writes each METHOD's C to `work/decomp/<build>/ghidra/c/`, and
copies it to `--out` when one is given. A METHOD is `Class.Method` or
`Namespace.Class$$Method`; every overload is written. A name `script.json`
does not have is reported and the rest are still written. That directory is
a cache: a method already in it is not decompiled again unless `--force`.
Ghidra runs on a clone of the project (`cp -c`, which APFS makes without
copying), so sessions decompiling at once never wait on its lock; where
the clone cannot be made it runs on the project itself. An interface call in
the C reads a slot of an interface's vtable, which `dump.cs` names: each one
found, through the runtime's lookup or the inline search of the class's
interface offsets, is annotated with the method in that slot.

Everything this writes goes under the main checkout's `work/`, from a
worktree as well, beside the build `decompile.py` made. It needs `dotnet`
(`nix profile add nixpkgs#dotnet-runtime_8`) and Ghidra
(`nix profile add nixpkgs#ghidra`) on the path; Il2CppDumper is pinned below
and fetched into `work/tools/` on first use.
"""

import argparse
import hashlib
import json
import os
import pathlib
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

SCRIPTS = pathlib.Path(__file__).resolve().parent / "ghidra"
DEFAULT_GAME = pathlib.Path.home() / (
    "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app"
)
IL2CPPDUMPER = {
    "name": "Il2CppDumper",
    "version": "6.7.46",
    "url": "https://github.com/Perfare/Il2CppDumper/releases/download/v6.7.46/Il2CppDumper-net6-v6.7.46.zip",
    "sha256": "db9bbbc538e33abfb057c7757ae5d6c1f16a05fdc0d13af8a5a67ea31faaba0c",
    "file": "Il2CppDumper-net6-v6.7.46.zip",
}
BINARY = "Contents/Frameworks/GameAssembly.dylib"
METADATA = "Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat"
PROJECT = "GameAssembly"
SLICE = "GameAssembly-arm64.dylib"
HEAP = os.environ.get("GHIDRA_HEADLESS_MAXMEM", "5G")


def say(message):
    print(f"ghidra: {message}", flush=True)


def fail(message):
    print(f"ghidra: {message}", file=sys.stderr)
    sys.exit(1)


def work_root():
    """The main checkout's `work/`, whichever checkout this runs from."""
    common = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=SCRIPTS, check=True, capture_output=True, text=True,
    ).stdout.strip()
    return pathlib.Path(common).parent / "work"


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def build_of(app):
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    build = info.get("CFBundleShortVersionString")
    if not build or not re.fullmatch(r"[0-9]+(\.[0-9]+)+", build):
        fail(f"{app}: no build number in CFBundleShortVersionString ({build!r})")
    return build


def require(program, install):
    if shutil.which(program) is None:
        fail(f"{program} is not on the path: {install}")


def il2cppdumper(work):
    tools = work / "tools"
    archive = tools / IL2CPPDUMPER["file"]
    if not archive.exists() or sha256(archive) != IL2CPPDUMPER["sha256"]:
        tools.mkdir(parents=True, exist_ok=True)
        say(f"downloading {IL2CPPDUMPER['name']} {IL2CPPDUMPER['version']}")
        partial = archive.with_name(archive.name + ".part")
        with urllib.request.urlopen(IL2CPPDUMPER["url"], timeout=120) as response:
            partial.write_bytes(response.read())
        if sha256(partial) != IL2CPPDUMPER["sha256"]:
            partial.unlink()
            fail(f"{IL2CPPDUMPER['url']} does not have the pinned SHA-256")
        os.replace(partial, archive)
    directory = tools / f"Il2CppDumper-v{IL2CPPDUMPER['version']}"
    stamp = directory / ".sha256"
    if not stamp.exists() or stamp.read_text().strip() != IL2CPPDUMPER["sha256"]:
        with zipfile.ZipFile(archive) as zipped:
            zipped.extractall(directory)
        # Headless: it would wait for a key before exiting.
        config = directory / "config.json"
        settings = json.loads(config.read_text(encoding="utf-8-sig"))
        settings["RequireAnyKey"] = False
        config.write_text(json.dumps(settings, indent=2))
        stamp.write_text(IL2CPPDUMPER["sha256"] + "\n")
    return directory / "Il2CppDumper.dll"


# What Ghidra's C parser lacks of what il2cpp.h assumes: the fixed-width
# integer names. The C++ base classes il2cpp.h writes are flattened, and the
# fields laid out where `dump.cs` has them, by `ghidra_header`.
GHIDRA_HEADER = """typedef unsigned __int8 uint8_t;
typedef unsigned __int16 uint16_t;
typedef unsigned __int32 uint32_t;
typedef unsigned __int64 uint64_t;
typedef __int8 int8_t;
typedef __int16 int16_t;
typedef __int32 int32_t;
typedef __int64 int64_t;
typedef __int64 intptr_t;
typedef __int64 uintptr_t;
typedef unsigned __int64 size_t;
typedef _Bool bool;
"""
# The size of each scalar a field of il2cpp.h has; any other field is a
# pointer or a value type's `_o`.
SCALARS = {
    "bool": 1, "int8_t": 1, "uint8_t": 1, "int16_t": 2, "uint16_t": 2,
    "int32_t": 4, "uint32_t": 4, "float": 4, "int64_t": 8, "uint64_t": 8,
    "double": 8, "intptr_t": 8, "uintptr_t": 8,
}
# A thread-static field's offset in `dump.cs`: it is not in `static_fields`.
THREAD_STATIC = 0x80000000


STRUCT = re.compile(r"^struct (\w+)(?: : (\w+))? \{\n(.*?)^\};\n", re.M | re.S)
MEMBER = re.compile(r"^\t(.+?)\s*\b(\w+);$", re.M)


def ghidra_header(dumper):
    """il2cpp.h as Ghidra's C parser reads it, beside it.

    A derived class's `X_Fields : Base_Fields` gets its bases' fields written
    inline ahead of its own. IL2CPP lays a derived field right after the
    base's last one, at the field's own alignment, which is what C does to
    the flattened fields; a `Base_Fields super;` member would instead start
    them after the base struct's tail padding. A base field the class hides
    with one of its own name is written `base_<name>`.

    Where C would still put a field elsewhere than `dump.cs` has it, the
    struct is written out at `dump.cs`'s offsets: an explicit layout's
    overlapping fields as a union `at_<offset>`, its gaps as bytes, a packed
    struct's under `#pragma pack`. A thread-static field is left out of
    `_StaticFields`, which does not hold it, and an empty value type gets a
    byte, which C# gives it and C does not.
    """
    target = dumper / "il2cpp_ghidra.h"
    source = (dumper / "il2cpp.h").read_text()
    header = Header(source, FieldOffsets(dumper / "dump.cs"))
    target.write_text(GHIDRA_HEADER + STRUCT.sub(header.write, source))
    say(
        f"{header.checked} structs checked against dump.cs, {len(header.moved)} laid out at its offsets"
        + (f", {len(header.unplaced)} it could not place: {', '.join(header.unplaced[:5])}"
           if header.unplaced else "")
    )
    return target


class FieldOffsets:
    """`dump.cs`'s field offsets, looked up by il2cpp.h's struct name.

    A class's instance offsets count its object header, which `_Fields` does
    not. A nested type has no namespace line: it takes the namespace of the
    next type named as its outermost one, which `dump.cs` writes right after
    its nested types, or else it is found by the end of the struct's name.
    Il2CppDumper numbers a name it repeats, `X_1`. Either way the fields'
    names decide between the types a name could be. A generic definition's
    fields are at 0x0 in `dump.cs`, and its instances are not in it.
    """

    TYPE = re.compile(r"^(?:[a-z]+ )*(class|struct) (\S+) (?::.*)?// TypeDefIndex")
    FIELD = re.compile(r"^\t(.*?) (\S+); // 0x([0-9A-F]+)$")

    def __init__(self, dump):
        self.named = {}
        self.nested = {}
        pending = {}
        namespace = None
        found = None
        with open(dump, encoding="utf-8") as lines:
            for line in lines:
                if line.startswith("// Namespace: "):
                    namespace = line[len("// Namespace: "):].strip()
                elif (match := self.TYPE.match(line)) is not None:
                    kind, name = match.groups()
                    if re.search(r"\w<", name):
                        continue  # a generic definition: no offsets of its own
                    found = {"class": kind == "class", "instance": [], "static": []}
                    if namespace or "." not in name:
                        for inner, nested in pending.pop(name, []) + [(name, found)]:
                            key = fixed(f"{namespace}.{inner}" if namespace else inner)
                            self.named.setdefault(key, []).append(nested)
                    else:
                        pending.setdefault(name.split(".")[0], []).append((name, found))
                elif line.startswith("}"):
                    found = None
                elif found is not None and (field := self.FIELD.match(line)) is not None:
                    modifiers, name, offset = field.groups()
                    kind = "static" if re.search(r"\bstatic\b", modifiers) else "instance"
                    found[kind].append((fixed(name), int(offset, 16)))
        for nested in pending.values():
            for name, found in nested:
                self.nested.setdefault(fixed(name), []).append(found)

    def of(self, struct, members):
        """Each member's offset in a `_Fields` or `_StaticFields` struct, or None."""
        if struct.endswith("_StaticFields"):
            name, kind = struct[: -len("_StaticFields")], "static"
        elif struct.endswith("_Fields"):
            name, kind = struct[: -len("_Fields")], "instance"
        else:
            return None
        names = [member for _, member in members]
        answers = set()
        for found in self.candidates(name):
            fields = found[kind]
            if len(fields) == len(names) and all(
                member in (field, "_" + field) for (field, _), member in zip(fields, names)
            ):
                base = 0x10 if found["class"] and kind == "instance" else 0
                answers.add(tuple(offset - base for _, offset in fields))
        return list(answers.pop()) if len(answers) == 1 else None

    def candidates(self, name):
        names = [name]
        if (numbered := re.fullmatch(r"(\w+)_\d+", name)) is not None:
            names.append(numbered.group(1))
        for name in names:
            yield from self.named.get(name, [])
            parts = name.split("_")
            for start in range(1, len(parts)):
                yield from self.nested.get("_".join(parts[start:]), [])


def fixed(name):
    """A C# name as Il2CppDumper writes it in C."""
    return re.sub(r"\W", "_", name)


class Header:
    """il2cpp.h's structs, each flattened and laid out as `dump.cs` has it."""

    def __init__(self, source, offsets):
        self.offsets = offsets
        self.structs = {
            name: (base or None, MEMBER.findall(body)) for name, base, body in STRUCT.findall(source)
        }
        self.flat = {}
        self.bodies = {}
        self.layouts = {}
        self.checked = 0
        self.moved = []
        self.unplaced = []

    def write(self, found):
        name = found.group(1)
        if not name.endswith("Fields"):
            return f"struct {name} {{\n{found.group(3)}}};\n"
        return self.body(name)

    def members(self, name):
        """The struct's members with bases' inline, each `(type, name, offset or None)`."""
        if name in self.flat:
            return self.flat[name]
        base, own = self.structs[name]
        offsets = self.offsets.of(name, own) or [None] * len(own)
        members = [(kind, member, offset) for (kind, member), offset in zip(own, offsets)]
        if base is not None:
            if base not in self.structs:
                fail(f"il2cpp.h: {name} derives from {base}, which it does not define")
            names = {member for _, member in own}
            taken = names | {member for _, member, _ in self.members(base)}
            inherited = []
            for kind, member, offset in self.members(base):
                if member in names:
                    renamed = f"base_{member}"
                    while renamed in taken:
                        renamed = f"base_{renamed}"
                    taken.add(renamed)
                    member = renamed
                inherited.append((kind, member, offset))
            members = inherited + members
        members = [member for member in members if (member[2] or 0) < THREAD_STATIC]
        if not members and self.is_value_type(name):
            members = [("uint8_t", "_empty", None)]
        self.flat[name] = members
        return members

    def is_value_type(self, name):
        if not name.endswith("_Fields"):
            return False
        boxed = self.structs.get(name[: -len("_Fields")] + "_o")
        return boxed is not None and all(member != "klass" for _, member in boxed[1])

    def body(self, name):
        """The struct's C, and its layout in `self.layouts`."""
        if name in self.bodies:
            return self.bodies[name]
        members = self.members(name)
        sized = [(kind, member, offset) + self.size(kind) for kind, member, offset in members]
        natural = Layout.natural(sized)
        wanted = [offset for _, _, offset, _, _ in sized]
        if any(offset is not None for offset in wanted):
            self.checked += 1
        if all(want in (None, have) for want, have in zip(wanted, natural.offsets)):
            text = "".join(f"\t{kind} {member};\n" for kind, member, *_ in sized)
            layout = natural
        else:
            placed = Layout.place(sized)
            if placed is None:
                self.unplaced.append(name)
                text = "".join(f"\t{kind} {member};\n" for kind, member, *_ in sized)
                layout = natural
            else:
                self.moved.append(name)
                text, layout = placed
        pack = "" if layout.pack is None else f"#pragma pack(push, {layout.pack})\n"
        self.bodies[name] = (
            f"{pack}struct {name} {{\n{text}}};\n" + ("#pragma pack(pop)\n" if pack else "")
        )
        self.layouts[name] = layout
        return self.bodies[name]

    def size(self, kind):
        """`(size, alignment)` of a member's C type."""
        if kind.endswith("*"):
            return 8, 8
        if kind in SCALARS:
            return SCALARS[kind], SCALARS[kind]
        name = kind.removeprefix("struct ").strip()
        if name not in self.layouts:
            if name not in self.structs:
                fail(f"il2cpp.h: a field has type {kind}, which it does not define")
            if name.endswith("Fields"):
                self.body(name)
            else:
                members = self.structs[name][1]
                self.layouts[name] = Layout.natural(
                    [(kind, member, None) + self.size(kind) for kind, member in members]
                )
        layout = self.layouts[name]
        return layout.size, layout.align


class Layout:
    """Where C puts a struct's members, and the struct's size and alignment."""

    def __init__(self, offsets, size, align, pack=None):
        self.offsets, self.size, self.align, self.pack = offsets, size, align, pack

    @staticmethod
    def natural(members):
        offsets, end, align = [], 0, 1
        for *_, size, alignment in members:
            end = -(-end // alignment) * alignment
            offsets.append(end)
            end += size
            align = max(align, alignment)
        return Layout(offsets, -(-end // align) * align, align)

    @staticmethod
    def place(members):
        """The members at their wanted offsets: `(C, Layout)`, or None.

        A member with no wanted offset follows the one before it.
        """
        items, end = [], 0
        for index, (kind, member, offset, size, align) in enumerate(members):
            if offset is None:
                offset = -(-end // align) * align
            items.append((offset, index, f"{kind} {member};", size, align))
            end = offset + size
        items.sort()
        for pack in (8, 4, 2, 1):
            written = Layout.struct(items, 0, pack, "\t")
            if written is not None:
                lines, size, align = written
                offsets = [offset for offset, *_ in sorted(items, key=lambda item: item[1])]
                return "".join(lines), Layout(offsets, size, align, None if pack == 8 else pack)
        return None

    @staticmethod
    def struct(items, start, pack, indent):
        """Members sorted by offset as a struct at `start`: `(lines, size, align)`, or None."""
        lines, end, align = [], 0, 1
        for cluster in Layout.clusters(items):
            at = cluster[0][0] - start
            if len(cluster) == 1:
                _, _, line, size, alignment = cluster[0]
                written = [indent + line + "\n"]
                alignment = min(alignment, pack)
            else:
                union = Layout.union(cluster, pack, indent)
                if union is None:
                    return None
                written, size, alignment = union
            if at < end or at % alignment:
                return None
            if at > end:
                lines.append(f"{indent}uint8_t _gap_{start + end:x}[{at - end}];\n")
            lines += written
            end = at + size
            align = max(align, alignment)
        return lines, -(-end // align) * align, align

    @staticmethod
    def union(cluster, pack, indent):
        """Overlapping members as a union of runs that do not overlap."""
        start = cluster[0][0]
        runs = []
        for item in cluster:
            for run in runs:
                if run[-1][0] + run[-1][3] <= item[0]:
                    run.append(item)
                    break
            else:
                runs.append([item])
        lines, size, align = [f"{indent}union {{\n"], 0, 1
        for run in runs:
            if len(run) == 1 and run[0][0] == start:
                _, _, line, length, alignment = run[0]
                lines.append(f"{indent}\t{line}\n")
                alignment = min(alignment, pack)
            else:
                written = Layout.struct(run, start, pack, indent + "\t\t")
                if written is None:
                    return None
                body, length, alignment = written
                name = run[0][2].rstrip(";").split()[-1]
                lines += [f"{indent}\tstruct {{\n", *body, f"{indent}\t}} as_{name};\n"]
            size, align = max(size, length), max(align, alignment)
        lines.append(f"{indent}}} at_{start:x};\n")
        return lines, -(-size // align) * align, align

    @staticmethod
    def clusters(items):
        """The members in runs whose bytes overlap."""
        cluster, end = [], None
        for item in items:
            if cluster and item[0] >= end:
                yield cluster
                cluster = []
            if not cluster:
                end = item[0] + item[3]
            cluster.append(item)
            end = max(end, item[0] + item[3])
        if cluster:
            yield cluster


def headless(project_dir, *arguments, log):
    command = [
        "ghidra-analyzeHeadless", str(project_dir), PROJECT,
        "-scriptPath", str(SCRIPTS), *arguments,
    ]
    environment = dict(os.environ, GHIDRA_HEADLESS_MAXMEM=HEAP)
    with open(log, "w") as out:
        result = subprocess.run(command, stdout=out, stderr=subprocess.STDOUT, env=environment)
    if result.returncode != 0:
        fail(f"ghidra-analyzeHeadless failed; see {log}")


def prepare(app, build_dir, work, force):
    require("dotnet", "nix profile add nixpkgs#dotnet-runtime_8")
    require("ghidra-analyzeHeadless", "nix profile add nixpkgs#ghidra")
    ghidra = build_dir / "ghidra"
    dumper = build_dir / "il2cppdumper"
    slice_path = ghidra / SLICE
    ghidra.mkdir(parents=True, exist_ok=True)
    if force or not slice_path.exists():
        say("taking the arm64 slice")
        subprocess.run(
            ["lipo", str(app / BINARY), "-thin", "arm64", "-output", str(slice_path)], check=True
        )
    if force or not (dumper / "script.json").exists():
        say("running Il2CppDumper")
        dumper.mkdir(parents=True, exist_ok=True)
        dll = il2cppdumper(work)
        subprocess.run(
            ["dotnet", str(dll), str(slice_path), str(app / METADATA), str(dumper) + "/"],
            check=True, cwd=dumper, env=dict(os.environ, DOTNET_ROLL_FORWARD="Major"),
            stdout=subprocess.DEVNULL,
        )
    project = ghidra / "project"
    if force or not (project / f"{PROJECT}.gpr").exists():
        say("importing into Ghidra and naming methods (about half an hour)")
        project.mkdir(parents=True, exist_ok=True)
        headless(
            project, "-import", str(slice_path), "-noanalysis", "-overwrite",
            "-postScript", "ApplyIl2Cpp.java", str(dumper / "script.json"),
            log=ghidra / "import.log",
        )
    typed = ghidra / "types.done"
    if force or not typed.exists():
        say("typing it from il2cpp.h and script.json (about twenty minutes)")
        header = ghidra_header(dumper)
        headless(
            project, "-process", SLICE, "-noanalysis",
            "-postScript", "ApplyIl2CppTypes.java", str(dumper / "script.json"), str(header),
            log=ghidra / "types.log",
        )
        typed.write_text(IL2CPPDUMPER["version"] + "\n")
    say(f"ready: {project}")


def method_names(build_dir, wanted):
    """Each METHOD as the full names script.json gives it, and those it lacks."""
    script = json.loads((build_dir / "il2cppdumper" / "script.json").read_text())
    names = {method["Name"] for method in script["ScriptMethod"]}
    resolved, unknown = [], []
    for name in wanted:
        if "$$" in name:
            matches = [name] if name in names else []
        else:
            type_name, _, method = name.rpartition(".")
            suffix = f"{type_name}$${method}"
            matches = sorted(
                full for full in names
                if full == suffix or full.endswith("." + suffix)
            )
        if not matches:
            say(f"no method named {name}")
            unknown.append(name)
        resolved.extend(
            match.replace(" ", "_") for match in matches
            if match.replace(" ", "_") not in resolved
        )
    return resolved, unknown


def outputs(directory, name):
    """The files `DecompileMethods.java` writes for a name: `name.c`, or
    `name.<n>.c` for each of its overloads."""
    exact = re.compile(re.escape(name) + r"(?:\.\d+)?\.c")
    return sorted(path for path in directory.glob(f"{glob_escape(name)}*.c") if exact.fullmatch(path.name))


class ProjectClone:
    """A clone of the Ghidra project for one run, or the project itself.

    `cp -c` asks APFS to clone, which shares every block until one is
    written, so it is made at once and takes no space; Ghidra's lock is the
    clone's alone. Where it cannot be made, the run takes the project's own
    lock, and waits on it as before.
    """

    def __init__(self, ghidra):
        self.ghidra = ghidra
        self.temporary = None

    def __enter__(self):
        project = self.ghidra / "project"
        self.temporary = pathlib.Path(tempfile.mkdtemp(prefix="clone-", dir=self.ghidra))
        clone = self.temporary / "project"
        made = subprocess.run(
            ["cp", "-c", "-R", str(project), str(clone)], capture_output=True,
        ).returncode == 0
        if not made:
            shutil.rmtree(self.temporary, ignore_errors=True)
            self.temporary = None
            say("cannot clone the project; running on it")
            return project
        return clone

    def __exit__(self, *_):
        if self.temporary is not None:
            shutil.rmtree(self.temporary, ignore_errors=True)


def decompile(build_dir, wanted, out, force):
    require("ghidra-analyzeHeadless", "nix profile add nixpkgs#ghidra")
    ghidra = build_dir / "ghidra"
    if not (ghidra / "project" / f"{PROJECT}.gpr").exists():
        fail("no Ghidra project for this build; run `ghidra.py prepare` first")
    names, unknown = method_names(build_dir, wanted)
    cache = ghidra / "c"
    cache.mkdir(parents=True, exist_ok=True)
    missing = [name for name in names if force or not outputs(cache, name)]
    if missing:
        logs = ghidra / "logs"
        logs.mkdir(exist_ok=True)
        with ProjectClone(ghidra) as project:
            headless(
                project, "-process", SLICE, "-noanalysis", "-readOnly",
                "-postScript", "DecompileMethods.java", str(cache), *missing,
                log=logs / f"decompile-{os.getpid()}.log",
            )
    slots = InterfaceSlots(build_dir / "il2cppdumper" / "dump.cs")
    failed = []
    for name in names:
        written = outputs(cache, name)
        if not written:
            failed.append(name)
            say(f"{name}: the decompiler wrote nothing")
            continue
        for path in written:
            text = slots.annotate(path.read_text())
            path.write_text(text)
            target = path
            if out is not None and out.resolve() != cache.resolve():
                out.mkdir(parents=True, exist_ok=True)
                target = out / path.name
                target.write_text(text)
            say(f"{'wrote' if name in missing else 'cached'} {target}")
    if unknown or failed:
        fail(f"not written: {', '.join(unknown + failed)}")


def glob_escape(name):
    return re.sub(r"([\[\]*?])", r"[\1]", name)


class InterfaceSlots:
    """`dump.cs`'s interfaces: the method each vtable slot holds."""

    TYPE = re.compile(r"^(?:public |internal |private |protected )*interface (\w+)")
    SLOT = re.compile(r"Slot: (\d+)")

    def __init__(self, dump):
        self.slots = {}
        namespace = None
        interface = None
        pending = None
        with open(dump, encoding="utf-8") as lines:
            for line in lines:
                if line.startswith("// Namespace: "):
                    namespace = line[len("// Namespace: "):].strip()
                    continue
                if (found := self.TYPE.match(line)) is not None:
                    interface = f"{namespace}.{found.group(1)}" if namespace else found.group(1)
                    continue
                if interface is None:
                    continue
                if line.startswith("}"):
                    interface = None
                    continue
                if (slot := self.SLOT.search(line)) is not None:
                    pending = int(slot.group(1))
                    continue
                if pending is not None and "(" in line:
                    method = line.strip().split("(")[0].split()[-1]
                    self.slots[(interface, pending)] = method
                    pending = None

    # The runtime's interface lookup, the slow path of every interface call:
    # `(object, interface TypeInfo, slot)`, answering the vtable entry. A
    # lookup already annotated is left as it is.
    LOOKUP = re.compile(
        r"(func_0x[0-9a-f]+\(\s*\w+\s*,\s*_?([\w$]+)_TypeInfo\s*,\s*(0x[0-9a-f]+|\d+)\s*\))(?!\s*/\*)"
    )
    # The fast path the compiler inlines: a search of the class's interface
    # offsets for the interface's TypeInfo, then the vtable entry that many
    # slots past the offset found.
    INLINE_TYPE = re.compile(r"interfaceType\s*==\s*\(Il2CppClass \*\)_?([\w$]+)_TypeInfo")
    INLINE_SLOT = re.compile(r"\.offset(?:\s*\+\s*(0x[0-9a-f]+|\d+))?\b")
    # How many lines after the TypeInfo's test the slot is read.
    INLINE_REACH = 4

    def annotate(self, text):
        """Each interface lookup, with the method its slot holds."""
        named = {}
        for (name, slot), method in self.slots.items():
            named[(name.replace(".", "_"), slot)] = f"{name}.{method}"

        def note(found):
            slot = int(found.group(3), 0)
            method = named.get((found.group(2), slot))
            return found.group(1) if method is None else f"{found.group(1)} /* {method} */"

        lines = self.LOOKUP.sub(note, text).split("\n")
        for index, line in enumerate(lines):
            tested = self.INLINE_TYPE.search(line)
            if tested is None:
                continue
            for after in range(index + 1, min(index + 1 + self.INLINE_REACH, len(lines))):
                read = self.INLINE_SLOT.search(lines[after])
                if read is None:
                    continue
                method = named.get((tested.group(1), int(read.group(1) or "0", 0)))
                if method is not None and "/*" not in lines[after]:
                    lines[after] += f" /* {method} */"
                break
        return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--game", type=pathlib.Path, default=DEFAULT_GAME)
    steps = parser.add_subparsers(dest="step", required=True)
    prepare_step = steps.add_parser("prepare", help="make the Ghidra project for the build")
    prepare_step.add_argument("--force", action="store_true", help="redo every part of it")
    decompile_step = steps.add_parser("decompile", help="write named methods' C")
    decompile_step.add_argument("--out", type=pathlib.Path, help="copy each method's C here too")
    decompile_step.add_argument("--force", action="store_true", help="decompile methods the cache holds again")
    decompile_step.add_argument("methods", nargs="+", metavar="METHOD")
    args = parser.parse_args()
    work = work_root()
    build_dir = work / "decomp" / build_of(args.game)
    if args.step == "prepare":
        prepare(args.game, build_dir, work, args.force)
    else:
        decompile(build_dir, args.methods, args.out, args.force)
    return 0


if __name__ == "__main__":
    sys.exit(main())
