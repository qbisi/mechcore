#!/usr/bin/env python3
"""Record fight documents again with the game, one command per fight.

A pinned fight is a fight document under ``tests/<topic>/fights/``. This
fights each one given in the game, headless, with ``mechcore convert <fight>
--to mcfr --backend game``, and writes the recording to ``<out>/<name>.mcfr``,
the fixture's own name. With ``--check`` each recording is read back as a
fight (``convert --to fight``) and compared with its fixture (``diff``); a
fight the game no longer records as its fixture states it is listed, and the
exit status is 1.

    scripts/record-fights.py --check tests/regression/fights/*.yaml
    scripts/record-fights.py --instrument skill_attackable_checker,group_slots \\
        --out /tmp/mechcore/wraith/slots tests/wraith/fights/*.yaml

A command joins a game somebody started, so the first fight that finds none
starts one: a run that declares ``game: launch`` and asks for its status,
after which the game lingers for its next client. Recordings already on disk
are kept, so an interrupted run resumes where it stopped; ``--force`` records
them again.
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
    parser.add_argument("fights", nargs="+", type=Path, help="fight documents")
    parser.add_argument("--mechcore", type=Path, default=ROOT / "target/release/mechcore")
    parser.add_argument("--out", type=Path, default=Path("/tmp/mechcore/record-fights"))
    parser.add_argument("--instrument", help="instrument channels, comma separated")
    parser.add_argument("--check", action="store_true", help="compare each recording with its fixture")
    parser.add_argument("--force", action="store_true", help="record again what is on disk")
    return parser.parse_args()


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, capture_output=True, text=True, cwd=ROOT)


def launch(mechcore: Path) -> None:
    """Starts a headless game that lingers for the next command."""
    with tempfile.NamedTemporaryFile("w", suffix=".mcscript", delete=False) as script:
        script.write("game: launch\nheadless: true\n\nsteps:\n  - game.status: {}\n")
    launched = run([str(mechcore), "run", script.name])
    Path(script.name).unlink()
    if launched.returncode != 0:
        sys.exit(f"cannot launch the game: {launched.stdout[-400:]}{launched.stderr[-400:]}")


def refusal(result: subprocess.CompletedProcess[str]) -> dict:
    for stream in (result.stdout, result.stderr):
        for line in reversed(stream.strip().splitlines()):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return {"reason": (result.stdout + result.stderr)[-400:]}


def record(arguments: argparse.Namespace, fight: Path, output: Path) -> dict | None:
    """Records one fight, starting a game once if none answers. Answers the
    refusal when the game would not record it."""
    command = [str(arguments.mechcore), "convert", str(fight), "--to", "mcfr",
               "--backend", "game", str(output), "--force"]
    if arguments.instrument:
        command += ["--instrument", arguments.instrument]
    for attempt in range(2):
        result = run(command)
        if result.returncode == 0:
            return None
        answer = refusal(result)
        if attempt == 0 and answer.get("kind") == "unavailable":
            launch(arguments.mechcore)
            continue
        return answer
    return None


def differs(mechcore: Path, fight: Path, recording: Path) -> str | None:
    """Reads the recording back as a fight and compares it with the fixture."""
    with tempfile.TemporaryDirectory() as directory:
        written = Path(directory) / fight.name
        converted = run([str(mechcore), "convert", str(recording), "--to", "fight", str(written)])
        if converted.returncode != 0:
            return refusal(converted).get("reason", "the recording is not a fight")
        compared = run([str(mechcore), "diff", str(fight), str(written)])
        if compared.returncode != 0:
            return compared.stdout.strip()[-400:] or refusal(compared).get("reason")
    return None


def main() -> int:
    arguments = parse_arguments()
    arguments.out.mkdir(parents=True, exist_ok=True)
    failed = 0
    for fight in arguments.fights:
        output = arguments.out / f"{fight.stem}.mcfr"
        if output.exists() and not arguments.force:
            status = "kept"
        else:
            answer = record(arguments, fight.resolve(), output)
            if answer is not None:
                print(f"{fight}: refused: {answer.get('reason', answer)}")
                failed += 1
                continue
            status = "recorded"
        if arguments.check:
            difference = differs(arguments.mechcore, fight.resolve(), output)
            if difference is not None:
                print(f"{fight}: differs from its recording: {difference}")
                failed += 1
                continue
            status += ", equal"
        print(f"{fight}: {status}")
    print(f"{len(arguments.fights) - failed} of {len(arguments.fights)} fights {'recorded and equal' if arguments.check else 'recorded'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
