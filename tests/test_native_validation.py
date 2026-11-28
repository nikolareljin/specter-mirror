import contextlib
import hashlib
import importlib.util
import io
import json
import os
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "native_validation.py"
spec = importlib.util.spec_from_file_location("native_validation", SCRIPT)
nv = importlib.util.module_from_spec(spec)
sys.modules["native_validation"] = nv
spec.loader.exec_module(nv)


def summary(values, bin_us=250, range_us=100_000):
    """Independent nearest-rank summary using the documented upper bounds."""
    if not values:
        return {"count": 0, "p50_us": None, "p50_overflow": None, "p95_us": None,
                "p95_overflow": None, "max_us": None, "bin_us": bin_us,
                "range_us": range_us, "overflow_count": 0}
    ordered = sorted(values)

    def rank(percent):
        value = ordered[max(1, -(-percent * len(ordered) // 100)) - 1]
        overflow = value >= range_us
        bound = min((value // bin_us + 1) * bin_us - 1, ordered[-1])
        return (ordered[-1] if overflow else bound), overflow

    (p50, o50), (p95, o95) = rank(50), rank(95)
    return {"count": len(ordered), "p50_us": p50, "p50_overflow": o50, "p95_us": p95,
            "p95_overflow": o95, "max_us": ordered[-1], "bin_us": bin_us,
            "range_us": range_us, "overflow_count": sum(v >= range_us for v in ordered)}


def source_part(new_key, new, idle=0):
    n = new + idle
    return {"submissions": n, new_key: new, "idle_ticks": idle,
            "processing_us": summary([800 + i for i in range(new)]),
            "prepare_us": summary([50] * n),
            "submit_us": summary([30_000] * n),
            "display_interval_us": summary([33_000] * max(n - 1, 0), 1000, 500_000)}


def valid_metrics(size="160x120", duration=100, camera=False, frames=0, samples=3,
                  idle=2, fallback=False, capture="auto", **overrides):
    w, h = map(int, size.split("x"))
    cam_part = source_part("frames", frames, idle if frames else 0)
    smp_part = source_part("updates", samples)
    if capture == "auto":
        capture = ({"published": frames + 3, "overwritten_unread": 2, "stale_dropped": 0,
                    "driver_drops": None} if camera and frames else None)
    loop_us = duration * 1000 + 700
    metrics = {
        "schema": "specter-mirror-native-metrics", "version": 1,
        "run": {"exit_reason": "timed", "requested_run_ms": duration,
                "elapsed_us": 5_000 + loop_us + 400, "startup_us": 5_000,
                "loop_us": loop_us, "shutdown_us": 400, "clock_errors": 0},
        "config": {"width": w, "height": h, "fit": "cover", "delay_ms": 250, "trails": 115,
                   "reduced_motion": False, "target_fps": 30},
        "source": {"requested": "camera" if camera else "sample",
                   "final": "camera" if camera and not fallback and frames else "sample",
                   "fallback_occurred": bool(camera and (fallback or not frames)),
                   "fallback_at_us": 9_000 if camera and fallback and frames else None,
                   "first_camera_frame_us": 4_000 if frames else None},
        "display_submissions": cam_part["submissions"] + smp_part["submissions"],
        "camera": cam_part, "capture": capture, "sample": smp_part,
    }
    for path, value in overrides.items():
        target = metrics
        *parents, last = path.split(".")
        for key in parents:
            target = target[key]
        target[last] = value
    return metrics


def gone(pid):
    try:
        with (Path("/proc") / str(pid) / "stat").open() as handle:
            return handle.read().rsplit(")", 1)[1].split()[0] == "Z"
    except OSError:
        return True


def wait_gone(pid, seconds=3):
    end = time.monotonic() + seconds
    while not gone(pid) and time.monotonic() < end:
        time.sleep(0.02)
    return gone(pid)


class RunnerCase(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="nv-")
        self.addCleanup(self.temp.cleanup)
        self.dir = Path(self.temp.name)
        # A listening socket stands in for an X server.
        self.x_dir = self.dir / "x"
        self.x_dir.mkdir()
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(self.x_dir / "X77"))
        self.server.listen(socket.SOMAXCONN)
        self.addCleanup(self.server.close)
        patches = [mock.patch.object(nv, "X11_SOCKET_DIR", str(self.x_dir)),
                   mock.patch.dict(os.environ, {"DISPLAY": ":77"}),
                   mock.patch.object(nv, "TERM_WAIT_S", 0.4)]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.count = 0
        self.pids = []
        self.addCleanup(self.kill_leftovers)

    def kill_leftovers(self):
        for pid in self.pids:
            with contextlib.suppress(OSError):
                os.kill(pid, signal.SIGKILL)

    def fake(self, body):
        self.count += 1
        path = self.dir / f"fake{self.count}"
        path.write_text(f"#!{sys.executable}\n" + textwrap.dedent(body))
        path.chmod(0o755)
        return path

    def emitter(self, stdout, code=0, stderr="", extra="", sleep=0.0):
        return self.fake(f"""
            import os, sys, time
            time.sleep({sleep!r})
            {extra}
            sys.stderr.write({stderr!r})
            sys.stdout.write({stdout!r})
            sys.stdout.flush()
            sys.exit({code})
        """)

    def invoke(self, binary, *extra, size="160x120", duration="100", name=None, grace="400"):
        self.count += 1
        out = self.dir / (name or f"out{self.count}")
        argv = ["--binary", str(binary), "--size", size, "--duration-ms", duration,
                "--source-revision", "test-rev.1", "--output-dir", str(out),
                "--watchdog-grace-ms", grace, *extra]
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = nv.main(argv)
        return code, out, err.getvalue()

    def report(self, out):
        return json.loads((out / "report.json").read_text())

    def ok(self, metrics, **kwargs):
        # Behave like a run that really lasted as long as the report claims.
        elapsed = metrics.get("run", {}).get("elapsed_us")
        sleep = elapsed / 1e6 if type(elapsed) is int and 0 < elapsed < 10**6 else 0.11
        return self.emitter(json.dumps(metrics) + "\n", sleep=sleep, **kwargs)


class SuccessAndClassification(RunnerCase):
    def test_sample_run_passes_at_both_sizes_with_private_files_and_hash(self):
        for size in ("160x120", "320x240"):
            binary = self.ok(valid_metrics(size))
            code, out, err = self.invoke(binary, size=size)
            self.assertEqual(code, 0, err)
            report = self.report(out)
            self.assertEqual(report["result"], "passed")
            self.assertEqual(report["evidence_class"], "sample_uninterrupted")
            self.assertEqual(report["child"]["status"], "exited")
            self.assertEqual(report["run"]["size"], size)
            self.assertEqual(report["binary"]["sha256"],
                             hashlib.sha256(binary.read_bytes()).hexdigest())
            self.assertEqual(report["binary"]["source_revision_asserted"], "test-rev.1")
            self.assertTrue(report["binary"]["unchanged_during_run"])
            self.assertEqual(report["resources"]["cpu_scope"],
                             "whole_process_aggregate_single_source")
            self.assertEqual(report["metrics"]["config"]["width"], int(size.split("x")[0]))
            self.assertEqual(stat.S_IMODE(out.stat().st_mode), 0o700)
            for name in ("report.json", "diagnostics.txt"):
                self.assertEqual(stat.S_IMODE((out / name).stat().st_mode), 0o600)
            self.assertNotIn(str(self.dir), (out / "report.json").read_text())

    def test_camera_uninterrupted_passes_with_counters(self):
        binary = self.ok(valid_metrics(camera=True, frames=5, samples=0))
        code, out, err = self.invoke(binary, "--camera", "/dev/video3")
        self.assertEqual(code, 0, err)
        report = self.report(out)
        self.assertEqual(report["evidence_class"], "camera_uninterrupted")
        self.assertEqual(report["metrics"]["capture"]["overwritten_unread"], 2)
        self.assertIsNone(report["metrics"]["capture"]["driver_drops"])

    def test_requested_camera_without_frames_is_fallback_evidence_not_a_pass(self):
        binary = self.ok(valid_metrics(camera=True, frames=0, samples=4))
        code, out, _ = self.invoke(binary, "--camera", "/dev/video0")
        self.assertEqual(code, 1)
        report = self.report(out)
        self.assertEqual(report["result"], "failed")
        self.assertIn("camera_no_frames_fallback", report["failures"])
        self.assertEqual(report["evidence_class"], "camera_requested_no_frames")
        self.assertEqual(report["resources"]["cpu_scope"],
                         "whole_process_aggregate_mixed_sources")

    def test_unexpected_fallback_fails_and_expected_unplug_is_labeled_mixed(self):
        mixed = valid_metrics(camera=True, frames=4, samples=3, fallback=True)
        code, out, _ = self.invoke(self.ok(mixed), "--camera", "/dev/video0")
        self.assertEqual(code, 1)
        self.assertIn("camera_fell_back_unexpectedly", self.report(out)["failures"])
        code, out, err = self.invoke(self.ok(mixed), "--camera", "/dev/video0", "--expect-unplug")
        self.assertEqual(code, 0, err)
        report = self.report(out)
        self.assertEqual(report["evidence_class"], "camera_unplug_fallback_observation")
        self.assertEqual(report["resources"]["cpu_scope"],
                         "whole_process_aggregate_mixed_sources")
        # An unplug that never happened is not an observation of fallback.
        steady = valid_metrics(camera=True, frames=4, samples=0)
        code, out, _ = self.invoke(self.ok(steady), "--camera", "/dev/video0", "--expect-unplug")
        self.assertEqual(code, 1)
        self.assertIn("unplug_not_observed", self.report(out)["failures"])

    def test_expected_percentiles_match_the_independent_summary(self):
        metrics = valid_metrics()
        metrics["sample"]["processing_us"] = summary([1000, 2000, 3000])
        self.assertEqual(metrics["sample"]["processing_us"]["p50_us"], 2249)
        self.assertEqual(metrics["sample"]["processing_us"]["p95_us"], 3000)
        code, _, err = self.invoke(self.ok(metrics))
        self.assertEqual(code, 0, err)


class InvalidOutput(RunnerCase):
    def failed(self, binary, expected, *extra):
        code, out, err = self.invoke(binary, *extra)
        self.assertEqual(code, 1, err)
        report = self.report(out)
        self.assertEqual(report["result"], "failed")
        self.assertIn(expected, report["failures"])
        self.assertIsNone(report["metrics"])
        return report

    def test_missing_malformed_truncated_and_duplicate_output_fail(self):
        good = json.dumps(valid_metrics())
        cases = {
            "empty": "",
            "garbage": "not json\n",
            "truncated": good[: len(good) // 2] + "\n",
            "two objects": good + "\n" + good + "\n",
            "no newline": good,
            "array": "[]\n",
            "duplicate key": good.replace('"version": 1', '"version": 1, "version": 1', 1) + "\n",
            "nan": good.replace('"clock_errors": 0', '"clock_errors": NaN') + "\n",
            "infinity": good.replace('"clock_errors": 0', '"clock_errors": Infinity') + "\n",
            "float": good.replace('"width": 160', '"width": 160.0') + "\n",
        }
        for name, text in cases.items():
            with self.subTest(name):
                self.failed(self.emitter(text), "metrics_missing_or_malformed")

    def test_contradictory_or_unsupported_objects_fail(self):
        capture_bad = {"published": 3, "overwritten_unread": 2, "stale_dropped": 0,
                       "driver_drops": None}
        cases = {
            "version": (valid_metrics(version=2), ()),
            "schema": (valid_metrics(schema="other"), ()),
            "size": (valid_metrics(**{"config.width": 320}), ()),
            "duration": (valid_metrics(**{"run.requested_run_ms": 200}), ()),
            "exit reason": (valid_metrics(**{"run.exit_reason": "escape"}), ()),
            "short loop": (valid_metrics(**{"run.loop_us": 1000}), ()),
            "phases": (valid_metrics(**{"run.elapsed_us": 1}), ()),
            "clock errors": (valid_metrics(**{"run.clock_errors": 1}), ()),
            "unavailable elapsed": (valid_metrics(**{"run.elapsed_us": None}), ()),
            "negative": (valid_metrics(**{"run.startup_us": -1}), ()),
            "bool as number": (valid_metrics(**{"run.startup_us": True}), ()),
            "count mismatch": (valid_metrics(**{"sample.updates": 99}), ()),
            "total mismatch": (valid_metrics(display_submissions=1), ()),
            "invented zero": (valid_metrics(**{"sample.processing_us": {
                **summary([]), "p50_us": 0}}), ()),
            "unordered": (valid_metrics(**{"sample.processing_us": {
                **summary([5, 6]), "p50_us": 900, "p95_us": 500}}), ()),
            "camera activity without request": (valid_metrics(
                **{"camera": source_part("frames", 1)}), ()),
            "unknown field": ({**valid_metrics(), "extra": 1}, ()),
            "wrong bins": (valid_metrics(**{"sample.prepare_us": summary([1], 100, 1000)}), ()),
            "claimed driver drops": (valid_metrics(
                camera=True, frames=2, samples=0,
                capture={**capture_bad, "published": 5, "driver_drops": 0}),
                ("--camera", "/dev/video1")),
            "capture over-count": (valid_metrics(
                camera=True, frames=2, samples=0, capture=capture_bad),
                ("--camera", "/dev/video1")),
            "frames without capture": (valid_metrics(
                camera=True, frames=2, samples=0, capture=None), ("--camera", "/dev/video1")),
        }
        for name, (metrics, extra) in cases.items():
            with self.subTest(name):
                self.failed(self.ok(metrics), "metrics_invalid", *extra)

    def test_source_claims_and_summary_overflow_must_be_coherent(self):
        cases = [
            (valid_metrics(**{"sample.display_interval_us": summary([], 1000, 500000)}), ()),
            (valid_metrics(**{"sample.processing_us": summary([200000] * 3)}), ()),
            (valid_metrics(camera=True, frames=3, samples=0, fallback=True), ("--camera", "/dev/video0")),
            (valid_metrics(**{"source.final": "camera"}), ()),
            (valid_metrics(**{"capture": {"published": 0, "overwritten_unread": 0,
                                          "stale_dropped": 0, "driver_drops": None}}), ()),
            (valid_metrics(camera=True, frames=3, samples=1), ("--camera", "/dev/video0")),
            (valid_metrics(camera=True, frames=3, samples=1, fallback=True,
                           **{"source.fallback_at_us": None}), ("--camera", "/dev/video0")),
            (valid_metrics(camera=True, frames=3, samples=0,
                           **{"source.first_camera_frame_us": 999999}), ("--camera", "/dev/video0")),
            (valid_metrics(**{"run.clock_errors": False}), ()),
            (valid_metrics(**{"sample.processing_us": {**summary([200000] * 3),
                          "p50_overflow": False, "p95_overflow": False}}), ()),
            (valid_metrics(**{"sample.processing_us": {**summary([]),
                          "overflow_count": 1}}), ()),
        ]
        for metrics, extra in cases:
            with self.subTest(metrics=metrics):
                self.failed(self.ok(metrics), "metrics_invalid", *extra)

    def test_nonzero_exit_fails_even_with_valid_metrics(self):
        code, out, _ = self.invoke(self.ok(valid_metrics(), code=3))
        self.assertEqual(code, 1)
        report = self.report(out)
        self.assertIn("application_nonzero_exit", report["failures"])
        self.assertEqual(report["child"]["exit_code"], 3)

    def test_signaled_child_fails(self):
        binary = self.fake("""
            import os, signal
            os.kill(os.getpid(), signal.SIGUSR1)  # terminates without a core dump
        """)
        code, out, _ = self.invoke(binary)
        self.assertEqual(code, 1)
        report = self.report(out)
        self.assertEqual(report["child"]["signal"], signal.SIGUSR1)
        self.assertIn("child_signaled", report["failures"])

    def test_excessive_output_is_bounded_and_fails(self):
        for stream in ("stdout", "stderr"):
            binary = self.fake(f"""
                import sys
                while True:
                    sys.{stream}.write("x" * 65536)
                    sys.{stream}.flush()
            """)
            started = time.monotonic()
            code, out, _ = self.invoke(binary, duration="100")
            self.assertLess(time.monotonic() - started, 5)
            self.assertEqual(code, 1)
            report = self.report(out)
            self.assertIn("output_limit_exceeded", report["failures"])
            self.assertTrue(report["output"]["limit_exceeded"])
            self.assertLessEqual(report["output"][f"{stream}_kept_bytes"],
                                 getattr(nv, f"MAX_{stream.upper()}"))
            self.assertLessEqual((out / "diagnostics.txt").stat().st_size,
                                 2 * nv.DIAGNOSTIC_BYTES + 200)


class ProcessControl(RunnerCase):
    def test_hang_is_terminated_by_the_watchdog_and_is_a_failure(self):
        pidfile = self.dir / "pid"
        binary = self.fake(f"""
            import os, time
            open({str(pidfile)!r}, "w").write(str(os.getpid()))
            time.sleep(60)
        """)
        started = time.monotonic()
        code, out, _ = self.invoke(binary)
        self.assertLess(time.monotonic() - started, 6)
        self.assertEqual(code, 1)
        report = self.report(out)
        self.assertEqual(report["child"]["status"], "watchdog_terminated")
        self.assertIn("child_watchdog_terminated", report["failures"])
        self.assertEqual(report["child"]["termination"], "sigterm")
        self.assertEqual(report["child"]["signal"], signal.SIGTERM)
        pid = int(pidfile.read_text())
        self.pids.append(pid)
        self.assertTrue(gone(pid))

    def test_child_refusing_termination_and_its_descendants_are_killed(self):
        pids = self.dir / "pids"
        binary = self.fake(f"""
            import os, signal, subprocess, sys, time
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            child = subprocess.Popen([sys.executable, "-c",
                "import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)"])
            open({str(pids)!r}, "w").write(f"{{os.getpid()}} {{child.pid}}")
            while True:
                time.sleep(1)
        """)
        code, out, _ = self.invoke(binary)
        self.assertEqual(code, 1)
        report = self.report(out)
        self.assertEqual(report["child"]["termination"], "sigkill")
        self.assertEqual(report["child"]["signal"], signal.SIGKILL)
        for pid in map(int, pids.read_text().split()):
            self.pids.append(pid)
            self.assertTrue(wait_gone(pid), pid)

    def test_success_with_a_lingering_descendant_leaves_nothing_behind(self):
        pids = self.dir / "pids"
        text = json.dumps(valid_metrics()) + "\n"
        binary = self.fake(f"""
            import subprocess, sys, time
            time.sleep(0.12)
            child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            open({str(pids)!r}, "w").write(str(child.pid))
            sys.stdout.write({text!r})
        """)
        code, _, _ = self.invoke(binary)
        pid = int(pids.read_text())
        self.pids.append(pid)
        self.assertTrue(wait_gone(pid))
        self.assertEqual(code, 0)

    def test_spawn_failure_retains_a_failure_report(self):
        binary = self.dir / "badinterp"
        binary.write_text("#!/nonexistent/interpreter\n")
        binary.chmod(0o755)
        code, out, _ = self.invoke(binary)
        self.assertEqual(code, 1)
        report = self.report(out)
        self.assertEqual(report["child"]["status"], "spawn_failed")
        self.assertIn("child_spawn_failed", report["failures"])
        self.assertIsNone(report["resources"]["cpu_total_s"])
        self.assertIsNone(report["resources"]["peak_rss"]["ru_maxrss_kib"])

    def test_setup_failure_after_spawn_still_reaps_child_and_reports_status(self):
        binary = self.fake("import time; time.sleep(60)")
        real_spawn = nv.spawn_capture
        spawned = []
        def spawn(*args, **kwargs):
            result = real_spawn(*args, **kwargs)
            spawned.append(result[0])
            self.pids.append(result[0])
            return result
        with mock.patch.object(nv, "spawn_capture", side_effect=spawn), \
             mock.patch.object(nv.os, "set_blocking", side_effect=OSError("setup failed")):
            code, out, err = self.invoke(binary)
        self.assertEqual(code, 1, err)
        report = self.report(out)
        self.assertEqual(report["child"]["status"], "supervision_failed")
        self.assertEqual(report["child"]["signal"], signal.SIGKILL)
        self.assertIsNotNone(report["resources"]["cpu_total_s"])
        self.assertTrue(all(gone(pid) for pid in spawned))

    def test_report_write_failure_is_an_error_and_leaves_no_child(self):
        pidfile = self.dir / "pid"
        binary = self.ok(valid_metrics(),
                         extra=f"open({str(pidfile)!r}, 'w').write(str(os.getpid()))")
        with mock.patch.object(nv, "write_private", side_effect=OSError("disk full")):
            code, _, err = self.invoke(binary)
        self.assertEqual(code, 1)
        self.assertIn("report could not be written", err)
        pid = int(pidfile.read_text())
        self.pids.append(pid)
        self.assertTrue(gone(pid))

    def test_cancellation_stops_the_child_and_keeps_a_failure_report(self):
        pidfile = self.dir / "pid"
        binary = self.fake(f"""
            import os, time
            open({str(pidfile)!r}, "w").write(str(os.getpid()))
            time.sleep(60)
        """)
        out = self.dir / "cancel"
        code_text = (
            "import importlib.util, sys\n"
            f"spec = importlib.util.spec_from_file_location('nv', {str(SCRIPT)!r})\n"
            "m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)\n"
            f"m.X11_SOCKET_DIR = {str(self.x_dir)!r}\n"
            "sys.exit(m.main(sys.argv[1:]))\n"
        )
        runner = subprocess.Popen(
            ["/usr/bin/env", sys.executable, "-c", code_text, "--binary", str(binary), "--size", "160x120",
             "--duration-ms", "100", "--source-revision", "r1", "--output-dir", str(out),
             "--watchdog-grace-ms", "60000"],
            env={**os.environ, "DISPLAY": ":77"}, stderr=subprocess.PIPE, text=True, shell=False)
        end = time.monotonic() + 5
        while not pidfile.exists() and time.monotonic() < end:
            time.sleep(0.02)
        self.assertTrue(pidfile.exists())
        time.sleep(0.1)
        runner.send_signal(signal.SIGTERM)
        _, err = runner.communicate(timeout=10)
        self.assertEqual(runner.returncode, 1, err)
        report = self.report(out)
        self.assertEqual(report["child"]["status"], "cancelled")
        self.assertEqual(report["child"]["signal"], signal.SIGKILL)
        self.assertIsNotNone(report["resources"]["cpu_total_s"])
        self.assertIn("child_cancelled", report["failures"])
        pid = int(pidfile.read_text())
        self.pids.append(pid)
        self.assertTrue(wait_gone(pid))


class ResourceAttribution(RunnerCase):
    def test_cpu_and_rss_belong_to_the_current_child(self):
        text = json.dumps(valid_metrics(duration=100)) + "\n"
        heavy = self.fake(f"""
            import sys, time
            time.sleep(0.12)
            block = bytearray(160 * 1024 * 1024)
            for i in range(0, len(block), 4096):
                block[i] = 1
            end = time.process_time() + 0.6
            while time.process_time() < end:
                pass
            sys.stdout.write({text!r})
        """)
        light = self.ok(valid_metrics(duration=100))
        code, first, err = self.invoke(heavy, grace="20000")
        self.assertEqual(code, 0, err)
        code, second, err = self.invoke(light)
        self.assertEqual(code, 0, err)
        a, b = self.report(first)["resources"], self.report(second)["resources"]
        self.assertGreaterEqual(a["cpu_total_s"], 0.5)
        self.assertLess(b["cpu_total_s"], 0.3)
        self.assertGreater(a["peak_rss"]["vmhwm_sampled_kib"], 100_000)
        self.assertLess(b["peak_rss"]["vmhwm_sampled_kib"] or 0, 60_000)
        self.assertGreater(a["peak_rss"]["ru_maxrss_kib"], 100_000)
        self.assertLess(b["peak_rss"]["ru_maxrss_kib"], 100_000)
        for resources in (a, b):
            self.assertAlmostEqual(
                resources["cpu_total_s"], resources["cpu_user_s"] + resources["cpu_system_s"],
                places=5)
            self.assertIn("every thread", resources["cpu_normalization"])
            self.assertIn("not divided", resources["cpu_normalization"])

    def test_cpu_sums_threads_and_ratio_is_not_core_normalized(self):
        text = json.dumps(valid_metrics(duration=100)) + "\n"
        binary = self.fake(f"""
            import sys, threading, time
            time.sleep(0.12)
            def burn():
                end = time.thread_time() + 0.4
                while time.thread_time() < end:
                    pass
            threads = [threading.Thread(target=burn) for _ in range(2)]
            [t.start() for t in threads]
            [t.join() for t in threads]
            sys.stdout.write({text!r})
        """)
        code, out, err = self.invoke(binary, grace="20000")
        self.assertEqual(code, 0, err)
        report = self.report(out)
        resources = report["resources"]
        self.assertGreaterEqual(resources["cpu_total_s"], 0.7)
        self.assertAlmostEqual(resources["cpu_to_wall_ratio"],
                               resources["cpu_total_s"] / report["child"]["wall_s"], places=3)

    def test_unavailable_sampling_is_reported_as_null(self):
        with mock.patch.object(nv, "read_vmhwm_kib", return_value=None):
            code, out, err = self.invoke(self.ok(valid_metrics()))
        self.assertEqual(code, 0, err)
        resources = self.report(out)["resources"]
        self.assertIsNone(resources["peak_rss"]["vmhwm_sampled_kib"])
        self.assertIsInstance(resources["peak_rss"]["ru_maxrss_kib"], int)


class ArgumentsAndDestination(RunnerCase):
    def base(self, **replace):
        binary = self.ok(valid_metrics())
        argv = {"--binary": str(binary), "--size": "160x120", "--duration-ms": "100",
                "--source-revision": "rev", "--output-dir": str(self.dir / "new")}
        argv.update(replace)
        return [item for pair in argv.items() for item in pair]

    def usage_error(self, argv):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = nv.main(argv)
        self.assertEqual(code, 2, err.getvalue())
        return err.getvalue()

    def test_invalid_arguments_exit_two_before_creating_anything(self):
        for replace in (
            {"--duration-ms": "99"}, {"--duration-ms": "600001"}, {"--duration-ms": "0"},
            {"--duration-ms": "1e3"}, {"--size": "640x480"}, {"--source-revision": "a b"},
            {"--source-revision": ""}, {"--binary": str(self.dir / "missing")},
            {"--binary": str(self.dir)}, {"--watchdog-grace-ms": "50"},
            {"--camera": "video0"}, {"--camera": "/dev/video01"}, {"--camera": "/dev/video65536"}, {"--camera": "/dev/video0;x"},
        ):
            with self.subTest(replace):
                self.usage_error(self.base(**replace))
        self.usage_error(self.base() + ["--expect-unplug"])
        self.assertFalse((self.dir / "new").exists())

    def test_relative_binary_executes_the_hashed_file_without_path_lookup(self):
        binary = self.ok(valid_metrics())
        before = Path.cwd()
        try:
            os.chdir(self.dir)
            code, out, err = self.invoke(Path(binary.name))
        finally:
            os.chdir(before)
        self.assertEqual(code, 0, err)
        self.assertEqual(self.report(out)["binary"]["sha256"],
                         hashlib.sha256(binary.read_bytes()).hexdigest())

    def test_binary_must_not_be_a_symlink_or_non_executable(self):
        real = self.ok(valid_metrics())
        link = self.dir / "link"
        link.symlink_to(real)
        self.usage_error(self.base(**{"--binary": str(link)}))
        plain = self.dir / "plain"
        plain.write_text("x")
        self.usage_error(self.base(**{"--binary": str(plain)}))

    def test_missing_or_unusable_display_is_refused(self):
        for value in (None, "", "garbage", "host:0", ":78"):
            env = {k: v for k, v in os.environ.items() if k != "DISPLAY"}
            if value is not None:
                env["DISPLAY"] = value
            with self.subTest(value), mock.patch.dict(os.environ, env, clear=True):
                self.assertIn("DISPLAY", self.usage_error(self.base()))
        self.assertFalse((self.dir / "new").exists())

    def test_existing_directory_and_symlink_outputs_are_refused_and_preserved(self):
        existing = self.dir / "existing"
        existing.mkdir()
        keep = existing / "report.json"
        keep.write_text("precious")
        self.usage_error(self.base(**{"--output-dir": str(existing)}))
        self.assertEqual(keep.read_text(), "precious")
        target = self.dir / "target"
        target.mkdir()
        (target / "report.json").write_text("precious")
        link = self.dir / "link"
        link.symlink_to(target)
        dangling = self.dir / "dangling"
        dangling.symlink_to(self.dir / "nowhere")
        for path in (link, dangling):
            self.usage_error(self.base(**{"--output-dir": str(path)}))
        self.assertEqual((target / "report.json").read_text(), "precious")
        self.assertFalse((self.dir / "nowhere").exists())
        file_output = self.dir / "afile"
        file_output.write_text("keep")
        self.usage_error(self.base(**{"--output-dir": str(file_output)}))
        self.assertEqual(file_output.read_text(), "keep")

    def test_output_within_checkout_requires_ignored_folder(self):
        message = self.usage_error(self.base(**{"--output-dir": str(ROOT / "unsafe-report")}))
        self.assertIn("native-validation-local", message)
        self.assertFalse((ROOT / "unsafe-report").exists())

    def test_output_parent_must_exist(self):
        self.usage_error(self.base(**{"--output-dir": str(self.dir / "no" / "dir")}))

    def test_help_is_side_effect_free_and_documents_exit_codes(self):
        result = subprocess.run(["/usr/bin/env", sys.executable, str(SCRIPT), "--help"], capture_output=True,
                                text=True, env={"PATH": os.environ["PATH"]}, timeout=10, shell=False)
        self.assertEqual(result.returncode, 0)
        self.assertIn("Exit codes", result.stdout)
        self.assertIn("--expect-unplug", result.stdout)

    def test_arguments_are_passed_as_an_array_without_a_shell(self):
        trace = self.dir / "argv.json"
        canned = json.dumps(valid_metrics("320x240", 150)) + "\n"
        binary = self.fake(f"""
            import json, sys
            json.dump(sys.argv[1:], open({str(trace)!r}, "w"))
            sys.stdout.write({canned!r})
        """)
        # The canned metrics say reduced_motion=false/fit=cover, so they are rejected.
        code, _, err = self.invoke(binary, "--reduced-motion", "--fit", "contain",
                                   size="320x240", duration="150")
        self.assertEqual(code, 1, err)
        self.assertEqual(json.loads(trace.read_text()),
                         ["--metrics", "--run-for-ms", "150", "--resolution", "320x240",
                          "--fit", "contain", "--reduced-motion"])
        self.invoke(binary, "--camera", "/dev/video7", size="320x240", duration="150")
        self.assertEqual(json.loads(trace.read_text())[-2:], ["--camera", "/dev/video7"])


if __name__ == "__main__":
    unittest.main()
