#!/usr/bin/env python3
"""Fetch the build's decompilation and its symbol index to where every tool reads them.

    scripts/decomp/decomp.py sync [--build BUILD]   clone qbisi/mechcore-decomp into work/decomp, one build, and index it
    scripts/decomp/decomp.py path [BUILD]           print work/decomp/<build>

The decompilation lives in the private repository
https://github.com/qbisi/mechcore-decomp, one directory per game build. The
symbol index `index.sqlite`, 449 MB of binary, is not stored anywhere: `sync`
builds it from the dump, as `scripts/decomp/decompile.py` does, which needs neither
the game nor the network. Both land under one path, which is the convention
every reader here follows:

    work/decomp/<build>/cpp2il/IsilDump/     the instruction dump, one file per class
    work/decomp/<build>/cpp2il/DiffableCs/   the C# stubs
    work/decomp/<build>/game-manifest.json   which game files the dump came from
    work/decomp/<build>/index.sqlite         the symbol and call index

`sync` looks before it fetches. A build already under `work/decomp` is left
alone, and so is an index already there. A machine that has run it once needs
the network only for a build it lacks.

`work/decomp` is a sparse, blob-less clone, so a second build adds only its
own files. Access is whatever git and `gh` have: a signed-in `gh` on a
machine, the GitHub proxy in a Claude Code cloud session with the repository
attached, or a read-only token in `MECHCORE_DECOMP_TOKEN` on this repository
alone, which is what a Codex container gets.

A new build comes from `scripts/decomp/decompile.py`, which decompiles the installed
game into `work/decomp/<build>`.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

import decompile

REPOSITORY = "qbisi/mechcore-decomp"
ROOT = Path(__file__).resolve().parents[2]
DESTINATION = ROOT / "work" / "decomp"
TOKEN = os.environ.get("MECHCORE_DECOMP_TOKEN")
INDEX = "index.sqlite"


def fail(message):
    print(f"decomp: {message}", file=sys.stderr)
    sys.exit(1)


def run(*command, cwd=None):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    if result.returncode != 0:
        fail(f"{' '.join(command[:2])} failed:\n{result.stderr.strip() or result.stdout.strip()}")
    return result.stdout.strip()


def clone_url():
    if TOKEN:
        return f"https://x-access-token:{TOKEN}@github.com/{REPOSITORY}"
    return f"https://github.com/{REPOSITORY}"


def builds_available():
    if not (DESTINATION / ".git").exists():
        return []
    listed = run("git", "ls-tree", "--name-only", "HEAD", cwd=DESTINATION).split()
    return sorted(name for name in listed if (name[0].isdigit()))


def checked_out(build):
    return (DESTINATION / build / "cpp2il").is_dir()


def clone():
    DESTINATION.parent.mkdir(parents=True, exist_ok=True)
    run(
        "git", "clone", "--quiet", "--filter=blob:none", "--sparse", "--depth", "1",
        clone_url(), str(DESTINATION),
    )
    if TOKEN:
        # The token stays in the environment, not in the clone's config.
        run("git", "remote", "set-url", "origin", f"https://github.com/{REPOSITORY}", cwd=DESTINATION)


def sync_build(build):
    if checked_out(build):
        print(f"{DESTINATION / build}: present")
    else:
        run("git", "sparse-checkout", "add", build, cwd=DESTINATION)
        if not checked_out(build):
            fail(f"{REPOSITORY} holds no build {build}; it holds {', '.join(builds_available())}")
        print(f"{DESTINATION / build}: fetched")
    sync_index(build)


def sync_index(build):
    target = DESTINATION / build / INDEX
    if target.exists():
        print(f"{target}: present")
        return
    # The index is the dump's, so it is built from the dump rather than
    # fetched: the same step `scripts/decomp/decompile.py` ends with.
    manifest = json.loads((DESTINATION / build / "game-manifest.json").read_text())
    print(f"{target}: building from the dump")
    decompile.step_index(DESTINATION / build, manifest, manifest.get("commands", {}))
    print(f"{target}: built")


def sync(build):
    if not (DESTINATION / ".git").exists():
        clone()
    builds = builds_available()
    if build is None:
        if len(builds) != 1:
            fail(f"name a build with --build: {REPOSITORY} holds {', '.join(builds)}")
        build = builds[0]
    sync_build(build)


def path(build):
    builds = sorted(p.name for p in DESTINATION.glob("*") if (p / "cpp2il").is_dir())
    if not builds:
        fail("no build under work/decomp; run scripts/decomp/decomp.py sync")
    if build is None:
        if len(builds) != 1:
            fail(f"several builds under work/decomp, name one: {', '.join(builds)}")
        build = builds[0]
    elif build not in builds:
        fail(f"no build {build} under work/decomp; it holds {', '.join(builds)}")
    print(DESTINATION / build)


def main(argv):
    if len(argv) >= 2 and argv[1] == "sync":
        if len(argv) == 2:
            sync(None)
        elif len(argv) == 4 and argv[2] == "--build":
            sync(argv[3])
        else:
            fail("usage: sync [--build BUILD]")
    elif len(argv) in (2, 3) and argv[1] == "path":
        path(argv[2] if len(argv) == 3 else None)
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
