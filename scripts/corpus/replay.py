#!/usr/bin/env python3
"""Fetch the corpus and publish collected match documents.

    uv run --with pyyaml python scripts/corpus/replay.py sync
    uv run --with pyyaml python scripts/corpus/replay.py path [VERSION]
    uv run --with pyyaml python scripts/corpus/replay.py publish [--source work/watch] [--dry-run]

Requires PyYAML. The public qbisi/mechcore-replay repository is read at master
under work/replay. New data is match YAML under matches/<GAME_VERSION>/,
including partial matches collected by collect-replays.py. Native GRBR files
are not uploaded; the earlier native archive remains in Git history.

Publish reads only BattleID-rN.yaml files in the source directory. Their match
header must name this checkout's build or omit it, and the final captured state
must be round N. An existing name must hold identical bytes; it is never
replaced. --dry-run validates and reports without fetching, committing or
pushing. Publication fetches over HTTPS and pushes over SSH port 443.
"""

import argparse
import filecmp
import re
import shutil
import subprocess
import sys
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402

REPOSITORY = "https://github.com/qbisi/mechcore-replay"
# Pushes go over SSH on port 443, which networks that block port 22 still let through.
PUSH_URL = "ssh://git@ssh.github.com:443/qbisi/mechcore-replay.git"
ROOT = Path(__file__).resolve().parents[2]
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
    version = version or build_data.configured_build()
    directory = DESTINATION / "matches" / version
    if not directory.is_dir():
        fail(f"no matches of {version} under work/replay; run scripts/corpus/replay.py sync")
    print(directory)


def publish(source, dry_run):
    version = build_data.configured_build()
    if not source.is_dir():
        fail(f"match source is not a directory: {source}")
    names = re.compile(r"^[A-Za-z0-9_.-]+-r([1-9][0-9]*)\.yaml$")
    matches = sorted(source.glob("*.yaml"))
    for match in matches:
        identity = names.fullmatch(match.name)
        if identity is None:
            fail(f"match name is not BattleID-rN.yaml: {match.name}")
        try:
            segments = list(yaml.safe_load_all(match.read_text()))
        except (OSError, UnicodeError, yaml.YAMLError) as error:
            fail(f"cannot read {match.name}: {error}")
        if not segments or not isinstance(segments[0], dict) or segments[0].get("kind") != "match":
            fail(f"{match.name} is not a match document")
        if segments[0].get("game_build", version) != version:
            fail(f"{match.name} names another game build")
        states = [segment for segment in segments[1:]
                  if isinstance(segment, dict) and segment.get("kind") == "state"]
        if not states or states[-1].get("round") != int(identity[1]):
            fail(f"{match.name} does not end on its named captured round")
    if dry_run:
        print(f"{len(matches)} matches of {version} ready for matches/{version}/")
        return
    if not matches:
        print(f"nothing new of {version} to publish")
        return
    if not (DESTINATION / ".git").exists():
        sync()
    if git("status", "--porcelain"):
        fail("corpus checkout has local changes; preserve them before publishing")
    git("fetch", "-q", "--depth", "1", "origin", "master")
    git("checkout", "-q", "-B", "master", "FETCH_HEAD")
    target = DESTINATION / "matches" / version
    target.mkdir(parents=True, exist_ok=True)
    added = []
    for match in matches:
        kept = target / match.name
        if kept.exists():
            if not filecmp.cmp(match, kept, shallow=False):
                fail(f"{kept.name} is already in the corpus with other bytes")
            continue
        added.append((match, kept))
    if not added:
        print(f"nothing new of {version} to publish")
        return
    for source_match, kept in added:
        shutil.copy2(source_match, kept)
    git("add", "--", *(str(kept.relative_to(DESTINATION)) for _, kept in added))
    git("commit", "-q", "-m", f"data: add {len(added)} matches of {version}\n\nCo-Authored-By: GPT-6 <noreply@openai.com>")
    git("push", "-q", PUSH_URL, "HEAD:master")
    print(f"{len(added)} matches of {version} published at {git('rev-parse', '--short', 'HEAD')}")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("sync")
    directory = commands.add_parser("path")
    directory.add_argument("version", nargs="?")
    publication = commands.add_parser("publish")
    publication.add_argument("--source", type=Path, default=ROOT / "work/watch")
    publication.add_argument("--dry-run", action="store_true")
    arguments = parser.parse_args(argv[1:])
    if arguments.command == "sync":
        sync()
    elif arguments.command == "publish":
        publish(arguments.source.resolve(), arguments.dry_run)
    else:
        path(arguments.version)


if __name__ == "__main__":
    main(sys.argv)
