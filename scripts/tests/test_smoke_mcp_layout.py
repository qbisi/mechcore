from __future__ import annotations

import contextlib
import importlib.util
import io
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "smoke_mcp_layout.py"
SPEC = importlib.util.spec_from_file_location("smoke_mcp_layout", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
smoke = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = smoke
SPEC.loader.exec_module(smoke)


class SmokeMcpLayoutCliTests(unittest.TestCase):
    def test_help_describes_single_and_manifest_batch_modes(self) -> None:
        help_text = smoke.build_parser().format_help()
        self.assertIn("--batch", help_text)
        self.assertIn("--manifest", help_text)
        self.assertIn("--repeat", help_text)
        self.assertIn("--dry-run", help_text)

    def test_single_layout_preserves_explicit_output_and_seed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "single.native.mcfr"
            arguments = smoke.build_parser().parse_args(
                [
                    str(smoke.REPOSITORY / "tests/layouts/marksman-vs-arclight.yaml"),
                    "--output",
                    str(output),
                    "--seed",
                    "1787720817",
                    "--dry-run",
                ]
            )
            cases = smoke.cases_from_args(arguments)
        self.assertEqual(len(cases), 1)
        self.assertEqual(cases[0].output, output.resolve())
        self.assertEqual(cases[0].seed, 1787720817)

    def test_single_layout_seed_is_used_and_zero_override_requests_random(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            layout = root / "layout.yaml"
            layout.write_text(
                """seed: -17
round: 1
sides:
  blue: {formations: [{type: marksman, x: 0, y: -50}]}
  red: {formations: [{type: arclight, x: 0, y: -50}]}
""",
                encoding="utf-8",
            )
            inherited = smoke.cases_from_args(
                smoke.build_parser().parse_args(
                    [str(layout), "--output", str(root / "inherited.mcfr")]
                )
            )
            randomized = smoke.cases_from_args(
                smoke.build_parser().parse_args(
                    [
                        str(layout),
                        "--output",
                        str(root / "random.mcfr"),
                        "--seed",
                        "0",
                    ]
                )
            )
        self.assertEqual(inherited[0].seed, -17)
        self.assertIsNone(randomized[0].seed)

    def test_default_manifest_builds_full_batch_with_manifest_seeds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            arguments = smoke.build_parser().parse_args(
                ["--batch", "--output-dir", directory, "--dry-run"]
            )
            cases = smoke.cases_from_args(arguments)
        self.assertEqual(len(cases), 80)
        self.assertEqual(cases[0].name, "marksman-vs-arclight")
        self.assertEqual(cases[0].seed, 1787720817)
        self.assertTrue(all(case.layout_path.is_absolute() for case in cases))
        self.assertTrue(all(case.output.is_absolute() for case in cases))
        self.assertEqual(len({case.output for case in cases}), len(cases))

    def test_repeated_single_layout_builds_one_generated_seed_queue(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            arguments = smoke.build_parser().parse_args(
                [
                    str(smoke.REPOSITORY / "tests/layouts/marksman-vs-arclight.yaml"),
                    "--repeat",
                    "5",
                    "--output-dir",
                    directory,
                    "--dry-run",
                ]
            )
            cases = smoke.cases_from_args(arguments)
        self.assertEqual(len(cases), 5)
        self.assertTrue(all(case.seed is None for case in cases))
        self.assertEqual(
            [case.output.name for case in cases],
            [f"marksman-vs-arclight-{index:02d}.native.mcfr" for index in range(1, 6)],
        )

    def test_smoke_and_exact_case_filters_keep_manifest_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            arguments = smoke.build_parser().parse_args(
                ["--batch", "--output-dir", directory, "--smoke-only", "--dry-run"]
            )
            smoke_cases = smoke.cases_from_args(arguments)
            arguments = smoke.build_parser().parse_args(
                [
                    "--batch",
                    "--output-dir",
                    directory,
                    "--case",
                    "stormcaller-vs-self-01",
                    "--case",
                    "marksman-vs-arclight",
                    "--dry-run",
                ]
            )
            selected_cases = smoke.cases_from_args(arguments)
        self.assertEqual(len(smoke_cases), 14)
        self.assertEqual(
            [case.name for case in selected_cases],
            ["marksman-vs-arclight", "stormcaller-vs-self-01"],
        )

    def test_dry_run_does_not_create_output_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output_dir = Path(directory) / "preview-only"
            with contextlib.redirect_stdout(io.StringIO()) as stdout:
                result = smoke.main(
                    [
                        "--batch",
                        "--smoke-only",
                        "--output-dir",
                        str(output_dir),
                        "--dry-run",
                    ]
                )
            self.assertEqual(result, 0)
            self.assertFalse(output_dir.exists())
            self.assertIn("queue: 14 capture(s)", stdout.getvalue())

    def test_existing_output_is_rejected_before_capture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "marksman-vs-arclight.native.mcfr"
            output.touch()
            arguments = smoke.build_parser().parse_args(
                [
                    "--batch",
                    "--output-dir",
                    directory,
                    "--case",
                    "marksman-vs-arclight",
                    "--dry-run",
                ]
            )
            with self.assertRaisesRegex(smoke.SmokeFailure, "refusing to overwrite"):
                smoke.cases_from_args(arguments)


if __name__ == "__main__":
    unittest.main()
