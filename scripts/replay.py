#!/usr/bin/env python3
"""Fetch the replay corpus this checkout is held to.

    scripts/replay.py sync            fetch qbisi/mechcore-replay at replay/REPLAY_REV into work/replay
    scripts/replay.py sync --rev REV  fetch another commit instead, to try a corpus before pinning it
    scripts/replay.py path [BUILD]    print the corpus directory for a build (default: the only one)

The native replays and the battle documents converted from them live in
https://github.com/qbisi/mechcore-replay, one directory per game build, and
this repository names the commit it reads them at in `replay/REPLAY_REV`.
`scripts/verify-battles.py`, `scripts/fight-coverage.py` and CI read
`work/replay/replays/<build>/{grbr,battle}`, which this script fills; `work/`
is not tracked, so a checkout runs `sync` once, and again after the pin moves.
The test suite reads no replay. Nothing here writes to the corpus: a replay is added there, and a
battle document is what its workflow converts.

The fetch is shallow and by commit, so it is the pinned tree and nothing else.
"""

import subprocess
import sys
from pathlib import Path

REPOSITORY = "https://github.com/qbisi/mechcore-replay"
ROOT = Path(__file__).resolve().parents[1]
PIN = ROOT / "replay" / "REPLAY_REV"
DESTINATION = ROOT / "work" / "replay"


def fail(message):
    print(f"replay: {message}", file=sys.stderr)
    sys.exit(1)


def git(*arguments, cwd=DESTINATION):
    result = subprocess.run(["git", *arguments], cwd=cwd, text=True, capture_output=True)
    if result.returncode != 0:
        fail(f"git {' '.join(arguments)} failed:\n{result.stderr.strip()}")
    return result.stdout.strip()


def pinned():
    if not PIN.exists():
        fail(f"{PIN.relative_to(ROOT)} is missing")
    return PIN.read_text().strip()


def sync(rev):
    if (DESTINATION / ".git").exists():
        if git("rev-parse", "HEAD") == rev:
            print(f"work/replay is at {rev}")
            return
    else:
        DESTINATION.mkdir(parents=True, exist_ok=True)
        git("init", "-q")
        git("remote", "add", "origin", REPOSITORY)
    git("fetch", "-q", "--depth", "1", "origin", rev)
    git("checkout", "-q", "--detach", "FETCH_HEAD")
    print(f"work/replay is at {rev}")


def path(build):
    builds = sorted(p.name for p in (DESTINATION / "replays").glob("*") if p.is_dir())
    if not builds:
        fail("no corpus under work/replay; run scripts/replay.py sync")
    if build is None:
        if len(builds) != 1:
            fail(f"several builds under work/replay, name one: {', '.join(builds)}")
        build = builds[0]
    elif build not in builds:
        fail(f"no build {build} under work/replay; it holds {', '.join(builds)}")
    print(DESTINATION / "replays" / build)


def main(argv):
    if len(argv) >= 2 and argv[1] == "sync":
        rev = argv[3] if len(argv) == 4 and argv[2] == "--rev" else (pinned() if len(argv) == 2 else None)
        if rev is None:
            fail("usage: sync [--rev REV]")
        sync(rev)
    elif len(argv) in (2, 3) and argv[1] == "path":
        path(argv[2] if len(argv) == 3 else None)
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
