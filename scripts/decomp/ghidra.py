#!/usr/bin/env python3
"""Decompile the installed game's methods to C with Ghidra.

    scripts/decomp/ghidra.py prepare [--game APP] [--force]
    scripts/decomp/ghidra.py decompile [--game APP] [--out DIR] METHOD...

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

`decompile` writes each METHOD's C to `work/decomp/<build>/ghidra/c/`, or
`--out`. A METHOD is `Class.Method` or `Namespace.Class$$Method`; every
overload is written. An interface call in the C reads a slot of an interface's
vtable, which `dump.cs` names: each one found is annotated with the method in
that slot.

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
# integer names. The C++ base classes il2cpp.h writes are flattened by
# `ghidra_header`.
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


STRUCT = re.compile(r"^struct (\w+)(?: : (\w+))? \{\n(.*?)^\};\n", re.M | re.S)
MEMBER = re.compile(r"(\w+);$", re.M)


def ghidra_header(dumper):
    """il2cpp.h as Ghidra's C parser reads it, beside it.

    A derived class's `X_Fields : Base_Fields` gets its bases' fields written
    inline ahead of its own. IL2CPP lays a derived field right after the
    base's last one, at the field's own alignment, which is what C does to
    the flattened fields; a `Base_Fields super;` member would instead start
    them after the base struct's tail padding. A base field the class hides
    with one of its own name is written `base_<name>`.
    """
    target = dumper / "il2cpp_ghidra.h"
    source = (dumper / "il2cpp.h").read_text()
    bodies = {}

    def flatten(found):
        name, base, body = found.groups()
        if base is not None:
            if base not in bodies:
                fail(f"il2cpp.h: {name} derives from {base}, not defined before it")
            own = set(MEMBER.findall(body))
            taken = own | set(MEMBER.findall(bodies[base]))

            def hidden(member):
                if member[1] not in own:
                    return member[0]
                renamed = f"base_{member[1]}"
                while renamed in taken:
                    renamed = f"base_{renamed}"
                taken.add(renamed)
                return f"{renamed};"

            body = MEMBER.sub(hidden, bodies[base]) + body
        bodies[name] = body
        return f"struct {name} {{\n{body}}};\n"

    target.write_text(GHIDRA_HEADER + STRUCT.sub(flatten, source))
    return target


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
    """Each METHOD as the full names script.json gives it."""
    script = json.loads((build_dir / "il2cppdumper" / "script.json").read_text())
    names = {method["Name"] for method in script["ScriptMethod"]}
    resolved = []
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
            fail(f"no method named {name}")
        resolved.extend(match.replace(" ", "_") for match in matches)
    return resolved


def decompile(build_dir, wanted, out):
    require("ghidra-analyzeHeadless", "nix profile add nixpkgs#ghidra")
    ghidra = build_dir / "ghidra"
    if not (ghidra / "project" / f"{PROJECT}.gpr").exists():
        fail("no Ghidra project for this build; run `ghidra.py prepare` first")
    names = method_names(build_dir, wanted)
    out.mkdir(parents=True, exist_ok=True)
    headless(
        ghidra / "project", "-process", SLICE, "-noanalysis", "-readOnly",
        "-postScript", "DecompileMethods.java", str(out), *names,
        log=ghidra / "decompile.log",
    )
    slots = InterfaceSlots(build_dir / "il2cppdumper" / "dump.cs")
    for name in names:
        for path in sorted(out.glob(f"{glob_escape(name)}*.c")):
            path.write_text(slots.annotate(path.read_text()))
            say(f"wrote {path}")


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
    # `(object, interface TypeInfo, slot)`, answering the vtable entry.
    LOOKUP = re.compile(r"(func_0x[0-9a-f]+\(\s*\w+\s*,\s*_?([\w$]+)_TypeInfo\s*,\s*(0x[0-9a-f]+|\d+)\s*\))")

    def annotate(self, text):
        """Each interface lookup, with the method its slot holds."""
        named = {}
        for (name, slot), method in self.slots.items():
            named[(name.replace(".", "_"), slot)] = f"{name}.{method}"

        def note(found):
            slot = int(found.group(3), 0)
            method = named.get((found.group(2), slot))
            return found.group(1) if method is None else f"{found.group(1)} /* {method} */"

        return self.LOOKUP.sub(note, text)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--game", type=pathlib.Path, default=DEFAULT_GAME)
    steps = parser.add_subparsers(dest="step", required=True)
    prepare_step = steps.add_parser("prepare", help="make the Ghidra project for the build")
    prepare_step.add_argument("--force", action="store_true", help="redo every part of it")
    decompile_step = steps.add_parser("decompile", help="write named methods' C")
    decompile_step.add_argument("--out", type=pathlib.Path)
    decompile_step.add_argument("methods", nargs="+", metavar="METHOD")
    args = parser.parse_args()
    work = work_root()
    build_dir = work / "decomp" / build_of(args.game)
    if args.step == "prepare":
        prepare(args.game, build_dir, work, args.force)
    else:
        decompile(build_dir, args.methods, args.out or build_dir / "ghidra" / "c")
    return 0


if __name__ == "__main__":
    sys.exit(main())
