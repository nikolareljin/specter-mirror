import hashlib
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]


class ArtworkTests(unittest.TestCase):
    def test_offline_sources_match_the_reviewed_object_set_and_file_hashes(self):
        directory = ROOT / "assets/artwork"
        sources = json.loads((directory / "sources.json").read_text())
        self.assertEqual(len(sources), 3)
        self.assertEqual({entry["object_id"] for entry in sources}, {435683, 334453, 435904})
        self.assertEqual({entry["file"] for entry in sources}, {"island.jpg", "monsters.jpg", "skull.jpg"})
        for entry in sources:
            with self.subTest(file=entry["file"]):
                content = (directory / entry["file"]).read_bytes()
                self.assertEqual(len(content), entry["bytes"])
                self.assertEqual(hashlib.sha256(content).hexdigest(), entry["sha256"])
                self.assertLessEqual(len(content), 2 * 1024 * 1024)
                self.assertEqual(entry["license"], "CC0-1.0")
                self.assertEqual(entry["object_url"], f'https://www.metmuseum.org/art/collection/search/{entry["object_id"]}')
                self.assertTrue(entry["image_url"].startswith("https://images.metmuseum.org/CRDImages/"))

    def test_native_bundle_includes_art_and_third_party_notices(self):
        manifest = json.loads((ROOT / "assets/manifest.json").read_text())["files"]
        expected = {"artwork/island.jpg", "artwork/monsters.jpg", "artwork/skull.jpg",
                    "artwork/sources.json", "models/SEETAFACE-LICENSE",
                    "licenses/rustface-0.1.7-LICENSE", "licenses/jpeg-decoder-0.3.2-LICENSE-MIT",
                    "licenses/jpeg-decoder-0.3.2-LICENSE-APACHE"}
        self.assertTrue(expected.issubset(manifest))
        for name in expected:
            self.assertTrue((ROOT / "assets" / name).is_file(), name)


if __name__ == "__main__":
    unittest.main()
