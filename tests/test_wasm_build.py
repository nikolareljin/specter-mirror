import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class WasmBuild(unittest.TestCase):
    def run_build(self, flags=None, encoded=None, args=()):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "project with spaces"
            (root / "scripts").mkdir(parents=True)
            (root / "web").mkdir()
            script = root / "scripts/build_wasm.sh"
            shutil.copy2(Path(__file__).resolve().parents[1] / "scripts/build_wasm.sh", script)
            binary = root / "bin"
            binary.mkdir()
            capture = root / "capture"
            pack = binary / "wasm-pack"
            pack.write_text('#!/bin/sh\nprintf "%s\\n" "$PWD" "$CARGO_ENCODED_RUSTFLAGS" "$@" > "$CAPTURE"\n')
            pack.chmod(0o755)
            env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ["PATH"],
                       HOME=str(root / "home with spaces"), CARGO_HOME=str(root / "cargo cache"),
                       CAPTURE=str(capture))
            env.pop("CARGO_ENCODED_RUSTFLAGS", None)
            env.pop("RUSTFLAGS", None)
            if flags is not None:
                env["RUSTFLAGS"] = flags
            if encoded is not None:
                env["CARGO_ENCODED_RUSTFLAGS"] = encoded
            result = subprocess.run(["bash", str(script), *args], env=env,
                                    capture_output=True, text=True, cwd=directory)
            lines = capture.read_text().splitlines() if capture.exists() else []
            return result, lines, root

    def test_source_mappings_preserve_spaces_and_enable_detector(self):
        result, lines, root = self.run_build(flags="--cfg existing")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(lines[0], str(root / "web"))
        self.assertEqual(lines[1].split("\x1f"), [
            "--cfg", "existing",
            f"--remap-path-prefix={root / 'home with spaces'}=/build/home",
            f"--remap-path-prefix={root / 'cargo cache'}=/build/cargo",
            f"--remap-path-prefix={root}=/build/specter-mirror",
        ])
        self.assertEqual(lines[-3:], ["--", "--features", "face-detection"])

    def test_encoded_flags_take_precedence_including_explicit_empty(self):
        for encoded in ["", "--cfg\x1fvalue=\"with spaces\""]:
            result, lines, _ = self.run_build(flags="--cfg ignored", encoded=encoded)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("ignored", lines[1])
            if encoded:
                self.assertTrue(lines[1].startswith(encoded + "\x1f"))

    def test_help_and_invalid_arguments_do_not_build(self):
        for args, code in [(('-h',), 0), (('--help',), 0), (('invalid',), 2)]:
            result, lines, _ = self.run_build(args=args)
            self.assertEqual(result.returncode, code)
            self.assertEqual(lines, [])
            if not code:
                self.assertIn("EXAMPLES:", result.stdout)
