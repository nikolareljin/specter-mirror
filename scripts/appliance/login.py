"""Start the selected account's dedicated X11 session without desktop autostarts."""

import json
import os
from pathlib import Path
import pwd
import signal
import subprocess
import sys
import time


def main():
    config = json.loads(Path("/etc/specter-mirror/config.json").read_text())
    if os.geteuid() == 0:
        sys.exit("The display manager must drop privileges before starting a session.")
    if pwd.getpwuid(os.geteuid()).pw_name != config["user"]:
        os.execv("/usr/local/bin/desktop-session", ["desktop-session", *sys.argv[1:]])
    manager = subprocess.Popen(["openbox", "--config-file", "/opt/specter-mirror/runtime/openbox.xml"])
    child = None

    def stop(_signal, _frame):
        manager.terminate()
        if child is not None and child.poll() is None:
            child.terminate()

    for value in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(value, stop)
    try:
        child = subprocess.Popen(["/usr/bin/python3", "/opt/specter-mirror/runtime/run.py"])
        while child.poll() is None and manager.poll() is None:
            time.sleep(0.2)
        # Keep the recovery key available after Escape or a failed launch.
        manager.wait()
    finally:
        for process in (child, manager):
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
