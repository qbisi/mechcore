#!/usr/bin/env python3
"""Decompile the installed game into work/decomp/<build>, whatever its build.

    scripts/decompile.py [--game APP] [--force STEP[,STEP...]]

`scripts/decomp.py sync` fetches a build someone already decompiled; this makes
one. It reads the build number from the game itself, fetches any tool it lacks
into `work/tools/`, and writes the same shape every reader here expects:

    work/decomp/<build>/cpp2il/IsilDump/          the instruction dump, one file per class
    work/decomp/<build>/cpp2il/DiffableCs/        the C# stubs, with call-graph attributes
    work/decomp/<build>/config-data-container.json    GameRiver.ConfigDataContainer from level0
    work/decomp/<build>/<file>/<Class>.json       every other GameRiver data object, by the file it is in
    work/decomp/<build>/game-manifest.json        the game files and tools it came from
    work/decomp/<build>/index.sqlite              the symbol and call index

Each step is skipped when its output is already there; `--force` names the
steps to redo: `dylib`, `isil`, `cs`, `config`, `manifest`, `index`, or `all`.

Choices a rerun must not change:

- **The x86_64 slice.** `GameAssembly.dylib` is universal. The dump is taken
  from its x86_64 slice, as every build's has been, so two builds' ISIL
  compare line by line. Both slices come from the same IL and the same
  metadata; methods, fields and field offsets are identical, and the Adapter,
  which runs in the arm64 process, finds everything by name. An address in the
  dump is not an address in the running arm64 process.
- **Cpp2IL's processors.** `callanalyzer,attributeinjector` and nothing else:
  they write the `[Calls]`, `[CalledBy]` and `[CallerCount]` attributes.
  `nativemethoddetector` runs for most of an hour under Rosetta and adds
  nothing the stubs use; `attributeanalyzer` adds metadata tokens.
- **Per-build noise is stripped.** `[Token(...)]`, `[Address(...)]` and
  `[FieldOffset(...)]` lines differ in every build for no reason a reader
  cares about, and would bury every real change in a diff. The field offset
  survives as the `//Field offset:` comment on the field's own line.
- **AssetRipper reads the whole `.app`.** Given only `Contents/Resources/Data`
  it cannot find `Contents/Frameworks/GameAssembly.dylib`, reports an
  "Unknown" scripting backend, and exports every MonoBehaviour without its
  fields. Loading takes about ten minutes.

The tools are pinned by version and SHA-256 below and downloaded from their
GitHub releases on first use.
"""

import argparse
import hashlib
import json
import os
import pathlib
import plistlib
import re
import socket
import sqlite3
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / "work" / "tools"
DECOMP = ROOT / "work" / "decomp"
DEFAULT_GAME = pathlib.Path.home() / "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app"

CPP2IL = {
    "name": "Cpp2IL",
    "version": "2022.1.0-pre-release.20",
    "url": "https://github.com/SamboyCoding/Cpp2IL/releases/download/2022.1.0-pre-release.20/Cpp2IL-2022.1.0-pre-release.20-OSX",
    "sha256": "fa869aae2d7b5a22faff95f9758f27a4cdc44fda1313f86ab6f1e287dfd208bc",
    "file": "Cpp2IL-2022.1.0-pre-release.20-OSX",
}
ASSETRIPPER = {
    "name": "AssetRipper",
    "version": "2.0.0",
    "url": "https://github.com/AssetRipper/AssetRipper/releases/download/2.0.0/AssetRipper_mac_arm64.tar.xz",
    "sha256": "237a6da404bf089c5e5c71a5a9a698c2ea580872292dfa5ce21e25fd1dbe9045",
    "file": "AssetRipper_mac_arm64.tar.xz",
}
PROCESSORS = "callanalyzer,attributeinjector"
NOISE = re.compile(r'^\s*\[(Token\(Token|Address\(RVA|FieldOffset\(Offset) = "[^"]*"[^\]]*\)\]\s*$')
STEPS = ("dylib", "isil", "cs", "config", "manifest", "index")


def say(message):
    print(f"decompile: {message}", flush=True)


def fail(message):
    print(f"decompile: {message}", file=sys.stderr)
    sys.exit(1)


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


# --- the game ---------------------------------------------------------------

def game_identity(app):
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    build = info.get("CFBundleShortVersionString")
    if not build or not re.fullmatch(r"[0-9]+(\.[0-9]+)+", build):
        fail(f"{app}: no build number in CFBundleShortVersionString ({build!r})")
    unity = re.search(r"Unity Player version (\S+)", info.get("CFBundleGetInfoString", ""))
    if not unity:
        fail(f"{app}: no Unity version in CFBundleGetInfoString")
    return build, unity.group(1)


def steam_build(app):
    """Steam's own identity for the installed game, when Steam installed it.

    The version string does not always move when the game does: an update can
    rebuild `GameAssembly.dylib` under the same `CFBundleShortVersionString`.
    Steam's `buildid` always moves, and it is one number for every platform of
    a branch, where each platform's files are a depot of their own.
    """
    steamapps = app.parent.parent.parent
    for acf in sorted(steamapps.glob("appmanifest_*.acf")):
        text = acf.read_text(errors="replace")
        fields = dict(re.findall(r'^\s*"(\w+)"\s+"([^"]*)"', text, re.M))
        if fields.get("installdir") == app.parent.name:
            return {"appid": fields.get("appid"), "buildid": fields.get("buildid"),
                    "branch": fields.get("BetaKey", "public")}
    return None


def artifacts(app):
    return {
        "native_binary": app / "Contents/Frameworks/GameAssembly.dylib",
        "global_metadata": app / "Contents/Resources/Data/il2cpp_data/Metadata/global-metadata.dat",
        "unity_version_source": app / "Contents/Resources/Data/globalgamemanagers",
    }


# --- tools ------------------------------------------------------------------

def fetch(tool):
    target = TOOLS / tool["file"]
    if target.exists() and sha256(target) == tool["sha256"]:
        return target
    TOOLS.mkdir(parents=True, exist_ok=True)
    say(f"downloading {tool['name']} {tool['version']}")
    partial = target.with_name(target.name + ".part")
    for attempt in range(5):
        try:
            with urllib.request.urlopen(tool["url"], timeout=120) as response, open(partial, "wb") as out:
                while block := response.read(1 << 20):
                    out.write(block)
            break
        except (urllib.error.URLError, OSError) as error:
            if attempt == 4:
                fail(f"cannot download {tool['url']}: {error}")
            time.sleep(3)
    if sha256(partial) != tool["sha256"]:
        partial.unlink()
        fail(f"{tool['url']} does not have the pinned SHA-256 {tool['sha256']}")
    os.replace(partial, target)
    return target


def cpp2il():
    binary = fetch(CPP2IL)
    binary.chmod(0o755)
    return binary


def assetripper():
    archive = fetch(ASSETRIPPER)
    directory = TOOLS / "AssetRipper"
    binary = directory / "AssetRipper.GUI.Free"
    stamp = directory / ".sha256"
    if not binary.exists() or not stamp.exists() or stamp.read_text().strip() != ASSETRIPPER["sha256"]:
        directory.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive, "r:xz") as tar:
            tar.extractall(directory, filter="data")
        binary.chmod(0o755)
        stamp.write_text(ASSETRIPPER["sha256"] + "\n")
    return binary


# --- steps ------------------------------------------------------------------

def step_dylib(app, work):
    target = work / "GameAssembly-x86_64.dylib"
    subprocess.run(
        ["lipo", str(artifacts(app)["native_binary"]), "-thin", "x86_64", "-output", str(target)],
        check=True,
    )
    return target


def run_cpp2il(app, work, unity, output_as, output_dir):
    command = [
        str(cpp2il()),
        "--force-binary-path", str(work / "GameAssembly-x86_64.dylib"),
        "--force-metadata-path", str(artifacts(app)["global_metadata"]),
        "--force-unity-version", unity,
    ]
    if output_as == "diffable-cs":
        command += ["--use-processor", PROCESSORS]
    command += ["--output-as", output_as, "--output-to", str(output_dir)]
    log = work / f"cpp2il-{output_as}.log"
    with open(log, "w") as out:
        result = subprocess.run(command, cwd=TOOLS, stdout=out, stderr=subprocess.STDOUT)
    if result.returncode != 0:
        fail(f"Cpp2IL --output-as {output_as} failed; see {log}")
    return command


def step_isil(app, work, unity, out):
    staging = work / "cpp2il-isil"
    subprocess.run(["rm", "-rf", str(staging), str(out / "cpp2il/IsilDump")], check=True)
    command = run_cpp2il(app, work, unity, "isil", staging)
    (out / "cpp2il").mkdir(parents=True, exist_ok=True)
    os.replace(staging / "IsilDump", out / "cpp2il/IsilDump")
    staging.rmdir()
    return command


def step_cs(app, work, unity, out):
    staging = work / "cpp2il-cs"
    subprocess.run(["rm", "-rf", str(staging), str(out / "cpp2il/DiffableCs")], check=True)
    command = run_cpp2il(app, work, unity, "diffable-cs", staging)
    for path in (staging / "DiffableCs").rglob("*.cs"):
        text = path.read_text(encoding="utf-8")
        kept = [line for line in text.splitlines(keepends=True) if not NOISE.match(line)]
        path.write_text("".join(kept), encoding="utf-8")
    (out / "cpp2il").mkdir(parents=True, exist_ok=True)
    os.replace(staging / "DiffableCs", out / "cpp2il/DiffableCs")
    staging.rmdir()
    return command


class Ripper:
    """AssetRipper's headless web server, for as long as one export needs it."""

    def __init__(self):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            self.port = probe.getsockname()[1]
        self.base = f"http://127.0.0.1:{self.port}"
        self.log = open(TOOLS / "assetripper.log", "w")
        self.process = subprocess.Popen(
            [str(assetripper()), "--headless", "--port", str(self.port)],
            cwd=TOOLS / "AssetRipper", stdout=self.log, stderr=subprocess.STDOUT,
        )
        for _ in range(120):
            try:
                urllib.request.urlopen(self.base + "/openapi.json", timeout=2).read()
                return
            except OSError:
                time.sleep(1)
        self.close()
        fail("AssetRipper did not start")

    def close(self):
        self.process.terminate()
        self.process.wait(timeout=30)
        self.log.close()

    def load(self, folder):
        body = urllib.parse.urlencode({"Path": str(folder)}).encode()
        request = urllib.request.Request(self.base + "/LoadFolder", data=body)
        with urllib.request.urlopen(request, timeout=3600) as response:
            response.read()

    def collections(self):
        page = self.get("/Bundles/View?Path=" + urllib.parse.quote('{"P":[]}')).decode()
        page = page.replace("&quot;", '"')
        found = re.findall(r'href="/Collections/View\?Path=([^"]*)"[^>]*>([^<]*)<', page)
        return {name: json.loads(urllib.parse.unquote(path)) for path, name in found}

    def asset(self, collection, path_id):
        path = json.dumps({"C": collection, "D": path_id}, separators=(",", ":"))
        try:
            return self.get("/Assets/Json?Path=" + urllib.parse.quote(path))
        except urllib.error.HTTPError:
            return None

    def get(self, route):
        with urllib.request.urlopen(self.base + route, timeout=900) as response:
            return response.read()


UNITYPY = "1.25.3"
# Which data objects are exported, by the file they are in. Every MonoBehaviour
# whose script is in the GameRiver namespace, except GameRiver.Client (the UI),
# and the language source the configuration's names are localized in.
EXPORT_FILES = ("level0", "resources.assets", "sharedassets0.assets")
LANGUAGE_SOURCE = ("I2.Loc.LanguageSourceAsset", "I2LanguagesForConfigData")
ENUMERATE = """
import json, sys, UnityPy
from pathlib import Path
data = Path(sys.argv[1])
found = []
for name in sys.argv[2:]:
    for obj in UnityPy.load(str(data / name)).objects:
        if obj.type.name != "MonoBehaviour":
            continue
        behaviour = obj.read(check_read=False)
        script = behaviour.m_Script.read()
        owner = ""
        if not behaviour.m_Name and behaviour.m_GameObject.path_id:
            owner = behaviour.m_GameObject.read().m_Name
        found.append({"file": name, "path_id": obj.path_id, "name": behaviour.m_Name or owner,
                      "class": f"{script.m_Namespace}.{script.m_ClassName}".lstrip(".")})
json.dump(found, sys.stdout)
"""


def unitypy():
    """A Python that has UnityPy, in its own environment under work/tools."""
    environment = TOOLS / f"unitypy-{UNITYPY}"
    python = environment / "bin" / "python"
    if not python.exists():
        say(f"creating a Python environment with UnityPy {UNITYPY}")
        if subprocess.run(["uv", "--version"], capture_output=True).returncode == 0:
            subprocess.run(["uv", "venv", "--quiet", str(environment)], check=True)
            subprocess.run(["uv", "pip", "install", "--quiet", "--python", str(python), f"UnityPy=={UNITYPY}"], check=True)
        else:
            subprocess.run([sys.executable, "-m", "venv", str(environment)], check=True)
            subprocess.run([str(python), "-m", "pip", "install", "--quiet", f"UnityPy=={UNITYPY}"], check=True)
    return python


def exported_objects(app):
    """The data objects to export: file, path id, name and script class of each."""
    data = app / "Contents/Resources/Data"
    result = subprocess.run([str(unitypy()), "-c", ENUMERATE, str(data), *EXPORT_FILES],
                            capture_output=True, text=True)
    if result.returncode != 0:
        fail(f"UnityPy could not list the game's objects:\n{result.stderr.strip()}")
    return [
        entry for entry in json.loads(result.stdout)
        if (entry["class"].startswith("GameRiver.") and not entry["class"].startswith("GameRiver.Client."))
        or (entry["class"], entry["name"]) == LANGUAGE_SOURCE
    ]


def export_path(entry, counts):
    """`<file>/<Class>.json` for a class the file holds once, else `<file>/<Class>/<name>.json`.

    A component's name is its GameObject's, so a prefab's RVOControllerFixed
    is `sharedassets0/RVOControllerFixed/Mech_Default_2.json`.
    """
    if entry["class"] == "GameRiver.ConfigDataContainer":
        return pathlib.Path("config-data-container.json")
    directory = pathlib.Path(entry["file"].removesuffix(".assets"))
    short = entry["class"].rsplit(".", 1)[-1]
    if counts[(entry["file"], entry["class"])] == 1:
        return directory / f"{short}.json"
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "_", entry["name"]) or str(entry["path_id"])
    return directory / short / f"{stem}.json"


def step_config(app, out):
    """Every data object of the build, as AssetRipper types it from the build's assemblies.

    UnityPy reads which script each MonoBehaviour runs, which needs no type
    tree; AssetRipper, which reconstructs the types from the IL2CPP metadata,
    exports the chosen ones by path id. `ConfigDataContainer` is
    `config-data-container.json`; the rest go under a directory per file.
    """
    objects = exported_objects(app)
    counts = {}
    for entry in objects:
        counts[(entry["file"], entry["class"])] = counts.get((entry["file"], entry["class"]), 0) + 1
    paths = {}
    for entry in objects:
        path = export_path(entry, counts)
        if path in paths.values():
            path = path.with_name(f"{path.stem}_{entry['path_id']}.json")
        paths[(entry["file"], entry["path_id"])] = path
    for directory in ("level0", "resources", "sharedassets0"):
        subprocess.run(["rm", "-rf", str(out / directory)], check=True)
    ripper = Ripper()
    try:
        say(f"AssetRipper is loading the game to export {len(objects)} objects; this takes a few minutes")
        ripper.load(app)
        collections = ripper.collections()
        exported = []
        for entry in objects:
            if entry["file"] not in collections:
                fail(f"AssetRipper found no {entry['file']}")
            raw = ripper.asset(collections[entry["file"]], entry["path_id"])
            if raw is None:
                fail(f"AssetRipper cannot export {entry['file']} path id {entry['path_id']} ({entry['class']})")
            target = out / paths[(entry["file"], entry["path_id"])]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
            exported.append({**entry, "export": str(paths[(entry["file"], entry["path_id"])])})
        if not any(entry["class"] == "GameRiver.ConfigDataContainer" for entry in exported):
            fail("the build holds no GameRiver.ConfigDataContainer")
        say(f"exported {len(exported)} data objects")
        return exported
    finally:
        ripper.close()


def step_manifest(app, build, unity, out, commands, level0):
    root = app
    listed = []
    for role, path in artifacts(app).items():
        listed.append({
            "path": str(path.relative_to(root)),
            "role": role,
            "sha256": sha256(path),
            "size": path.stat().st_size,
        })
    identity = hashlib.sha256(
        "".join(f"{a['path']}\t{a['sha256']}\n" for a in sorted(listed, key=lambda a: a["path"])).encode()
    ).hexdigest()
    manifest = {
        "steam": steam_build(app),
        "artifacts": listed,
        "backend": "il2cpp",
        "build": build,
        "game_identity_sha256": identity,
        "schema": "unity-decomp-index/v1",
        "tools": {
            "cpp2il": {"version": CPP2IL["version"], "sha256": CPP2IL["sha256"],
                       "processors": PROCESSORS, "slice": "x86_64"},
            "assetripper": {"version": ASSETRIPPER["version"], "sha256": ASSETRIPPER["sha256"]},
        },
        "exports": level0,
        "unity_version": unity,
        "warnings": [],
    }
    (out / "game-manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return manifest


# --- index ------------------------------------------------------------------

CS_TYPE = re.compile(r"^(\t*)(?:\[.*\]\s*)?(?:(?:public|internal|private|protected|static|sealed|abstract|readonly|ref|unsafe|partial|new)\s+)*(class|struct|interface|enum)\s+([^\s:<{]+(?:<[^>]*>)?)")
CS_METHOD = re.compile(r"^\t+(?!\[)(?P<signature>[^=;]*?\b(?P<name>[A-Za-z_][\w`.<>]*)\((?P<params>.*)\)[^;=]*)\{ \}\s*$")


def index_csharp(database, source_id, assembly, relative, text):
    namespace = ".".join(relative.parts[1:-1])
    stack = []
    lines = text.splitlines()
    for number, line in enumerate(lines, 1):
        match = CS_TYPE.match(line)
        if match:
            depth = len(match.group(1))
            stack = stack[:depth]
            name = match.group(3)
            stack.append(name)
            type_name = "+".join(stack)
            symbol = f"{assembly}!{namespace}.{type_name}".replace("!.", "!")
            database.execute(
                "insert or ignore into symbols values (?,?,?,?,?,?,?,?,?,?,0)",
                (symbol, assembly, namespace, type_name, name, "type", name, source_id, number, number),
            )
            continue
        match = CS_METHOD.match(line)
        if match and stack:
            type_name = "+".join(stack)
            name = match.group("name").split(".")[-1]
            kind = "constructor" if name == stack[-1] and " static " not in f" {line.strip()} " else "method"
            symbol = f"{assembly}!{namespace}.{type_name}::{name}({match.group('params')})@cs:{number}".replace("!.", "!")
            database.execute(
                "insert or ignore into symbols values (?,?,?,?,?,?,?,?,?,?,0)",
                (symbol, assembly, namespace, type_name, name, kind, line.strip(), source_id, number, number),
            )


ISIL_METHOD = re.compile(r"^Method: (?P<signature>.*?(?P<name>[^\s(]+)\(.*\))\s*$")
ISIL_CALL = re.compile(r"^\t\d+ Call (?P<label>[^,\s]+)")


def index_isil(database, source_id, assembly, text, pending):
    lines = text.splitlines()
    if not lines or not lines[0].startswith("Type: "):
        return
    full = lines[0][len("Type: "):].strip()
    namespace, _, type_name = full.rpartition(".")
    type_symbol = f"{assembly}!{full}@isil"
    database.execute(
        "insert or ignore into symbols values (?,?,?,?,?,?,?,?,?,?,1)",
        (type_symbol, assembly, namespace, type_name, type_name, "type", full, source_id, 1, len(lines)),
    )
    starts = [i for i, line in enumerate(lines) if line.startswith("Method: ")]
    for position, start in enumerate(starts):
        end = starts[position + 1] if position + 1 < len(starts) else len(lines)
        match = ISIL_METHOD.match(lines[start])
        if not match:
            continue
        symbol = f"{assembly}!{full}::{match.group('signature')}@isil:{start + 1}"
        name = match.group("name").split(".")[-1]
        database.execute(
            "insert or ignore into symbols values (?,?,?,?,?,?,?,?,?,?,1)",
            (symbol, assembly, namespace, type_name, name, "method", match.group("signature"),
             source_id, start + 1, end),
        )
        labels = []
        for line in lines[start:end]:
            call = ISIL_CALL.match(line)
            if call and call.group("label") not in labels:
                labels.append(call.group("label"))
        pending.extend((symbol, label) for label in labels)


def step_index(out, manifest, commands):
    target = out / "index.sqlite"
    partial = target.with_name("index.sqlite.part")
    partial.unlink(missing_ok=True)
    database = sqlite3.connect(partial)
    database.executescript("""
        create table metadata (key text primary key, value text not null);
        create table sources (id integer primary key, path text not null unique, kind text not null,
                              sha256 text not null, size integer not null);
        create table symbols (id text primary key, assembly text not null, namespace text not null,
                              type_name text not null, name text not null, kind text not null,
                              signature text not null, source_id integer not null references sources(id),
                              start_line integer not null, end_line integer not null,
                              low_level integer not null check(low_level in (0, 1)));
        create table calls (caller_id text not null references symbols(id), target_label text not null,
                            target_id text,
                            resolution text not null check(resolution in ('resolved', 'ambiguous', 'external')));
    """)
    base = out / "cpp2il"
    pending = []
    for kind, root, suffix in (("csharp", base / "DiffableCs", ".cs"), ("isil", base / "IsilDump", ".txt")):
        for path in sorted(root.rglob(f"*{suffix}")):
            relative = path.relative_to(base)
            data = path.read_bytes()
            cursor = database.execute(
                "insert into sources (path, kind, sha256, size) values (?,?,?,?)",
                (str(relative), kind, hashlib.sha256(data).hexdigest(), len(data)),
            )
            text = data.decode("utf-8", "replace")
            assembly = relative.parts[1]
            if kind == "csharp":
                index_csharp(database, cursor.lastrowid, assembly, relative.relative_to(relative.parts[0]), text)
            else:
                index_isil(database, cursor.lastrowid, assembly, text, pending)
    by_label = {}
    for symbol, type_name, name in database.execute(
        "select id, type_name, name from symbols where low_level = 1 and kind = 'method'"
    ):
        by_label.setdefault(f"{type_name.split('+')[-1]}.{name}", []).append(symbol)
    rows = []
    for caller, label in pending:
        targets = by_label.get(label, [])
        if len(targets) == 1:
            rows.append((caller, label, targets[0], "resolved"))
        elif targets:
            rows.append((caller, label, None, "ambiguous"))
        else:
            rows.append((caller, label, None, "external"))
    database.executemany("insert into calls values (?,?,?,?)", rows)
    database.executescript("""
        create index symbols_type on symbols(type_name);
        create index symbols_name on symbols(name);
        create index calls_caller on calls(caller_id);
        create index calls_target on calls(target_id);
    """)
    metadata = {
        "schema": "unity-decomp-index/v1",
        "decomp_root": str(base),
        "game_manifest": manifest,
        "tool": "Cpp2IL",
        "tool_version": CPP2IL["version"],
        "tool_command": commands,
        "assemblies": sorted(p.name for p in (base / "IsilDump").iterdir() if p.is_dir()),
    }
    database.executemany(
        "insert into metadata values (?,?)", [(k, json.dumps(v)) for k, v in metadata.items()]
    )
    database.commit()
    database.close()
    os.replace(partial, target)


# --- main -------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--game", type=pathlib.Path, default=pathlib.Path(os.environ.get("MECHABELLUM_APP", DEFAULT_GAME)))
    parser.add_argument("--force", default="", help="steps to redo, comma separated, or all")
    arguments = parser.parse_args()
    app = arguments.game
    if not (app / "Contents/Info.plist").exists():
        fail(f"no game at {app}; pass --game or set MECHABELLUM_APP")
    force = set(STEPS) if arguments.force == "all" else set(filter(None, arguments.force.split(",")))
    if force - set(STEPS):
        fail(f"unknown steps {', '.join(sorted(force - set(STEPS)))}; the steps are {', '.join(STEPS)}")

    build, unity = game_identity(app)
    out = DECOMP / build
    work = ROOT / "work" / "inputs" / build
    # One version is one set of rules, so a version has one directory. Steam
    # can still ship new files under it; a directory made from other files is
    # not reused step by step, which would keep the old dump, but replaced
    # whole when asked to.
    manifest_path = out / "game-manifest.json"
    if manifest_path.exists() and force != set(STEPS):
        made = json.loads(manifest_path.read_text())
        made_from = {a["path"]: a["sha256"] for a in made["artifacts"]}
        installed = {str(path.relative_to(app)): sha256(path) for path in artifacts(app).values()}
        if made_from != installed:
            fail(f"{out} was made from other files of {build} (Steam build "
                 f"{(made.get('steam') or {}).get('buildid')}, installed "
                 f"{(steam_build(app) or {}).get('buildid')}); --force all replaces it")
    out.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    say(f"build {build}, Steam build {(steam_build(app) or {}).get('buildid')}, Unity {unity}, into {out}")

    commands = {}
    if "dylib" in force or not (work / "GameAssembly-x86_64.dylib").exists():
        step_dylib(app, work)
        say("x86_64 slice extracted")
    if "isil" in force or not (out / "cpp2il/IsilDump").is_dir():
        commands["isil"] = step_isil(app, work, unity, out)
        say("IsilDump written")
    if "cs" in force or not (out / "cpp2il/DiffableCs").is_dir():
        commands["diffable-cs"] = step_cs(app, work, unity, out)
        say("DiffableCs written")
    level0 = None
    if manifest_path.exists():
        level0 = json.loads(manifest_path.read_text()).get("exports")
    if "config" in force or not (out / "config-data-container.json").exists() or not level0:
        level0 = step_config(app, out)
    if "manifest" in force or not manifest_path.exists() or force & {"dylib", "isil", "cs", "config"}:
        manifest = step_manifest(app, build, unity, out, commands, level0)
        say("game-manifest.json written")
    else:
        manifest = json.loads(manifest_path.read_text())
    if "index" in force or not (out / "index.sqlite").exists() or force & {"isil", "cs"}:
        say("building index.sqlite")
        step_index(out, manifest, commands)
        say("index.sqlite written")
    say("done")


if __name__ == "__main__":
    main()
