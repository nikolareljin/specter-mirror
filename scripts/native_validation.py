#!/usr/bin/env python3
"""Local, bounded validation run of a prebuilt spook-kiosk executable.

Runs the executable once with --metrics and --run-for-ms, supervises it, and
writes a private local report. Standard library only; Linux only. It never
uploads, installs, uses sudo or changes permissions. It opens a camera only
when --camera /dev/videoN is passed explicitly.
"""
import argparse
import errno
import hashlib
import json
import os
import platform
import re
import selectors
import signal
import socket
import sys
import time
from pathlib import Path

SCHEMA = "specter-mirror-native-validation"
VERSION = 1
METRICS_SCHEMA = "specter-mirror-native-metrics"
METRICS_VERSION = 1
MAX_STDOUT = 64 * 1024
MAX_STDERR = 64 * 1024
DIAGNOSTIC_BYTES = 16 * 1024
SAMPLE_INTERVAL_S = 0.02
DRAIN_S = 0.5
TERM_WAIT_S = 2.0
KILL_WAIT_S = 5.0
X11_SOCKET_DIR = "/tmp/.X11-unix"
ROOT = Path(__file__).resolve().parents[1]
LOCAL_OUTPUT = ROOT / "native-validation-local"
SIZES = {"320x240": (320, 240), "160x120": (160, 120)}
DURATION_BIN = {"processing": (250, 100_000), "interval": (1000, 500_000)}
REVISION = re.compile(r"[A-Za-z0-9._-]{1,64}")
CAMERA = re.compile(r"/dev/video(0|[1-9][0-9]{0,4})")
DISPLAY = re.compile(r":([0-9]{1,4})(\.[0-9]{1,3})?")
CPU_NORMALIZATION = (
    "cpu_total_s / wall_s. CPU time is summed over every thread of the child "
    "process. 1.0 means one core fully busy on average; the value can exceed "
    "1.0 and is not divided by the number of cores."
)
RESOURCE_METHOD = (
    "CPU: ru_utime + ru_stime from wait4 for this child only. Peak RSS: "
    "vmhwm_sampled_kib is the highest VmHWM read from /proc/<pid>/status about "
    "every 20 ms while the child ran (a lower bound; the last 20 ms can be "
    "missed). ru_maxrss_kib comes from wait4 for this child and is an upper "
    "bound: it can include the launcher's resident size at fork."
)
SCOPE_NOTE = (
    "Measured on the machine that ran this report only. It does not establish "
    "antiX execution, target CPU support, webcam compatibility or acceptance."
)


class UsageError(Exception):
    pass


def positive_int(text):
    if not re.fullmatch(r"[1-9][0-9]*", text):
        raise argparse.ArgumentTypeError("expected a positive decimal integer")
    return int(text)


def build_parser():
    parser = argparse.ArgumentParser(
        prog="native_validation.py",
        description="Run one bounded, local measurement of a prebuilt spook-kiosk.",
        epilog=(
            "Example: python3 scripts/native_validation.py --binary ./spook-kiosk "
            "--size 160x120 --duration-ms 30000 --source-revision abc1234 "
            "--output-dir ~/validation/run1\n"
            "Exit codes: 0 completed and valid, 1 run or validation failed, "
            "2 invalid usage or environment."
        ),
    )
    parser.add_argument("--binary", required=True, help="existing spook-kiosk executable")
    parser.add_argument("--size", required=True, choices=sorted(SIZES), help="processing size")
    parser.add_argument("--duration-ms", required=True, type=positive_int,
                        help="requested run time, 100..600000")
    parser.add_argument("--output-dir", required=True,
                        help="new directory for the private report (must not exist)")
    parser.add_argument("--source-revision", required=True,
                        help="operator-asserted source revision label (not verified)")
    parser.add_argument("--camera", help="explicit /dev/videoN to open; default is the sample")
    parser.add_argument("--expect-unplug", action="store_true",
                        help="the operator will unplug the camera during the run")
    parser.add_argument("--reduced-motion", action="store_true")
    parser.add_argument("--fit", choices=("cover", "contain", "stretch"), default="cover")
    parser.add_argument("--watchdog-grace-ms", type=positive_int, default=10_000,
                        help="extra time after the duration before termination, 100..60000")
    return parser


def check_args(args):
    if not 100 <= args.duration_ms <= 600_000:
        raise UsageError("--duration-ms must be 100..600000")
    if not 100 <= args.watchdog_grace_ms <= 60_000:
        raise UsageError("--watchdog-grace-ms must be 100..60000")
    if not REVISION.fullmatch(args.source_revision):
        raise UsageError("--source-revision must match [A-Za-z0-9._-]{1,64}")
    if args.camera is not None and (not CAMERA.fullmatch(args.camera)
                                    or int(args.camera[10:]) > 65535):
        raise UsageError("--camera must be /dev/videoN with a canonical decimal N")
    if args.expect_unplug and args.camera is None:
        raise UsageError("--expect-unplug needs --camera")
    if sys.version_info < (3, 9):
        raise UsageError("this runner needs Python 3.9 or newer")
    if not sys.platform.startswith("linux"):
        raise UsageError("this runner needs Linux (wait4 and /proc)")
    binary = Path(args.binary)
    if binary.is_symlink() or not binary.is_file() or not os.access(binary, os.X_OK):
        raise UsageError("--binary must be an existing executable file, not a symlink")
    check_display()
    out = Path(args.output_dir)
    if os.path.lexists(out):
        raise UsageError("--output-dir already exists; choose a new directory")
    parent = out.resolve(strict=False).parent
    if not parent.is_dir() or not os.access(parent, os.W_OK | os.X_OK):
        raise UsageError("the parent of --output-dir must be an existing writable directory")
    resolved = out.resolve(strict=False)
    inside_repo = resolved == ROOT or ROOT in resolved.parents
    if inside_repo and LOCAL_OUTPUT not in resolved.parents:
        raise UsageError(
            "reports inside the repository must go under native-validation-local/ (ignored by Git)"
        )
    return binary.resolve(), out


def check_display():
    display = os.environ.get("DISPLAY", "")
    match = DISPLAY.fullmatch(display)
    if not match:
        raise UsageError("DISPLAY must name a local X display such as :0 (it is unset or unsupported)")
    path = f"{X11_SOCKET_DIR}/X{match.group(1)}"
    for address in (path, "\0" + path):
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        probe.settimeout(1.0)
        try:
            probe.connect(address)
            return
        except OSError:
            continue
        finally:
            probe.close()
    raise UsageError("DISPLAY does not accept connections; start or select an X server")


def sha256_of(path):
    digest = hashlib.sha256()
    size = 0
    path = Path(path).resolve(strict=True)
    if not path.is_file():
        raise OSError("executable is not a regular file")
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
            size += len(block)
    return digest.hexdigest(), size


def write_private(path, data):
    """Creates a new 0600 file; never overwrites and never follows a symlink."""
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(path, flags, 0o600)
    with os.fdopen(fd, "wb") as handle:
        handle.write(data)


def read_vmhwm_kib(pid):
    if type(pid) is not int or pid <= 0:
        return None
    try:
        with (Path("/proc") / str(pid) / "status").open("r", encoding="ascii", errors="replace") as handle:
            for line in handle:
                if line.startswith("VmHWM:"):
                    fields = line.split()
                    return int(fields[1]) if fields[2:3] == ["kB"] else None
    except (OSError, ValueError, IndexError):
        return None
    return None


def child_exited(pid):
    """True once the child has exited. Leaves it unreaped so its process
    group id stays reserved until cleanup."""
    try:
        return os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None
    except ChildProcessError:
        return True


def signal_group(pid, sig):
    try:
        os.killpg(pid, sig)
    except (ProcessLookupError, PermissionError):
        pass


def reap(pid, limit_s):
    """Reaps this child with wait4 so rusage belongs to it alone."""
    end = time.monotonic() + limit_s
    while True:
        try:
            done, status, usage = os.wait4(pid, os.WNOHANG)
        except ChildProcessError:
            return None
        if done:
            return status, usage
        if time.monotonic() >= end:
            return None
        time.sleep(0.005)


class Capture:
    def __init__(self):
        self.data = {"stdout": bytearray(), "stderr": bytearray()}
        self.limits = {"stdout": MAX_STDOUT, "stderr": MAX_STDERR}
        self.total = {"stdout": 0, "stderr": 0}
        self.exceeded = False

    def add(self, name, chunk):
        self.total[name] += len(chunk)
        room = self.limits[name] - len(self.data[name])
        if len(chunk) > room:
            self.exceeded = True
        if room > 0:
            self.data[name] += chunk[:room]


def spawn_capture(command):
    """Spawn an explicit argv without a shell or a Python pre-exec child."""
    if not isinstance(command, list) or not command or not all(isinstance(v, str) for v in command):
        raise ValueError("expected an argument array")
    if not Path(command[0]).is_absolute():
        raise ValueError("executable must be absolute")
    descriptors = []
    pid = None
    try:
        stdin = os.open(os.devnull, os.O_RDONLY | os.O_CLOEXEC)
        descriptors.append(stdin)
        stdout_r, stdout_w = os.pipe2(os.O_CLOEXEC)
        descriptors.extend((stdout_r, stdout_w))
        stderr_r, stderr_w = os.pipe2(os.O_CLOEXEC)
        descriptors.extend((stderr_r, stderr_w))
        actions = [(os.POSIX_SPAWN_DUP2, stdin, 0),
                   (os.POSIX_SPAWN_DUP2, stdout_w, 1),
                   (os.POSIX_SPAWN_DUP2, stderr_w, 2)]
        actions.extend((os.POSIX_SPAWN_CLOSE, fd) for fd in descriptors if fd > 2)
        pid = os.posix_spawn(command[0], command, os.environ, file_actions=actions,
                             setsid=True, setsigmask=(),
                             setsigdef=(signal.SIGINT, signal.SIGTERM, signal.SIGPIPE))
        pipes = {}
        try:
            for name, fd in (("stdout", stdout_r), ("stderr", stderr_r)):
                pipes[name] = os.fdopen(fd, "rb")
                descriptors.remove(fd)
        except BaseException:
            for pipe in pipes.values():
                pipe.close()
            raise
        return pid, pipes
    except BaseException:
        if pid is not None:
            signal_group(pid, signal.SIGKILL)
            reap(pid, KILL_WAIT_S)
        raise
    finally:
        for fd in descriptors:
            os.close(fd)


def supervise(command, duration_ms, grace_ms, cancelled=None):
    """Runs the command once. Always leaves no child behind."""
    result = {
        "status": "spawn_failed", "exit_code": None, "signal": None, "termination": "none",
        "wall_s": None, "usage": None, "vmhwm_kib": None, "pipes_held_open": False,
        "unreaped": False,
    }
    capture = Capture()
    started = time.monotonic()
    try:
        pid, pipes = spawn_capture(command)
    except (OSError, ValueError, NotImplementedError) as error:
        result["spawn_error"] = errno.errorcode.get(getattr(error, "errno", None) or 0, "other")
        return result, capture
    result["status"] = "running"
    selector = None
    reaped, peak = None, None

    def pump(timeout):
        for key, _ in selector.select(timeout):
            try:
                chunk = os.read(key.fileobj.fileno(), 65536)
            except BlockingIOError:
                continue
            if chunk:
                capture.add(key.data, chunk)
            else:
                selector.unregister(key.fileobj)

    try:
        selector = selectors.DefaultSelector()
        for name, pipe in pipes.items():
            os.set_blocking(pipe.fileno(), False)
            selector.register(pipe, selectors.EVENT_READ, name)
        deadline = started + duration_ms / 1000 + grace_ms / 1000
        last_sample = 0.0
        while not child_exited(pid):
            if cancelled is not None and cancelled():
                result["status"] = "cancelled"
                result["termination"] = "sigkill"
                break
            now = time.monotonic()
            reason = ("output_limit" if capture.exceeded else
                      "watchdog_terminated" if now >= deadline else None)
            if reason:
                result["status"] = reason
                signal_group(pid, signal.SIGTERM)
                result["termination"] = "sigterm"
                end = time.monotonic() + TERM_WAIT_S
                while time.monotonic() < end and not child_exited(pid):
                    pump(0.01)
                if not child_exited(pid):
                    signal_group(pid, signal.SIGKILL)
                    result["termination"] = "sigkill"
                break
            if now - last_sample >= SAMPLE_INTERVAL_S:
                last_sample = now
                value = read_vmhwm_kib(pid)
                if value is not None:
                    peak = value if peak is None else max(peak, value)
            pump(min(0.01, max(0.0, deadline - now)))
        end = time.monotonic() + DRAIN_S
        while selector.get_map() and time.monotonic() < end:
            pump(0.02)
        result["pipes_held_open"] = bool(selector.get_map())
        if cancelled is not None and cancelled():
            result["status"] = "cancelled"
            result["termination"] = "sigkill"
    except KeyboardInterrupt:
        result["status"] = "cancelled"
        result["termination"] = "sigkill"
    except (OSError, ValueError):
        result["status"] = "supervision_failed"
        result["termination"] = "sigkill"
    finally:
        # Reserve the leader's process group until all cleanup signals are sent.
        handlers = {sig: signal.signal(sig, signal.SIG_IGN)
                    for sig in (signal.SIGTERM, signal.SIGINT)}
        try:
            signal_group(pid, signal.SIGKILL)
            reaped = reap(pid, KILL_WAIT_S)
            if selector is not None:
                selector.close()
            for pipe in pipes.values():
                pipe.close()
        finally:
            for sig, handler in handlers.items():
                signal.signal(sig, handler)
        result["wall_s"] = time.monotonic() - started
        if reaped is None:
            result["unreaped"] = True
        else:
            status, usage = reaped
            result["usage"] = usage
            returncode = os.waitstatus_to_exitcode(status)
            if returncode < 0:
                result["signal"] = -returncode
            else:
                result["exit_code"] = returncode
            if result["status"] == "running":
                result["status"] = "signaled" if result["signal"] else "exited"
    result["vmhwm_kib"] = peak
    return result, capture


def strict_json(text):
    """Parses exactly one JSON object; rejects duplicates, NaN/Infinity and floats."""
    def pairs(items):
        keys = [key for key, _ in items]
        if len(keys) != len(set(keys)):
            raise ValueError("duplicate key")
        return dict(items)

    def refuse(_):
        raise ValueError("non-finite or fractional number")

    value = json.loads(text, object_pairs_hook=pairs, parse_constant=refuse, parse_float=refuse)
    if not isinstance(value, dict):
        raise ValueError("not an object")
    return value


def is_count(value):
    return type(value) is int and 0 <= value < 2**64


def is_opt_count(value):
    return value is None or is_count(value)


def check_summary(name, summary, bins, problems):
    keys = {"count", "p50_us", "p50_overflow", "p95_us", "p95_overflow", "max_us",
            "bin_us", "range_us", "overflow_count"}
    if not isinstance(summary, dict) or set(summary) != keys:
        problems.append(f"{name}: unexpected fields")
        return None
    bin_us, range_us = bins
    if summary["bin_us"] != bin_us or summary["range_us"] != range_us:
        problems.append(f"{name}: unexpected bin geometry")
    if not all(is_count(summary[k]) for k in ("count", "overflow_count")):
        problems.append(f"{name}: invalid counts")
        return None
    if summary["overflow_count"] > summary["count"]:
        problems.append(f"{name}: overflow exceeds count")
    quad = [summary["p50_us"], summary["p95_us"], summary["max_us"]]
    flags = [summary["p50_overflow"], summary["p95_overflow"]]
    count, overflow = summary["count"], summary["overflow_count"]
    if count == 0:
        if quad != [None, None, None] or flags != [None, None] or overflow:
            problems.append(f"{name}: empty population must be null without overflow")
    elif not all(is_count(v) for v in quad) or not all(type(f) is bool for f in flags):
        problems.append(f"{name}: missing values for a populated summary")
    else:
        if not quad[0] <= quad[1] <= quad[2]:
            problems.append(f"{name}: p50/p95/max are not ordered")
        if (quad[2] >= range_us) != (overflow > 0):
            problems.append(f"{name}: maximum contradicts overflow count")
        for percent, value, flag in zip((50, 95), quad[:2], flags):
            expected = (percent * count + 99) // 100 > count - overflow
            if flag != expected:
                problems.append(f"{name}: overflow flag contradicts percentile rank")
            if flag:
                if value != quad[2]:
                    problems.append(f"{name}: overflow percentile must equal maximum")
            elif value >= range_us or (value != quad[2] and (value + 1) % bin_us):
                problems.append(f"{name}: percentile is not a bounded bin edge")
    return summary["count"]


def validate_metrics(metrics, args, wall_s):
    """Returns a list of problem codes; empty when the object is coherent."""
    problems = []
    top = {"schema", "version", "run", "config", "source", "display_submissions",
           "camera", "capture", "sample"}
    if set(metrics) != top:
        return ["metrics: unexpected top-level fields"]
    if metrics["schema"] != METRICS_SCHEMA or metrics["version"] != METRICS_VERSION \
            or type(metrics["version"]) is not int:
        return ["metrics: unsupported schema or version"]
    run, config, source = metrics["run"], metrics["config"], metrics["source"]
    for name, value, keys in (
        ("run", run, {"exit_reason", "requested_run_ms", "elapsed_us", "startup_us",
                      "loop_us", "shutdown_us", "clock_errors"}),
        ("config", config, {"width", "height", "fit", "delay_ms", "trails",
                            "reduced_motion", "target_fps"}),
        ("source", source, {"requested", "final", "fallback_occurred", "fallback_at_us",
                            "first_camera_frame_us"}),
    ):
        if not isinstance(value, dict) or set(value) != keys:
            return [f"metrics.{name}: unexpected fields"]
    if not all(is_count(config[k]) for k in ("width", "height", "target_fps", "delay_ms", "trails")):
        problems.append("config: invalid numeric types")
    if not is_count(metrics["display_submissions"]):
        problems.append("display: invalid submissions type")
    if type(source["fallback_occurred"]) is not bool:
        problems.append("source: invalid fallback type")
    width, height = SIZES[args.size]
    if (config["width"], config["height"]) != (width, height):
        problems.append("config: size differs from the request")
    if config["fit"] != args.fit or config["reduced_motion"] is not args.reduced_motion:
        problems.append("config: fit or reduced motion differs from the request")
    if config["target_fps"] != 30 or config["delay_ms"] != 250 or config["trails"] != 115:
        problems.append("config: unexpected defaults")
    if type(run["requested_run_ms"]) is not int or run["requested_run_ms"] != args.duration_ms:
        problems.append("run: requested duration differs")
    if run["exit_reason"] != "timed":
        problems.append("run: did not end by the requested timer")
    for key in ("elapsed_us", "startup_us", "loop_us", "shutdown_us"):
        if not is_count(run[key]):
            problems.append(f"run: {key} is unavailable or invalid")
    if type(run["clock_errors"]) is not int or run["clock_errors"] != 0:
        problems.append("run: clock errors were reported")
    if all(is_count(run[k]) for k in ("elapsed_us", "startup_us", "loop_us", "shutdown_us")):
        if run["startup_us"] + run["loop_us"] + run["shutdown_us"] != run["elapsed_us"]:
            problems.append("run: phases do not add up to the elapsed time")
        if run["loop_us"] < args.duration_ms * 1000:
            problems.append("run: loop was shorter than requested")
        if wall_s is not None and run["elapsed_us"] > wall_s * 1e6 + 1000:
            problems.append("run: elapsed time exceeds the supervised wall time")
    want = "camera" if args.camera else "sample"
    if source["requested"] != want or source["final"] not in ("camera", "sample"):
        problems.append("source: requested or final source is inconsistent")
    cam, smp, cap = metrics["camera"], metrics["sample"], metrics["capture"]
    summary_keys = {"processing_us", "prepare_us", "submit_us", "display_interval_us"}
    counts = {}
    for label, part, new_key in (("camera", cam, "frames"), ("sample", smp, "updates")):
        if not isinstance(part, dict) or set(part) != {"submissions", new_key, "idle_ticks"} | summary_keys:
            return problems + [f"metrics.{label}: unexpected fields"]
        if not all(is_count(part[k]) for k in ("submissions", new_key, "idle_ticks")):
            problems.append(f"{label}: invalid counters")
            continue
        if part["submissions"] != part[new_key] + part["idle_ticks"]:
            problems.append(f"{label}: submissions differ from new items plus idle ticks")
        if label == "sample" and part["idle_ticks"] != 0:
            problems.append("sample: idle ticks are not expected")
        processing = check_summary(f"{label}.processing", part["processing_us"],
                                   DURATION_BIN["processing"], problems)
        prepare = check_summary(f"{label}.prepare", part["prepare_us"],
                                DURATION_BIN["processing"], problems)
        submit = check_summary(f"{label}.submit", part["submit_us"],
                               DURATION_BIN["processing"], problems)
        interval = check_summary(f"{label}.interval", part["display_interval_us"],
                                 DURATION_BIN["interval"], problems)
        if processing is not None and processing != part[new_key]:
            problems.append(f"{label}: processing samples differ from new items")
        if prepare is not None and prepare != part["submissions"]:
            problems.append(f"{label}: presentation samples differ from submissions")
        if submit is not None and submit != part["submissions"]:
            problems.append(f"{label}: submit samples differ from submissions")
        if interval is not None and interval != max(part["submissions"] - 1, 0):
            problems.append(f"{label}: interval samples differ from consecutive submissions")
        if is_count(run["loop_us"]):
            for key in summary_keys:
                value = part[key]
                if isinstance(value, dict) and is_count(value.get("max_us")) and value["max_us"] > run["loop_us"]:
                    problems.append(f"{label}: timing exceeds the measured loop")
        counts[label] = part
    if len(counts) == 2:
        total = counts["camera"]["submissions"] + counts["sample"]["submissions"]
        if metrics["display_submissions"] != total:
            problems.append("display submissions differ from the per-source sum")
        frames = counts["camera"]["frames"]
        if not args.camera and (source["final"] != "sample" or cap is not None):
            problems.append("source: sample request has camera state")
        if args.camera and source["final"] == "camera" and counts["sample"]["submissions"]:
            problems.append("source: sample submissions without camera fallback")
        prior_camera = counts["camera"]["submissions"] > 0
        if source["final"] == "sample" and prior_camera and not counts["sample"]["submissions"]:
            problems.append("source: final sample has no sample submission")
        if source["fallback_occurred"] and prior_camera and source["fallback_at_us"] is None:
            problems.append("source: fallback after camera use needs a timestamp")
        if not source["fallback_occurred"] and source["fallback_at_us"] is not None:
            problems.append("source: fallback time without fallback")
        for key in ("first_camera_frame_us", "fallback_at_us"):
            stamp = source[key]
            if is_count(stamp) and is_count(run["loop_us"]) and stamp > run["loop_us"]:
                problems.append("source: event time exceeds loop duration")
        first, fallback = source["first_camera_frame_us"], source["fallback_at_us"]
        if is_count(first) and is_count(fallback) and fallback < first:
            problems.append("source: fallback precedes the first camera frame")
        if not args.camera and counts["camera"]["submissions"] != 0:
            problems.append("camera activity without a camera request")
        if source["fallback_occurred"] is not (bool(args.camera) and source["final"] != "camera"):
            problems.append("source: fallback flag contradicts the final source")
        if (source["first_camera_frame_us"] is not None) != (frames > 0):
            problems.append("source: first camera frame time contradicts the frame count")
        if source["fallback_at_us"] is not None and not source["fallback_occurred"]:
            problems.append("source: fallback time without fallback")
        if not is_opt_count(source["first_camera_frame_us"]) or not is_opt_count(source["fallback_at_us"]):
            problems.append("source: invalid times")
        if cap is None:
            if frames > 0:
                problems.append("capture counters are missing although frames were processed")
        elif not isinstance(cap, dict) or set(cap) != {"published", "overwritten_unread",
                                                       "stale_dropped", "driver_drops"}:
            problems.append("capture: unexpected fields")
        else:
            if cap["driver_drops"] is not None:
                problems.append("capture: driver drops are not measured and must be null")
            values = [cap["published"], cap["overwritten_unread"], cap["stale_dropped"]]
            if not all(is_count(v) for v in values):
                problems.append("capture: invalid counters")
            elif frames + cap["overwritten_unread"] + cap["stale_dropped"] > cap["published"]:
                problems.append("capture: frames plus replaced and stale exceed published")
    return problems


def classify(args, metrics, problems):
    """Evidence class and failure codes derived from a coherent report."""
    failures = list(problems)
    if metrics is None or problems:
        return "none", failures
    source = metrics["source"]
    frames = metrics["camera"]["frames"]
    if not args.camera:
        return "sample_uninterrupted", failures
    if frames == 0:
        failures.append("camera_no_frames_fallback")
        return "camera_requested_no_frames", failures
    if args.expect_unplug:
        if not source["fallback_occurred"]:
            failures.append("unplug_not_observed")
        return "camera_unplug_fallback_observation", failures
    if source["fallback_occurred"]:
        failures.append("camera_fell_back_unexpectedly")
        return "camera_interrupted", failures
    return "camera_uninterrupted", failures


def build_report(args, binary_info, outcome, capture, metrics, failures, evidence):
    usage = outcome["usage"]
    mixed = bool(args.camera) and (
        metrics is None or metrics["source"]["fallback_occurred"]
        or metrics["camera"]["frames"] == 0
    )
    resources = {
        "method": RESOURCE_METHOD,
        "cpu_normalization": CPU_NORMALIZATION,
        "cpu_scope": "whole_process_aggregate_mixed_sources" if mixed
        else "whole_process_aggregate_single_source",
        "cpu_user_s": None, "cpu_system_s": None, "cpu_total_s": None,
        "cpu_to_wall_ratio": None,
        "peak_rss": {"vmhwm_sampled_kib": outcome["vmhwm_kib"],
                     "ru_maxrss_kib": None, "unit": "KiB"},
    }
    if usage is not None:
        user, system = round(usage.ru_utime, 6), round(usage.ru_stime, 6)
        resources.update(cpu_user_s=user, cpu_system_s=system, cpu_total_s=round(user + system, 6))
        resources["peak_rss"]["ru_maxrss_kib"] = int(usage.ru_maxrss)
        wall = outcome["wall_s"]
        if wall and wall > 0:
            resources["cpu_to_wall_ratio"] = round((user + system) / wall, 6)
    return {
        "schema": SCHEMA, "version": VERSION,
        "result": "failed" if failures else "passed",
        "failures": failures,
        "evidence_class": evidence,
        "scope_note": SCOPE_NOTE,
        "run": {
            "requested_source": "camera" if args.camera else "sample",
            "size": args.size, "duration_ms": args.duration_ms,
            "reduced_motion": args.reduced_motion, "fit": args.fit,
            "expect_unplug": args.expect_unplug,
            "watchdog_grace_ms": args.watchdog_grace_ms,
            "runner_machine": platform.machine(),
        },
        "binary": binary_info,
        "child": {k: outcome[k] for k in ("status", "exit_code", "signal", "termination",
                                          "pipes_held_open", "unreaped")}
        | {"wall_s": None if outcome["wall_s"] is None else round(outcome["wall_s"], 6)},
        "resources": resources,
        "output": {
            "stdout_bytes": capture.total["stdout"], "stderr_bytes": capture.total["stderr"],
            "stdout_kept_bytes": len(capture.data["stdout"]),
            "stderr_kept_bytes": len(capture.data["stderr"]),
            "limit_exceeded": capture.exceeded,
        },
        "metrics": metrics,
    }


def run(argv):
    args = build_parser().parse_args(argv)
    try:
        binary, out = check_args(args)
    except (UsageError, OSError, RuntimeError) as error:
        message = str(error) if isinstance(error, UsageError) else "invalid input path"
        print(f"native_validation: {message}", file=sys.stderr)
        return 2
    try:
        digest, size = sha256_of(binary)
    except OSError:
        print("native_validation: cannot read the executable", file=sys.stderr)
        return 2
    try:
        os.mkdir(out, 0o700)
    except OSError as error:
        print(f"native_validation: cannot create the output directory ({error.strerror})",
              file=sys.stderr)
        return 2
    command = [str(binary), "--metrics", "--run-for-ms", str(args.duration_ms),
               "--resolution", args.size, "--fit", args.fit]
    if args.reduced_motion:
        command.append("--reduced-motion")
    if args.camera:
        command += ["--camera", args.camera]

    cancellation = [False]
    def request_cancel(*_):
        cancellation[0] = True
    handlers = {sig: signal.signal(sig, request_cancel)
                for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        outcome, capture = supervise(command, args.duration_ms, args.watchdog_grace_ms,
                                     cancelled=lambda: cancellation[0])
    finally:
        for sig, handler in handlers.items():
            signal.signal(sig, handler)

    failures, metrics = [], None
    status = outcome["status"]
    if status != "exited":
        failures.append(f"child_{status}")
    if status == "exited" and outcome["exit_code"] != 0:
        failures.append("application_nonzero_exit")
    if outcome["unreaped"]:
        failures.append("child_not_reaped")
    if outcome["pipes_held_open"]:
        failures.append("output_pipes_held_open")
    if capture.exceeded:
        failures.append("output_limit_exceeded")
    if outcome["usage"] is None and status not in ("spawn_failed", "cancelled"):
        failures.append("resource_usage_unavailable")
    try:
        after, _ = sha256_of(binary)
        unchanged = after == digest
    except OSError:
        unchanged = False
    if not unchanged:
        failures.append("binary_changed_during_run")
    binary_info = {"sha256": digest, "size_bytes": size,
                   "source_revision_asserted": args.source_revision,
                   "unchanged_during_run": unchanged}
    problems = []
    if status == "exited" and not capture.exceeded:
        try:
            text = bytes(capture.data["stdout"]).decode("utf-8")
            if text.count("\n") != 1 or not text.endswith("\n"):
                raise ValueError("stdout is not exactly one line")
            metrics = strict_json(text)
            problems = validate_metrics(metrics, args, outcome["wall_s"])
        except (ValueError, UnicodeDecodeError, RecursionError):
            metrics, problems = None, ["metrics_missing_or_malformed"]
        if problems:
            failures.append("metrics_invalid")
            failures.extend(problems[:8])
    evidence = "none"
    if metrics is not None and not problems:
        evidence, more = classify(args, metrics, problems)
        failures.extend(code for code in more if code not in failures)
    if problems:
        metrics = None  # an incoherent object is not retained as evidence
    report = build_report(args, binary_info, outcome, capture, metrics, failures, evidence)
    diagnostics = (
        b"Local diagnostics. Inspect and redact before sharing.\n--- stderr ---\n"
        + bytes(capture.data["stderr"][:DIAGNOSTIC_BYTES])
        + b"\n--- stdout (first bytes) ---\n"
        + bytes(capture.data["stdout"][:DIAGNOSTIC_BYTES]) + b"\n"
    )
    try:
        write_private(out / "report.json",
                      (json.dumps(report, indent=2, sort_keys=True, allow_nan=False) + "\n").encode())
        write_private(out / "diagnostics.txt", diagnostics)
    except (OSError, ValueError) as error:
        print(f"native_validation: report could not be written ({type(error).__name__})",
              file=sys.stderr)
        return 1
    print(f"native_validation: {report['result']} ({evidence}); report in the output directory",
          file=sys.stderr)
    for code in failures:
        print(f"native_validation: failure: {code}", file=sys.stderr)
    return 1 if failures else 0


def main(argv=None):
    try:
        return run(sys.argv[1:] if argv is None else argv)
    except SystemExit as exit_request:
        return exit_request.code if isinstance(exit_request.code, int) else 2


if __name__ == "__main__":
    sys.exit(main())
