#!/usr/bin/env python3
"""Convert every replay of a corpus build into its battle document.

The corpus is https://github.com/qbisi/mechcore-replay, fetched to
``work/replay`` by ``scripts/replay.py sync``. Each GRBR under the build's
``grbr/`` is converted offline with ``mechcore replay convert`` into the
battle YAML of the same basename under ``battle/``, replacing what is there,
and the build's ``SHA256SUMS`` is rewritten over both directories. A replay
the converter refuses is reported and makes the run fail, so a corpus that no
longer converts cannot pass unnoticed.

With ``--battle-dir`` the documents go elsewhere and no checksum file is
written, which is how CI asks whether the checked-out corpus is what this
converter writes: convert into a scratch directory and diff. The corpus's own
workflow runs this script at the mechcore commit its ``MECHCORE_REV`` names,
and commits what it writes.

Run from anywhere inside the checkout, after a release build:

    cargo build --release -p mechcore
    python3 scripts/replay.py sync
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
    parser.add_argument(
        "--corpus",
        type=Path,
        default=root / "work/replay/replays/1.11.1.3.2259",
        help="a build directory of mechcore-replay, holding grbr/ and battle/",
    )
    parser.add_argument(
        "--battle-dir",
        type=Path,
        help="write the documents here instead of the corpus's battle/, without checksums",
    )
    return parser.parse_args()


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    args = parse_arguments(root)
    executable = args.mechcore.resolve()
    corpus = args.corpus.resolve()
    grbr_dir = corpus / "grbr"
    battle_dir = args.battle_dir.resolve() if args.battle_dir else corpus / "battle"

    if not executable.is_file():
        print(f"mechcore executable does not exist: {executable}", file=sys.stderr)
        return 2
    sources = sorted(grbr_dir.glob("*.grbr"))
    if not sources:
        print(f"no GRBR inputs in {grbr_dir}; run scripts/replay.py sync", file=sys.stderr)
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

    if battle_dir == corpus / "battle":
        save_checksums(sources + list(battle_dir.glob("*.yaml")), corpus / "SHA256SUMS", corpus)
    print(f"{len(sources) - len(refused)}/{len(sources)} replays converted")
    return 1 if refused else 0


if __name__ == "__main__":
    raise SystemExit(main())
