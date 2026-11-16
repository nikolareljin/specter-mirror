import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PublicationHookTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="publication-hook-test-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        shutil.copytree(ROOT / ".githooks", self.repo / ".githooks")

    def hook(self, name, *args, data=""):
        return subprocess.run(
            ["bash", str(self.repo / ".githooks" / name), *args],
            cwd=self.repo, input=data, text=True, capture_output=True,
        )

    def gate(self, source, executable=True):
        path = self.repo / "gate with spaces"
        path.write_text("#!/usr/bin/env bash\n" + source + "\n")
        if executable:
            path.chmod(0o755)
        subprocess.run(
            ["git", "config", "publication.gate", str(path)], cwd=self.repo, check=True,
        )

    def test_missing_configuration_blocks_both_hooks(self):
        for name in ("pre-commit", "pre-push"):
            with self.subTest(name=name):
                self.assertEqual(self.hook(name).returncode, 1)

    def test_nonexecutable_gate_blocks_both_hooks(self):
        self.gate("exit 0", executable=False)
        for name in ("pre-commit", "pre-push"):
            with self.subTest(name=name):
                self.assertEqual(self.hook(name).returncode, 1)

    def test_gate_failure_blocks_both_hooks(self):
        self.gate("exit 7")
        for name in ("pre-commit", "pre-push"):
            with self.subTest(name=name):
                self.assertEqual(self.hook(name).returncode, 7)

    def test_commit_requests_staged_inspection(self):
        self.gate('[[ "$#" == 1 && "$1" == staged ]]')
        self.assertEqual(self.hook("pre-commit").returncode, 0)

    def test_push_preserves_remote_arguments_and_ref_input(self):
        self.gate('[[ "$#" == 3 && "$1" == history && "$2" == upstream && "$3" == target ]] || exit 9\n'
                  '[[ "$(cat)" == "ref-input" ]]')
        self.assertEqual(
            self.hook("pre-push", "upstream", "target", data="ref-input\n").returncode, 0,
        )


if __name__ == "__main__":
    unittest.main()
