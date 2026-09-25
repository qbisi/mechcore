#!/usr/bin/env python3
r"""Fetch the replay corpus, and add to it.

    scripts/replay.py sync             fetch qbisi/mechcore-replay's master into work/replay
    scripts/replay.py path [VERSION]   print a version's replay directory (default: this checkout's)
    scripts/replay.py publish          add this machine's new replays of the installed version, and push

The native replays live in https://github.com/qbisi/mechcore-replay, one
directory per game version, `replays/<version>/<name>.grbr`. The repository
only grows, so it is read at `master` and a newer fetch never takes away what
an older one had. `work/` is not tracked, so a checkout runs `sync` once, and
again to pick up replays added since. The version this checkout describes is
the one `scripts/build_data.py` reads. Nothing is generated in the corpus: `scripts/export-replay-corpus.py` converts a
version's replays into `work/battle/<version>/`.

`publish` is the one writer. It files the replays the installed game recorded
itself under the installed game's version: a locally recorded replay is named
`<last version component>_<date>--<scene>_[<player>]VS[<player>].grbr`, and a
downloaded one, `<version>\<id>.rep.grbr`, is a server-side reconstruction
and never admitted. A replay whose prefix is not the installed version's was
recorded by another version and is left alone, as is one written in the last
minute, which may still be growing. A name already in the corpus must hold the
same bytes; the corpus is never rewritten.
"""

import filecmp
import os
import plistlib
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

import build_data

REPOSITORY = "https://github.com/qbisi/mechcore-replay"
ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "work" / "replay"
APP = Path(os.environ.get(
    "MECHABELLUM_APP",
    Path.home() / "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app"))


def fail(message):
    print(f"replay: {message}", file=sys.stderr)
    sys.exit(1)


def git(*arguments, cwd=DESTINATION):
    result = subprocess.run(["git", *arguments], cwd=cwd, text=True, capture_output=True)
    if result.returncode != 0:
        fail(f"git {' '.join(arguments)} failed:\n{result.stderr.strip()}")
    return result.stdout.strip()


def sync():
    if not (DESTINATION / ".git").exists():
        DESTINATION.mkdir(parents=True, exist_ok=True)
        git("init", "-q")
        git("remote", "add", "origin", REPOSITORY)
    git("fetch", "-q", "--depth", "1", "origin", "master")
    git("checkout", "-q", "--detach", "FETCH_HEAD")
    print(f"work/replay is at {git('rev-parse', '--short', 'HEAD')}")


def path(version):
    version = version or build_data.build()
    directory = DESTINATION / "replays" / version
    if not directory.is_dir():
        fail(f"no replays of {version} under work/replay; run scripts/replay.py sync")
    print(directory)


def publish():
    info = plistlib.loads((APP / "Contents/Info.plist").read_bytes())
    version = info["CFBundleShortVersionString"]
    prefix = version.rsplit(".", 1)[-1]
    local = re.compile(rf"^{re.escape(prefix)}_\d{{8}}--\d+_\[.*\]VS\[.*\]\.grbr$")
    source = APP / "ProjectDatas" / "Replay"
    fresh = time.time() - 60
    replays = sorted(p for p in source.iterdir()
                     if local.match(p.name) and p.stat().st_mtime < fresh)
    if not (DESTINATION / ".git").exists():
        sync()
    git("fetch", "-q", "--depth", "1", "origin", "master")
    git("checkout", "-q", "-B", "master", "FETCH_HEAD")
    target = DESTINATION / "replays" / version
    target.mkdir(parents=True, exist_ok=True)
    added = []
    for replay in replays:
        kept = target / replay.name
        if kept.exists():
            if not filecmp.cmp(replay, kept, shallow=False):
                fail(f"{kept.name} is already in the corpus with other bytes")
            continue
        shutil.copy2(replay, kept)
        added.append(kept)
    if not added:
        print(f"nothing new of {version} to publish")
        return
    git("add", "--", *(str(p.relative_to(DESTINATION)) for p in added))
    git("commit", "-q", "-m", f"replay: {len(added)} of {version}")
    git("push", "-q", "origin", "HEAD:master")
    print(f"{len(added)} replays of {version} published at {git('rev-parse', '--short', 'HEAD')}")


def main(argv):
    if argv[1:] == ["sync"]:
        sync()
    elif argv[1:] == ["publish"]:
        publish()
    elif len(argv) in (2, 3) and argv[1] == "path":
        path(argv[2] if len(argv) == 3 else None)
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
