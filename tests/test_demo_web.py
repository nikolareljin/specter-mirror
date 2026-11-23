import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class DemoWebTests(unittest.TestCase):
    def test_web_selects_pinned_rust_and_finds_standard_cargo_tools(self):
        with tempfile.TemporaryDirectory(prefix="mirror demo ") as temporary:
            root = Path(temporary)
            shutil.copyfile(ROOT / "demo", root / "demo")
            for folder in ("bin", "toolchain", "cargo/bin", "web/node_modules/.bin"):
                (root / folder).mkdir(parents=True)
            scripts = {
                "bin/rustup": '#!/bin/sh\nprintf "%s\\n" "$TEST_COMPILER"\n',
                "cargo/bin/wasm-pack": "#!/bin/sh\nexit 0\n",
                "web/node_modules/.bin/vite": "#!/bin/sh\nexit 0\n",
                "bin/npm": '#!/usr/bin/python3\nimport json, os\nprint(json.dumps({k: os.environ[k] for k in ("RUSTC", "PATH")}))\n',
            }
            for name, content in scripts.items():
                path = root / name
                path.write_text(content)
                path.chmod(0o755)
            environment = dict(os.environ, PATH=f"{root / 'bin'}:/usr/bin:/bin",
                               CARGO_HOME=str(root / "cargo"),
                               TEST_COMPILER=str(root / "toolchain/rustc"))
            result = subprocess.run(["bash", str(root / "demo"), "web"],
                                    env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            selected = json.loads(result.stdout.splitlines()[-1])
            self.assertEqual(selected["RUSTC"], environment["TEST_COMPILER"])
            self.assertIn(str(root / "cargo/bin"), selected["PATH"].split(os.pathsep))
