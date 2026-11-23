import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("pages", ROOT / "scripts/check_pages.py")
pages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pages)


class PagesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="pages-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.site = self.root / "site"
        (self.site / "assets").mkdir(parents=True)
        html = '<script src="/specter-mirror/assets/app.js"></script><a href="./about.html">About</a>'
        for name in ("index.html", "about.html"):
            (self.site / name).write_text(html)
        (self.site / "favicon.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg"/>')
        (self.site / "assets/app.js").write_text("export const value = 1;")
        (self.site / "assets/engine.wasm").write_bytes(b"\0asm\x01\0\0\0")
        self.manifest = self.root / "manifest.json"
        self.manifest.write_text(json.dumps(pages.inventory(self.site)))

    def test_complete_tree_matches_and_byte_changes_are_refused(self):
        self.assertEqual(pages.verify(self.site, self.manifest), pages.inventory(self.site))
        (self.site / "assets/app.js").write_text("export const value = 2;")
        with self.assertRaisesRegex(ValueError, "differs"):
            pages.verify(self.site, self.manifest)

    def test_extra_files_and_directories_are_refused(self):
        for name in ("notes.md", "assets/source.map", "assets/report.json"):
            with self.subTest(name=name):
                path = self.site / name
                path.write_text("unexpected")
                with self.assertRaises(ValueError):
                    pages.inventory(self.site)
                path.unlink()
        (self.site / "extra").mkdir()
        with self.assertRaises(ValueError):
            pages.inventory(self.site)

    def test_symlinks_are_refused(self):
        target = self.site / "assets/linked.js"
        target.symlink_to(self.site / "assets/app.js")
        with self.assertRaisesRegex(ValueError, "link"):
            pages.inventory(self.site)
        target.unlink()
        linked = self.root / "linked"
        linked.symlink_to(self.site, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "linked"):
            pages.inventory(linked)

    def test_missing_wasm_and_entry_documents_are_refused(self):
        (self.site / "assets/engine.wasm").unlink()
        with self.assertRaisesRegex(ValueError, "WASM"):
            pages.inventory(self.site)
        (self.site / "about.html").unlink()
        with self.assertRaisesRegex(ValueError, "entry"):
            pages.inventory(self.site)

    def test_missing_or_wrong_base_asset_urls_are_refused(self):
        for target in ("/assets/app.js", "/specter-mirror/assets/missing.js", "../app.js"):
            with self.subTest(target=target):
                (self.site / "index.html").write_text(
                    f'<script src="/specter-mirror/assets/app.js"></script><script src="{target}"></script>'
                )
                with self.assertRaises(ValueError):
                    pages.inventory(self.site)

    def test_invalid_wasm_content_is_refused(self):
        (self.site / "assets/engine.wasm").write_bytes(b"not a wasm module")
        with self.assertRaisesRegex(ValueError, "WASM header"):
            pages.inventory(self.site)

    def test_oversized_files_are_refused_before_reading(self):
        with mock.patch.object(pages, "LIMIT", 5):
            with self.assertRaisesRegex(ValueError, "oversized"):
                pages.inventory(self.site)

    def test_ci_cannot_replace_the_reviewed_manifest(self):
        output = io.StringIO()
        before = self.manifest.read_bytes()
        with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "true"}), mock.patch(
            "sys.argv", ["check_pages.py", "--record", "--manifest", str(self.manifest)]
        ), contextlib.redirect_stdout(output):
            self.assertEqual(pages.main(), 1)
        self.assertEqual(before, self.manifest.read_bytes())
        self.assertNotIn("Local diagnostics:", output.getvalue())

    def test_ci_does_not_publish_diagnostics_when_inspection_fails(self):
        output = io.StringIO()
        with mock.patch.dict(os.environ, {"GITHUB_ACTIONS": "true", "GITHUB_SHA": "a" * 40}), mock.patch(
            "sys.argv", ["check_pages.py", "--site", str(self.site), "--manifest",
                         str(self.manifest), "--helpers", str(self.root)]
        ), mock.patch.object(pages, "verify", return_value=pages.inventory(self.site)), \
                mock.patch.object(pages, "inspect", side_effect=ValueError("LOCAL_DIAGNOSTIC_SENTINEL")), \
                contextlib.redirect_stdout(output):
            self.assertEqual(pages.main(), 1)
        self.assertNotIn("LOCAL_DIAGNOSTIC_SENTINEL", output.getvalue())
        self.assertNotIn("Local diagnostics:", output.getvalue())

    def test_ci_refuses_output_even_if_the_working_manifest_is_replaced(self):
        committed = self.manifest.read_text()
        (self.site / "assets/app.js").write_text("export const value = 9;")
        self.manifest.write_text(json.dumps(pages.inventory(self.site)))
        pages.verify(self.site, self.manifest)
        with mock.patch.object(subprocess, "check_output", return_value=committed):
            with self.assertRaisesRegex(ValueError, "differs"):
                pages.verify(self.site, self.manifest, "a" * 40)

    def test_missing_tools_refuse_inspection(self):
        helpers = self.root / "helpers"
        (helpers / "scripts").mkdir(parents=True)
        for name in ("check_publication.sh", "check_private_names.sh"):
            (helpers / "scripts" / name).write_text("exit 0\n")
        with mock.patch.object(subprocess, "check_output", side_effect=[pages.HELPERS_REVISION, ""]), \
                mock.patch.object(pages.shutil, "which", return_value=None), \
                mock.patch.dict(os.environ, {"SPECTER_GITLEAKS": "", "SPECTER_FOXGUARD": ""}):
            with self.assertRaisesRegex(ValueError, "tool is missing"):
                pages.inspect(self.site, helpers, self.root / "reports")

    def test_environment_cannot_select_another_executable(self):
        helpers = self.root / "helpers"
        (helpers / "scripts").mkdir(parents=True)
        reports = self.root / "reports"
        reports.mkdir()
        for name in ("check_publication.sh", "check_private_names.sh"):
            (helpers / "scripts" / name).write_text("exit 0\n")
        versions = [pages.HELPERS_REVISION, "", "gitleaks version 8.30.1", "foxguard 0.14.1"]
        with mock.patch.object(subprocess, "check_output", side_effect=versions) as version_calls, \
                mock.patch.object(subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as runs, \
                mock.patch.object(pages.shutil, "which", return_value="present"), \
                mock.patch.dict(os.environ, {"SPECTER_GITLEAKS": "unexpected-program", "SPECTER_FOXGUARD": "unexpected-program"}):
            pages.inspect(self.site, helpers, reports)
        for call in [*version_calls.call_args_list, *runs.call_args_list]:
            self.assertIn(call.args[0][0], {"git", "gitleaks", "foxguard", "bash"})
            self.assertNotIn("unexpected-program", call.args[0])
