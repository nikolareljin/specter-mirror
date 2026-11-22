import hashlib
import fcntl
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("qemu_runner", ROOT / "vm/qemu.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class QemuTests(unittest.TestCase):
    def invoke(self, temporary, *extra, recorder=None):
        directory = Path(temporary)
        iso = directory / "base.iso"
        iso.write_bytes(b"fixture")
        checksum = hashlib.sha256(iso.read_bytes()).hexdigest()
        arguments = ["qemu.py", "--iso", str(iso), "--iso-sha256", checksum,
                     "--directory", str(directory / "guest"), *extra]
        fake = recorder if recorder is not None else mock.Mock()
        with mock.patch("sys.argv", arguments), mock.patch.object(runner.subprocess, "run", fake) as run:
            runner.main()
            return [call.args[0] for call in run.call_args_list]

    def test_default_guest_has_no_network_and_only_a_new_virtual_disk(self):
        with tempfile.TemporaryDirectory() as temporary:
            commands = self.invoke(temporary)
            self.assertEqual(commands[0][:4], ["qemu-img", "create", "-f", "qcow2"])
            command = commands[1]
            self.assertEqual(command[command.index("-nic") + 1], "none")
            self.assertNotIn("-virtfs", command)
            self.assertNotIn("-fsdev", command)
            self.assertIn("format=qcow2", command[command.index("-drive") + 1])

    def test_existing_directory_is_not_reused_without_resume(self):
        with tempfile.TemporaryDirectory() as temporary:
            (Path(temporary) / "guest").mkdir()
            with self.assertRaises(SystemExit) as result:
                self.invoke(temporary)
            self.assertEqual(result.exception.code, 2)

    def test_resume_never_recreates_the_disk(self):
        with tempfile.TemporaryDirectory() as temporary:
            guest = Path(temporary) / "guest"
            guest.mkdir()
            (guest / "appliance.qcow2").write_bytes(b"keep")
            commands = self.invoke(temporary, "--resume", "--headless")
            self.assertEqual(len(commands), 1)
            self.assertEqual(commands[0][commands[0].index("-boot") + 1], "c")
            self.assertEqual((guest / "appliance.qcow2").read_bytes(), b"keep")

    def test_active_controller_is_refused_before_any_qemu_command(self):
        with tempfile.TemporaryDirectory() as temporary:
            guest = Path(temporary) / "guest"
            guest.mkdir()
            (guest / "appliance.qcow2").write_bytes(b"keep")
            descriptor = runner.os.open(guest, runner.os.O_RDONLY | runner.os.O_DIRECTORY)
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                run = mock.Mock(side_effect=AssertionError("VM commands must not run before the lock"))
                with self.assertRaises(SystemExit) as result:
                    self.invoke(temporary, "--resume", "--headless", recorder=run)
                self.assertEqual(result.exception.code, 2)
                run.assert_not_called()
            finally:
                runner.os.close(descriptor)


if __name__ == "__main__":
    unittest.main()
