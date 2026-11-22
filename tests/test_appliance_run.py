import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("appliance_run", ROOT / "scripts/appliance/run.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class SessionTests(unittest.TestCase):
    def invoke(self, status, arguments=()):
        with tempfile.TemporaryDirectory(prefix="session-test-") as temporary:
            child = mock.Mock()
            child.poll.return_value = status
            child.wait.return_value = status
            with (mock.patch.object(runner.Path, "home", return_value=Path(temporary)),
                  mock.patch.object(runner.os, "geteuid", return_value=1000),
                  mock.patch.dict(runner.os.environ, {"DISPLAY": ":test"}),
                  mock.patch.object(runner.signal, "signal"),
                  mock.patch.object(runner.time, "sleep"),
                  mock.patch.object(runner.subprocess, "Popen", return_value=child) as popen,
                  mock.patch("sys.argv", ["run.py", *arguments])):
                runner.main()
                return popen.call_count

    def test_clean_exit_does_not_restart(self):
        self.assertEqual(self.invoke(0), 1)

    def test_crash_stops_after_three_attempts(self):
        self.assertEqual(self.invoke(1), 3)

    def test_maintenance_does_not_start_the_application(self):
        self.assertEqual(self.invoke(0, ["--maintenance", "disable"]), 0)

    def test_window_without_pid_is_fullscreened_only_for_the_exact_proof_title(self):
        for title, expected in (("SpecterMirror - synthetic frame proof", True), ("Other window", False)):
            with self.subTest(title=title), tempfile.TemporaryDirectory() as temporary:
                child = mock.Mock(pid=501)
                child.poll.side_effect = [None, 0, 0] if not expected else [None, 0]
                child.wait.return_value = 0
                response = mock.Mock(stdout="0x001 0 0 test " + title + "\n")
                with (mock.patch.object(runner.Path, "home", return_value=Path(temporary)),
                      mock.patch.object(runner.os, "geteuid", return_value=1000),
                      mock.patch.dict(runner.os.environ, {"DISPLAY": ":test"}),
                      mock.patch.object(runner.signal, "signal"),
                      mock.patch.object(runner.time, "sleep"),
                      mock.patch.object(runner.subprocess, "Popen", return_value=child),
                      mock.patch.object(runner.subprocess, "run", return_value=response) as run,
                      mock.patch("sys.argv", ["run.py"])):
                    runner.main()
                    commands = [call.args[0] for call in run.call_args_list]
                    self.assertEqual(["wmctrl", "-ir", "0x001", "-b", "add,fullscreen"] in commands, expected)


if __name__ == "__main__":
    unittest.main()
