import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest


class AllTestCommand(unittest.TestCase):
    def run_suites(self, rust=0, web=0, python=0, toolchain=0):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            project = root / "project with spaces"
            project.mkdir()
            (project / "web").mkdir()
            shutil.copy2(Path(__file__).resolve().parents[1] / "dev", project / "dev")
            binary = root / "bin"
            binary.mkdir()
            log = root / "calls"
            commands = {
                "rustup": f"exit {toolchain}" if toolchain else f"printf '%s\\n' {shlex.quote(str(binary / 'rustc'))}",
                "cargo": f'echo rust >> "$SUITE_LOG"\nexit {rust}',
                "npm": f'echo web >> "$SUITE_LOG"\nexit {web}',
                "python3": f'echo python >> "$SUITE_LOG"\nexit {python}',
            }
            for command, body in commands.items():
                path = binary / command
                path.write_text("#!/bin/sh\n" + body + "\n")
                path.chmod(0o755)
            result = subprocess.run(
                ["bash", "project with spaces/dev", "all-test"], cwd=root,
                env=dict(os.environ, PATH=str(binary) + os.pathsep + os.environ["PATH"],
                         SUITE_LOG=str(log)), capture_output=True, text=True,
            )
            return result, log.read_text().splitlines()

    def test_success_runs_all_suites_from_another_directory(self):
        result, calls = self.run_suites()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ["rust", "web", "python"])

    def test_each_failure_preserves_later_suites_and_returns_nonzero(self):
        for failures in [(1, 0, 0), (0, 2, 0), (0, 0, 3), (1, 2, 3)]:
            with self.subTest(failures=failures):
                result, calls = self.run_suites(*failures)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(calls, ["rust", "web", "python"])
                for code, suite in zip(failures, ["rust workspace", "web", "python"]):
                    self.assertEqual(suite in result.stderr, bool(code))

    def test_missing_rust_toolchain_does_not_skip_web_and_python(self):
        result, calls = self.run_suites(toolchain=1)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(calls, ["web", "python"])
        self.assertIn("rust workspace", result.stderr)
