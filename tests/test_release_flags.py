import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class ReleaseFlagsTests(unittest.TestCase):
    def invoke(self, variables):
        with tempfile.TemporaryDirectory(prefix="release-flags-") as temporary:
            directory = Path(temporary)
            rustc = directory / "rustc"
            rustc.touch()
            scripts = {
                "rustup": "#!/bin/sh\nprintf '%s\\n' " + shlex.quote(str(rustc)) + "\n",
                "cargo": "#!/usr/bin/env python3\nimport os,json\nfrom pathlib import Path\n"
                         "Path(os.environ['SPECTER_FLAGS_OUTPUT']).write_text(json.dumps(os.environ['CARGO_ENCODED_RUSTFLAGS']))\n",
            }
            for name, text in scripts.items():
                path = directory / name
                path.write_text(text)
                path.chmod(0o755)
            output = directory / "flags.json"
            environment = os.environ.copy()
            for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"):
                environment.pop(name, None)
            environment.update(variables)
            environment.update(PATH=str(directory) + os.pathsep + environment["PATH"],
                               SPECTER_FLAGS_OUTPUT=str(output))
            subprocess.run(["bash", str(ROOT / "dev"), "build-i686"], check=True,
                           env=environment, capture_output=True)
            return json.loads(output.read_text()).split("\x1f")

    def test_explicit_empty_encoded_flags_override_plain_flags(self):
        flags = self.invoke({"CARGO_ENCODED_RUSTFLAGS": "", "RUSTFLAGS": "--cfg ignored"})
        self.assertNotIn("ignored", flags)
        self.assertNotIn("--cfg", flags)

    def test_encoded_flags_keep_argument_boundaries(self):
        flags = self.invoke({"CARGO_ENCODED_RUSTFLAGS": "--cfg\x1fvalue=\"two words\""})
        self.assertEqual(flags[:2], ["--cfg", 'value="two words"'])

    def test_plain_flags_are_kept_before_release_mapping(self):
        flags = self.invoke({"RUSTFLAGS": "-C opt-level=2"})
        self.assertEqual(flags[:2], ["-C", "opt-level=2"])
        self.assertTrue(any(flag.endswith("=/src/specter-mirror") for flag in flags))


if __name__ == "__main__":
    unittest.main()
