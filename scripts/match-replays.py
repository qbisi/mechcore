#!/usr/bin/env python3
"""Fight every corpus round from its replay and from the replay its match writes.

``mechcore convert <match.yaml> --to grbr <replay.grbr>`` writes a match back as
a replay that converts to the same match again. This asks the other half:
whether the game plays that replay as it plays the match's own. For every
match ``scripts/export-replay-corpus.py`` writes under
``work/match/<version>/``, the replay of the same basename in
``work/replay/replays/<version>/`` and the replay the match writes are fought
round by round headlessly (``game record --round``), and each pair of
recordings is compared tick for tick, physics and content.

A match is recorded in one game session; a round the game refuses is reported
and the match's remaining rounds carry on in a new session. A recording on
disk is kept, so an interrupted run resumes where it stopped. The game runs
headless, and each session's game log is kept beside the recordings as
``game-<n>.log``, since the game names every decision it refused there.

Run from anywhere inside the checkout, with the game installed:

    cargo build --release -p mechcore
    python3 scripts/replay.py sync
    python3 scripts/export-replay-corpus.py
    python3 scripts/match-replays.py
    python3 scripts/match-replays.py --only Chemtrails --json

A round in which a side concedes is listed and not compared: a concession made
during the fight ends it at a moment the match does not record.

The exit status is 0 only when every other round records both ways and compares
equal.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

import build_data


def parse_arguments(root: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--mechcore", type=Path, default=root / "target/release/mechcore")
    parser.add_argument("--version", help="the game version (default: this checkout's)")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("/tmp/mechcore/match-replays"),
        help="where the written replays and the recordings go",
    )
    parser.add_argument("--only", help="only matches whose name contains this text")
    parser.add_argument("--json", action="store_true", help="print one JSON object per round")
    return parser.parse_args()


def rounds_of(match_doc: Path) -> tuple[list[int], set[int]]:
    """The rounds a match decides, and those of them in which a side concedes.

    A concession is kept as its round's last decision, but one made during the
    fight ends it at a moment the match does not record, so such a round is not
    compared."""
    text = match_doc.read_text(encoding="utf-8")
    rounds, conceded = [], set()
    for segment in re.split(r"^---$", text, flags=re.MULTILINE):
        found = re.match(r"\s*kind: action\nround: (\d+)$", segment, flags=re.MULTILINE)
        if found and int(found.group(1)) >= 1:
            rounds.append(int(found.group(1)))
            if re.search(r"^- \{type: concede\}$", segment, flags=re.MULTILINE):
                conceded.add(int(found.group(1)))
    return sorted(rounds), conceded


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, capture_output=True, text=True, check=False)


def reason(output: str) -> str:
    for line in output.splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(record, dict) and "reason" in record:
            return str(record["reason"])
    return output.strip()[-400:]


def record(mechcore: Path, steps: list[dict], folder: Path) -> dict[int, str]:
    """Records the steps in one game session, resuming after a refused one.
    Answers the refusal of each step that failed, by position."""
    failed: dict[int, str] = {}
    pending = [index for index, step in enumerate(steps) if not Path(step["output"]).exists()]
    while pending:
        # A JSON string is a YAML scalar only while it escapes nothing outside
        # the Basic Multilingual Plane, which a player's name can hold.
        script = "game: launch\nheadless: true\n\nsteps:\n" + "".join(
            "  - game.record:\n"
            f"      input: {json.dumps(steps[index]['grbr'], ensure_ascii=False)}\n"
            f"      round: {steps[index]['round']}\n"
            f"      output: {json.dumps(steps[index]['output'], ensure_ascii=False)}\n"
            for index in pending
        )
        path = folder / "session.mcscript"
        path.write_text(script, encoding="utf-8")
        start = GAME_LOG.stat().st_size if game_running() and GAME_LOG.exists() else 0
        result = run([str(mechcore), "run", str(path)])
        keep_game_log(folder, start)
        remaining = [index for index in pending if not Path(steps[index]["output"]).exists()]
        if result.returncode == 0 or not remaining:
            for index in remaining:
                failed[index] = "the session ended before recording it"
            break
        # The first step without a recording is the one the session stopped on.
        failed[remaining[0]] = reason(result.stdout + result.stderr)
        pending = remaining[1:]
    return failed


GAME_LOG = Path(f"/tmp/mechcore-game-{os.getuid()}.log")
"""Where mechcore sends a launched game's output. A headless game writes
Unity's log there too, and a launch truncates it."""


def game_running() -> bool:
    """Whether a game is running, which the next session will reuse rather
    than launch."""
    return run(["pgrep", "-f", "Mechabellum.app/Contents/MacOS/Mechabellum"]).returncode == 0


def keep_game_log(folder: Path, start: int) -> None:
    """Keeps what the game logged during the session just run beside its
    recordings: the game names each decision it refused there. A session that
    reused the game left running by the previous one continues its log from
    ``start``; one that launched a new game began it again."""
    if not GAME_LOG.exists():
        return
    with GAME_LOG.open("rb") as log:
        size = log.seek(0, 2)
        log.seek(start if start <= size else 0)
        session = log.read()
    sessions = len(list(folder.glob("game-*.log")))
    (folder / f"game-{sessions}.log").write_bytes(session)


def compare(mechcore: Path, left: str, right: str) -> dict:
    result = run([str(mechcore), "diff", left, right, "--format", "json"])
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"error": reason(result.stdout + result.stderr)}
    return {
        "equal": report.get("equal"),
        "first_divergence": report.get("first_divergence"),
        "ticks": [
            report.get("left", {}).get("tick_count"),
            report.get("right", {}).get("tick_count"),
        ],
        "differences": [
            f"{item.get('object')} {item.get('field')}"
            for item in (report.get("at") or {}).get("differences", [])[:3]
        ],
    }


def verdict(entry: dict) -> str:
    if entry.get("equal"):
        return "equal"
    if entry.get("conceded"):
        return "conceded, not compared"
    if "refused" in entry:
        return f"refused: {entry['refused']}"
    if "failed" in entry:
        return f"failed: {entry['failed']}"
    return f"differs from tick {entry.get('first_divergence')}: " + "; ".join(
        entry.get("differences", [])
    )


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    arguments = parse_arguments(root)
    version = arguments.version or build_data.build()
    matches = sorted((root / "work/match" / version).glob("*.yaml"))
    replays = root / "work/replay/replays" / version
    if arguments.only:
        matches = [match_doc for match_doc in matches if arguments.only in match_doc.name]
    if not matches:
        print(f"no match documents under work/match/{version}", file=sys.stderr)
        return 1

    results = []
    for match_doc in matches:
        folder = arguments.out / match_doc.stem
        folder.mkdir(parents=True, exist_ok=True)
        written = folder / "written.grbr"
        converted = run(
            [
                str(arguments.mechcore),
                "convert",
                str(match_doc),
                "--to",
                "grbr",
                str(written),
                "--force",
            ]
        )
        numbers, conceded = rounds_of(match_doc)
        skipped = [
            {"match": match_doc.stem, "round": number, "conceded": True} for number in conceded
        ]
        entries = [
            {"match": match_doc.stem, "round": number}
            for number in numbers
            if number not in conceded
        ]
        if converted.returncode != 0:
            for entry in entries:
                entry["refused"] = reason(converted.stdout + converted.stderr)
        else:
            steps = []
            for entry in entries:
                for tag, source in (("replay", replays / f"{match_doc.stem}.grbr"), ("match", written)):
                    steps.append(
                        {
                            "grbr": str(source.resolve()),
                            "round": entry["round"],
                            "output": str(folder / f"{entry['round']}-{tag}.mcfr"),
                        }
                    )
            failed = record(arguments.mechcore, steps, folder)
            for at, entry in enumerate(entries):
                left, right = 2 * at, 2 * at + 1
                if left in failed or right in failed:
                    entry["failed"] = failed.get(left) or failed.get(right)
                else:
                    entry.update(
                        compare(arguments.mechcore, steps[left]["output"], steps[right]["output"])
                    )
        for entry in sorted(entries + skipped, key=lambda entry: entry["round"]):
            results.append(entry)
            if arguments.json:
                print(json.dumps(entry, ensure_ascii=False), flush=True)
            else:
                print(f"{entry['match']} round {entry['round']}: {verdict(entry)}", flush=True)

    compared = [entry for entry in results if not entry.get("conceded")]
    equal = sum(1 for entry in compared if entry.get("equal"))
    print(
        f"{equal} of {len(compared)} rounds fight the same from the match's replay as from "
        f"the match's own; {len(results) - len(compared)} conceded, not compared",
        file=sys.stderr,
    )
    return 0 if equal == len(compared) else 1


if __name__ == "__main__":
    sys.exit(main())
