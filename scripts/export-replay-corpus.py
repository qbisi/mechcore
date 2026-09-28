#!/usr/bin/env python3
"""Convert every replay of one game version into its match document.

The corpus is https://github.com/qbisi/mechcore-replay, fetched to
``work/replay`` by ``scripts/replay.py sync``: the replays of a version sit
directly under ``replays/<version>/``. Each is converted without the game by
``mechcore convert --to match`` into the match YAML of the same basename under
``work/match/<version>/``, replacing what is there. The corpus holds no
documents; they are generated where they are read. A replay the converter
refuses is reported and makes the run fail, so a version the converter no
longer reads cannot pass unnoticed.

Conversion is idempotent across the match format: each match is written back
as a replay (``mechcore convert <match.yaml> --to grbr <replay.grbr>``) and
converted again, and the second document has to be the first byte for byte. A
match that does not come back the same fails the run too.

Run from anywhere inside the checkout, after a release build:

    cargo build --release -p mechcore
    python3 scripts/replay.py sync
    python3 scripts/export-replay-corpus.py

The version is this checkout's unless ``--version`` names another.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

import build_data




def parse_arguments(root: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=root / "target/release/mechcore")
    parser.add_argument("--version", help="the game version to convert (default: this checkout's)")
    parser.add_argument(
        "--match-dir",
        type=Path,
        help="where the documents go (default: work/match/<version>)",
    )
    return parser.parse_args()


def round_trip(executable: Path, root: Path, match_doc: Path) -> str | None:
    """Writes a match back as a replay and converts that again; answers what
    went wrong, or nothing when the second document is the first."""
    with tempfile.TemporaryDirectory() as scratch:
        replay = Path(scratch) / "written.grbr"
        again = Path(scratch) / "again.yaml"
        for command in (
            [str(executable), "convert", str(match_doc), "--to", "grbr", str(replay), "--force"],
            [str(executable), "convert", str(replay), "--to", "match", str(again), "--force"],
        ):
            step = subprocess.run(command, cwd=root, text=True, capture_output=True, check=False)
            if step.returncode != 0:
                return step.stderr.strip() or step.stdout.strip()
        if again.read_bytes() != match_doc.read_bytes():
            return "the match converted back differs from the match written"
    return None


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    args = parse_arguments(root)
    executable = args.mechcore.resolve()
    version = args.version or build_data.build()
    corpus = root / "work" / "replay" / "replays" / version
    match_dir = (args.match_dir or root / "work" / "match" / version).resolve()
    if not executable.is_file():
        print(f"mechcore executable does not exist: {executable}", file=sys.stderr)
        return 2
    sources = sorted(corpus.glob("*.grbr"))
    if not sources:
        print(f"no replays of {version} in {corpus}; run scripts/replay.py sync", file=sys.stderr)
        return 2

    match_dir.mkdir(parents=True, exist_ok=True)
    refused = []
    for index, source in enumerate(sources, 1):
        print(f"[{index}/{len(sources)}] {source.name}", flush=True)
        match_doc = match_dir / f"{source.stem}.yaml"
        converted = subprocess.run(
            [str(executable), "convert", str(source), "--to", "match", str(match_doc), "--force"],
            cwd=root,
            text=True,
            capture_output=True,
            check=False,
        )
        if converted.returncode != 0 or not match_doc.is_file():
            refused.append(source.name)
            print(f"  refused: {converted.stderr.strip() or converted.stdout.strip()}", flush=True)
            continue
        problem = round_trip(executable, root, match_doc)
        if problem:
            refused.append(source.name)
            print(f"  does not round-trip: {problem}", flush=True)

    print(f"{len(sources) - len(refused)}/{len(sources)} replays converted and round-trip")
    return 1 if refused else 0


if __name__ == "__main__":
    raise SystemExit(main())
