#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p "python3.withPackages (packages: [ packages.pyyaml ])"

from __future__ import annotations

import argparse
import json
import os
import queue
import re
import subprocess
import sys
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml


STATUS_URI = "mechcore://status"
EXPECTED_TOOLS = {
    "apply_layout",
    "connect_adapter",
    "record_battle",
    "record_replay_round",
    "quit_game",
    "quit_match",
    "speed_up",
    "start_test",
    "status",
    "toggle_fight",
}
TRANSITION_TIMEOUT = 120.0
BATTLE_TIMEOUT = 180.0
MAX_ACTIVATION_ROUND = 15
REPOSITORY = Path(__file__).resolve().parent.parent
MECHCORE = REPOSITORY / "target/release/mechcore"
ADAPTER = REPOSITORY / "target/release/libmechcore_adapter.dylib"
GAME = (
    Path.home()
    / "Library/Application Support/Steam/steamapps/common/Mechabellum/Mechabellum.app/Contents/MacOS/Mechabellum"
)
DEFAULT_MANIFEST = REPOSITORY / "tests/mcfr-regressions.yaml"
DEFAULT_CAPTURE_ROOT = REPOSITORY / "work/captures"


class SmokeFailure(RuntimeError):
    pass


class ToolFailure(SmokeFailure):
    def __init__(
        self, name: str, detail: dict[str, Any] | None, result: dict[str, Any]
    ):
        super().__init__(f"tool {name} failed: {detail or result.get('content')}")
        self.name = name
        self.detail = detail


@dataclass(frozen=True)
class CaptureCase:
    name: str
    layout_path: Path
    output: Path
    seed: int | None = None
    video_output: Path | None = None
    instrumentation_output: Path | None = None
    instrumentation_profile: str | None = None


class McpClient:
    def __init__(self) -> None:
        if not MECHCORE.is_file():
            raise SmokeFailure(f"release binary is missing: {MECHCORE}")
        self.process = subprocess.Popen(
            [str(MECHCORE), "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        if self.process.stdin is None or self.process.stdout is None:
            raise SmokeFailure("MCP process pipes are unavailable")
        self.stdin = self.process.stdin
        self.stdout = self.process.stdout
        self.next_id = 1
        self.incoming: queue.Queue[dict[str, Any] | BaseException | None] = queue.Queue()
        self.notifications: deque[dict[str, Any]] = deque()
        threading.Thread(target=self._read_messages, daemon=True).start()

    def _read_messages(self) -> None:
        try:
            for line in self.stdout:
                value = json.loads(line)
                if not isinstance(value, dict):
                    raise SmokeFailure(f"MCP message is not an object: {value!r}")
                self.incoming.put(value)
        except BaseException as error:
            self.incoming.put(error)
        finally:
            self.incoming.put(None)

    def _send(self, message: dict[str, Any]) -> None:
        if self.process.poll() is not None:
            raise SmokeFailure(f"MCP exited with code {self.process.returncode}")
        self.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.stdin.flush()

    def request(
        self, method: str, params: dict[str, Any] | None, timeout: float
    ) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            message["params"] = params
        self._send(message)
        deadline = time.monotonic() + timeout
        while True:
            received = self._receive(deadline)
            if "method" in received and "id" not in received:
                self.notifications.append(received)
                continue
            if received.get("id") != request_id:
                raise SmokeFailure(
                    f"unexpected MCP response id: expected {request_id}, got {received}"
                )
            if "error" in received:
                raise SmokeFailure(f"MCP request {method} failed: {received['error']}")
            result = received.get("result")
            if not isinstance(result, dict):
                raise SmokeFailure(f"MCP request {method} omitted object result")
            return result

    def notify(self, method: str) -> None:
        self._send({"jsonrpc": "2.0", "method": method})

    def call_tool(
        self, name: str, arguments: dict[str, Any], timeout: float = TRANSITION_TIMEOUT
    ) -> dict[str, Any]:
        result = self.request(
            "tools/call", {"name": name, "arguments": arguments}, timeout
        )
        structured = result.get("structuredContent")
        if result.get("isError") is True:
            raise ToolFailure(
                name,
                structured if isinstance(structured, dict) else None,
                result,
            )
        if not isinstance(structured, dict):
            raise SmokeFailure(f"tool {name} omitted structuredContent: {result}")
        return structured

    def read_status_resource(self, timeout: float) -> dict[str, Any]:
        result = self.request(
            "resources/read", {"uri": STATUS_URI}, timeout
        )
        contents = result.get("contents")
        if not isinstance(contents, list) or len(contents) != 1:
            raise SmokeFailure(f"status resource has invalid contents: {result}")
        text = contents[0].get("text")
        if not isinstance(text, str):
            raise SmokeFailure(f"status resource is not text: {contents[0]}")
        status = json.loads(text)
        if not isinstance(status, dict):
            raise SmokeFailure(f"status resource JSON is not an object: {status!r}")
        return status

    def wait_stream_status(
        self, expected: dict[str, Any], timeout: float
    ) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        status = self.read_status_resource(self._remaining(deadline))
        while not all(status.get(key) == value for key, value in expected.items()):
            notification = self._next_status_notification(deadline)
            if notification.get("params", {}).get("uri") != STATUS_URI:
                continue
            status = self.read_status_resource(self._remaining(deadline))
        print(f"ok: streamed status {json.dumps(status, ensure_ascii=False)}")
        return status

    def _next_status_notification(self, deadline: float) -> dict[str, Any]:
        while True:
            while self.notifications:
                notification = self.notifications.popleft()
                if notification.get("method") == "notifications/resources/updated":
                    return notification
            received = self._receive(deadline)
            if "method" in received and "id" not in received:
                if received.get("method") == "notifications/resources/updated":
                    return received
                self.notifications.append(received)
                continue
            raise SmokeFailure(f"unexpected MCP response without a request: {received}")

    def _receive(self, deadline: float) -> dict[str, Any]:
        try:
            received = self.incoming.get(timeout=self._remaining(deadline))
        except queue.Empty as error:
            raise SmokeFailure("timed out waiting for MCP message") from error
        if received is None:
            raise SmokeFailure(
                f"MCP output closed; process code is {self.process.poll()}"
            )
        if isinstance(received, BaseException):
            raise SmokeFailure(f"cannot read MCP output: {received}") from received
        return received

    @staticmethod
    def _remaining(deadline: float) -> float:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SmokeFailure("timed out waiting for streamed game status")
        return remaining

    def close(self) -> None:
        if not self.stdin.closed:
            try:
                self.stdin.close()
            except BrokenPipeError:
                pass
        try:
            returncode = self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                returncode = self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                returncode = self.process.wait(timeout=10)
        if returncode != 0:
            raise SmokeFailure(f"MCP exited with code {returncode}")


def load_layout(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as stream:
            layout = yaml.safe_load(stream)
    except (OSError, yaml.YAMLError) as error:
        raise SmokeFailure(f"cannot load layout {path}: {error}") from error
    if not isinstance(layout, dict):
        raise SmokeFailure("layout root must be an object")
    activation_round = layout.get("round")
    if (
        not isinstance(activation_round, int)
        or isinstance(activation_round, bool)
        or not 1 <= activation_round <= MAX_ACTIVATION_ROUND
    ):
        raise SmokeFailure(
            f"layout round must be within 1..={MAX_ACTIVATION_ROUND}"
        )
    seed = layout.get("seed", 0)
    if (
        not isinstance(seed, int)
        or isinstance(seed, bool)
        or not -(2**31) <= seed < 2**31
    ):
        raise SmokeFailure("layout seed must be a signed 32-bit integer")
    return layout


def resolve_path(path: Path) -> Path:
    return path.expanduser().resolve()


def require_new_output(path: Path, suffix: str, label: str) -> None:
    if not path.is_absolute():
        raise SmokeFailure(f"{label} must be an absolute path: {path}")
    if path.suffix != suffix:
        raise SmokeFailure(f"{label} must use the {suffix} extension: {path}")
    if path.exists():
        raise SmokeFailure(f"refusing to overwrite {path}")


def load_manifest(path: Path) -> list[dict[str, Any]]:
    try:
        with path.open(encoding="utf-8") as stream:
            manifest = yaml.safe_load(stream)
    except (OSError, yaml.YAMLError) as error:
        raise SmokeFailure(f"cannot load manifest {path}: {error}") from error
    if not isinstance(manifest, list) or not manifest:
        raise SmokeFailure(f"manifest must be a non-empty list: {path}")
    entries: list[dict[str, Any]] = []
    names: set[str] = set()
    for index, entry in enumerate(manifest, start=1):
        if not isinstance(entry, dict):
            raise SmokeFailure(f"manifest entry {index} must be an object")
        name = entry.get("name")
        if (
            not isinstance(name, str)
            or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name) is None
        ):
            raise SmokeFailure(f"manifest entry {index} has an invalid name: {name!r}")
        if name in names:
            raise SmokeFailure(f"manifest contains duplicate case name: {name}")
        names.add(name)
        layout = entry.get("layout")
        if not isinstance(layout, str) or not layout:
            raise SmokeFailure(f"manifest entry {name} has an invalid layout")
        seed = entry.get("seed")
        if (
            not isinstance(seed, int)
            or isinstance(seed, bool)
            or seed == 0
            or not -(2**31) <= seed < 2**31
        ):
            raise SmokeFailure(f"manifest entry {name} has an invalid seed: {seed!r}")
        smoke = entry.get("smoke")
        if not isinstance(smoke, bool):
            raise SmokeFailure(f"manifest entry {name} has an invalid smoke flag")
        entries.append(entry)
    return entries


def build_batch_cases(
    manifest_path: Path,
    output_dir: Path,
    selected_names: list[str],
    smoke_only: bool,
) -> list[CaptureCase]:
    manifest_path = resolve_path(manifest_path)
    output_dir = resolve_path(output_dir)
    entries = load_manifest(manifest_path)
    available_names = {entry["name"] for entry in entries}
    unknown_names = sorted(set(selected_names) - available_names)
    if unknown_names:
        raise SmokeFailure(f"unknown manifest cases: {', '.join(unknown_names)}")
    selected = set(selected_names)
    cases: list[CaptureCase] = []
    for entry in entries:
        if selected and entry["name"] not in selected:
            continue
        if smoke_only and entry["smoke"] is not True:
            continue
        layout_path = Path(entry["layout"])
        if not layout_path.is_absolute():
            layout_path = REPOSITORY / layout_path
        layout_path = resolve_path(layout_path)
        load_layout(layout_path)
        output = output_dir / f"{entry['name']}.native.mcfr"
        require_new_output(output, ".mcfr", "recording output")
        cases.append(
            CaptureCase(
                name=entry["name"],
                layout_path=layout_path,
                output=output,
                seed=entry["seed"],
            )
        )
    if not cases:
        raise SmokeFailure("capture queue is empty")
    return cases


def build_single_case(
    layout_path: Path,
    output: Path | None,
    seed: int | None,
    video_output: Path | None,
    instrumentation_output: Path | None,
    instrumentation_profile: str | None,
) -> CaptureCase:
    layout_path = resolve_path(layout_path)
    layout = load_layout(layout_path)
    if output is None:
        output = DEFAULT_CAPTURE_ROOT / (
            f"{layout_path.stem}-{time.time_ns()}.native.mcfr"
        )
    output = resolve_path(output)
    require_new_output(output, ".mcfr", "recording output")
    if seed is not None and not -(2**31) <= seed < 2**31:
        raise SmokeFailure("--seed must be a signed 32-bit integer")
    effective_seed = layout.get("seed", 0) if seed is None else seed
    seed = None if effective_seed == 0 else effective_seed
    if (instrumentation_output is None) != (instrumentation_profile is None):
        raise SmokeFailure(
            "--instrumentation-output and --instrumentation-profile must be used together"
        )
    if video_output is not None:
        video_output = resolve_path(video_output)
        require_new_output(video_output, ".mov", "video output")
    if instrumentation_output is not None:
        instrumentation_output = resolve_path(instrumentation_output)
        require_new_output(instrumentation_output, ".h5", "instrumentation output")
    outputs = [path for path in (output, video_output, instrumentation_output) if path]
    if len(outputs) != len(set(outputs)):
        raise SmokeFailure("recording output paths must differ")
    return CaptureCase(
        name=layout_path.stem,
        layout_path=layout_path,
        output=output,
        seed=seed,
        video_output=video_output,
        instrumentation_output=instrumentation_output,
        instrumentation_profile=instrumentation_profile,
    )


def launch_game() -> subprocess.Popen[str]:
    if not ADAPTER.is_file():
        raise SmokeFailure(f"release Adapter is missing: {ADAPTER}")
    if not GAME.is_file():
        raise SmokeFailure(f"game executable is missing: {GAME}")
    environment = os.environ.copy()
    environment["DYLD_INSERT_LIBRARIES"] = str(ADAPTER)
    environment.pop("MECHCORE_ADAPTER_SOCKET", None)
    return subprocess.Popen(
        [str(GAME)],
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )


def status_from_tool(result: dict[str, Any], name: str) -> dict[str, Any]:
    status = result.get("status")
    if not isinstance(status, dict):
        raise SmokeFailure(f"tool {name} omitted final status: {result}")
    return status


def require_status(status: dict[str, Any], expected: dict[str, Any], label: str) -> None:
    if not all(status.get(key) == value for key, value in expected.items()):
        raise SmokeFailure(f"{label} returned unexpected status: {status}")
    print(f"ok: {label}: {json.dumps(status, ensure_ascii=False)}")


def initialize_client(client: McpClient) -> None:
    initialized = client.request(
        "initialize",
        {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "mechcore-layout-smoke", "version": "0.2.0"},
        },
        10,
    )
    if initialized.get("protocolVersion") != "2025-03-26":
        raise SmokeFailure(f"unexpected MCP protocol: {initialized}")
    client.notify("notifications/initialized")
    tools = client.request("tools/list", {}, 10).get("tools")
    names = {tool.get("name") for tool in tools} if isinstance(tools, list) else set()
    if names != EXPECTED_TOOLS:
        raise SmokeFailure(f"unexpected MCP tool surface: {sorted(names)}")
    resources = client.request("resources/list", {}, 10).get("resources")
    uris = (
        {resource.get("uri") for resource in resources}
        if isinstance(resources, list)
        else set()
    )
    if uris != {STATUS_URI}:
        raise SmokeFailure(f"unexpected MCP resources: {sorted(uris)}")
    client.request("resources/subscribe", {"uri": STATUS_URI}, 10)


def record_case(client: McpClient, case: CaptureCase) -> None:
    layout = load_layout(case.layout_path)
    activation_round = layout["round"]
    start_arguments = {"seed": case.seed} if case.seed is not None else {}
    test = client.call_tool("start_test", start_arguments)
    expected_test_status: dict[str, Any] = {
        "status": "training_ground",
        "round_count": 1,
        "deploying": True,
        "fighting": False,
    }
    if case.seed is not None:
        expected_test_status["match_seed"] = case.seed
    require_status(
        status_from_tool(test, "start_test"), expected_test_status, "start_test"
    )
    applied = client.call_tool("apply_layout", layout)
    operation = applied.get("operation")
    if not isinstance(operation, dict) or operation.get("applied") is not True:
        raise SmokeFailure(f"apply_layout was not confirmed: {applied}")
    if operation.get("round") != activation_round:
        raise SmokeFailure(f"layout activated in an unexpected round: {applied}")
    require_status(
        status_from_tool(applied, "apply_layout"),
        {
            "status": "training_ground",
            "round_count": activation_round,
            "deploying": True,
            "fighting": False,
        },
        "apply_layout",
    )
    print(f"ok: applied {case.name}: {case.layout_path}")
    record_arguments: dict[str, Any] = {"output": str(case.output)}
    if case.video_output is not None:
        record_arguments["video_output"] = str(case.video_output)
    if (
        case.instrumentation_output is not None
        and case.instrumentation_profile is not None
    ):
        record_arguments["instrumentation"] = {
            "output": str(case.instrumentation_output),
            "profile": case.instrumentation_profile,
        }
    recorded = client.call_tool("record_battle", record_arguments, BATTLE_TIMEOUT)
    recording = recorded.get("operation")
    if not isinstance(recording, dict) or recording.get("recorded") is not True:
        raise SmokeFailure(f"record_battle was not confirmed: {recorded}")
    if recording.get("output") != str(case.output):
        raise SmokeFailure(f"record_battle published an unexpected path: {recording}")
    cleanup = recorded.get("cleanup")
    if (
        not isinstance(cleanup, dict)
        or cleanup.get("match_exited") is not True
        or cleanup.get("game_reusable") is not True
    ):
        raise SmokeFailure(f"record_battle did not complete match cleanup: {recorded}")
    require_status(
        status_from_tool(recorded, "record_battle"),
        {"status": "main_menu"},
        "record_battle cleanup",
    )
    if not case.output.is_file() or case.output.stat().st_size == 0:
        raise SmokeFailure(f"record_battle did not publish {case.output}")
    tick_count = recording.get("tick_count")
    if not isinstance(tick_count, int) or isinstance(tick_count, bool) or tick_count < 0:
        raise SmokeFailure(f"record_battle omitted a valid tick_count: {recording}")
    hashes = recording.get("hashes")
    if not isinstance(hashes, dict):
        raise SmokeFailure(f"record_battle omitted verified hashes: {recording}")
    if case.video_output is not None:
        video = recording.get("video")
        if not isinstance(video, dict):
            raise SmokeFailure(f"record_battle omitted video metadata: {recording}")
        if not case.video_output.is_file():
            raise SmokeFailure(f"record_battle did not publish {case.video_output}")
        if video.get("frame_count") != tick_count + 1:
            raise SmokeFailure(
                f"video/MCFR frame count mismatch: {video} versus {recording}"
            )
        if video.get("view") != "calibration_topdown":
            raise SmokeFailure(f"unexpected video view metadata: {video}")
        expected_calibration = {
            "width": 2560,
            "height": 1600,
            "projection": "perspective",
            "camera_position": [0.0, 1070.0, -1070.0],
            "camera_euler_degrees": [45.0, 0.0, 0.0],
            "field_of_view_degrees": 20.0,
        }
        for key, expected in expected_calibration.items():
            if video.get(key) != expected:
                raise SmokeFailure(f"unexpected video {key}: {video}")
    if case.instrumentation_output is not None:
        instrumentation = recording.get("instrumentation")
        if not isinstance(instrumentation, dict):
            raise SmokeFailure(
                f"record_battle omitted instrumentation metadata: {recording}"
            )
        if not case.instrumentation_output.is_file():
            raise SmokeFailure(
                f"record_battle did not publish {case.instrumentation_output}"
            )
        if instrumentation.get("profile") != case.instrumentation_profile:
            raise SmokeFailure(f"unexpected instrumentation profile: {instrumentation}")
        if instrumentation.get("record_count") != tick_count + 1:
            raise SmokeFailure(
                "instrumentation/MCFR tick count mismatch: "
                f"{instrumentation} versus {recording}"
            )
    print(
        f"ok: recorded {case.name}: output={case.output} tick_count={tick_count} "
        f"scenario_hash={hashes.get('scenario_hash')} result_hash={hashes.get('result_hash')}"
    )


def recover_failed_session(client: McpClient, error: BaseException) -> None:
    required = None
    if isinstance(error, ToolFailure) and isinstance(error.detail, dict):
        next_step = error.detail.get("next")
        if isinstance(next_step, dict):
            required = next_step.get("required")
    try:
        status = client.read_status_resource(TRANSITION_TIMEOUT)
        if required == "quit_match" or status.get("status") in {
            "training_ground",
            "replay",
        }:
            client.call_tool("quit_match", {})
            status = client.read_status_resource(TRANSITION_TIMEOUT)
        if status.get("status") == "main_menu":
            client.call_tool("quit_game", {})
    except BaseException:
        pass


def run(cases: list[CaptureCase]) -> None:
    if not cases:
        raise SmokeFailure("capture queue is empty")
    outputs = [case.output for case in cases]
    if len(outputs) != len(set(outputs)):
        raise SmokeFailure("capture queue contains duplicate output paths")
    for case in cases:
        require_new_output(case.output, ".mcfr", "recording output")
        case.output.parent.mkdir(parents=True, exist_ok=True)
    client = McpClient()
    game: subprocess.Popen[str] | None = None
    failure: BaseException | None = None
    try:
        initialize_client(client)
        game = launch_game()
        connected = client.call_tool("connect_adapter", {})
        status_from_tool(connected, "connect_adapter")
        client.wait_stream_status({"status": "main_menu"}, TRANSITION_TIMEOUT)
        for index, case in enumerate(cases, start=1):
            print(f"capture {index}/{len(cases)}: {case.name}")
            record_case(client, case)
        stopped = client.call_tool("quit_game", {})
        require_status(
            status_from_tool(stopped, "quit_game"),
            {"status": "game_off"},
            "quit_game",
        )
        try:
            returncode = game.wait(timeout=30)
        except subprocess.TimeoutExpired as error:
            raise SmokeFailure("game did not exit after quit_game") from error
        if returncode != 0:
            raise SmokeFailure(f"game exited with code {returncode}")
    except BaseException as error:
        failure = error
        if game is not None:
            recover_failed_session(client, error)
        raise
    finally:
        if game is not None and game.poll() is None:
            game.terminate()
            try:
                game.wait(timeout=10)
            except subprocess.TimeoutExpired:
                game.kill()
                game.wait(timeout=10)
        try:
            client.close()
        except SmokeFailure:
            if failure is None:
                raise


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Record one layout or a manifest-backed batch through one `mechcore mcp` "
            "connection and game process."
        )
    )
    parser.add_argument(
        "layout",
        type=Path,
        nargs="?",
        help="single-layout YAML; omit only when --batch is used",
    )
    parser.add_argument(
        "--batch",
        action="store_true",
        help="record a queue from --manifest (defaults to tests/mcfr-regressions.yaml)",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=DEFAULT_MANIFEST,
        help="batch manifest (default: tests/mcfr-regressions.yaml)",
    )
    parser.add_argument(
        "--case",
        action="append",
        default=[],
        dest="cases",
        metavar="NAME",
        help="batch-only exact manifest case name; repeat to select multiple cases",
    )
    parser.add_argument(
        "--smoke-only",
        action="store_true",
        help="batch-only filter for manifest entries with smoke: true",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="single-layout output; defaults to a new path below work/captures",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        help="batch or repeated-layout output directory; defaults below work/captures",
    )
    parser.add_argument(
        "--repeat",
        type=int,
        default=1,
        help="single-layout capture count in one game process (default: 1)",
    )
    parser.add_argument(
        "--seed",
        type=int,
        help="single-layout signed 32-bit seed override; 0 requests system random",
    )
    parser.add_argument(
        "--video-output",
        type=Path,
        help="single-layout logic-frame-aligned calibration_topdown .mov path",
    )
    parser.add_argument(
        "--instrumentation-output",
        type=Path,
        help="also export a temporary Adapter-native HDF5 research sidecar",
    )
    parser.add_argument(
        "--instrumentation-profile",
        help="Adapter-defined temporary research profile",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the validated capture queue without starting MCP or the game",
    )
    return parser


def cases_from_args(arguments: argparse.Namespace) -> list[CaptureCase]:
    if arguments.repeat < 1:
        raise SmokeFailure("--repeat must be positive")
    if arguments.batch:
        if arguments.layout is not None:
            raise SmokeFailure("a positional layout cannot be used with --batch")
        if arguments.output is not None:
            raise SmokeFailure("--output is single-layout only; use --output-dir")
        if arguments.seed is not None:
            raise SmokeFailure(
                "--seed is single-layout only; batch seeds come from the manifest"
            )
        if arguments.repeat != 1:
            raise SmokeFailure("--repeat cannot be used with --batch")
        if arguments.video_output is not None:
            raise SmokeFailure("--video-output is single-layout only")
        if (
            arguments.instrumentation_output is not None
            or arguments.instrumentation_profile is not None
        ):
            raise SmokeFailure("instrumentation options are single-layout only")
        output_dir = arguments.output_dir
        if output_dir is None:
            output_dir = DEFAULT_CAPTURE_ROOT / f"mcfr-regression-{time.time_ns()}"
        return build_batch_cases(
            arguments.manifest,
            output_dir,
            arguments.cases,
            arguments.smoke_only,
        )
    if arguments.layout is None:
        raise SmokeFailure("provide a layout or use --batch")
    if arguments.cases:
        raise SmokeFailure("--case is batch-only")
    if arguments.smoke_only:
        raise SmokeFailure("--smoke-only is batch-only")
    if arguments.manifest != DEFAULT_MANIFEST:
        raise SmokeFailure("--manifest is batch-only")
    if arguments.repeat == 1:
        if arguments.output_dir is not None:
            raise SmokeFailure("--output-dir requires --batch or --repeat greater than 1")
        return [
            build_single_case(
                arguments.layout,
                arguments.output,
                arguments.seed,
                arguments.video_output,
                arguments.instrumentation_output,
                arguments.instrumentation_profile,
            )
        ]
    if arguments.output is not None:
        raise SmokeFailure("--output cannot be used with repeated capture; use --output-dir")
    if arguments.video_output is not None:
        raise SmokeFailure("--video-output cannot be used with repeated capture")
    if (
        arguments.instrumentation_output is not None
        or arguments.instrumentation_profile is not None
    ):
        raise SmokeFailure("instrumentation options cannot be used with repeated capture")
    layout_path = resolve_path(arguments.layout)
    output_dir = arguments.output_dir
    if output_dir is None:
        output_dir = DEFAULT_CAPTURE_ROOT / f"{layout_path.stem}-{time.time_ns()}"
    output_dir = resolve_path(output_dir)
    cases = []
    for index in range(1, arguments.repeat + 1):
        case = build_single_case(
            layout_path,
            output_dir / f"{layout_path.stem}-{index:02d}.native.mcfr",
            arguments.seed,
            None,
            None,
            None,
        )
        cases.append(
            CaptureCase(
                name=f"{case.name}-{index:02d}",
                layout_path=case.layout_path,
                output=case.output,
                seed=case.seed,
            )
        )
    return cases


def preview(cases: list[CaptureCase]) -> None:
    for index, case in enumerate(cases, start=1):
        print(
            json.dumps(
                {
                    "index": index,
                    "name": case.name,
                    "layout": str(case.layout_path),
                    "seed": case.seed,
                    "output": str(case.output),
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
    print(f"queue: {len(cases)} capture(s)")


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    try:
        arguments = parser.parse_args(argv)
        cases = cases_from_args(arguments)
        preview(cases)
        if not arguments.dry_run:
            run(cases)
    except (OSError, SmokeFailure, ValueError) as error:
        print(f"smoke failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
