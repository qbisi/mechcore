#!/usr/bin/env python3
"""Collect native replays by watching live standard 1v1 matches unattended.

Each capture is one ``mechcore game record --watch`` at level 0, the lowest,
so anything else takes the machine from the collector: a client of a higher
level ends the watch in progress at its next poll, the game returns to the
main menu, and the collector stops, leaving that game to its claimant. The
game keeps each match it admitted at round one in its own replay directory,
where ``scripts/replay.py publish`` finds it.

    scripts/collect-replays.py                       until 10000 matches, or one fails
    scripts/collect-replays.py --count 20 > collected.jsonl

One JSON line per capture goes to standard output, the recording's result, so
the per-file path, publication mode and scene metadata can travel with the
corpus. The collector fails closed: it stops at the first capture that does
not succeed, and exits 1. A command joins a game somebody started, so when
none answers it starts one, headless, which lingers for the next capture.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=ROOT / "target/release/mechcore")
    parser.add_argument("--count", type=int, default=10000, help="captures to attempt")
    parser.add_argument("--wait-for-scene-seconds", type=int, default=900)
    parser.add_argument("--match-timeout-seconds", type=int, default=7200)
    parser.add_argument("--output-dir", type=Path, help="also copy each replay here")
    return parser.parse_args()


def launch(mechcore: Path) -> None:
    """Starts a headless game at level 0 that lingers for the next capture."""
    with tempfile.NamedTemporaryFile("w", suffix=".mcscript", delete=False) as script:
        script.write("game: launch\nheadless: true\nlevel: 0\n\nsteps:\n  - game.status: {}\n")
    launched = subprocess.run([str(mechcore), "run", script.name], capture_output=True, text=True)
    Path(script.name).unlink()
    if launched.returncode != 0:
        sys.exit(f"cannot launch the game: {launched.stdout[-400:]}{launched.stderr[-400:]}")


def answer(result: subprocess.CompletedProcess[str]) -> dict:
    for stream in (result.stdout, result.stderr):
        for line in reversed(stream.strip().splitlines()):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return {"reason": (result.stdout + result.stderr)[-400:]}


def capture(arguments: argparse.Namespace) -> subprocess.CompletedProcess[str]:
    command = [
        str(arguments.mechcore), "game", "record", "--watch", "--level", "0",
        "--wait-for-scene-seconds", str(arguments.wait_for_scene_seconds),
        "--match-timeout-seconds", str(arguments.match_timeout_seconds),
    ]
    if arguments.output_dir:
        command += ["--output-dir", str(arguments.output_dir)]
    return subprocess.run(command, capture_output=True, text=True, cwd=ROOT)


def main() -> int:
    arguments = parse_arguments()
    for _ in range(arguments.count):
        result = capture(arguments)
        if result.returncode != 0 and answer(result).get("kind") == "unavailable":
            launch(arguments.mechcore)
            result = capture(arguments)
        print(json.dumps(answer(result), ensure_ascii=False), flush=True)
        if result.returncode != 0:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
