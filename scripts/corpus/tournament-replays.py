#!/usr/bin/env python3
"""List recent in-game tournaments, or download a tournament ID's replays as matches.

Run against an online game with the current Adapter:
    mechcore game launch --online --level 0
    uv run --with pyyaml python scripts/corpus/tournament-replays.py list
    uv run --with pyyaml python scripts/corpus/tournament-replays.py download \
        --id 12345 --output-dir work/tournaments --publish
    uv run --with pyyaml python scripts/corpus/tournament-replays.py publish \
        --source work/tournaments [--dry-run]

The script speaks directly to the Adapter's tournament API. List includes
archived tournaments and ended regular-list events such as midweek minis.
Both use the same public tournaments/ archive. The downloader walks every
subdivision and ranking page, deduplicates the two players' replay references,
then uses the existing `mechcore convert --to match` command. Final names are
{BattleID}.yaml, read from the downloaded replay, not its server file ID.
Matches are collected under <output-dir>/<date>-<region>/. The date is the
one shown by list; regions are asia, europe and americas. Native GRBRs,
tournament metadata and a resumable manifest stay under each .downloads/.
An existing match is never overwritten. Failed downloads or
conversions are reported and other matches continue; any failure exits 1.
The converter still requires its own build and standard 1v1 match mode.
Dates use local time unless --timezone names an IANA timezone. Requires PyYAML.
Download --publish uploads only after all conversions succeed. Offline publish
also accepts a single date-region directory. Both publish only match YAML to
mechcore-replay tournaments/<date>-<region>/, skip identical files and refuse
changed contents at existing names. --dry-run validates without Git or a game.
"""
from __future__ import annotations

import argparse
from datetime import date, datetime
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from zoneinfo import ZoneInfo

sys.path.insert(0, str(Path(__file__).resolve().parent))
import replay  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
PROTOCOL = "mechcore.adapter.v14"
MAX_MESSAGE_BYTES = 1024 * 1024
# MatchSlotConfig.param0 localization keys, read alongside native history titles.
REGIONS = {"1001": "asia", "1002": "europe", "1003": "americas"}


class AdapterError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.code = code


class Adapter:
    def __init__(self, endpoint: str, level: int):
        self.connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.connection.settimeout(10)
        self.reader = None
        self.sequence = 0
        try:
            self.connection.connect(endpoint)
            self.reader = self.connection.makefile("rb")
            self.send({"kind": "claim", "protocol": PROTOCOL, "level": level,
                       "client": "tournament-replays"})
            hello = self.receive()
            if hello.get("kind") != "hello" or hello.get("protocol") != PROTOCOL:
                raise AdapterError("protocol_mismatch", str(hello))
            if "tournament" not in hello.get("capabilities", []):
                raise AdapterError("unavailable", "Adapter has no tournament API; rebuild and relaunch")
            if hello.get("game", {}).get("offline"):
                raise AdapterError("offline", "launch the game with --online")
        except BaseException:
            self.close()
            raise

    def send(self, value: dict) -> None:
        self.connection.sendall((json.dumps(value) + "\n").encode())

    def receive(self) -> dict:
        line = self.reader.readline(MAX_MESSAGE_BYTES + 1)
        if not line:
            raise AdapterError("disconnected", "Adapter closed the connection")
        if len(line) > MAX_MESSAGE_BYTES or not line.endswith(b"\n"):
            raise AdapterError("protocol_error", "oversized or truncated Adapter message")
        return json.loads(line)

    def request(self, **arguments) -> dict:
        self.sequence += 1
        request_id = self.sequence
        self.connection.settimeout(900)
        self.send({"id": request_id, "operation": "tournament", "arguments": arguments})
        while True:
            value = self.receive()
            if value.get("kind") in ("evicted", "leaving"):
                raise AdapterError(value["kind"], str(value))
            if value.get("id") != request_id:
                raise AdapterError("protocol_error", f"unexpected response: {value}")
            if value.get("kind") == "queued":
                continue
            if value.get("kind") == "started":
                self.connection.settimeout(75)
                continue
            if value.get("kind") != "response":
                raise AdapterError("protocol_error", str(value))
            if not value.get("ok"):
                error = value.get("error", {})
                raise AdapterError(error.get("code", "failed"), error.get("message", str(value)))
            return value["result"]

    def close(self) -> None:
        if self.reader is not None:
            self.reader.close()
            self.reader = None
        self.connection.close()


def region_of(tournament: dict) -> str:
    params = tournament.get("params", [])
    return REGIONS.get(params[0], "unknown") if params else "unknown"


def tournaments(adapter: Adapter, timezone: ZoneInfo | None) -> list[dict]:
    entries = adapter.request(action="list")["tournaments"]
    for entry in entries:
        started = datetime.fromtimestamp(entry["start_time"], timezone).astimezone(timezone)
        entry.update(date=started.date().isoformat(), starts_at=started.isoformat(), region=region_of(entry))
    return sorted(entries, key=lambda item: (item["start_time"], item["match_id"]), reverse=True)


def replay_paths(adapter: Adapter, entries: list[dict]) -> list[str]:
    paths = set()
    for entry in entries:
        for sub in entry["sub_ids"]:
            page, pages = 1, 1
            while page <= pages:
                result = adapter.request(action="page", match_id=entry["match_id"], sub_id=sub, page=page)
                if result["page"] != page or result["match_id"] != entry["match_id"] or result["sub_id"] != sub:
                    raise AdapterError("protocol_error", "tournament page belongs to another request")
                pages = result["pages"]
                if pages < 0 or pages > 10000 or (pages and page > pages):
                    raise AdapterError("protocol_error", f"invalid page count: {pages}")
                paths.update(result["replays"])
                page += 1
    return sorted(paths)


def keep_match(source: Path, destination: Path) -> bool:
    """Never replace a match; equal bytes are duplicates, other bytes a collision."""
    try:
        os.link(source, destination)
        return True
    except FileExistsError:
        if source.read_bytes() != destination.read_bytes():
            raise RuntimeError(f"BattleID collision with different match contents: {destination}")
        return False


def save_manifest(path: Path, manifest: dict) -> None:
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as pending:
        name = Path(pending.name)
        try:
            json.dump(manifest, pending, ensure_ascii=False, indent=2)
            pending.write("\n")
            pending.flush()
            os.fsync(pending.fileno())
            os.replace(name, path)
        finally:
            name.unlink(missing_ok=True)


def native_battle_id(path: Path) -> str:
    """Recover a completed native download if publication was interrupted."""
    data = path.read_bytes()
    start = data.find(b'<?xml')
    end = data.find(b'</BattleRecord>', start)
    if start < 0 or end < 0:
        raise RuntimeError(f"incomplete native replay: {path}")
    root = ET.fromstring(data[start:end + len(b'</BattleRecord>')])
    battle_id = root.findtext("BattleInfo/BattleID")
    if not battle_id:
        raise RuntimeError(f"native replay has no BattleID: {path}")
    return battle_id


def download_one(adapter: Adapter, replay_path: str, output: Path, mechcore: Path, manifest: dict) -> dict:
    cached = manifest.get(replay_path)
    if cached and cached.get("match"):
        destination = output / cached["match"]
        if destination.is_file() and hashlib.sha256(destination.read_bytes()).hexdigest() == cached["sha256"]:
            return {"replay_path": replay_path, "output": str(destination), "duplicate": True}
        if destination.exists():
            raise RuntimeError(f"previously published match changed: {destination}")
    native = output / ".downloads" / (hashlib.sha256(replay_path.encode()).hexdigest() + ".grbr")
    battle_id = cached.get("battle_id") if cached else None
    if not native.exists():
        result = adapter.request(action="download", replay_path=replay_path, output=str(native))
        battle_id = result["battle_id"]
    recorded_id = native_battle_id(native)
    if battle_id and battle_id != recorded_id:
        raise RuntimeError(f"download identity differs from the GRBR: {battle_id} != {recorded_id}")
    battle_id = recorded_id
    manifest[replay_path] = {"battle_id": battle_id, "native": native.name}
    save_manifest(output / ".downloads/manifest.json", manifest)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", battle_id) or battle_id in (".", ".."):
        raise RuntimeError(f"unsafe native BattleID: {battle_id!r}")
    destination = output / f"{battle_id}.yaml"
    with tempfile.TemporaryDirectory(prefix=".match-", dir=output) as pending:
        temporary = Path(pending) / destination.name
        converted = subprocess.run([str(mechcore), "convert", str(native), "--to", "match", str(temporary)],
                                   capture_output=True, text=True, cwd=ROOT)
        if converted.returncode:
            raise RuntimeError(f"conversion failed; retained {native}: {converted.stderr.strip()}")
        created = keep_match(temporary, destination)
    manifest[replay_path].update(match=destination.name, sha256=hashlib.sha256(destination.read_bytes()).hexdigest())
    save_manifest(output / ".downloads/manifest.json", manifest)
    return {"replay_path": replay_path, "battle_id": battle_id, "output": str(destination), "duplicate": not created}


def tournament_directory(tournament: dict) -> str:
    day = tournament["date"]
    region = tournament["region"]
    if date.fromisoformat(day).isoformat() != day or region not in REGIONS.values():
        raise RuntimeError(f"cannot archive tournament with date {day!r} and region {region!r}")
    return f"{day}-{region}"


def is_tournament_directory(name: str) -> bool:
    parts = name.rsplit("-", 1)
    if len(parts) != 2:
        return False
    try:
        return tournament_directory({"date": parts[0], "region": parts[1]}) == name
    except (ValueError, RuntimeError):
        return False


def publish_tournaments(source: Path, dry_run: bool) -> None:
    if not source.is_dir():
        raise RuntimeError(f"tournament source is not a directory: {source}")
    if is_tournament_directory(source.name):
        directories = [source]
    else:
        if any(source.glob("*.yaml")):
            raise RuntimeError(f"matches must be collected under <date>-<region>/ directories: {source}")
        directories = sorted(path for path in source.iterdir() if path.is_dir() and not path.name.startswith("."))
    matches = []
    for directory in directories:
        files = sorted(directory.glob("*.yaml"))
        if not files:
            continue
        if not is_tournament_directory(directory.name):
            raise RuntimeError(f"not a YYYY-MM-DD-region directory: {directory}")
        for match in files:
            if not re.fullmatch(r"[A-Za-z0-9_.-]+\.yaml", match.name):
                raise RuntimeError(f"match name is not BattleID.yaml: {match.name}")
            segments, _ = replay.read_match(match)
            if not any(isinstance(segment, dict) and segment.get("kind") == "state" for segment in segments[1:]):
                raise RuntimeError(f"{match.name} contains no captured state")
            matches.append((match, Path("tournaments") / directory.name / match.name))
        print(f"{len(files)} matches ready for tournaments/{directory.name}/", flush=True)
    if not dry_run:
        replay.publish_files(matches, "tournament matches")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--socket", default=os.environ.get("MECHCORE_ADAPTER_SOCKET", f"/tmp/mechcore-adapter-{os.getuid()}.sock"))
    parser.add_argument("--level", type=int, choices=range(5), default=0)
    parser.add_argument("--timezone", type=ZoneInfo, help="IANA timezone for displayed dates; default: local")
    sub = parser.add_subparsers(dest="command", required=True)
    listing = sub.add_parser("list", help="list the recent tournaments retained by the game server")
    listing.add_argument("--limit", type=int, default=20)
    downloading = sub.add_parser("download", help="download all subdivisions and rounds for one tournament ID")
    downloading.add_argument("--id", type=int, required=True, help="match_id from list")
    downloading.add_argument("--output-dir", type=Path, default=ROOT / "work/tournaments")
    downloading.add_argument("--mechcore", type=Path, default=ROOT / "target/release/mechcore")
    downloading.add_argument("--publish", action="store_true", help="publish the tournament after every replay converts successfully")
    publication = sub.add_parser("publish", help="publish collected tournaments without a running game")
    publication.add_argument("--source", type=Path, default=ROOT / "work/tournaments",
                             help="collection root or one YYYY-MM-DD-region directory")
    publication.add_argument("--dry-run", action="store_true", help="validate without fetching, committing or pushing")
    return parser.parse_args()


def main() -> int:
    args = parse_arguments()
    adapter = None
    try:
        if args.command == "publish":
            publish_tournaments(args.source.resolve(), args.dry_run)
            return 0
        adapter = Adapter(args.socket, args.level)
        entries = tournaments(adapter, args.timezone)
        if args.command == "list":
            for entry in entries[:args.limit]:
                print(json.dumps(entry, ensure_ascii=False), flush=True)
            return 0
        chosen = [entry for entry in entries if entry["match_id"] == args.id]
        if not chosen:
            raise RuntimeError("tournament ID is not in the retained tournament list; run list to inspect available IDs")
        paths = replay_paths(adapter, chosen)
        output = args.output_dir.resolve() / tournament_directory(chosen[0])
        (output / ".downloads").mkdir(parents=True, exist_ok=True)
        save_manifest(output / ".downloads" / f"tournament-{args.id}.json", chosen[0])
        manifest_path = output / ".downloads/manifest.json"
        manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
        failed, downloaded, duplicates = 0, 0, 0
        print(json.dumps({"tournaments": chosen, "unique_replays": len(paths)}, ensure_ascii=False), flush=True)
        for path in paths:
            try:
                result = download_one(adapter, path, output, args.mechcore.resolve(), manifest)
                duplicates += result["duplicate"]
                downloaded += not result["duplicate"]
                print(json.dumps(result, ensure_ascii=False), flush=True)
            except AdapterError as error:
                if error.code in ("evicted", "cancelled", "game_stopped", "disconnected"):
                    raise
                failed += 1
                print(json.dumps({"replay_path": path, "error": str(error)}, ensure_ascii=False), flush=True)
            except (OSError, ValueError, RuntimeError) as error:
                failed += 1
                print(json.dumps({"replay_path": path, "error": str(error)}, ensure_ascii=False), flush=True)
        print(json.dumps({"downloaded": downloaded, "duplicates": duplicates, "failed": failed}), flush=True)
        adapter.close()
        adapter = None
        if args.publish and not failed:
            publish_tournaments(output, False)
        return int(failed > 0)
    except (OSError, ValueError, RuntimeError) as error:
        print(str(error), file=sys.stderr)
        return 1
    finally:
        if adapter:
            adapter.close()


if __name__ == "__main__":
    sys.exit(main())
