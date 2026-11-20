"""Run the synthetic proof without privileged execution or an unbounded restart loop."""

import argparse
import fcntl
import os
from pathlib import Path
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--maintenance", choices=("status", "disable", "enable"))
    args = parser.parse_args()
    if os.geteuid() == 0:
        parser.exit(1, "Run the kiosk or maintenance command as the desktop user, not root.\n")
    state = Path.home() / ".local/state/specter-mirror"
    state.mkdir(parents=True, exist_ok=True, mode=0o700)
    disabled = state / "disabled"
    if args.maintenance:
        if args.maintenance == "disable":
            disabled.touch(mode=0o600)
        elif args.maintenance == "enable":
            disabled.unlink(missing_ok=True)
        print("disabled" if disabled.exists() else "enabled")
        return
    with (state / "session.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        if not os.environ.get("DISPLAY") or disabled.exists():
            return
        log = state / "session.log"
        stopped = False
        child = None

        def stop(_signal, _frame):
            nonlocal stopped
            stopped = True
            if child is not None and child.poll() is None:
                child.terminate()

        for value in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(value, stop)
        for attempt in range(3):
            if stopped or disabled.exists():
                break
            if log.exists() and log.stat().st_size > 65536:
                log.replace(state / "session.log.1")
            with log.open("a") as stream:
                stream.write(f"{int(time.time())} start attempt={attempt + 1} mode=synthetic-proof\n")
            child = subprocess.Popen(["/opt/specter-mirror/bin/spook-kiosk"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(30):
                if stopped or child.poll() is not None:
                    break
                try:
                    windows = subprocess.run(["wmctrl", "-lp"], capture_output=True, text=True,
                                             timeout=3, check=False)
                except (OSError, subprocess.TimeoutExpired):
                    break
                matched = False
                for line in windows.stdout.splitlines():
                    fields = line.split()
                    if len(fields) >= 3 and fields[2] == str(child.pid):
                        try:
                            subprocess.run(["wmctrl", "-ir", fields[0], "-b", "add,fullscreen"],
                                           timeout=3, check=False, stdout=subprocess.DEVNULL,
                                           stderr=subprocess.DEVNULL)
                        except (OSError, subprocess.TimeoutExpired):
                            pass
                        matched = True
                if matched:
                    break
                time.sleep(0.1)
            while child.poll() is None:
                if disabled.exists() or stopped:
                    child.terminate()
                    try:
                        child.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        child.kill()
                    break
                time.sleep(0.2)
            result = child.wait()
            with log.open("a") as stream:
                stream.write(f"{int(time.time())} exit status={result}\n")
            if result == 0 or stopped or disabled.exists():
                break
            time.sleep(2 ** attempt)


if __name__ == "__main__":
    main()
