import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("iso_builder", ROOT / "vm/iso.py")
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class BuilderIdentityTests(unittest.TestCase):
    def test_build_and_provenance_use_the_same_resolved_image_id(self):
        image_id = "sha256:" + "a" * 64
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            base = directory / "base.iso"
            base.touch()
            bundle = directory / "bundle"
            (bundle / "packages").mkdir(parents=True)
            args = argparse.Namespace(base=base, base_sha256="b" * 64,
                                      bundle=bundle, bundle_sha256="c" * 64,
                                      output=directory / "output.iso", allow_proof=True,
                                      appliance=True, builder_image="builder:local")
            provenance = {}

            def run(command, **_options):
                if command[:2] == ["docker", "run"]:
                    self.assertIn(image_id, command)
                    self.assertNotIn(args.builder_image, command)
                    mount = next(value for value in command if value.endswith(",dst=/work"))
                    work = Path(mount.removeprefix("type=bind,src=").removesuffix(",dst=/work"))
                    (work / "root-provenance.json").write_text("{}")
                if "-outdev" in command:
                    Path(command[command.index("-outdev") + 1]).touch()
                    toolkit = Path(command[command.index("-map") + 1])
                    provenance.update(json.loads((toolkit / "PROVENANCE.json").read_text()))
                return subprocess.CompletedProcess(command, 0, "El Torito boot img", "")

            with (mock.patch.object(builder.appliance, "digest", return_value=args.base_sha256),
                  mock.patch.object(builder.appliance, "check_bundle"),
                  mock.patch.object(builder.appliance, "regular_tree", return_value={}),
                  mock.patch.object(builder.shutil, "copytree"),
                  mock.patch.object(builder.shutil, "copy2"),
                  mock.patch.object(builder.subprocess, "run", side_effect=run),
                  mock.patch.object(builder.subprocess, "check_output",
                                    side_effect=[image_id + "\n", "d" * 32 + "  linuxfs\n"]) as output):
                builder.build(args)
            self.assertEqual(output.call_count, 2)
            self.assertEqual(provenance["root_build"]["builder_image_id"], image_id)
            self.assertTrue(args.output.is_file())


if __name__ == "__main__":
    unittest.main()
