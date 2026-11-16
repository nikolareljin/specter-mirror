import importlib.util
import unittest
from pathlib import Path


path = Path(__file__).resolve().parents[1] / "scripts/check_codeowners.py"
spec = importlib.util.spec_from_file_location("check_codeowners", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CodeOwnerTests(unittest.TestCase):
    def test_template_with_comments_is_accepted(self):
        self.assertTrue(module.valid_default_owner("# Default owner\n\n* @maintainer\n", "maintainer"))

    def test_later_override_is_refused(self):
        self.assertFalse(module.valid_default_owner("* @maintainer\n* @different-owner\n", "maintainer"))

    def test_narrower_override_is_refused(self):
        self.assertFalse(module.valid_default_owner("* @maintainer\n/scripts/ @different-owner\n", "maintainer"))

    def test_missing_default_is_refused(self):
        self.assertFalse(module.valid_default_owner("# Default owner\n", "maintainer"))

    def test_additional_approver_is_refused(self):
        self.assertFalse(module.valid_default_owner("* @maintainer @different-owner\n", "maintainer"))


if __name__ == "__main__":
    unittest.main()
