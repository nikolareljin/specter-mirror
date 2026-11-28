# Native validation

This page defines the opt-in measurements of the native kiosk, the local runner
that collects them and the checklist an operator follows on a target laptop.
Nothing here is a result. No physical camera, antiX guest or target laptop has
been measured with these tools, and SM-09 stays open until its physical
criteria are observed. The runner prepares evidence collection only.

## Evidence levels

Keep these separate in every report and in any claim built from one.

| Level | What it shows | What it does not show |
| --- | --- | --- |
| Host unit and integration tests | Logic, counters and parsing with fake devices and clocks | Real timing, drivers, displays |
| Xvfb run on a workstation | Rendering through a virtual X11 display and the runner end to end | Physical display, camera, target CPU |
| Linked i686 binary | That the executable links, plus `file`/`ldd` inspection | That it runs on antiX or on a CPU without SSE2 |
| antiX guest (VM) | Execution in the intended OS, still on workstation hardware | Target CPU, camera, laptop display |
| Physical laptop | Actual format list, stride, timing, memory, unplug behavior | Other laptops or camera models |

## Native options

`spook-kiosk --metrics` prints one JSON object to stdout after the render loop
has ended and the capture worker is joined. `--run-for-ms N` (100..600000,
canonical decimal) ends the run through the normal path: the loop exits, the
worker is cancelled and joined, the device is closed. Escape and window close
still end a run early. Both options may be used separately, may not repeat and
are rejected with `--self-test` or `--help` before a window or device opens.
Without them the pixels and lifecycle are unchanged and no clock is read for
measurement. Existing diagnostics stay on stderr. An error exit keeps its
nonzero status after a partial report is printed, including startup failure.
Per-source populations cover completed display submissions; an iteration that
fails before submission is omitted from those populations.

## Metrics contract (schema `specter-mirror-native-metrics`, version 1)

All times are integers. A `_us` field is microseconds on a process-local
monotonic clock. Fields hold only numbers, booleans and fixed labels. They hold
no paths, device identifiers, hashes, environment values or image data.

| Field | Meaning |
| --- | --- |
| `run.exit_reason` | `timed`, `escape`, `window_closed` or `error` |
| `run.elapsed_us` | Whole run: metrics created to after worker cleanup. Equals `startup_us + loop_us + shutdown_us` |
| `run.startup_us` | Metrics creation (after argument parsing) to the first loop iteration. Includes window creation and starting the capture worker |
| `run.loop_us` | First loop iteration to loop exit |
| `run.shutdown_us` | Cancel, join and device close |
| `run.clock_errors` | Intervals discarded because the clock went backward |
| `config` | Processing size, fit, delay, trails, reduced motion and the 30 FPS target passed to the window library |
| `source.requested` / `final` | `sample` or `camera`. `final` is the source active at the last submission |
| `source.fallback_occurred` | Camera requested and not the active source at exit. Also true when the capture worker could not start |
| `source.fallback_at_us` | Loop-relative time of the first sample submission after camera use, else `null` |
| `source.first_camera_frame_us` | Loop-relative time at which the first new camera frame finished processing, else `null`. This is not exposure latency |
| `display_submissions` | Iterations that submitted a buffer to the window, both sources |
| `camera` / `sample` | Per-source statistics, described below. A source that never ran has zero counts and `null` summaries |
| `capture` | Camera exchange counters, or `null` when streaming never started |

Per source:

| Field | Meaning |
| --- | --- |
| `submissions` | Display submissions while this source was active |
| `frames` (camera) / `updates` (sample) | New camera frames converted and processed, or generated sample updates. Each new item counts once |
| `idle_ticks` | Submissions that re-showed the previous pixels without new source data. They never count as camera frames and have no processing sample |
| `processing_us` | One sample per new item: from before the source update to after conversion and temporal processing. For the camera this includes taking the newest frame, the integer conversion and the history step, and excludes presentation |
| `prepare_us` | Presentation crop and pack, one sample per submission |
| `submit_us` | Window submission call. The window library paces to its 30 FPS target inside this call, so it contains waiting. It is not processing time |
| `display_interval_us` | Time between consecutive submissions of the same source. An interval that spans a source change is dropped. It is a display cadence, separate from processing time and from the configured 30 FPS |

`capture` holds `published` (complete frames accepted into the exchange),
`overwritten_unread` (an unread frame replaced by a newer one), `stale_dropped`
(an unread frame discarded because it was more than 500 ms old) and
`driver_drops`. `driver_drops` is always `null`: the application does not count
driver-side drops, and nothing estimates them. Overwrite, stale drops, driver
drops and idle display ticks are different events. The counters are read after
the worker is joined (after fallback, once the retired worker finishes), so
they survive fallback and shutdown.

### Summaries and bounds

Each summary has `count`, `p50_us`, `p95_us`, `max_us`, `bin_us`, `range_us`,
`overflow_count` and `p50_overflow`/`p95_overflow`. Storage is a fixed-size
histogram allocated once at start. Duration histograms use 400 bins of
250 us (range 100 ms). Interval histograms use 500 bins of 1 ms (range
500 ms). Values at or beyond the range go to an overflow counter. `count`,
`max_us` and `overflow_count` are exact. Percentiles use the nearest-rank
method and report the inclusive upper edge of the containing bin, capped at the
exact maximum. An in-range percentile is never below the true value and is at
most one bin above it. When the rank falls in the overflow population, the
value is the exact maximum and the overflow flag is `true`; its error can
exceed one bin. An empty population
reports `null` for every percentile and the maximum, never zero. Memory does
not grow with run length, and recording does not allocate.

## Runner

```bash
python3 scripts/native_validation.py --binary ./spook-kiosk --size 160x120 \
  --duration-ms 30000 --source-revision <label> --output-dir ~/validation/run1
```

Add `--camera /dev/videoN` to open that one device. The runner never discovers
or tries devices. Other options: `--reduced-motion`, `--fit`, `--expect-unplug`
(the operator will unplug the camera during the run) and
`--watchdog-grace-ms` (100..60000, default 10000).

The runner requires Linux, Python 3.9 or newer and an explicit existing executable file
(not a symlink). `DISPLAY` must name a local X display (`:N`) that accepts a
connection. Remote display forms are refused. The output directory must not
exist; an existing path or symlink is refused and left alone. Reports inside
the repository must go under `native-validation-local/`, which Git ignores.
Prefer a directory outside the checkout. Arguments are validated before
anything is created or launched. The runner never uploads, installs, uses sudo
or changes permissions or session configuration.

It launches the executable with POSIX spawn file actions from an argument
array without a shell or Python pre-exec work, in its own process group, with
stdin connected to the null device. Stdout and stderr are each limited to 64 KiB;
exceeding either terminates the child and fails the run. The child gets the
requested duration plus the watchdog grace. After that the group receives
SIGTERM, then SIGKILL after 2 seconds. Watchdog termination is a failure.
Cancellation (SIGTERM or Ctrl-C on the runner) kills the supervised group,
reaps the child and keeps a failure report with its actual status and available
resource usage. After every run the process group is killed and the
child reaped.

### Resource measurement

* CPU is `ru_utime + ru_stime` from `wait4` for this child, in seconds. It sums
  every thread. `cpu_to_wall_ratio` is `cpu_total_s / wall_s`. It is aggregate:
  1.0 means one core fully busy on average, and it is not divided by the number
  of cores. Do not compare it with a figure from another machine.
* Peak RSS is reported as two numbers in KiB. `vmhwm_sampled_kib` is the highest
  `VmHWM` read from `/proc/<pid>/status` about every 20 ms (a lower bound).
  `ru_maxrss_kib` is the kernel's figure from `wait4` for this child (an upper
  bound, because it can include the launcher's resident size at fork). The
  true peak lies between them. A value that cannot be read is `null`.
* `cpu_scope` is `whole_process_aggregate_single_source`, or
  `whole_process_aggregate_mixed_sources` when the run included a fallback or
  never delivered camera frames. A mixed value is not a camera-only CPU figure.
* CPU and RAM are never inferred from buffer sizes or taken from another
  machine.

### Report files

The output directory (mode 0700) holds `report.json` and `diagnostics.txt`
(both 0600). `report.json` holds the verdict, failure codes, evidence class,
run options, the executable's SHA-256 and size, the operator's
`source_revision_asserted` label (an assertion, not a proven mapping to the
binary), child status, resources, output sizes and the validated metrics
object. `diagnostics.txt` holds at most 16 KiB each of the child's stderr and
first stdout bytes. Inspect and redact it before sharing. Raw reports stay
outside Git.

The runner accepts exactly one metrics line. It refuses missing, malformed,
truncated, duplicate-key, non-finite, fractional or contradictory data
(mismatched counts, unordered percentiles, a loop shorter than requested,
elapsed time longer than the supervised wall time, capture counters that do not
add up, an unplugged-camera claim without fallback). A nonzero application exit
fails even with valid metrics.

| Evidence class | Outcome |
| --- | --- |
| `sample_uninterrupted` | Passes the mechanical checks |
| `camera_uninterrupted` | Camera frames and no fallback. Passes the mechanical checks |
| `camera_requested_no_frames` | Fails. A camera that delivered no frames is fallback evidence |
| `camera_interrupted` | Fails. The camera fell back without `--expect-unplug` |
| `camera_unplug_fallback_observation` | Passes only with `--expect-unplug` and an observed fallback. It describes the unplug path, not uninterrupted camera performance |

A passed report means the run and its numbers are coherent. It is not a
performance acceptance. Exit codes: 0 passed, 1 failed, 2 invalid usage or
environment.

## Operator checklist

No target build toolchain, new camera backend, installer change or release is
part of this work.

1. Build the i686 executable on the development host (`./dev build-i686`).
   Record `sha256sum` and `file`. Copy only the binary to the target and
   compare the checksum there.
2. Run `bash scripts/discover_hardware.sh > hardware-local.txt` on the target.
   Record exact hardware, OS, libc version, CPU flags (including SSE2),
   graphics and camera formats. Do not publish raw device identifiers.
3. Confirm the executable runs (`--help`) and that `ldd` finds every library.
4. Sample baseline: for each size (320x240, 160x120), run the runner for the
   baseline duration. Use at least 30 seconds, and record that choice.
5. Repeat with `--reduced-motion` and each `--fit` mode. Observe fit and
   legibility at the real display size by eye.
6. Observe startup: window appears, first frame appears, Escape and window
   close each exit cleanly. Repeat start and stop at least ten times.
7. Camera, with an explicit `--camera /dev/videoN` the operator chooses: run at
   both sizes. Confirm frames arrive, the history resets at fallback, and the
   device is released afterward (no process holds it).
8. Silent or stale capture: block or stall the camera if the hardware allows,
   and confirm fallback to the sample within the documented limits.
9. Unplug fallback: run with `--expect-unplug` and unplug during the run.
10. Run offline: disconnect the network and repeat a sample run.
11. Measured run: use a longer duration (the operator decides; 5 minutes is a
    reasonable starting point) at each size and source.
12. Store reports outside Git, record the operator's source revision label with
    each, and write the hardware and OS record separately.

Numeric performance thresholds (frame processing time, cadence, CPU, memory)
need the operator's agreement and actual target measurements. No default in
this tooling declares a laptop passed. Run on both the Toshiba NB205 and Dell
D630 when available, and treat each unit separately. Passing this checklist
would not complete the ISO, supply artwork or support a daily-driver claim.
