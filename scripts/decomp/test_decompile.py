"""Test AssetRipper process and temporary-file ownership without the game.

Run: uv run --with pyyaml python scripts/decomp/test_decompile.py
"""

import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("decompile", Path(__file__).with_name("decompile.py"))
decompile = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(decompile)

SERVER = """#!/usr/bin/env python3
from http.server import BaseHTTPRequestHandler, HTTPServer
import os
from pathlib import Path
import signal
import sys

cache = Path(__file__).parent / 'temp' / 'loaded-game'
cache.mkdir(parents=True)
(cache / 'unpacked').write_bytes(b'temporary resource')
if os.environ.get('RIPPER_TEST_EXIT'):
    sys.exit(7)
if os.environ.get('RIPPER_TEST_IGNORE_TERM'):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'{}')
    def log_message(self, *args):
        pass

port = int(sys.argv[sys.argv.index('--port') + 1])
HTTPServer(('127.0.0.1', port), Handler).serve_forever()
"""


class RipperLifetime(unittest.TestCase):
    def setUp(self):
        room = tempfile.TemporaryDirectory()
        self.addCleanup(room.cleanup)
        self.tools = Path(room.name)
        installed = self.tools / "AssetRipper"
        installed.mkdir()
        binary = installed / "AssetRipper.GUI.Free"
        binary.write_text(SERVER)
        binary.chmod(0o755)
        (installed / "libcapstone.dylib").write_bytes(b"native companion")
        self.foreign = installed / "temp" / "another-process" / "resource"
        self.foreign.parent.mkdir(parents=True)
        self.foreign.write_bytes(b"leave untouched")
        for setting in (patch.object(decompile, "TOOLS", self.tools),
                        patch.object(decompile, "assetripper", return_value=binary)):
            setting.start()
            self.addCleanup(setting.stop)

    def assert_clean(self):
        self.assertEqual(list(self.tools.glob("assetripper-*")), [])
        self.assertEqual(self.foreign.read_bytes(), b"leave untouched")

    def test_independent_runs_and_repeated_close(self):
        with decompile.Ripper() as first, decompile.Ripper() as second:
            self.assertNotEqual(first.directory, second.directory)
            self.assertEqual((first.directory / "libcapstone.dylib").read_bytes(), b"native companion")
            first.close()
            first.close()
            self.assertIsNotNone(first.process.returncode)
            self.assertFalse(first.directory.exists())
            self.assertTrue((second.directory / "temp/loaded-game/unpacked").exists())
            self.assertEqual(second.get("/openapi.json"), b"{}")
        self.assertIsNotNone(second.process.returncode)
        self.assert_clean()

    def test_export_failure_and_keyboard_interrupt_clean_the_child(self):
        for error in (RuntimeError("export failed"), KeyboardInterrupt()):
            servers = []

            def fail_load(server, _app):
                servers.append(server)
                raise error

            with patch.object(decompile.Ripper, "load", fail_load):
                with self.assertRaises(type(error)):
                    decompile.step_config(Path("unused"), self.tools)
            self.assertIsNotNone(servers[0].process.returncode)
            self.assertTrue(servers[0].log.closed)
            self.assert_clean()

    def test_failed_start_and_failed_spawn_remove_the_run_directory(self):
        with patch.dict(os.environ, {"RIPPER_TEST_EXIT": "1"}):
            with self.assertRaises(SystemExit):
                decompile.Ripper()
        self.assert_clean()
        with patch.object(decompile.subprocess, "Popen", side_effect=OSError("spawn failed")):
            with self.assertRaisesRegex(OSError, "spawn failed"):
                decompile.Ripper()
        self.assert_clean()

    def test_interruption_during_startup_reaps_the_child(self):
        children = []
        spawn = subprocess.Popen

        def start(*args, **kwargs):
            child = spawn(*args, **kwargs)
            children.append(child)
            return child

        with patch.object(decompile.subprocess, "Popen", side_effect=start), \
                patch.object(decompile.urllib.request, "urlopen", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                decompile.Ripper()
        self.assertIsNotNone(children[0].returncode)
        self.assert_clean()

    def test_unresponsive_child_is_killed_before_removing_its_files(self):
        with patch.dict(os.environ, {"RIPPER_TEST_IGNORE_TERM": "1"}):
            server = decompile.Ripper()
        self.addCleanup(server.close)
        wait = server.process.wait
        with patch.object(server.process, "wait", side_effect=lambda timeout=None: wait(
                timeout=0.1 if timeout is not None else None)):
            server.close()
        self.assertEqual(server.process.returncode, -signal.SIGKILL)
        self.assert_clean()

    def test_sigterm_unwinds_an_active_run(self):
        previous = signal.signal(signal.SIGTERM, decompile.exit_on_signal)
        self.addCleanup(signal.signal, signal.SIGTERM, previous)
        with self.assertRaises(SystemExit) as caught:
            with decompile.Ripper() as server:
                os.kill(os.getpid(), signal.SIGTERM)
        self.assertEqual(caught.exception.code, 128 + signal.SIGTERM)
        self.assertIsNotNone(server.process.returncode)
        self.assert_clean()


if __name__ == "__main__":
    unittest.main()
