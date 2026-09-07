#!/usr/bin/env python3
"""Exercise the real build.rs with a tiny cdylib; no game or native capture.

Run from a nightly Cargo environment: python3 scripts/check-adapter-packaging.py
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


REPO = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="mechcore-packaging-") as temporary:
        root = Path(temporary)
        (root / "src").mkdir()
        (root / "adapter/src").mkdir(parents=True)
        (root / ".cargo").mkdir()
        shutil.copyfile(REPO / "rust-toolchain.toml", root / "rust-toolchain.toml")
        shutil.copyfile(REPO / "crates/mechcore/build.rs", root / "build.rs")
        (root / ".cargo/config.toml").write_text("[unstable]\nbindeps = true\n")
        (root / "Cargo.toml").write_text('''[package]
name = "packaging-check"
version = "0.0.0"
edition = "2024"
[workspace]
[build-dependencies]
mechcore-adapter = { path = "adapter", artifact = "cdylib", target = "target" }
tempfile = "3"
''')
        (root / "adapter/Cargo.toml").write_text('''[package]
name = "mechcore-adapter"
version = "0.0.0"
edition = "2024"
[lib]
name = "mechcore_adapter"
crate-type = ["cdylib"]
''')
        (root / "src/main.rs").write_text('''fn main() {
    let exe = std::env::current_exe().unwrap();
    assert!(exe.parent().unwrap().join("libmechcore_adapter.dylib").is_file());
}
''')
        source = root / "adapter/src/lib.rs"

        def marker(value):
            source.write_text(
                '#[unsafe(no_mangle)]\n'
                f'pub extern "C" fn marker() -> u32 {{ {value} }}\n'
            )

        environment = os.environ.copy()
        # Keep the test isolated even when the caller shares a build cache.
        environment["CARGO_TARGET_DIR"] = str(root / "output with spaces")
        environment.pop("CARGO_BUILD_BUILD_DIR", None)

        def run(*arguments, fresh=False):
            result = subprocess.run(
                ["cargo", "run", "--offline", "--message-format=json", *arguments],
                cwd=root, env=environment, text=True, capture_output=True,
            )
            if result.returncode:
                raise RuntimeError(result.stdout + result.stderr)
            artifacts = [json.loads(line) for line in result.stdout.splitlines()]
            if fresh:
                assert all(item.get("fresh", True) for item in artifacts), (
                    "no-op build recompiled a target", result.stderr
                )
            executable = next(Path(item["executable"]) for item in artifacts
                              if item.get("executable"))
            dylib = executable.parent / "libmechcore_adapter.dylib"
            artifact = next(Path(name) for item in artifacts
                            if item.get("target", {}).get("name") == "mechcore_adapter"
                            for name in item.get("filenames", [])
                            if name.endswith(".dylib"))
            assert dylib.read_bytes() == artifact.read_bytes()
            return dylib

        marker(1)
        dylib = run("--release")
        original = dylib.read_bytes()
        timestamp = dylib.stat().st_mtime_ns
        run("--release", fresh=True)
        assert dylib.stat().st_mtime_ns == timestamp, "no-op rewrote the dylib"
        marker(2)
        run("--release")
        updated = dylib.read_bytes()
        assert updated != original, "Adapter change did not update the sibling"
        dylib.unlink()
        run("--release")
        assert dylib.read_bytes() == updated, "missing sibling was not repaired"
        run()  # debug profile
        host = subprocess.check_output(["rustc", "-vV"], text=True)
        triple = next(line.removeprefix("host: ") for line in host.splitlines()
                      if line.startswith("host: "))
        run("--release", "--target", triple)
        print("PASS: fresh build, no-op, Adapter update, missing sibling, debug, "
              "custom target directory with spaces, explicit target triple")


if __name__ == "__main__":
    main()
