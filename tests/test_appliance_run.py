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


if __name__ == "__main__":
    unittest.main()
