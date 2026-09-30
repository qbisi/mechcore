#!/usr/bin/env python3
"""Fight every corpus round in the game into the fight document it records.

For every match ``scripts/corpus/export-replay-corpus.py`` writes under
``work/match/<version>/``, each round of the replay of the same basename in
``work/replay/replays/<version>/`` is fought headlessly and written as
``work/fight/<version>/<replay>-rNN.yaml``:
``mechcore convert <replay.grbr> --round <n> --to fight --backend game``, a
fight document of ``source: game`` with the recording's ticks and hash.
Such a document is a fixture candidate for ``tests/corpus/``: fighting its
projection with its seed in the simulator is checked against it by
``mechcore verify``.

A round in which a side concedes is left out: the concession ends the fight at
a moment the match does not record. The rounds are recorded in one game
session as ``scripts/corpus/match-replays.py`` records them, resuming in a new
one after a round the game refuses; a document already on disk is kept, so an
interrupted run carries on where it stopped. The game's log of each session is
kept beside the documents.

Run from anywhere inside the checkout, with the game installed:

    cargo build --release -p mechcore
    python3 scripts/corpus/replay.py sync
    python3 scripts/corpus/export-replay-corpus.py
    python3 scripts/corpus/corpus-fights.py [--only <text>]
"""

import argparse
import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import build_data  # noqa: E402

spec = importlib.util.spec_from_file_location("match_replays", ROOT / "scripts/corpus/match-replays.py")
match_replays = importlib.util.module_from_spec(spec)
spec.loader.exec_module(match_replays)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mechcore", type=pathlib.Path, default=ROOT / "target/release/mechcore")
    parser.add_argument("--only", help="only matches whose name contains this text")
    arguments = parser.parse_args()
    version = build_data.configured_build()
    matches = sorted((ROOT / "work/match" / version).glob("*.yaml"))
    replays = ROOT / "work/replay/replays" / version
    folder = ROOT / "work/fight" / version
    folder.mkdir(parents=True, exist_ok=True)
    steps, skipped = [], []
    for match_doc in matches:
        if arguments.only and arguments.only not in match_doc.stem:
            continue
        grbr = replays / f"{match_doc.stem}.grbr"
        if not grbr.exists():
            skipped.append(f"{match_doc.stem}: no replay")
            continue
        rounds, conceded = match_replays.rounds_of(match_doc)
        for number in rounds:
            if number in conceded:
                skipped.append(f"{match_doc.stem} round {number}: a side concedes")
                continue
            steps.append({
                "grbr": str(grbr),
                "round": number,
                "output": str(folder / f"{match_doc.stem}-r{number:02}.yaml"),
            })
    failed = match_replays.record(arguments.mechcore, steps, folder, to="fight")
    for index, why in sorted(failed.items()):
        print(f"{pathlib.Path(steps[index]['output']).name}: {why}")
    for line in skipped:
        print(f"skipped {line}")
    written = sum(1 for step in steps if pathlib.Path(step["output"]).exists())
    print(f"{written} of {len(steps)} rounds written to {folder.relative_to(ROOT)}", file=sys.stderr)
    return 0 if written == len(steps) else 1


if __name__ == "__main__":
    sys.exit(main())
