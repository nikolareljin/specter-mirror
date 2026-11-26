import test from 'node:test';
import assert from 'node:assert/strict';
import {
  CAMERA_CONSTRAINTS,
  FRAME_HEIGHT,
  FRAME_WIDTH,
  MirrorController,
  describeCameraError,
  samplePhase,
} from '../src/mirror.ts';
import { sampleHeadBounds } from '../src/sample.ts';

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function fakeStream() {
  const listeners = [];
  const track = {
    stopped: 0,
    stop() {
      this.stopped += 1;
    },
    addEventListener(type, listener) {
      if (type === 'ended') listeners.push(listener);
    },
  };
  return { stream: { getTracks: () => [track] }, track, end: () => listeners.forEach((l) => l()) };
}

function setup(overrides = {}) {
  const log = {
    userMedia: 0,
    engines: [],
    attached: 0,
    detached: 0,
    rendered: [],
    captured: [],
    calls: [],
  };
  let clock = 0;
  const pending = new Map();
  let nextHandle = 1;
  const deps = {
    getUserMedia: async () => {
      log.userMedia += 1;
      return fakeStream().stream;
    },
    createEngine: (w, h) => {
      const engine = {
        w,
        h,
        freed: 0,
        configured: [],
        timed: [],
        automatic: [],
        automaticCalls: [],
        configure_automatic(enabled, reduced) {
          if (overrides.configureError) throw new Error('configuration failed');
          this.automatic.push({ enabled, reduced });
        },
        process_automatic_rgba(input, elapsed) {
          this.automaticCalls.push(elapsed);
          return this.process_rgba(input, 0, 0);
        },
        haunt_state() { return 2; },
        haunt_strength() { return 1500; },
        motion_activity() { return 10; },
        configure_temporal(delay, strength) {
          if (overrides.configureError) throw new Error('configuration failed');
          this.configured.push({ delay, strength });
        },
        process_timed_rgba(input, mode, intensity, elapsed) {
          this.timed.push(elapsed);
          return this.process_rgba(input, mode, intensity);
        },
        process_rgba(input, mode, intensity) {
          log.calls.push({ mode, intensity, length: input.length });
          if (overrides.processError) throw new Error('boom');
          return new Uint8Array(input.length).fill(mode + 1);
        },
        skull: [],
        observed: [],
        cleared: 0,
        seeds: [],
        configure_seed(seed) { this.seeds.push(seed); },
        configure_skull(enabled) {
          if (overrides.skullConfigError) throw new Error('skull configuration failed');
          this.skull.push(enabled);
        },
        observe_head(...args) {
          if (overrides.observeError) throw new Error('observation rejected');
          this.observed.push(args);
        },
        clear_head() {
          this.cleared += 1;
        },
        skull_opacity() { return 0; },
        free() {
          this.freed += 1;
        },
      };
      log.engines.push(engine);
      return engine;
    },
    attachStream: async () => {
      log.attached += 1;
    },
    detachStream: () => {
      log.detached += 1;
    },
    captureFrame: (source, elapsed) => {
      log.captured.push({ source, elapsed });
      return new Uint8Array(FRAME_WIDTH * FRAME_HEIGHT * 4);
    },
    render: (rgba) => log.rendered.push(rgba),
    schedule: (cb) => {
      const handle = nextHandle++;
      pending.set(handle, cb);
      return handle;
    },
    cancel: (handle) => pending.delete(handle),
    now: () => {
      clock += 10;
      return clock;
    },
    ...overrides.deps,
  };
  const statuses = [];
  const controller = new MirrorController(deps, (s) => statuses.push(s));
  const step = () => {
    const [handle, cb] = [...pending.entries()][0];
    pending.delete(handle);
    cb();
  };
  return { controller, log, statuses, pending, step };
}

const last = (statuses) => statuses[statuses.length - 1];

test('nothing is requested on construction', () => {
  const { log, pending, statuses } = setup();
  assert.equal(log.userMedia, 0);
  assert.equal(log.engines.length, 0);
  assert.equal(pending.size, 0);
  assert.equal(last(statuses).state, 'idle');
});

test('camera constraints are video only, 320x240 and prefer a facing mode', () => {
  assert.equal(CAMERA_CONSTRAINTS.audio, false);
  assert.deepEqual(CAMERA_CONSTRAINTS.video.width, { ideal: 320 });
  assert.deepEqual(CAMERA_CONSTRAINTS.video.height, { ideal: 240 });
  assert.deepEqual(CAMERA_CONSTRAINTS.video.facingMode, { ideal: 'user' });
});

test('temporal controls configure the engine and timed frames receive integer elapsed time', async () => {
  const { controller, log, step } = setup();
  controller.setTemporal(250, 115);
  await controller.startSample();
  assert.deepEqual(log.engines[0].configured, [{ delay: 250, strength: 115 }]);
  step(); step();
  assert.equal(log.engines[0].timed.length, 2);
  assert.ok(log.engines[0].timed.every(Number.isSafeInteger));
  assert.ok(log.engines[0].timed[1] >= log.engines[0].timed[0]);
  controller.setTemporal(0, 128);
  assert.deepEqual(log.engines[0].configured.at(-1), { delay: 0, strength: 128 });
  controller.stop();
});

test('reduced motion disables temporal layers and restores the requested combination', async () => {
  const { controller, log } = setup();
  controller.setTemporal(300, 100);
  controller.setReducedMotion(true);
  await controller.startSample();
  const engine = log.engines[0];
  assert.deepEqual(engine.configured, [{ delay: 0, strength: 0 }]);
  controller.setTemporal(600, 255);
  assert.deepEqual(engine.configured.at(-1), { delay: 0, strength: 0 });
  controller.setReducedMotion(false);
  assert.deepEqual(engine.configured.at(-1), { delay: 600, strength: 255 });
  controller.stop();
});

test('invalid temporal settings are ignored without resetting working history', async () => {
  const { controller, log } = setup();
  controller.setTemporal(200, 100);
  await controller.startSample();
  const engine = log.engines[0];
  for (const [delay, strength] of [[NaN, 0], [-1, 0], [601, 0], [0.5, 0],
    [0, Infinity], [0, -1], [0, 256], [0, 1.5]]) controller.setTemporal(delay, strength);
  controller.setTemporal(200, 100);
  assert.deepEqual(engine.configured, [{ delay: 200, strength: 100 }]);
  controller.stop();
});

test('configuration failure cleans up acquired camera and engine', async () => {
  const media = fakeStream();
  const { controller, log } = setup({ configureError: true,
    deps: { getUserMedia: async () => media.stream } });
  await controller.startCamera();
  assert.equal(controller.getState(), 'error');
  assert.equal(media.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(log.attached, 0);
});

test('automatic processing reports shared state and switching back restores manual controls', async () => {
  const { controller, log, statuses, step } = setup();
  controller.setMode(2);
  controller.setIntensity(80);
  controller.setTemporal(300, 100);
  controller.setAutomatic(true);
  await controller.startSample();
  const engine = log.engines[0];
  assert.deepEqual(engine.automatic, [{ enabled: true, reduced: false }]);
  assert.deepEqual(engine.configured, []);
  step();
  assert.equal(engine.automaticCalls.length, 1);
  assert.equal(engine.timed.length, 0);
  assert.equal(last(statuses).hauntState, 'Transforming');
  assert.equal(last(statuses).hauntStrength, 1500);
  controller.setAutomatic(false);
  assert.deepEqual(engine.configured.at(-1), { delay: 300, strength: 100 });
  step();
  assert.equal(engine.timed.length, 1);
  assert.equal(log.calls.at(-1).mode, 2);
  assert.equal(log.calls.at(-1).intensity, 80);
  assert.equal(last(statuses).hauntState, null);
  controller.stop();
});

test('automatic reduced-motion preferences and startup failures preserve cleanup', async () => {
  const { controller, log } = setup();
  controller.setAutomatic(true);
  await controller.startSample();
  controller.setReducedMotion(true);
  assert.deepEqual(log.engines[0].automatic.at(-1), { enabled: true, reduced: true });
  controller.stop();
  const media = fakeStream();
  const failed = setup({ configureError: true, deps: { getUserMedia: async () => media.stream } });
  failed.controller.setAutomatic(true);
  await failed.controller.startCamera();
  assert.equal(failed.controller.getState(), 'error');
  assert.equal(media.track.stopped, 1);
  assert.equal(failed.log.engines[0].freed, 1);
});

test('sample start does not ask for camera permission and renders frames', async () => {
  const { controller, log, step, statuses } = setup();
  await controller.startSample();
  assert.equal(log.userMedia, 0);
  assert.equal(controller.getState(), 'sample');
  step();
  step();
  step();
  assert.equal(log.rendered.length, 3);
  assert.equal(log.captured[0].source, 'sample');
  assert.equal(log.calls[0].length, FRAME_WIDTH * FRAME_HEIGHT * 4);
  const status = last(statuses);
  assert.ok(status.fps > 0);
  assert.ok(status.processMs > 0 && status.captureMs > 0 && status.renderMs > 0);
  assert.match(status.message, /not webcam footage/);
});

test('camera start attaches the stream and stop releases everything', async () => {
  const cam = fakeStream();
  const { controller, log, step, pending } = setup({
    deps: { getUserMedia: async () => cam.stream },
  });
  await controller.startCamera();
  assert.equal(controller.getState(), 'camera');
  assert.equal(log.attached, 1);
  step();
  assert.equal(log.rendered.length, 1);
  controller.stop();
  assert.equal(cam.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
  assert.ok(log.detached >= 1);
  assert.equal(controller.getState(), 'idle');
  controller.stop();
  assert.equal(log.engines[0].freed, 1, 'a second stop does not double-free');
});

test('repeated starts keep one frame loop and release abandoned resources', async () => {
  const streams = [];
  const { controller, log, pending } = setup({
    deps: {
      getUserMedia: async () => {
        const s = fakeStream();
        streams.push(s);
        return s.stream;
      },
    },
  });
  await controller.startCamera();
  await controller.startCamera();
  await controller.startCamera();
  assert.equal(pending.size, 1);
  assert.equal(streams[0].track.stopped, 1);
  assert.equal(streams[1].track.stopped, 1);
  assert.equal(streams[2].track.stopped, 0);
  assert.equal(log.engines.filter((e) => e.freed === 0).length, 1);
  await controller.startSample();
  assert.equal(pending.size, 1);
  assert.equal(streams[2].track.stopped, 1);
  assert.equal(log.engines.filter((e) => e.freed === 0).length, 1);
});

test('rapid concurrent camera clicks stop every abandoned stream', async () => {
  const gates = [deferred(), deferred(), deferred()];
  const streams = gates.map(() => fakeStream());
  let call = 0;
  const { controller, pending, log } = setup({
    deps: { getUserMedia: () => gates[call++].promise },
  });
  const runs = [controller.startCamera(), controller.startCamera(), controller.startCamera()];
  gates.forEach((g, i) => g.resolve(streams[i].stream));
  await Promise.all(runs);
  assert.equal(streams[0].track.stopped, 1);
  assert.equal(streams[1].track.stopped, 1);
  assert.equal(streams[2].track.stopped, 0);
  assert.equal(pending.size, 1);
  assert.equal(log.engines.length, 1);
  assert.equal(log.attached, 1);
});

test('late getUserMedia result after stop is stopped and never renders', async () => {
  const gate = deferred();
  const late = fakeStream();
  const { controller, log, pending } = setup({ deps: { getUserMedia: () => gate.promise } });
  const run = controller.startCamera();
  controller.stop();
  gate.resolve(late.stream);
  await run;
  assert.equal(late.track.stopped, 1);
  assert.equal(pending.size, 0);
  assert.equal(log.engines.length, 0);
  assert.equal(log.attached, 0);
  assert.equal(controller.getState(), 'idle');
});

test('late getUserMedia result after switching to the sample is stopped', async () => {
  const gate = deferred();
  const late = fakeStream();
  const { controller, log, pending } = setup({ deps: { getUserMedia: () => gate.promise } });
  const run = controller.startCamera();
  await controller.startSample();
  gate.resolve(late.stream);
  await run;
  assert.equal(late.track.stopped, 1);
  assert.equal(controller.getState(), 'sample');
  assert.equal(pending.size, 1);
  assert.equal(log.engines.length, 1);
  assert.equal(log.attached, 0);
});

test('late engine creation after stop frees the engine and stops the stream', async () => {
  const gate = deferred();
  const cam = fakeStream();
  const engine = { freed: 0, process_rgba: () => new Uint8Array(), free() { this.freed += 1; } };
  const { controller, pending } = setup({
    deps: { getUserMedia: async () => cam.stream, createEngine: () => gate.promise },
  });
  const run = controller.startCamera();
  await Promise.resolve();
  await Promise.resolve();
  controller.stop();
  gate.resolve(engine);
  await run;
  assert.equal(engine.freed, 1);
  assert.equal(cam.track.stopped >= 1, true);
  assert.equal(pending.size, 0);
});

test('permission rejection reports status, keeps controls usable and allows the sample', async () => {
  const denied = Object.assign(new Error('no'), { name: 'NotAllowedError' });
  const { controller, log, statuses, pending } = setup({
    deps: { getUserMedia: async () => Promise.reject(denied) },
  });
  await controller.startCamera();
  assert.equal(controller.getState(), 'error');
  assert.match(last(statuses).message, /permission was denied/);
  assert.match(last(statuses).message, /sample scene/);
  assert.equal(log.engines.length, 0);
  assert.equal(pending.size, 0);
  await controller.startSample();
  assert.equal(controller.getState(), 'sample');
  assert.equal(pending.size, 1);
});

test('missing and busy cameras have distinct messages', () => {
  assert.match(describeCameraError({ name: 'NotFoundError' }), /No camera was found/);
  assert.match(describeCameraError({ name: 'NotReadableError' }), /unavailable or in use/);
  assert.match(describeCameraError(new Error('x')), /could not be started/);
  assert.match(describeCameraError(undefined), /could not be started/);
  assert.match(describeCameraError(null), /could not be started/);
});

test('track loss releases the camera and stops the loop', async () => {
  const cam = fakeStream();
  const { controller, log, pending, statuses, step } = setup({
    deps: { getUserMedia: async () => cam.stream },
  });
  await controller.startCamera();
  step();
  cam.end();
  assert.equal(controller.getState(), 'error');
  assert.match(last(statuses).message, /disconnected/);
  assert.equal(cam.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
});

test('track loss from an abandoned stream does not stop the current source', async () => {
  const streams = [];
  const { controller, pending } = setup({
    deps: {
      getUserMedia: async () => {
        const s = fakeStream();
        streams.push(s);
        return s.stream;
      },
    },
  });
  await controller.startCamera();
  await controller.startCamera();
  streams[0].end();
  assert.equal(controller.getState(), 'camera');
  assert.equal(pending.size, 1);
});

test('track loss while WASM is loading cannot start a dead camera', async () => {
  const gate = deferred();
  const cam = fakeStream();
  const engine = { freed: 0, process_rgba: () => new Uint8Array(), free() { this.freed += 1; } };
  const { controller, pending, log } = setup({
    deps: { getUserMedia: async () => cam.stream, createEngine: () => gate.promise },
  });
  const run = controller.startCamera();
  await Promise.resolve();
  cam.end();
  gate.resolve(engine);
  await run;
  assert.equal(controller.getState(), 'error');
  assert.equal(pending.size, 0);
  assert.equal(log.attached, 0);
  assert.equal(engine.freed, 1);
});

test('video.play failure cleans up and leaves the sample available', async () => {
  const cam = fakeStream();
  const { controller, log, pending, statuses } = setup({
    deps: {
      getUserMedia: async () => cam.stream,
      attachStream: async () => {
        throw new Error('play blocked');
      },
    },
  });
  await controller.startCamera();
  assert.equal(controller.getState(), 'error');
  assert.match(last(statuses).message, /play blocked/);
  assert.equal(cam.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
  await controller.startSample();
  assert.equal(controller.getState(), 'sample');
});

test('engine initialization failure cleans up the stream', async () => {
  const cam = fakeStream();
  const { controller, pending } = setup({
    deps: {
      getUserMedia: async () => cam.stream,
      createEngine: async () => {
        throw new Error('wasm failed');
      },
    },
  });
  await controller.startCamera();
  assert.equal(controller.getState(), 'error');
  assert.equal(cam.track.stopped, 1);
  assert.equal(pending.size, 0);
  await controller.startSample();
  assert.equal(controller.getState(), 'error', 'sample reports the same failure and stays usable');
});

test('processing failure frees the engine, stops the loop and reports status', async () => {
  const { controller, log, pending, statuses, step } = setup({ processError: true });
  await controller.startSample();
  step();
  assert.equal(controller.getState(), 'error');
  assert.match(last(statuses).message, /boom/);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
  assert.equal(log.rendered.length, 0);
});

test('capture failure is handled like a processing failure', async () => {
  const cam = fakeStream();
  const { controller, pending, log, step } = setup({
    deps: {
      getUserMedia: async () => cam.stream,
      captureFrame: () => {
        throw new Error('draw failed');
      },
    },
  });
  await controller.startCamera();
  step();
  assert.equal(controller.getState(), 'error');
  assert.equal(cam.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
});

test('not-ready frames are skipped without rendering and the loop continues', async () => {
  const { controller, log, pending, step } = setup({ deps: { captureFrame: () => null } });
  await controller.startSample();
  step();
  step();
  assert.equal(log.rendered.length, 0);
  assert.equal(pending.size, 1);
});

test('at most one frame callback is ever pending', async () => {
  const { controller, pending, step } = setup();
  await controller.startSample();
  for (let i = 0; i < 5; i += 1) {
    assert.equal(pending.size, 1);
    step();
  }
  assert.equal(pending.size, 1);
});

test('mode and intensity reach the engine, invalid values are handled', async () => {
  const { controller, log, step } = setup();
  await controller.startSample();
  controller.setMode(2);
  controller.setIntensity(77.6);
  step();
  controller.setMode(9);
  controller.setIntensity(9999);
  step();
  controller.setIntensity(-5);
  step();
  assert.deepEqual(
    log.calls.map((c) => [c.mode, c.intensity]),
    [
      [2, 78],
      [2, 255],
      [2, 0],
    ],
  );
});

test('dispose (pagehide) releases the camera', async () => {
  const cam = fakeStream();
  const { controller, log, pending } = setup({ deps: { getUserMedia: async () => cam.stream } });
  await controller.startCamera();
  controller.dispose();
  assert.equal(cam.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
});

test('sample phase is deterministic and static under reduced motion', () => {
  assert.equal(samplePhase(1234, false), samplePhase(1234, false));
  assert.notEqual(samplePhase(0, false), samplePhase(500, false));
  assert.equal(samplePhase(0, true), samplePhase(99999, true));
  for (const ms of [0, 1000, 12345, 1e9]) {
    const p = samplePhase(ms, false);
    assert.ok(p >= 0 && p < Math.PI * 2);
  }
});

// Skull apparitions and head observations.
function headProvider(calls) {
  return {
    observeHead: (source, elapsed) => {
      calls.push({ source, elapsed });
      if (source !== 'sample') return null;
      const bounds = sampleHeadBounds(FRAME_WIDTH, FRAME_HEIGHT, elapsed, false);
      return bounds ? { ...bounds, confidence: 1000 } : null;
    },
  };
}

test('skull option is off by default and the provider is never consulted', async () => {
  const calls = [];
  const { controller, log, step } = setup({ deps: headProvider(calls) });
  await controller.startSample();
  step(); step();
  assert.deepEqual(calls, []);
  assert.deepEqual(log.engines[0].skull, [false]);
  assert.equal(log.engines[0].observed.length + log.engines[0].cleared, 0);
});

test('capture, head provider and engine share one elapsed clock', async () => {
  const calls = [];
  const { controller, log, step } = setup({ deps: headProvider(calls) });
  controller.setSkull(true);
  controller.setAutomatic(true);
  await controller.startSample();
  step(); step(); step();
  const engine = log.engines[0];
  assert.deepEqual(engine.skull, [true]);
  assert.equal(calls.length, 3);
  assert.deepEqual(calls.map((c) => c.elapsed), log.captured.map((c) => c.elapsed));
  assert.ok(calls.every((c) => c.source === 'sample'));
  const floors = calls.map((c) => Math.floor(c.elapsed));
  assert.deepEqual(engine.automaticCalls, floors);
  // Sample at t=0 has an entering, clipped visitor: no usable head yet.
  const expected = floors.map((_, i) => sampleHeadBounds(FRAME_WIDTH, FRAME_HEIGHT, calls[i].elapsed, false));
  assert.equal(engine.observed.length, expected.filter(Boolean).length);
  assert.equal(engine.cleared, expected.filter((b) => !b).length);
});

test('a head observation is passed with its confidence and the processing time', async () => {
  const head = { x: 100, y: 40, width: 30, height: 40, confidence: 1000 };
  const { controller, log, step } = setup({ deps: { observeHead: () => head } });
  controller.setSkull(true);
  controller.setAutomatic(true);
  await controller.startSample();
  step();
  const [observed] = log.engines[0].observed;
  assert.deepEqual(observed.slice(0, 5), [100, 40, 30, 40, 1000]);
  assert.equal(observed[5], log.engines[0].automaticCalls[0]);
  assert.equal(log.engines[0].cleared, 0);
});

test('camera frames never receive sample bounds and clear missing heads', async () => {
  const calls = [];
  const { controller, log, step } = setup({ deps: headProvider(calls) });
  controller.setSkull(true);
  controller.setAutomatic(true);
  await controller.startSample();
  // Advance to a time with a valid sample head, then switch to the camera.
  for (let i = 0; i < 200 && log.engines[0].observed.length === 0; i += 1) step();
  assert.ok(log.engines[0].observed.length > 0, 'sample head must be observed before switching');
  controller.setSkull(true);
  await controller.startCamera();
  step(); step();
  const [sampleEngine, cameraEngine] = log.engines;
  assert.equal(sampleEngine.freed, 1);
  assert.equal(cameraEngine.observed.length, 0);
  assert.equal(cameraEngine.cleared, 2);
  assert.deepEqual(cameraEngine.skull, [true]);
  assert.ok(calls.slice(-2).every((c) => c.source === 'camera'));
  controller.stop();
});

test('a missing provider means no observation, never a guessed head', async () => {
  const { controller, log, step } = setup();
  controller.setSkull(true);
  controller.setAutomatic(true);
  await controller.startCamera();
  step(); step();
  assert.equal(log.engines[0].observed.length, 0);
  assert.equal(log.engines[0].cleared, 2);
  controller.stop();
});

test('toggling skulls keeps the engine, history and automatic envelope', async () => {
  const calls = [];
  const { controller, log, step } = setup({ deps: headProvider(calls) });
  controller.setSkull(true);
  controller.setAutomatic(true);
  await controller.startSample();
  step();
  const engine = log.engines[0];
  const before = [engine.automatic.length, engine.configured.length, engine.freed];
  controller.setSkull(true);
  assert.equal(engine.skull.length, 1);
  controller.setSkull(false);
  assert.deepEqual(engine.skull, [true, false]);
  const providerCalls = calls.length;
  step(); step();
  assert.equal(calls.length, providerCalls);
  controller.setSkull(true);
  assert.deepEqual(engine.skull, [true, false, true]);
  assert.deepEqual([engine.automatic.length, engine.configured.length, engine.freed], before);
  assert.equal(log.engines.length, 1);
  assert.equal(controller.getState(), 'sample');
  controller.stop();
});

test('manual reflection and reduced motion leave the automatic settings alone', async () => {
  const calls = [];
  const { controller, log, step } = setup({ deps: headProvider(calls) });
  controller.setSkull(true);
  controller.setAutomatic(false);
  await controller.startSample();
  step();
  assert.equal(calls.length, 0);
  assert.equal(log.engines[0].timed.length, 1);
  controller.setAutomatic(true);
  controller.setReducedMotion(true);
  assert.deepEqual(log.engines[0].automatic.at(-1), { enabled: true, reduced: true });
  assert.deepEqual(log.engines[0].skull, [true]);
  controller.stop();
});

for (const [name, overrides] of [
  ['provider failure', { deps: { observeHead: () => { throw new Error('no head'); } } }],
  ['engine rejection', { observeError: true, deps: { observeHead: () => ({ x: 1, y: 1, width: 2, height: 2, confidence: 1000 }) } }],
]) {
  test(`${name} stops the camera and frees the engine`, async () => {
    const cam = fakeStream();
    const { controller, log, pending, statuses, step } = setup({
      ...overrides,
      deps: { getUserMedia: async () => cam.stream, ...overrides.deps },
    });
    controller.setSkull(true);
    controller.setAutomatic(true);
    await controller.startCamera();
    step();
    assert.equal(controller.getState(), 'error');
    assert.equal(cam.track.stopped, 1);
    assert.equal(log.engines[0].freed, 1);
    assert.ok(log.detached >= 1);
    assert.equal(pending.size, 0);
    assert.match(last(statuses).message, /Processing stopped/);
  });
}

test('a failing skull toggle stops the mirror and releases resources', async () => {
  const cam = fakeStream();
  const { controller, log, pending } = setup({
    deps: { getUserMedia: async () => cam.stream },
  });
  await controller.startCamera();
  log.engines[0].configure_skull = () => { throw new Error('no memory'); };
  controller.setSkull(true);
  assert.equal(controller.getState(), 'error');
  assert.equal(cam.track.stopped, 1);
  assert.equal(log.engines[0].freed, 1);
  assert.equal(pending.size, 0);
});

test('each engine gets one host seed when automatic haunting starts, kept across preference changes', async () => {
  const seeds = [0, 4294967295, -1, 1.5, 2 ** 32];
  const { controller, log } = setup({ deps: { randomSeed: () => seeds.shift() } });
  controller.setAutomatic(true);
  await controller.startSample();
  assert.deepEqual(log.engines[0].seeds, [0]);
  assert.equal(controller.seed, 0);
  controller.setReducedMotion(true);
  controller.setAutomatic(false);
  controller.setAutomatic(true);
  assert.deepEqual(log.engines[0].seeds, [0]);
  await controller.startSample();
  assert.deepEqual(log.engines[1].seeds, [4294967295]);
  for (const bad of [-1, 1.5, 2 ** 32]) {
    await controller.startSample();
    assert.deepEqual(log.engines.at(-1).seeds, [], `seed ${bad} is ignored`);
  }
  controller.stop();
});

test('manual start seeds later, when automatic haunting is turned on', async () => {
  const { controller, log } = setup({ deps: { randomSeed: () => 42 } });
  controller.setAutomatic(false);
  await controller.startSample();
  assert.deepEqual(log.engines[0].seeds, []);
  controller.setAutomatic(true);
  assert.deepEqual(log.engines[0].seeds, [42]);
  controller.stop();
});

test('no seed provider keeps the fixed timing', async () => {
  const { controller, log } = setup();
  await controller.startSample();
  assert.deepEqual(log.engines[0].seeds, []);
  assert.equal(controller.seed, null);
  controller.stop();
});
