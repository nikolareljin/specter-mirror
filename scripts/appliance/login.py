"""Start the selected account's dedicated X11 session without desktop autostarts."""

import json
import os
from pathlib import Path
import pwd
import re
import signal
import subprocess
import sys
import time


def main():
    config = json.loads(Path("/etc/specter-mirror/config.json").read_text())
    if os.geteuid() == 0:
        sys.exit("The display manager must drop privileges before starting a session.")
    user = config["user"]
    if config.get("follow_default_user", False):
        filename = "/etc/slimski.local.conf" if config["display_manager"] == "slimski" else "/etc/slim.conf"
        names = re.findall(r"^\s*default_user\s+([a-z_][a-z0-9_-]*)\s*$", Path(filename).read_text(), re.M)
        if len(names) != 1 or names[0] == "root":
            sys.exit("A non-root default kiosk account is required.")
        user = names[0]
    if pwd.getpwuid(os.geteuid()).pw_name != user:
        os.execv("/usr/local/bin/desktop-session", ["desktop-session", *sys.argv[1:]])
    manager = subprocess.Popen(["openbox", "--config-file", "/opt/specter-mirror/runtime/openbox.xml"])
    child = None
    stopped = False

    def stop(_signal, _frame):
        nonlocal stopped
        stopped = True
        manager.terminate()
        if child is not None and child.poll() is None:
            child.terminate()

    for value in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(value, stop)
    try:
        if os.path.ismount("/live/boot-dev") and Path("/usr/local/sbin/specter-mirror-live-install").is_file():
            child = subprocess.Popen(["/usr/bin/sudo", "-n", "/usr/local/sbin/specter-mirror-live-install"])
            while child.poll() is None and manager.poll() is None:
                time.sleep(0.2)
        if stopped or manager.poll() is not None:
            return
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
