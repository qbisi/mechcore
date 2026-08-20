#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p "python3.withPackages (packages: [ packages.pyyaml ])"

import argparse
import json
import os
import select
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import yaml


PROTOCOL = "mechcore.adapter.v1"
MAX_ACTIVATION_ROUND = 15
REPOSITORY = Path(__file__).resolve().parent.parent
GAME_EXECUTABLE = Path.home() / (
    "Library/Application Support/Steam/steamapps/common/Mechabellum/"
    "Mechabellum.app/Contents/MacOS/Mechabellum"
)
ADAPTER_DYLIB = REPOSITORY / "target/release/libmechcore_adapter.dylib"
ADAPTER_SOCKET = Path("/tmp/mechcore-v2-smoke.sock")


class SmokeFailure(RuntimeError):
    pass


class AdapterDisconnected(SmokeFailure):
    pass


class AdapterClient:
    def __init__(self, path: Path) -> None:
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            self.socket.connect(str(path))
        except OSError as error:
            raise SmokeFailure(f"cannot connect to {path}: {error}") from error
        self.buffer = bytearray()
        self.next_id = 1

        hello = self._read_json(10.0)
        if hello.get("kind") != "hello" or hello.get("protocol") != PROTOCOL:
            raise SmokeFailure(f"unexpected adapter hello: {hello}")
        capabilities = hello.get("capabilities")
        if not isinstance(capabilities, list):
            raise SmokeFailure("hello.capabilities is not an array")
        required = {
            "status",
            "start_test",
            "apply_layout",
            "toggle_fight",
            "speed_up",
            "quit_match",
            "quit_game",
        }
        missing = sorted(required.difference(capabilities))
        if missing:
            raise SmokeFailure(f"adapter is missing capabilities: {missing}")

    def close(self) -> None:
        self.socket.close()

    def _read_json(self, timeout: float) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise SmokeFailure("timed out waiting for adapter response")
            self.socket.settimeout(remaining)
            try:
                chunk = self.socket.recv(65536)
            except TimeoutError as error:
                raise SmokeFailure("timed out waiting for adapter response") from error
            except OSError as error:
                raise AdapterDisconnected(f"adapter connection failed: {error}") from error
            if not chunk:
                raise AdapterDisconnected("adapter disconnected")
            self.buffer.extend(chunk)

        line, _, rest = self.buffer.partition(b"\n")
        self.buffer = bytearray(rest)
        try:
            value = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise SmokeFailure(f"adapter returned invalid JSON: {line!r}") from error
        if not isinstance(value, dict):
            raise SmokeFailure(f"adapter returned a non-object: {value!r}")
        return value

    def _send(self, operation: str, arguments: dict[str, Any]) -> int:
        request_id = self.next_id
        self.next_id += 1
        request = {
            "id": request_id,
            "operation": operation,
            "arguments": arguments,
        }
        encoded = json.dumps(request, separators=(",", ":")).encode() + b"\n"
        try:
            self.socket.sendall(encoded)
        except OSError as error:
            raise AdapterDisconnected(f"cannot send {operation}: {error}") from error
        return request_id

    def request(
        self,
        operation: str,
        arguments: dict[str, Any],
        timeout: float = 10.0,
    ) -> dict[str, Any]:
        request_id = self._send(operation, arguments)
        response = self._read_json(timeout)
        if response.get("kind") != "response" or response.get("id") != request_id:
            raise SmokeFailure(f"unexpected {operation} response: {response}")
        if response.get("ok") is not True:
            raise SmokeFailure(f"{operation} failed: {response.get('error')}")
        result = response.get("result")
        if not isinstance(result, dict):
            raise SmokeFailure(f"{operation} returned a non-object result: {result!r}")
        return result

    def request_game_quit(self, timeout: float) -> None:
        request_id = self._send("quit_game", {})
        try:
            response = self._read_json(10.0)
        except AdapterDisconnected:
            return
        if response.get("kind") != "response" or response.get("id") != request_id:
            raise SmokeFailure(f"unexpected quit_game response: {response}")
        if response.get("ok") is not True:
            raise SmokeFailure(f"quit_game failed: {response.get('error')}")

        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise SmokeFailure("game process did not close the adapter socket")
            self.socket.settimeout(min(1.0, remaining))
            try:
                chunk = self.socket.recv(1)
            except TimeoutError:
                continue
            except OSError:
                return
            if not chunk:
                return
            raise SmokeFailure("adapter sent unexpected data after quit_game")


def poll_status(
    client: AdapterClient,
    description: str,
    timeout: float,
    expected: dict[str, Any],
) -> dict[str, Any]:
    if timeout <= 0:
        raise SmokeFailure(f"timeout for {description} must be positive")
    deadline = time.monotonic() + timeout
    last: dict[str, Any] | None = None
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        last = client.request("status", {}, remaining)
        if all(last.get(key) == value for key, value in expected.items()):
            print(f"ok: {description}: {json.dumps(last, ensure_ascii=False)}")
            return last
        time.sleep(0.25)
    raise SmokeFailure(f"timed out waiting for {description}; last status: {last}")


def endpoint_is_live(path: Path) -> bool:
    probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    probe.settimeout(0.25)
    try:
        probe.connect(str(path))
    except OSError:
        return False
    finally:
        probe.close()
    return True


def launch_game() -> subprocess.Popen[bytes]:
    if not GAME_EXECUTABLE.is_file():
        raise SmokeFailure(f"game executable does not exist: {GAME_EXECUTABLE}")
    if not ADAPTER_DYLIB.is_file():
        raise SmokeFailure(f"release adapter does not exist: {ADAPTER_DYLIB}")
    if endpoint_is_live(ADAPTER_SOCKET):
        raise SmokeFailure(f"adapter socket is already live: {ADAPTER_SOCKET}")

    environment = os.environ.copy()
    environment["MECHCORE_ADAPTER_SOCKET"] = str(ADAPTER_SOCKET)
    environment["DYLD_INSERT_LIBRARIES"] = str(ADAPTER_DYLIB)
    try:
        process = subprocess.Popen([str(GAME_EXECUTABLE)], env=environment)
    except OSError as error:
        raise SmokeFailure(f"cannot launch {GAME_EXECUTABLE}: {error}") from error
    print(f"ok: launched game pid {process.pid}")
    return process


def wait_adapter(process: subprocess.Popen[bytes], timeout: float) -> AdapterClient:
    deadline = time.monotonic() + timeout
    last_error: SmokeFailure | None = None
    directory_fd = os.open(ADAPTER_SOCKET.parent, os.O_RDONLY)
    queue = select.kqueue()
    changes = [
        select.kevent(
            directory_fd,
            filter=select.KQ_FILTER_VNODE,
            flags=select.KQ_EV_ADD | select.KQ_EV_ENABLE | select.KQ_EV_CLEAR,
            fflags=(
                select.KQ_NOTE_WRITE
                | select.KQ_NOTE_EXTEND
                | select.KQ_NOTE_RENAME
                | select.KQ_NOTE_DELETE
            ),
        ),
        select.kevent(
            process.pid,
            filter=select.KQ_FILTER_PROC,
            flags=select.KQ_EV_ADD | select.KQ_EV_ENABLE,
            fflags=select.KQ_NOTE_EXIT,
        ),
    ]
    try:
        queue.control(changes, 0, 0)
        while True:
            returncode = process.poll()
            if returncode is not None:
                raise SmokeFailure(
                    f"game exited with code {returncode} before adapter became ready"
                )
            try:
                return AdapterClient(ADAPTER_SOCKET)
            except SmokeFailure as error:
                last_error = error

            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise SmokeFailure(
                    f"timed out waiting for adapter; last error: {last_error}"
                )
            queue.control(None, 2, remaining)
    finally:
        queue.close()
        os.close(directory_fd)


def load_layout(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as stream:
            value = yaml.safe_load(stream)
    except (OSError, yaml.YAMLError) as error:
        raise SmokeFailure(f"cannot load layout {path}: {error}") from error
    if not isinstance(value, dict):
        raise SmokeFailure(f"layout root must be an object: {path}")
    activation_round = value.get("round")
    if (
        not isinstance(activation_round, int)
        or isinstance(activation_round, bool)
        or not 1 <= activation_round <= MAX_ACTIVATION_ROUND
    ):
        raise SmokeFailure(
            f"layout round must be within 1..={MAX_ACTIVATION_ROUND}"
        )
    return value


def force_stop_game(process: subprocess.Popen[bytes], timeout: float) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=timeout)


def shutdown_game(
    client: AdapterClient | None,
    process: subprocess.Popen[bytes],
    timeout: float,
) -> None:
    returncode = process.poll()
    if returncode is not None:
        if returncode != 0:
            raise SmokeFailure(f"game exited with code {returncode}")
        return

    try:
        if client is None:
            raise SmokeFailure("adapter was unavailable during shutdown")

        status = client.request("status", {}, timeout)
        if status.get("status") != "main_menu":
            quit_result = client.request("quit_match", {}, timeout)
            if quit_result.get("performed") is not True:
                raise SmokeFailure(f"quit_match was not accepted: {quit_result}")
            print(f"ok: quit_match initiated: {json.dumps(quit_result)}")
            poll_status(client, "main menu after quit_match", timeout, {"status": "main_menu"})

        client.request_game_quit(timeout)
        try:
            returncode = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            raise SmokeFailure("game process did not exit after quit_game") from error
        if returncode != 0:
            raise SmokeFailure(f"game exited with code {returncode}")
        print("ok: game process exited with code 0")
    except (SmokeFailure, OSError) as error:
        try:
            force_stop_game(process, timeout)
        except (OSError, subprocess.TimeoutExpired) as force_error:
            raise SmokeFailure(
                "graceful game shutdown failed and the owned process could not "
                f"be stopped: graceful error: {error}; stop error: {force_error}"
            ) from force_error
        raise SmokeFailure(
            f"graceful game shutdown failed; stopped owned process: {error}"
        ) from error


def run(args: argparse.Namespace) -> None:
    layout = load_layout(args.layout)
    activation_round = layout["round"]
    process = launch_game()
    client: AdapterClient | None = None
    try:
        client = wait_adapter(process, args.transition_timeout)
        poll_status(
            client,
            "main menu",
            args.transition_timeout,
            {"status": "main_menu"},
        )

        started = client.request(
            "start_test",
            {},
            args.transition_timeout,
        )
        if started.get("created") is not True:
            raise SmokeFailure(f"start_test was not accepted: {started}")
        print(f"ok: start_test initiated: {json.dumps(started)}")
        poll_status(
            client,
            "round-one deployment",
            args.transition_timeout,
            {
                "status": "training_ground",
                "round_count": 1,
                "deploying": True,
                "fighting": False,
            },
        )

        applied = client.request("apply_layout", layout, args.transition_timeout)
        if applied.get("applied") is not True:
            raise SmokeFailure(f"layout was not confirmed: {applied}")
        if applied.get("round") != activation_round:
            raise SmokeFailure(f"layout activated in an unexpected round: {applied}")
        print(f"ok: applied layout from {args.layout}")

        poll_status(
            client,
            "activation-round deployment",
            args.transition_timeout,
            {
                "status": "training_ground",
                "round_count": activation_round,
                "deploying": True,
                "fighting": False,
            },
        )

        client.request("toggle_fight", {})
        poll_status(
            client,
            "activation-round fight",
            args.transition_timeout,
            {
                "status": "training_ground",
                "round_count": activation_round,
                "deploying": False,
                "fighting": True,
            },
        )
        speed_up = client.request("speed_up", {})
        if speed_up.get("requested") is not True:
            raise SmokeFailure(f"speed-up vote was not confirmed: {speed_up}")
        print(f"ok: speed-up requested: {json.dumps(speed_up)}")
        poll_status(
            client,
            "deployment after the activation fight",
            args.battle_timeout,
            {
                "status": "training_ground",
                "round_count": activation_round + 1,
                "deploying": True,
                "fighting": False,
            },
        )
    except BaseException:
        try:
            shutdown_game(client, process, args.transition_timeout)
        except SmokeFailure as cleanup_error:
            print(f"smoke cleanup failed: {cleanup_error}", file=sys.stderr)
        raise
    else:
        shutdown_game(client, process, args.transition_timeout)
    finally:
        if client is not None:
            client.close()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Launch Mechabellum, apply a layout, and run one Training Ground battle."
        )
    )
    parser.add_argument("layout", type=Path, help="layout.yaml to apply")
    parser.add_argument("--transition-timeout", type=timeout_seconds, default=60.0)
    parser.add_argument("--battle-timeout", type=timeout_seconds, default=60.0)
    return parser.parse_args()


def timeout_seconds(value: str) -> float:
    timeout = float(value)
    if not 0 < timeout <= 60:
        raise argparse.ArgumentTypeError("timeout must be greater than 0 and at most 60 seconds")
    return timeout


def main() -> int:
    try:
        run(parse_args())
    except (SmokeFailure, TypeError, ValueError) as error:
        print(f"smoke failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
