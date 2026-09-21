#!/usr/bin/env python3
"""Regenerate every tracked battle document from its replay.

Each GRBR under ``replay/grbr`` is converted offline with ``mechcore replay convert``
into the battle YAML of the same basename under ``replay/battle``, replacing
what is there. Both directories' ``SHA256SUMS`` are rewritten afterwards. A
replay the converter refuses is reported and makes the run fail, so a corpus
that no longer converts cannot pass unnoticed.

Run from anywhere inside the checkout, after a release build:

    cargo build --release -p mechcore
    python3 scripts/export-replay-corpus.py
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def save_checksums(paths: list[Path], destination: Path, root: Path) -> None:
    lines = [f"{digest(path)}  {path.relative_to(root)}\n" for path in sorted(paths)]
    temporary = destination.with_name(f".{destination.name}.tmp")
    temporary.write_text("".join(lines))
    os.replace(temporary, destination)


def parse_arguments(root: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=root / "target/release/mechcore")
    parser.add_argument("--grbr-dir", type=Path, default=root / "replay/grbr")
    parser.add_argument("--battle-dir", type=Path, default=root / "replay/battle")
    return parser.parse_args()


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    args = parse_arguments(root)
    executable = args.mechcore.resolve()
    grbr_dir = args.grbr_dir.resolve()
    battle_dir = args.battle_dir.resolve()

    if not executable.is_file():
        print(f"mechcore executable does not exist: {executable}", file=sys.stderr)
        return 2
    sources = sorted(grbr_dir.glob("*.grbr"))
    if not sources:
        print(f"no GRBR inputs in {grbr_dir}", file=sys.stderr)
        return 2

    battle_dir.mkdir(parents=True, exist_ok=True)
    refused = []
    for index, source in enumerate(sources, 1):
        print(f"[{index}/{len(sources)}] {source.name}", flush=True)
        battle = battle_dir / f"{source.stem}.yaml"
        converted = subprocess.run(
            [str(executable), "replay", "convert", str(source), str(battle), "--force"],
            cwd=root,
            text=True,
            capture_output=True,
            check=False,
        )
        if converted.returncode != 0 or not battle.is_file():
            refused.append(source.name)
            print(f"  refused: {converted.stderr.strip() or converted.stdout.strip()}", flush=True)

    save_checksums(sources, grbr_dir / "SHA256SUMS", root)
    save_checksums(list(battle_dir.glob("*.yaml")), battle_dir / "SHA256SUMS", root)
    print(f"{len(sources) - len(refused)}/{len(sources)} replays converted")
    return 1 if refused else 0


if __name__ == "__main__":
    raise SystemExit(main())
