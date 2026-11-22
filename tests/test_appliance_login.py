import importlib.util
from pathlib import Path
import signal
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("appliance_login", ROOT / "scripts/appliance/login.py")
login = importlib.util.module_from_spec(spec)
spec.loader.exec_module(login)


class LoginTests(unittest.TestCase):
    def exercise(self, interrupt):
        manager = mock.Mock()
        manager.poll.return_value = None
        installer = mock.Mock()
        installer.poll.return_value = None
        runner = mock.Mock()
        runner.poll.return_value = 0
        handlers = {}

        def register(number, handler):
            handlers[number] = handler

        def sleep(_seconds):
            if interrupt:
                handlers[signal.SIGTERM](signal.SIGTERM, None)
            if interrupt is not None:
                manager.poll.return_value = 0
            installer.poll.return_value = 0

        with (mock.patch.object(login.Path, "read_text", return_value='{"user":"mirror"}'),
              mock.patch.object(login.Path, "is_file", return_value=True),
              mock.patch.object(login.os, "geteuid", return_value=1000),
              mock.patch.object(login.os.path, "ismount", return_value=True),
              mock.patch.object(login.pwd, "getpwuid", return_value=mock.Mock(pw_name="mirror")),
              mock.patch.object(login.signal, "signal", side_effect=register),
              mock.patch.object(login.time, "sleep", side_effect=sleep),
              mock.patch.object(login.subprocess, "Popen", side_effect=[manager, installer, runner]) as popen):
            login.main()
        self.assertEqual(popen.call_count, 3 if interrupt is None else 2)

    def test_shutdown_during_installer_does_not_launch_application(self):
        self.exercise(interrupt=True)

    def test_window_manager_exit_during_installer_does_not_launch_application(self):
        self.exercise(interrupt=False)

    def test_normal_installer_completion_starts_application(self):
        self.exercise(interrupt=None)


if __name__ == "__main__":
    unittest.main()
