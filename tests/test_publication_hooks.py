import os
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
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True, env=self.env)
        shutil.copytree(ROOT / ".githooks", self.repo / ".githooks")
        subprocess.run(
            ["git", "config", "core.hooksPath", ".githooks"], cwd=self.repo, check=True, env=self.env,
        )

    def hook(self, name, *args, data="", env=None):
        stdin = self.repo / "hook-input"
        stdin.write_text(data)
        return subprocess.run(
            ["git", "hook", "run", "--to-stdin=" + str(stdin), name, "--", *args],
            cwd=self.repo, text=True, capture_output=True, env=env or self.env,
        )

    def gate(self, source, executable=True):
        path = self.repo / "gate with spaces"
        path.write_text("#!/usr/bin/env bash\n" + source + "\n")
        if executable:
            path.chmod(0o755)
        subprocess.run(
            ["git", "config", "publication.gate", str(path)], cwd=self.repo, check=True, env=self.env,
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

    def test_relative_gate_cannot_select_another_program_from_path(self):
        intended = self.repo / "publication-check"
        intended.write_text("#!/bin/sh\nexit 7\n")
        intended.chmod(0o755)
        directory = self.repo / "bin"
        directory.mkdir()
        replacement = directory / intended.name
        replacement.write_text("#!/bin/sh\nexit 0\n")
        replacement.chmod(0o755)
        subprocess.run(
            ["git", "config", "publication.gate", intended.name], cwd=self.repo, check=True, env=self.env,
        )
        env = self.env.copy()
        env["PATH"] = str(directory) + os.pathsep + env["PATH"]
        for name in ("pre-commit", "pre-push"):
            with self.subTest(name=name):
                self.assertEqual(self.hook(name, env=env).returncode, 1)

    def test_directory_is_not_an_executable_gate(self):
        subprocess.run(
            ["git", "config", "publication.gate", str(self.repo)], cwd=self.repo, check=True, env=self.env,
        )
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
