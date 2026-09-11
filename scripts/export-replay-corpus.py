#!/usr/bin/env python3
"""Export every tracked GRBR as an independent, resumable corpus unit.

Battle YAML is generated offline with ``mechcore convert``. Native deployment
observations are generated one replay per ``mechcore run`` so a refused or
failed replay cannot stop later inputs. The manifest is replaced atomically
after every stage and complete JSONL outputs are never overwritten.

Run from anywhere inside the checkout:

    python3 scripts/export-replay-corpus.py --offline-only --force-battles
    python3 scripts/export-replay-corpus.py
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any


SCHEMA = "mechcore.replay-corpus.v1"


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def load_manifest(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"schema": SCHEMA, "replays": {}}
    value = json.loads(path.read_text())
    if value.get("schema") != SCHEMA or not isinstance(value.get("replays"), dict):
        raise ValueError(f"{path} is not a {SCHEMA} manifest")
    return value


def save_manifest(path: Path, manifest: dict[str, Any]) -> None:
    manifest["updated_at"] = now()
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    os.replace(temporary, path)


def save_checksums(paths: list[Path], destination: Path, root: Path) -> None:
    lines = [f"{digest(path)}  {path.relative_to(root)}\n" for path in sorted(paths)]
    temporary = destination.with_name(f".{destination.name}.tmp")
    temporary.write_text("".join(lines))
    os.replace(temporary, destination)


def artifact(path: Path, root: Path) -> dict[str, Any]:
    return {
        "path": str(path.relative_to(root)),
        "bytes": path.stat().st_size,
        "sha256": digest(path),
    }


def result_text(result: subprocess.CompletedProcess[str]) -> str:
    lines = [line for line in (result.stdout + "\n" + result.stderr).splitlines() if line]
    return "\n".join(lines[-20:])


def valid_observation(path: Path, source: Path) -> tuple[bool, str]:
    if not path.is_file():
        return False, "missing"

    header = None
    summary = None
    expected_sequence = 0
    try:
        with path.open() as stream:
            for line_number, line in enumerate(stream, 1):
                record = json.loads(line)
                if record.get("sequence") != expected_sequence:
                    return False, f"line {line_number} has non-consecutive sequence"
                expected_sequence += 1
                if line_number == 1:
                    header = record
                summary = record
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        return False, str(error)

    if not header or header.get("kind") != "header":
        return False, "first record is not a header"
    if header.get("schema") != "mechcore.battle-observation.v1":
        return False, "unexpected observation schema"
    recorded_source = Path(header.get("source", {}).get("grbr", ""))
    if recorded_source != source.resolve():
        return False, "header names a different GRBR"
    if not summary or summary.get("kind") != "summary" or summary.get("complete") is not True:
        return False, "final record is not a complete summary"
    if summary.get("records") != expected_sequence:
        return False, "summary record count does not match the stream"
    return True, "complete"


def run(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True, check=False)


def record_job(job: Path, source: Path, output: Path) -> None:
    job.write_text(
        "game: launch\n\n"
        "vars:\n"
        f"  grbr: {json.dumps(str(source.resolve()), ensure_ascii=False)}\n"
        f"  output: {json.dumps(str(output.resolve()), ensure_ascii=False)}\n\n"
        "steps:\n"
        "  - record_replay_battle:\n"
        "      grbr: $grbr\n"
        "      output: $output\n"
    )


def parse_arguments(root: Path) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mechcore", type=Path, default=root / "target/release/mechcore")
    parser.add_argument("--grbr-dir", type=Path, default=root / "tests/grbr")
    parser.add_argument("--battle-dir", type=Path, default=root / "tests/battle")
    parser.add_argument(
        "--corpus-dir", type=Path, default=root / "work/replay-corpus"
    )
    parser.add_argument(
        "--offline-only",
        action="store_true",
        help="convert battle YAML and update the manifest without launching the game",
    )
    parser.add_argument(
        "--force-battles",
        action="store_true",
        help="regenerate existing battle YAML; observation JSONL is still never replaced",
    )
    return parser.parse_args()


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    args = parse_arguments(root)
    executable = args.mechcore.resolve()
    grbr_dir = args.grbr_dir.resolve()
    battle_dir = args.battle_dir.resolve()
    corpus_dir = args.corpus_dir.resolve()
    observation_dir = corpus_dir / "observations"
    job_dir = corpus_dir / "jobs"
    manifest_path = corpus_dir / "manifest.json"

    if not executable.is_file():
        print(f"mechcore executable does not exist: {executable}", file=sys.stderr)
        return 2
    sources = sorted(grbr_dir.glob("*.grbr"))
    if not sources:
        print(f"no GRBR inputs in {grbr_dir}", file=sys.stderr)
        return 2

    battle_dir.mkdir(parents=True, exist_ok=True)
    observation_dir.mkdir(parents=True, exist_ok=True)
    job_dir.mkdir(parents=True, exist_ok=True)
    manifest = load_manifest(manifest_path)
    manifest["root"] = str(root)

    for index, source in enumerate(sources, 1):
        name = source.name
        stem = source.stem
        print(f"[{index}/{len(sources)}] {name}", flush=True)
        entry = manifest["replays"].setdefault(name, {})
        source_record = artifact(source, root)
        previous_source = entry.get("source")
        if previous_source and previous_source.get("sha256") != source_record["sha256"]:
            entry["status"] = "source_changed"
            entry["error"] = "tracked GRBR bytes changed; existing outputs were not touched"
            entry["source"] = source_record
            save_manifest(manifest_path, manifest)
            print("  source changed; refused", flush=True)
            continue
        entry["source"] = source_record

        battle = battle_dir / f"{stem}.yaml"
        previous_battle = entry.get("battle", {})
        if battle.exists() and not args.force_battles:
            entry["battle"] = {
                **previous_battle,
                "status": "complete",
                **artifact(battle, root),
            }
            print("  battle: complete (existing)", flush=True)
        elif previous_battle.get("status") == "refused" and not args.force_battles:
            print("  battle: refused (recorded)", flush=True)
        else:
            command = [str(executable), "convert", str(source), str(battle)]
            if args.force_battles:
                command.append("--force")
            converted = run(command, root)
            if converted.returncode == 0 and battle.is_file():
                entry["battle"] = {
                    "status": "complete",
                    **artifact(battle, root),
                    "result": result_text(converted),
                }
                print("  battle: complete", flush=True)
            else:
                entry["battle"] = {
                    "status": "refused",
                    "exit_code": converted.returncode,
                    "error": result_text(converted),
                }
                print("  battle: refused", flush=True)
        save_manifest(manifest_path, manifest)

        if args.offline_only:
            entry.setdefault("observation", {"status": "not_attempted"})
            save_manifest(manifest_path, manifest)
            continue
        observation = observation_dir / f"{stem}.jsonl"
        valid, reason = valid_observation(observation, source)
        if valid:
            entry["observation"] = {
                **entry.get("observation", {}),
                "status": "complete",
                **artifact(observation, root),
            }
            save_manifest(manifest_path, manifest)
            print("  observation: complete (existing)", flush=True)
            continue
        if observation.exists():
            entry["observation"] = {
                "status": "invalid_existing",
                "path": str(observation.relative_to(root)),
                "error": reason,
            }
            save_manifest(manifest_path, manifest)
            print(f"  observation: invalid existing output ({reason})", flush=True)
            continue

        job = job_dir / f"{stem}.mcscript"
        record_job(job, source, observation)
        recorded = run([str(executable), "run", str(job)], root)
        job.unlink(missing_ok=True)
        valid, reason = valid_observation(observation, source)
        if recorded.returncode == 0 and valid:
            entry["observation"] = {
                "status": "complete",
                **artifact(observation, root),
                "result": result_text(recorded),
            }
            print("  observation: complete", flush=True)
        else:
            entry["observation"] = {
                "status": "failed",
                "exit_code": recorded.returncode,
                "error": result_text(recorded) or reason,
            }
            print("  observation: failed", flush=True)
        save_manifest(manifest_path, manifest)

    statuses: dict[str, int] = {}
    for entry in manifest["replays"].values():
        status = entry.get("observation" if not args.offline_only else "battle", {}).get(
            "status", "unknown"
        )
        statuses[status] = statuses.get(status, 0) + 1
    save_checksums(sources, grbr_dir / "SHA256SUMS", root)
    save_checksums(list(battle_dir.glob("*.yaml")), battle_dir / "SHA256SUMS", root)
    print(json.dumps(statuses, sort_keys=True))
    return 0 if set(statuses) <= {"complete", "not_attempted"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
