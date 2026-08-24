#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p "python3.withPackages (packages: [ packages.pyyaml ])"

from __future__ import annotations

import argparse
import json
import queue
import subprocess
import sys
import threading
import time
from collections import deque
from pathlib import Path
from typing import Any

import yaml


STATUS_URI = "mechcore://status"
EXPECTED_TOOLS = {
    "apply_layout",
    "record_battle",
    "quit_game",
    "quit_match",
    "speed_up",
    "start_game",
    "start_test",
    "status",
    "toggle_fight",
}
TRANSITION_TIMEOUT = 60.0
BATTLE_TIMEOUT = 180.0
MAX_ACTIVATION_ROUND = 15
REPOSITORY = Path(__file__).resolve().parent.parent
MECHCORE = REPOSITORY / "target/release/mechcore"


class SmokeFailure(RuntimeError):
    pass


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
            raise SmokeFailure(f"tool {name} failed: {structured or result.get('content')}")
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
    return layout


def status_from_tool(result: dict[str, Any], name: str) -> dict[str, Any]:
    status = result.get("status")
    if not isinstance(status, dict):
        raise SmokeFailure(f"tool {name} omitted final status: {result}")
    return status


def require_status(status: dict[str, Any], expected: dict[str, Any], label: str) -> None:
    if not all(status.get(key) == value for key, value in expected.items()):
        raise SmokeFailure(f"{label} returned unexpected status: {status}")
    print(f"ok: {label}: {json.dumps(status, ensure_ascii=False)}")


def run(layout_path: Path, output: Path, video_output: Path | None) -> None:
    layout = load_layout(layout_path)
    activation_round = layout["round"]
    client = McpClient()
    failure: BaseException | None = None
    try:
        initialized = client.request(
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "mechcore-layout-smoke", "version": "0.1.0"},
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
        uris = {resource.get("uri") for resource in resources} if isinstance(resources, list) else set()
        if uris != {STATUS_URI}:
            raise SmokeFailure(f"unexpected MCP resources: {sorted(uris)}")
        client.request("resources/subscribe", {"uri": STATUS_URI}, 10)

        started = client.call_tool("start_game", {})
        require_status(status_from_tool(started, "start_game"), {"status": "main_menu"}, "start_game")
        test = client.call_tool("start_test", {})
        require_status(
            status_from_tool(test, "start_test"),
            {"status": "training_ground", "round_count": 1, "deploying": True, "fighting": False},
            "start_test",
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
        print(f"ok: applied {layout_path}")
        record_arguments = {"output": str(output.resolve())}
        if video_output is not None:
            record_arguments["video_output"] = str(video_output.resolve())
        recorded = client.call_tool("record_battle", record_arguments, BATTLE_TIMEOUT)
        recording = recorded.get("operation")
        if not isinstance(recording, dict) or recording.get("recorded") is not True:
            raise SmokeFailure(f"record_battle was not confirmed: {recorded}")
        if not output.is_file():
            raise SmokeFailure(f"record_battle did not publish {output}")
        if video_output is not None:
            video = recording.get("video")
            if not isinstance(video, dict):
                raise SmokeFailure(f"record_battle omitted video metadata: {recording}")
            if not video_output.is_file():
                raise SmokeFailure(f"record_battle did not publish {video_output}")
            if video.get("frame_count") != recording.get("tick_count"):
                raise SmokeFailure(
                    f"video/MCFR frame count mismatch: {video} versus {recording}"
                )
            if video.get("view") != "calibration_topdown":
                raise SmokeFailure(f"unexpected video view metadata: {video}")
            expected_calibration = {
                "projection": "perspective",
                "camera_position": [0.0, 1070.0, -1070.0],
                "camera_euler_degrees": [45.0, 0.0, 0.0],
                "field_of_view_degrees": 20.0,
            }
            for key, expected in expected_calibration.items():
                if video.get(key) != expected:
                    raise SmokeFailure(f"unexpected video {key}: {video}")
        print(
            "ok: recorded "
            f"{output}: states={recording.get('state_count')} "
            f"transitions={recording.get('transition_count')}"
        )
        menu = client.call_tool("quit_match", {})
        require_status(status_from_tool(menu, "quit_match"), {"status": "main_menu"}, "quit_match")
        stopped = client.call_tool("quit_game", {})
        require_status(status_from_tool(stopped, "quit_game"), {"status": "game_off"}, "quit_game")
        if stopped.get("exit_code") != 0:
            raise SmokeFailure(f"quit_game returned nonzero exit: {stopped}")
    except BaseException as error:
        failure = error
        raise
    finally:
        try:
            client.close()
        except SmokeFailure:
            if failure is None:
                raise


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run a complete layout battle through `mechcore mcp`."
    )
    parser.add_argument("layout", type=Path)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(f"/tmp/mechcore-mcp-smoke-{int(time.time_ns())}.mcfr"),
    )
    parser.add_argument(
        "--video-output",
        type=Path,
        help="also export logic-frame-aligned calibration_topdown video to this new .mov path",
    )
    try:
        arguments = parser.parse_args()
        run(arguments.layout, arguments.output, arguments.video_output)
    except (OSError, SmokeFailure, ValueError) as error:
        print(f"smoke failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
