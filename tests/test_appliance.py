import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("appliance", ROOT / "scripts/appliance/manage.py")
app = importlib.util.module_from_spec(spec)
spec.loader.exec_module(app)


class ApplianceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="appliance-test-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.binary = self.directory / "proof"
        header = bytearray(52)
        header[:7] = b"\x7fELF\x01\x01\x01"
        struct.pack_into("<HH", header, 16, 3, 3)
        self.binary.write_bytes(header)
        self.bundle = self.directory / "bundle"
        app.build(argparse.Namespace(binary=self.binary, output=self.bundle))
        self.sha = app.digest(self.bundle / "SHA256SUMS")
        self.root = self.directory / "target"
        (self.root / "etc").mkdir(parents=True)
        (self.root / "etc/os-release").write_text('ID="antiX"\n')
        (self.root / "etc/passwd").write_text("mirror:x:1000:1000::/home/mirror:/bin/bash\n")
        self.startup = self.root / "home/mirror/.desktop-session/startup"
        self.startup.parent.mkdir(parents=True)
        self.startup.write_text("#!/bin/sh\necho existing-command\n")
        self.args = argparse.Namespace(root=self.root, bundle=self.bundle, sha256=self.sha,
                                       user="mirror", profile="virtualbox", allow_proof=True,
                                       offline=True, install_deps=False, session="desktop", no_autologin=False)

    def snapshot(self):
        return {str(path.relative_to(self.root)): path.read_bytes()
                for path in self.root.rglob("*") if path.is_file()}

    def test_repeat_install_preserves_config_and_unrelated_startup(self):
        app.install(self.args)
        config = self.root / "etc/specter-mirror/config.json"
        value = json.loads(config.read_text())
        value["profile"] = "nb205"
        config.write_text(json.dumps(value))
        before = self.snapshot()
        app.install(self.args)
        self.assertEqual(before, self.snapshot())
        self.assertEqual(self.startup.read_text().count(app.BEGIN), 1)
        app.verify(self.args)
        state = self.root / "home/mirror/.local/state/specter-mirror"
        state.mkdir(parents=True)
        (state / "saved-user-data").write_text("keep")
        app.uninstall(self.args)
        self.assertFalse((self.root / app.APP).exists())
        self.assertEqual(self.startup.read_text(), "#!/bin/sh\necho existing-command\n")
        self.assertEqual((state / "saved-user-data").read_text(), "keep")

    def test_corruption_and_extra_files_refuse_before_writes(self):
        before = self.snapshot()
        (self.bundle / "bin/spook-kiosk").write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            app.install(self.args)
        self.assertEqual(before, self.snapshot())

    def test_manifest_pin_is_required(self):
        with self.assertRaises(ValueError):
            app.check_bundle(self.bundle, "0" * 64)
        (self.bundle / "unlisted").write_text("not in the manifest")
        with self.assertRaisesRegex(ValueError, "unlisted"):
            app.check_bundle(self.bundle, self.sha)

    def test_symlink_payload_and_parent_are_refused(self):
        (self.bundle / "link").symlink_to(self.binary)
        with self.assertRaisesRegex(ValueError, "link"):
            app.install(self.args)
        (self.bundle / "link").unlink()
        self.startup.unlink()
        self.startup.symlink_to(self.directory / "outside")
        with self.assertRaisesRegex(ValueError, "symbolic link"):
            app.install(self.args)
        self.assertFalse((self.directory / "outside").exists())

    def test_proof_offline_and_account_gates(self):
        self.args.allow_proof = False
        with self.assertRaisesRegex(ValueError, "allow-proof"):
            app.install(self.args)
        self.args.allow_proof = True
        self.args.install_deps = True
        with self.assertRaisesRegex(ValueError, "offline"):
            app.install(self.args)
        self.args.install_deps = False
        self.args.user = "root"
        with self.assertRaisesRegex(ValueError, "non-root"):
            app.install(self.args)

    def test_wrong_architecture_is_refused(self):
        self.binary.write_bytes(b"\x7fELF\x02" + bytes(100))
        with self.assertRaisesRegex(ValueError, "ELF32"):
            app.build(argparse.Namespace(binary=self.binary, output=self.directory / "wrong"))

    def test_malformed_startup_is_not_overwritten(self):
        self.startup.write_text(app.BEGIN + "\n")
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "Malformed"):
            app.install(self.args)
        self.assertEqual(before, self.snapshot())

    def test_failed_startup_write_restores_previous_application(self):
        app.install(self.args)
        before = self.snapshot()
        with mock.patch.object(app, "atomic_text", side_effect=OSError("write failed")):
            with self.assertRaises(OSError):
                app.install(self.args)
        self.assertEqual(before, self.snapshot())

    def test_verify_detects_changed_installed_bytes(self):
        app.install(self.args)
        (self.root / app.APP / "bin/spook-kiosk").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            app.verify(self.args)

    def test_kiosk_default_login_and_uninstall_restore_original_display_manager(self):
        self.args.session = "kiosk"
        slim = self.root / "etc/slim.conf"
        original = "# original\nlogin_cmd setsid /usr/local/bin/desktop-session %session\nsessions icewm\n"
        slim.write_text(original)
        app.install(self.args)
        self.assertIn(app.SLIM_LOGIN, slim.read_text())
        self.assertIn("auto_login yes\ndefault_user mirror\n", slim.read_text())
        self.assertNotIn(app.BEGIN, self.startup.read_text())
        before = self.snapshot()
        app.install(self.args)
        self.assertEqual(before, self.snapshot())
        app.verify(self.args)
        app.uninstall(self.args)
        self.assertEqual(slim.read_text(), original)

    def test_custom_display_manager_login_is_not_replaced(self):
        self.args.session = "kiosk"
        (self.root / "etc/slim.conf").write_text("login_cmd custom-login\n")
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "Unsupported SLiM"):
            app.install(self.args)
        self.assertEqual(before, self.snapshot())

    def test_operator_display_manager_edits_are_preserved(self):
        self.args.session = "kiosk"
        slim = self.root / "etc/slim.conf"
        slim.write_text("login_cmd setsid /usr/local/bin/desktop-session %session\n")
        app.install(self.args)
        slim.write_text(slim.read_text() + "# later operator change\n")
        before = self.snapshot()
        with self.assertRaisesRegex(ValueError, "changed"):
            app.uninstall(self.args)
        self.assertEqual(before, self.snapshot())

    def test_failed_login_write_rolls_back_files_and_application(self):
        self.args.session = "kiosk"
        slim = self.root / "etc/slim.conf"
        slim.write_text("login_cmd setsid /usr/local/bin/desktop-session %session\n")
        before = self.snapshot()
        original_write = app.atomic_text

        def fail(path, text, mode=0o644):
            if path.name == "slim.conf.installed":
                raise OSError("injected write failure")
            original_write(path, text, mode)

        with mock.patch.object(app, "atomic_text", side_effect=fail):
            with self.assertRaises(OSError):
                app.install(self.args)
        self.assertEqual(before, self.snapshot())
        self.assertFalse((self.root / app.APP).exists())

    @unittest.skipUnless(shutil.which("xorriso"), "xorriso is not installed")
    def test_iso_replay_preserves_boot_image_and_adds_verified_payload(self):
        source = self.directory / "iso-root"
        (source / "boot").mkdir(parents=True)
        boot = source / "boot/test.img"
        boot.write_bytes(bytes(2048))
        base = self.directory / "base.iso"
        subprocess.run(["xorriso", "-as", "mkisofs", "-b", "boot/test.img", "-no-emul-boot",
                        "-o", str(base), str(source)], check=True, capture_output=True)
        output = self.directory / "preview.iso"
        result = subprocess.run(["python3", str(ROOT / "vm/iso.py"), "--base", str(base),
                                 "--base-sha256", app.digest(base), "--bundle", str(self.bundle),
                                 "--bundle-sha256", self.sha, "--output", str(output), "--allow-proof"],
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        extracted = self.directory / "preserved.img"
        subprocess.run(["xorriso", "-indev", str(output), "-osirrox", "on", "-extract",
                        "/boot/test.img", str(extracted)], check=True, capture_output=True)
        self.assertEqual(extracted.read_bytes(), boot.read_bytes())
        report = subprocess.run(["xorriso", "-indev", str(output), "-report_el_torito", "plain"],
                                capture_output=True, text=True, check=True)
        self.assertIn("El Torito boot img", report.stdout + report.stderr)


if __name__ == "__main__":
    unittest.main()
