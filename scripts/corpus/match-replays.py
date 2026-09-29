#!/usr/bin/env python3
"""Fight every corpus round from its replay and from the replay its match writes.

``mechcore convert <match.yaml> --to grbr <replay.grbr>`` writes a match back as
a replay that converts to the same match again. This asks the other half:
whether the game plays that replay as it plays the match's own. For every
match ``scripts/corpus/export-replay-corpus.py`` writes under
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
    python3 scripts/corpus/replay.py sync
    python3 scripts/corpus/export-replay-corpus.py
    python3 scripts/corpus/match-replays.py
    python3 scripts/corpus/match-replays.py --only Chemtrails --json

A round in which a side concedes is listed and not compared: a concession made
during the fight ends it at a moment the match does not record.

Each round's recording from the corpus replay is also converted to the fight
document it records (``mechcore convert <recording> --to fight``), and every
leaf the fight decides is compared with the next state in the match document,
which is the game's own answer: how far each ``reactor_core`` falls, each
unit's ``exp`` by its index, which ``contraptions`` remain, and what the panel
slots' ``standing`` objects are. ``--recordings <dir>`` makes only that
comparison, gamelessly, over recordings already made, named ``mNN-rRR.mcfr``:
the round ``RR`` of the ``NN``-th match document in name order, and reports
it per field.

The exit status is 0 only when every other round records both ways, compares
equal, and states every leaf the fight decides as the match document does.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_data  # noqa: E402


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
    parser.add_argument(
        "--recordings",
        type=Path,
        help="compare only what each fight decided, over the recordings already in this "
        "directory, named mNN-rRR.mcfr",
    )
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


FIELDS = ("core_damage", "exp", "contraptions", "standing")
"""The leaves a fight decides, as ``match.md`` names them."""


def states(match_doc: Path) -> dict[int, dict]:
    """Each state segment of a match document, by the round it opens."""
    text = match_doc.read_text(encoding="utf-8")
    found = {}
    for segment in re.split(r"^---$", text, flags=re.MULTILINE):
        if re.match(r"\s*kind: state\n", segment):
            state = parse_yaml(segment)
            found[state["round"]] = state
    return found


def fight_leaves(mechcore: Path, recording: str, cores: dict, number: int) -> dict:
    """What the fight recorded decided, against what the match document's next
    state says it did, field by field. A field is ``equal`` or says how the two
    differ."""
    if number not in cores or number + 1 not in cores:
        missing = f"the match holds no state for round {number} and the next"
        return {field: missing for field in FIELDS}
    result = run([str(mechcore), "convert", recording, "--to", "fight"])
    if result.returncode != 0:
        refused = "not converted: " + reason(result.stdout + result.stderr)
        return {field: refused for field in FIELDS}
    fight = parse_yaml(result.stdout)
    before, after = cores[number], cores[number + 1]
    compared = {field: [] for field in FIELDS}
    for side in ("blue", "red"):
        fought, was, now = fight[side], before[side], after[side]
        fell = was["reactor_core"] - now["reactor_core"]
        if fought.get("core_damage", 0) != fell:
            compared["core_damage"].append(
                f"{side} takes {fought.get('core_damage', 0)}, the match says {fell}"
            )
        units = {unit["index"]: unit for unit in now.get("units", [])}
        for unit in fought.get("units", []):
            ended = int(str(unit.get("exp", "0/0/0")).split("/")[1])
            if unit["index"] not in units:
                compared["exp"].append(f"{side} unit {unit['index']} is not in the next state")
                continue
            stated = units[unit["index"]].get("exp")
            holds = int(str(stated).split("/")[0]) if stated else 0
            if ended != holds:
                compared["exp"].append(
                    f"{side} unit {unit['index']} {unit['name']} ends on {ended}, "
                    f"the match says {stated or 0}"
                )
        kept = sorted(
            entry["index"] for entry in fought.get("contraptions", []) if entry.get("retained", True)
        )
        fought_indices = {entry["index"] for entry in fought.get("contraptions", [])}
        remain = sorted(
            entry["index"]
            for entry in now.get("contraptions", [])
            if entry["index"] in fought_indices
        )
        if kept != remain:
            compared["contraptions"].append(f"{side} keeps {kept}, the match says {remain}")
        left = []
        for entry in fought.get("battle_skills", []):
            if entry.get("retained", True) is False:
                continue
            if "standing" in entry:
                if "position" in entry["standing"]:
                    left.append(entry["standing"])
            elif entry["name"] == "shield_airdrop":
                left.append({"position": entry["positions"][0]})
            elif entry["name"] == "sticky_oil_bomb":
                area = {"control_points": entry["positions"]}
                if entry.get("grid_rows"):
                    area["grid_rows"] = entry["grid_rows"]
                left.append(area)
        standing = [
            standing
            for slot in now.get("battle_skills", [])
            for standing in slot.get("standing", [])
        ]
        key = lambda value: json.dumps(value, sort_keys=True)
        if sorted(map(key, left)) != sorted(map(key, standing)):
            compared["standing"].append(
                f"{side} leaves {sorted(map(key, left))}, the match says "
                f"{sorted(map(key, standing))}"
            )
    return {field: "; ".join(found) or "equal" for field, found in compared.items()}


def parse_yaml(text: str):
    """The YAML subset mechcore's canonical writer spells a document in: block
    mappings and sequences, flow collections and plain scalars. A match
    document's header, which can quote a player's name, is not read."""
    lines = [
        line.rstrip()
        for line in text.splitlines()
        if line.strip() and line.strip() != "---" and not line.lstrip().startswith("#")
    ]
    value, _ = parse_block(lines, 0, 0)
    return value


def parse_block(lines: list[str], at: int, indent: int):
    if at < len(lines) and lines[at][indent:].startswith("- "):
        items = []
        while at < len(lines) and indent_of(lines[at]) == indent and lines[at][indent:].startswith("- "):
            rest = lines[at][indent + 2 :]
            if re.match(r"^[\w]+:( |$)", rest) and not rest.startswith("{"):
                # A block mapping inside a sequence item.
                item, at = parse_block([" " * (indent + 2) + rest] + lines[at + 1 :], 0, indent + 2)
                items.append(item)
                continue
            items.append(parse_flow(rest))
            at += 1
        return items, at
    mapping = {}
    while at < len(lines) and indent_of(lines[at]) == indent:
        key, _, rest = lines[at][indent:].partition(":")
        rest = rest.strip()
        at += 1
        if rest:
            mapping[scalar(key)] = parse_flow(rest)
        elif at < len(lines) and indent_of(lines[at]) == indent and lines[at][indent:].startswith("- "):
            mapping[scalar(key)], at = parse_block(lines, at, indent)
        elif at < len(lines) and indent_of(lines[at]) > indent:
            mapping[scalar(key)], at = parse_block(lines, at, indent_of(lines[at]))
        else:
            mapping[scalar(key)] = None
    return mapping, at


def indent_of(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def parse_flow(text: str):
    """One value written on one line: a flow collection or a plain scalar."""
    if not text.startswith(("{", "[")):
        return scalar(text)
    value, at = flow_at(text, 0)
    if text[at:].strip():
        raise ValueError(f"unread YAML after {text[:at]!r}")
    return value


def flow_at(text: str, at: int):
    """The flow value starting at ``at``, and where it ends."""
    while text[at] == " ":
        at += 1
    opener = text[at]
    if opener in "{[":
        closer = "}" if opener == "{" else "]"
        collection = {} if opener == "{" else []
        at += 1
        while True:
            while text[at] in " ,":
                at += 1
            if text[at] == closer:
                return collection, at + 1
            if opener == "{":
                colon = text.index(":", at)
                key = scalar(text[at:colon])
                collection[key], at = flow_at(text, colon + 1)
            else:
                item, at = flow_at(text, at)
                collection.append(item)
    if opener in "'\"":
        end = text.index(opener, at + 1)
        return text[at + 1 : end], end + 1
    end = at
    while end < len(text) and text[end] not in ",]}":
        end += 1
    return scalar(text[at:end]), end


def scalar(text: str):
    text = text.strip()
    if re.fullmatch(r"-?\d+", text):
        return int(text)
    if text in ("true", "false"):
        return text == "true"
    if text in ("null", "~"):
        return None
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
        return text[1:-1]
    return text


def check_recordings(arguments: argparse.Namespace, matches: list[Path]) -> int:
    """The comparison of what each fight decided alone, over recordings
    already made, reported per field."""
    compared = 0
    equal = {field: 0 for field in FIELDS}
    whole = 0
    for recording in sorted(arguments.recordings.glob("m*-r*.mcfr")):
        found = re.fullmatch(r"m(\d+)-r(\d+)", recording.stem)
        if not found or int(found.group(1)) >= len(matches):
            continue
        match_doc, number = matches[int(found.group(1))], int(found.group(2))
        entry = {"match": match_doc.stem, "round": number}
        entry.update(fight_leaves(arguments.mechcore, str(recording), states(match_doc), number))
        compared += 1
        for field in FIELDS:
            equal[field] += entry[field] == "equal"
        whole += all(entry[field] == "equal" for field in FIELDS)
        if arguments.json:
            print(json.dumps(entry, ensure_ascii=False), flush=True)
        else:
            for field in FIELDS:
                if entry[field] != "equal":
                    print(f"{recording.stem} {entry['match']} round {number} {field}: "
                          f"{entry[field]}", flush=True)
    for field in FIELDS:
        print(f"{field}: {equal[field]} of {compared} recorded rounds state what the match says",
              file=sys.stderr)
    print(f"{whole} of {compared} recorded rounds state every leaf the fight decides as the "
          "match says", file=sys.stderr)
    return 0 if compared and whole == compared else 1


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
            "  - convert:\n"
            "      to: mcfr\n"
            "      backend: game\n"
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
    decided = "; ".join(
        f"{field} {entry[field]}" for field in FIELDS if entry.get(field, "equal") != "equal"
    )
    if entry.get("equal"):
        return "equal" if not decided else f"equal; {decided}"
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
    root = Path(__file__).resolve().parents[2]
    arguments = parse_arguments(root)
    version = arguments.version or build_data.configured_build()
    matches = sorted((root / "work/match" / version).glob("*.yaml"))
    replays = root / "work/replay/replays" / version
    if arguments.only:
        matches = [match_doc for match_doc in matches if arguments.only in match_doc.name]
    if not matches:
        print(f"no match documents under work/match/{version}", file=sys.stderr)
        return 1
    if arguments.recordings:
        # The numbering counts every match document, so a filter would move it.
        return check_recordings(arguments, sorted((root / "work/match" / version).glob("*.yaml")))

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
            cores = states(match_doc)
            for at, entry in enumerate(entries):
                left, right = 2 * at, 2 * at + 1
                if left in failed or right in failed:
                    entry["failed"] = failed.get(left) or failed.get(right)
                else:
                    entry.update(
                        compare(arguments.mechcore, steps[left]["output"], steps[right]["output"])
                    )
                    entry.update(
                        fight_leaves(arguments.mechcore, steps[left]["output"], cores, entry["round"])
                    )
        for entry in sorted(entries + skipped, key=lambda entry: entry["round"]):
            results.append(entry)
            if arguments.json:
                print(json.dumps(entry, ensure_ascii=False), flush=True)
            else:
                print(f"{entry['match']} round {entry['round']}: {verdict(entry)}", flush=True)

    compared = [entry for entry in results if not entry.get("conceded")]
    equal = sum(1 for entry in compared if entry.get("equal"))
    decided = sum(
        1 for entry in compared if all(entry.get(field) == "equal" for field in FIELDS)
    )
    print(
        f"{equal} of {len(compared)} rounds fight the same from the match's replay as from "
        f"the match's own; {decided} state every leaf the fight decides as the match says; "
        f"{len(results) - len(compared)} conceded, not compared",
        file=sys.stderr,
    )
    return 0 if equal == len(compared) and decided == len(compared) else 1


if __name__ == "__main__":
    sys.exit(main())
