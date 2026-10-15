#!/usr/bin/env python3
"""Collect available partial matches from live standard 1v1 matches.

Each capture is one ``mechcore game record --watch`` at level 0, the lowest,
so anything else takes the machine from the collector: a client of a higher
level ends the watch in progress at its next poll, the game returns to the
main menu, and the collector stops, leaving that game to its claimant. The
collector enters rooms displayed at round 2 or later, saves the available
native cache, and immediately leaves. The underlying game command writes GRBR; this script
converts it, atomically publishes YAML without overwriting, then removes GRBR.
The final file is BattleID-rN.yaml. Conversion failure retains GRBR and stops
the collector. N is the spectator's recorded round, which can lag the lobby. Existing
files are skipped. Visits in that directory survive restarts: rooms left less
than 60 seconds ago are excluded, and unvisited rooms are preferred.

    scripts/corpus/collect-replays.py                       up to 10000 entries
    scripts/corpus/collect-replays.py --count 20 > collected.jsonl
    scripts/corpus/collect-replays.py --duration-seconds 120 --output-dir work/watch

One JSON line per capture goes to standard output, the recording's result, so
the file identity, duplicate/skip result and scene metadata can travel with
the collection. A stale or unavailable room is skipped; other errors stop
the collector with exit 1. A command joins a game somebody started, so when
none answers it starts one, headless and online, since watching needs the
network, which lingers for the next capture.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=ROOT / "target/release/mechcore")
    parser.add_argument("--count", type=int, default=10000, help="captures to attempt")
    parser.add_argument("--wait-for-scene-seconds", type=int, default=900)
    parser.add_argument("--entry-timeout-seconds", type=int, default=30)
    parser.add_argument("--duration-seconds", type=float, help="collection window, then finish cleanup")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "work/watch")
    arguments = parser.parse_args()
    if arguments.count < 1 or arguments.wait_for_scene_seconds < 1:
        parser.error("count and wait-for-scene-seconds must be positive")
    if not 1 <= arguments.entry_timeout_seconds <= 180:
        parser.error("entry-timeout-seconds must be 1..180")
    if arguments.duration_seconds is not None and arguments.duration_seconds <= 0:
        parser.error("duration-seconds must be positive")
    arguments.output_dir = arguments.output_dir.resolve()
    return arguments


def launch(mechcore: Path) -> None:
    """Starts a headless, online game at level 0 that lingers for the next
    capture."""
    launched = subprocess.run(
        [str(mechcore), "game", "launch", "--online", "--level", "0"],
        capture_output=True, text=True, cwd=ROOT,
    )
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


def capture(arguments: argparse.Namespace, remaining: float | None) -> subprocess.CompletedProcess[str]:
    budget = arguments.wait_for_scene_seconds
    if remaining is not None:
        budget = min(budget, max(1, int(remaining)))
    command = [
        str(arguments.mechcore), "game", "record", "--watch", "--level", "0",
        "--wait-for-scene-seconds", str(budget),
        "--entry-timeout-seconds", str(arguments.entry_timeout_seconds),
        "--output-dir", str(arguments.output_dir),
    ]
    return subprocess.run(command, capture_output=True, text=True, cwd=ROOT)


def export_match(mechcore: Path, captured: dict) -> bool:
    """Publish a match without overwriting a round; retain GRBR on failure."""
    operation = captured.get("operation", {})
    if not operation.get("ready"):
        return True
    native = Path(operation["output"])
    destination = native.with_suffix(".yaml")
    duplicate = destination.exists()
    if not duplicate:
        with tempfile.TemporaryDirectory(prefix=".watch-match-", dir=destination.parent) as pending:
            temporary = Path(pending) / destination.name
            converted = subprocess.run(
                [str(mechcore), "convert", str(native), "--to", "match", str(temporary)],
                capture_output=True, text=True, cwd=ROOT,
            )
            if converted.returncode != 0:
                captured["conversion_error"] = answer(converted)
                return False
            try:
                os.link(temporary, destination)
            except FileExistsError:
                duplicate = True
    native.unlink()
    operation.update(output=str(destination), recorded=not duplicate, duplicate=duplicate)
    return True


def main() -> int:
    arguments = parse_arguments()
    deadline = None if arguments.duration_seconds is None else time.monotonic() + arguments.duration_seconds
    for _ in range(arguments.count):
        remaining = None if deadline is None else deadline - time.monotonic()
        if remaining is not None and remaining < 1:
            break
        result = capture(arguments, remaining)
        if result.returncode != 0 and answer(result).get("kind") == "unavailable":
            launch(arguments.mechcore)
            remaining = None if deadline is None else deadline - time.monotonic()
            if remaining is not None and remaining < 1:
                break
            result = capture(arguments, remaining)
        captured = answer(result)
        success = result.returncode == 0
        if success:
            try:
                success = export_match(arguments.mechcore, captured)
            except OSError as error:
                captured["conversion_error"] = {"kind": "failed", "reason": str(error)}
                success = False
        print(json.dumps(captured, ensure_ascii=False), flush=True)
        if not success:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
