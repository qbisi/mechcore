#!/usr/bin/env python3
"""Fight every corpus round from its replay and from the replay its battle writes.

``mechcore replay convert <battle.yaml> <replay.grbr>`` writes a battle back as
a replay that converts to the same battle again. This asks the other half:
whether the game plays that replay as it plays the match's own. For every
battle ``scripts/export-replay-corpus.py`` writes under
``work/battle/<version>/``, the replay of the same basename in
``work/replay/replays/<version>/`` and the replay the battle writes are fought
round by round headlessly (``record_replay_round``), and each pair of
recordings is compared tick for tick, physics and content.

A battle is recorded in one game session; a round the game refuses is reported
and the battle's remaining rounds carry on in a new session. A recording on
disk is kept, so an interrupted run resumes where it stopped. Each session's
game log is kept beside the recordings as ``player-<n>.log``, since the game
names every decision it refused there.

Run from anywhere inside the checkout, with the game installed:

    cargo build --release -p mechcore
    python3 scripts/replay.py sync
    python3 scripts/export-replay-corpus.py
    python3 scripts/battle-replays.py
    python3 scripts/battle-replays.py --only Chemtrails --json

A round in which a side concedes is listed and not compared: a concession made
during the fight ends it at a moment the battle does not record.

The exit status is 0 only when every other round records both ways and compares
equal.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shutil
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
        default=Path("/tmp/mechcore/battle-replays"),
        help="where the written replays and the recordings go",
    )
    parser.add_argument("--only", help="only battles whose name contains this text")
    parser.add_argument("--json", action="store_true", help="print one JSON object per round")
    return parser.parse_args()


def rounds_of(battle: Path) -> tuple[list[int], set[int]]:
    """The rounds a battle decides, and those of them in which a side concedes.

    A concession is kept as its round's last decision, but one made during the
    fight ends it at a moment the battle does not record, so such a round is not
    compared."""
    text = battle.read_text(encoding="utf-8")
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
        script = "game: launch\n\nsteps:\n" + "".join(
            "  - game.record_replay_round:\n"
            f"      grbr: {json.dumps(steps[index]['grbr'], ensure_ascii=False)}\n"
            f"      round: {steps[index]['round']}\n"
            f"      output: {json.dumps(steps[index]['output'], ensure_ascii=False)}\n"
            for index in pending
        )
        path = folder / "session.mcscript"
        path.write_text(script, encoding="utf-8")
        result = run([str(mechcore), "run", str(path)])
        keep_game_log(folder)
        remaining = [index for index in pending if not Path(steps[index]["output"]).exists()]
        if result.returncode == 0 or not remaining:
            for index in remaining:
                failed[index] = "the session ended before recording it"
            break
        # The first step without a recording is the one the session stopped on.
        failed[remaining[0]] = reason(result.stdout + result.stderr)
        pending = remaining[1:]
    return failed


def keep_game_log(folder: Path) -> None:
    """Keeps the log of the game session just run beside its recordings: the
    game names each decision it refused there, and the next launch rotates it
    away."""
    log = Path.home() / "Library/Logs/GameRiver/Mechabellum/Player.log"
    if log.exists():
        sessions = len(list(folder.glob("player-*.log")))
        shutil.copy(log, folder / f"player-{sessions}.log")


def compare(mechcore: Path, left: str, right: str) -> dict:
    result = run([str(mechcore), "fight", "compare", left, right, "--format", "json"])
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
    battles = sorted((root / "work/battle" / version).glob("*.yaml"))
    replays = root / "work/replay/replays" / version
    if arguments.only:
        battles = [battle for battle in battles if arguments.only in battle.name]
    if not battles:
        print(f"no battle documents under work/battle/{version}", file=sys.stderr)
        return 1

    results = []
    for battle in battles:
        folder = arguments.out / battle.stem
        folder.mkdir(parents=True, exist_ok=True)
        written = folder / "written.grbr"
        converted = run(
            [str(arguments.mechcore), "replay", "convert", str(battle), str(written), "--force"]
        )
        numbers, conceded = rounds_of(battle)
        skipped = [
            {"battle": battle.stem, "round": number, "conceded": True} for number in conceded
        ]
        entries = [
            {"battle": battle.stem, "round": number}
            for number in numbers
            if number not in conceded
        ]
        if converted.returncode != 0:
            for entry in entries:
                entry["refused"] = reason(converted.stdout + converted.stderr)
        else:
            steps = []
            for entry in entries:
                for tag, source in (("replay", replays / f"{battle.stem}.grbr"), ("battle", written)):
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
                print(f"{entry['battle']} round {entry['round']}: {verdict(entry)}", flush=True)

    compared = [entry for entry in results if not entry.get("conceded")]
    equal = sum(1 for entry in compared if entry.get("equal"))
    print(
        f"{equal} of {len(compared)} rounds fight the same from the battle's replay as from "
        f"the match's own; {len(results) - len(compared)} conceded, not compared",
        file=sys.stderr,
    )
    return 0 if equal == len(compared) else 1


if __name__ == "__main__":
    sys.exit(main())
