#!/usr/bin/env python3
"""Fetch the replay corpus.

    scripts/replay.py sync             fetch qbisi/mechcore-replay's master into work/replay
    scripts/replay.py path [VERSION]   print a version's replay directory (default: this checkout's)

The native replays live in https://github.com/qbisi/mechcore-replay, one
directory per game version, `replays/<version>/<name>.grbr`. The repository
only grows, so it is read at `master` and a newer fetch never takes away what
an older one had. `work/` is not tracked, so a checkout runs `sync` once, and
again to pick up replays added since. The version this checkout describes is
the one `scripts/build_data.py` reads. Nothing here writes to the corpus, and
nothing is generated in it: `scripts/export-replay-corpus.py` converts a
version's replays into `work/battle/<version>/`.
"""

import subprocess
import sys
from pathlib import Path

import build_data

REPOSITORY = "https://github.com/qbisi/mechcore-replay"
ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "work" / "replay"


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


def main(argv):
    if argv[1:] == ["sync"]:
        sync()
    elif len(argv) in (2, 3) and argv[1] == "path":
        path(argv[2] if len(argv) == 3 else None)
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
