import configparser
import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("remaster", ROOT / "vm/remaster_root.py")
remaster = importlib.util.module_from_spec(spec)
spec.loader.exec_module(remaster)


class InstallerDefaultsTests(unittest.TestCase):
    def test_only_autologin_is_seeded_and_other_defaults_are_preserved(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "etc").mkdir()
            path = root / "etc/minstall.conf"
            path.write_text("[Localization]\nTimezone=UTC\n")
            remaster.installer_defaults(root)
            configuration = configparser.ConfigParser()
            configuration.read(path)
            self.assertEqual(configuration.get("User", "Autologin"), "true")
            self.assertEqual(configuration.get("Localization", "Timezone"), "UTC")
            self.assertEqual(set(configuration.sections()), {"User", "Localization"})
            self.assertFalse(configuration.has_section("Storage"))
            previous = path.read_bytes()
            remaster.installer_defaults(root)
            self.assertEqual(path.read_bytes(), previous)


if __name__ == "__main__":
    unittest.main()
