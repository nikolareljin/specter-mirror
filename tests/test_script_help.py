import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SHELL_SCRIPTS = (
    "dev", "check", "demo", "scripts/discover_hardware.sh", "scripts/pages.sh",
    ".githooks/pre-commit", ".githooks/pre-push",
) + tuple(str(path.relative_to(ROOT)) for folder in ("scripts/appliance", "vm")
          for path in sorted((ROOT / folder).glob("*.sh")))


class ScriptHelpTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="script-help-test-")
        self.addCleanup(self.temp.cleanup)
        directory = Path(self.temp.name)
        for command in ("bash", "sed", "dirname"):
            (directory / command).symlink_to(shutil.which(command))
        self.env = os.environ.copy()
        self.env["PATH"] = str(directory)

    def invoke(self, script, *arguments):
        return subprocess.run(
            ["bash", str(ROOT / script), *arguments], cwd=ROOT,
            env=self.env, text=True, capture_output=True, timeout=5,
        )

    def test_help_has_examples_and_exit_codes_without_development_tools(self):
        for script in SHELL_SCRIPTS:
            for flag in ("-h", "--help"):
                with self.subTest(script=script, flag=flag):
                    result = self.invoke(script, flag)
                    self.assertEqual(result.returncode, 0)
                    for section in ("SCRIPT:", "DESCRIPTION:", "USAGE:",
                                    "PARAMETERS:", "EXAMPLES:", "EXIT_CODES:"):
                        self.assertIn(section, result.stdout)
                    self.assertEqual(result.stderr, "")

    def test_unknown_user_script_options_fail_before_running_checks(self):
        (Path(self.temp.name) / "python3").symlink_to(shutil.which("python3"))
        for script in (path for path in SHELL_SCRIPTS if not path.startswith(".githooks/")):
            with self.subTest(script=script):
                result = self.invoke(script, "--unknown-option")
                self.assertEqual(result.returncode, 2)
                self.assertIn("usage", result.stderr.lower())

    def test_help_rejects_trailing_arguments(self):
        for script in (path for path in SHELL_SCRIPTS if not path.startswith(".githooks/")):
            with self.subTest(script=script):
                self.assertEqual(self.invoke(script, "--help", "extra").returncode, 2)

    def test_help_reads_the_header_instead_of_a_separate_copy(self):
        for script in SHELL_SCRIPTS:
            with self.subTest(script=script):
                target = Path(self.temp.name) / Path(script).name
                source = (ROOT / script).read_text()
                target.write_text(source.replace(
                    "# EXAMPLES:\n", "# EXAMPLES:\n#   header-only-example\n", 1,
                ))
                result = self.invoke(str(target), "--help")
                self.assertEqual(result.returncode, 0)
                self.assertIn("header-only-example", result.stdout)


if __name__ == "__main__":
    unittest.main()
