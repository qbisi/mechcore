#!/usr/bin/env python3
"""Fetch the build's decompilation and its symbol index to where every tool reads them.

    scripts/decomp.py sync [--build BUILD]   clone qbisi/mechcore-decomp into work/decomp, one build, and its index
    scripts/decomp.py path [BUILD]           print work/decomp/<build>
    scripts/decomp.py publish BUILD          commit a build scripts/decompile.py made, and release its index

The decompilation lives in the private repository
https://github.com/qbisi/mechcore-decomp, one directory per game build, with
the symbol index `index.sqlite` as the release `index/<build>` because it is
449 MB of binary. This script puts both under one path, which is the
convention every reader here follows:

    work/decomp/<build>/cpp2il/IsilDump/     the instruction dump, one file per class
    work/decomp/<build>/cpp2il/DiffableCs/   the C# stubs
    work/decomp/<build>/game-manifest.json   which game files the dump came from
    work/decomp/<build>/index.sqlite         the symbol and call index

`sync` looks before it fetches. A build already under `work/decomp` is left
alone, an index already there is left alone, and an index in the older
`work/unity-index/<build>/` is linked across rather than downloaded again. A
machine that has run it once needs the network only for a build it lacks.

`work/decomp` is a sparse, blob-less clone, so a second build adds only its
own files. Access is whatever git and `gh` have: a signed-in `gh` on a
machine, the GitHub proxy in a Claude Code cloud session with the repository
attached, or a read-only token in `MECHCORE_DECOMP_TOKEN` on this repository
alone, which is what a Codex container gets.

A new build comes from `scripts/decompile.py`, which decompiles the installed
game into `work/decomp/<build>`. `publish` is the only verb here that writes:
it commits that directory to the repository and pushes it, and uploads the
index as the release `index/<build>`. Only the session that holds the game has
anything to publish.
"""

import gzip
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

REPOSITORY = "qbisi/mechcore-decomp"
ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "work" / "decomp"
LEGACY_INDEX = ROOT / "work" / "unity-index"
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


def gh_signed_in():
    return shutil.which("gh") is not None and subprocess.run(["gh", "auth", "status"], capture_output=True).returncode == 0


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
    legacy = LEGACY_INDEX / build / INDEX
    if legacy.exists():
        try:
            os.link(legacy, target)
        except OSError:
            shutil.copyfile(legacy, target)
        print(f"{target}: linked from {legacy}")
        return
    tag = f"index/{build}"
    partial = target.with_suffix(".sqlite.part")
    if gh_signed_in():
        with open(partial, "wb") as out:
            gh = subprocess.Popen(
                ["gh", "release", "download", tag, "--repo", REPOSITORY, "--pattern", f"{INDEX}.gz", "--output", "-"],
                stdout=subprocess.PIPE,
            )
            with gzip.GzipFile(fileobj=gh.stdout) as unzipped:
                shutil.copyfileobj(unzipped, out)
            if gh.wait() != 0:
                partial.unlink(missing_ok=True)
                fail(f"gh release download {tag} failed")
    elif TOKEN:
        headers = {"Authorization": f"Bearer {TOKEN}", "Accept": "application/vnd.github+json"}
        request = urllib.request.Request(
            f"https://api.github.com/repos/{REPOSITORY}/releases/tags/{tag.replace('/', '%2F')}", headers=headers
        )
        with urllib.request.urlopen(request) as response:
            assets = json.load(response)["assets"]
        asset = next((a for a in assets if a["name"] == f"{INDEX}.gz"), None)
        if asset is None:
            fail(f"release {tag} carries no {INDEX}.gz")
        request = urllib.request.Request(asset["url"], headers={**headers, "Accept": "application/octet-stream"})
        with urllib.request.urlopen(request) as response, open(partial, "wb") as out:
            with gzip.GzipFile(fileobj=response) as unzipped:
                shutil.copyfileobj(unzipped, out)
    else:
        fail(f"no signed-in gh and no MECHCORE_DECOMP_TOKEN; cannot download the index release {tag}")
    os.replace(partial, target)
    print(f"{target}: downloaded")


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
        fail("no build under work/decomp; run scripts/decomp.py sync")
    if build is None:
        if len(builds) != 1:
            fail(f"several builds under work/decomp, name one: {', '.join(builds)}")
        build = builds[0]
    elif build not in builds:
        fail(f"no build {build} under work/decomp; it holds {', '.join(builds)}")
    print(DESTINATION / build)


def publish(build):
    directory = DESTINATION / build
    for required in ("cpp2il/IsilDump", "cpp2il/DiffableCs", "config-data-container.json",
                     "game-manifest.json", INDEX):
        if not (directory / required).exists():
            fail(f"{directory} has no {required}; run scripts/decompile.py first")
    if not (DESTINATION / ".git").exists():
        fail(f"{DESTINATION} is not a clone of {REPOSITORY}; run sync first")
    if subprocess.run(["git", "sparse-checkout", "list"], cwd=DESTINATION, capture_output=True).returncode == 0:
        run("git", "sparse-checkout", "add", build, cwd=DESTINATION)
    run("git", "add", "--", build, cwd=DESTINATION)
    status = run("git", "status", "--porcelain", "--", build, cwd=DESTINATION)
    if status:
        run("git", "commit", "--quiet", "-m",
            f"decomp: build {build}, Cpp2IL dump and stubs, manifest, config", cwd=DESTINATION)
    # A push that failed last time is retried by running publish again.
    for attempt in range(5):
        pushed = subprocess.run(["git", "push", "--quiet", "origin", "HEAD"], cwd=DESTINATION,
                                capture_output=True, text=True)
        if pushed.returncode == 0:
            break
        if attempt == 4:
            fail(f"git push failed:\n{pushed.stderr.strip()}")
        time.sleep(5)
    print(f"{REPOSITORY}: {build} pushed")
    tag = f"index/{build}"
    if subprocess.run(["gh", "release", "view", tag, "--repo", REPOSITORY], capture_output=True).returncode == 0:
        print(f"{tag}: already released")
        return
    packed = directory / f"{INDEX}.gz"
    with open(directory / INDEX, "rb") as source, gzip.open(packed, "wb") as out:
        shutil.copyfileobj(source, out)
    try:
        run("gh", "release", "create", tag, str(packed), "--repo", REPOSITORY,
            "--title", tag, "--notes", f"The symbol and call index of build {build}.")
    finally:
        packed.unlink(missing_ok=True)
    print(f"{tag}: released")


def main(argv):
    if len(argv) >= 2 and argv[1] == "sync":
        if len(argv) == 2:
            sync(None)
        elif len(argv) == 4 and argv[2] == "--build":
            sync(argv[3])
        else:
            fail("usage: sync [--build BUILD]")
    elif len(argv) == 3 and argv[1] == "publish":
        publish(argv[2])
    elif len(argv) in (2, 3) and argv[1] == "path":
        path(argv[2] if len(argv) == 3 else None)
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
