import importlib.util
import subprocess
import tempfile
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

    def test_oversized_comment_is_refused(self):
        text = "#" + "x" * 3_000_000 + "\n* @maintainer\n"
        self.assertFalse(module.valid_default_owner(text, "maintainer"))

    def test_size_limit_uses_utf8_bytes(self):
        text = "#" + "\u00e9" * 1_500_000 + "\n* @maintainer\n"
        self.assertLess(len(text), 3_000_000)
        self.assertFalse(module.valid_default_owner(text, "maintainer"))

    def test_file_below_size_limit_is_accepted(self):
        suffix = "\n* @maintainer\n"
        text = "#" + "x" * (3_000_000 - len(suffix) - 2) + suffix
        self.assertEqual(len(text.encode("utf-8")), 2_999_999)
        self.assertTrue(module.valid_default_owner(text, "maintainer"))

    def test_file_at_size_limit_is_refused(self):
        suffix = "\n* @maintainer\n"
        text = "#" + "x" * (3_000_000 - len(suffix) - 1) + suffix
        self.assertEqual(len(text.encode("utf-8")), 3_000_000)
        self.assertFalse(module.valid_default_owner(text, "maintainer"))

    def test_cli_measures_crlf_file_before_newline_conversion(self):
        suffix = b"* @maintainer\r\n"
        remaining = 3_000_000 - len(suffix)
        text = b"#\r\n" * (remaining // 3) + b" " * (remaining % 3) + suffix
        self.assertEqual(len(text), 3_000_000)
        with tempfile.TemporaryDirectory(prefix="codeowners-size-test-") as name:
            target = Path(name) / "CODEOWNERS"
            target.write_bytes(text)
            result = subprocess.run(
                ["python3", str(path), str(target), "--owner", "maintainer"],
                capture_output=True, text=True,
            )
        self.assertEqual(result.returncode, 1)


if __name__ == "__main__":
    unittest.main()
