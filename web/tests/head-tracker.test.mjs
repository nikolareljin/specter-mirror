import test from 'node:test';
import assert from 'node:assert/strict';
import {
  HeadTracker,
  downsampleGray,
  expandFaceToHead,
  scaleBox,
  INFERENCE_TIMEOUT_MS,
  INIT_TIMEOUT_MS,
} from '../src/head-tracker.ts';

// A fake worker, wall clock and timers. Nothing here detects a face: results
// are literal protocol messages, so these tests cover lifecycle and transforms
// only, not detector accuracy.
function setup() {
  let clock = 1000;
  const timers = new Map();
  let nextTimer = 1;
  const workers = [];
  const statuses = [];
  const deps = {
    createWorker() {
      const worker = {
        posted: [],
        terminated: 0,
        onmessage: null,
        onerror: null,
        onmessageerror: null,
        postMessage(message) { this.posted.push(message); },
        terminate() { this.terminated += 1; },
        send(data) { this.onmessage?.({ data }); },
      };
      workers.push(worker);
      return worker;
    },
    now: () => clock,
    setTimer(callback, ms) {
      const handle = nextTimer++;
      timers.set(handle, { callback, at: clock + ms });
      return handle;
    },
    clearTimer: (handle) => timers.delete(handle),
    onStatus: (s) => statuses.push(s.state),
  };
  const tracker = new HeadTracker(deps);
  const advance = (ms) => {
    clock += ms;
    for (const [handle, t] of [...timers]) {
      if (t.at <= clock && timers.has(handle)) {
        timers.delete(handle);
        t.callback();
      }
    }
  };
  return { tracker, workers, timers, statuses, advance, now: () => clock };
}

const W = 320;
const H = 240;
const frame = (w = W, h = H, value = 100) => new Uint8Array(w * h * 4).fill(value);
const FACE = [100, 20, 30, 40, 900];
// Elapsed time of the clock in `setup` is wall clock minus 1000.
const elapsed = (ctx) => ctx.now() - 1000;

/** Starts a worker and finishes initialization. */
function ready(ctx) {
  ctx.tracker.offer(frame(), W, H, elapsed(ctx));
  const worker = ctx.workers.at(-1);
  worker.send({ type: 'ready', generation: worker.posted[0].generation });
  return worker;
}

function submit(ctx, w = W, h = H) {
  const worker = ctx.workers.at(-1);
  const before = worker.posted.length;
  const sent = ctx.tracker.offer(frame(w, h), w, h, elapsed(ctx));
  return { worker, sent, request: sent ? worker.posted[before] : null };
}

function answer(worker, request, face = FACE, patch = {}) {
  worker.send({
    type: 'result',
    generation: request.generation,
    id: request.id,
    observedMs: request.observedMs,
    face,
    ...patch,
  });
}

test('box helpers: expansion, outward integer scaling and clipping', () => {
  assert.deepEqual(expandFaceToHead([100, 20, 30, 40], 160, 120), [97, 8, 36, 54]);
  // Clipped at the left/top edge and the right/bottom edge.
  assert.deepEqual(expandFaceToHead([2, 5, 50, 20], 160, 120), [0, 0, 57, 26]);
  assert.deepEqual(expandFaceToHead([130, 100, 30, 20], 160, 120), [127, 94, 33, 26]);
  assert.deepEqual(scaleBox([97, 8, 36, 54], 160, 120, 160, 120), [97, 8, 36, 54]);
  assert.deepEqual(scaleBox([97, 8, 36, 54], 160, 120, 320, 240), [194, 16, 72, 108]);
  // Non-integer ratio: edges round outward, then clip.
  assert.deepEqual(scaleBox([1, 1, 3, 3], 160, 120, 250, 190), [1, 1, 6, 6]);
  assert.equal(scaleBox([0, 0, 0, 5], 160, 120, 320, 240), null);
});

test('downsampling reads 160x120 and 320x240 frames without modifying them', () => {
  const out = new Uint8Array(160 * 120);
  const small = frame(160, 120, 200);
  assert.ok(downsampleGray(small, 160, 120, out));
  assert.ok(out.every((v) => v === 200));
  const big = new Uint8Array(W * H * 4);
  big.fill(255, 0, 4); // only the first source pixel is white
  const copy = big.slice();
  assert.ok(downsampleGray(big, W, H, out));
  assert.equal(out[0], 63, 'one white pixel of four, floored');
  assert.equal(out[1], 0);
  assert.deepEqual(big, copy);
  assert.equal(downsampleGray(new Uint8Array(10), W, H, out), false);
  assert.equal(downsampleGray(frame(100, 100), 100, 100, out), false);
});

test('the worker starts lazily, drops frames while starting and activates on ready', () => {
  const ctx = setup();
  assert.equal(ctx.workers.length, 0);
  assert.equal(ctx.tracker.offer(frame(), W, H, 0), false);
  assert.equal(ctx.workers.length, 1);
  const [init] = ctx.workers[0].posted;
  assert.equal(init.type, 'init');
  assert.deepEqual([init.width, init.height], [160, 120]);
  assert.equal(ctx.tracker.offer(frame(), W, H, 10), false, 'no frames queue during init');
  assert.equal(ctx.workers[0].posted.length, 1);
  ctx.workers[0].send({ type: 'ready', generation: init.generation });
  assert.equal(ctx.tracker.getStatus(), 'active');
  assert.deepEqual(ctx.statuses, ['initializing', 'active']);
});

test('one request in flight, 200 ms cadence and no pending queue', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(250);
  const first = submit(ctx);
  assert.ok(first.sent);
  assert.equal(first.request.type, 'detect');
  assert.equal(first.request.pixels.length, 160 * 120);
  ctx.advance(100);
  assert.equal(submit(ctx).sent, false, 'busy: dropped');
  assert.equal(worker.posted.length, 2);
  answer(worker, first.request);
  ctx.advance(99);
  assert.equal(submit(ctx).sent, false, '199 ms after the last submission');
  ctx.advance(1);
  assert.ok(submit(ctx).sent, '200 ms after the last submission');
  assert.equal(worker.posted.length, 3);
  // Slow inference never builds a backlog: offers while busy are dropped.
  for (let i = 0; i < 20; i += 1) {
    ctx.advance(50);
    assert.equal(submit(ctx).sent, false);
  }
  assert.equal(worker.posted.length, 3);
});

test('staging storage is copied on post and reused; the renderer frame is untouched', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(250);
  const source = frame();
  const copy = source.slice();
  assert.ok(ctx.tracker.offer(source, W, H, elapsed(ctx)));
  assert.deepEqual(source, copy);
  assert.equal(source.buffer.byteLength, W * H * 4);
  assert.equal(source.byteLength, W * H * 4, 'buffer still attached');
  assert.equal(worker.posted[1].pixels.length, 160 * 120);
});

test('results carry generation, id and the original timestamp into one observation', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  assert.equal(request.observedMs, 300);
  assert.equal(request.generation, worker.posted[0].generation);
  ctx.advance(100);
  answer(worker, request);
  assert.deepEqual(ctx.tracker.observe(elapsed(ctx)), {
    x: 194, y: 16, width: 72, height: 108, confidence: 900, observedMs: 300,
  });
  assert.equal(ctx.tracker.observe(elapsed(ctx)).observedMs, 300, 'never re-stamped');
});

test('160x120 frames use unscaled coordinates; no mirroring happens here', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx, 160, 120);
  answer(worker, request, [10, 20, 30, 40, 1000]);
  // Face on the left stays on the left of the unmirrored frame.
  assert.deepEqual(ctx.tracker.observe(elapsed(ctx)), {
    x: 7, y: 8, width: 36, height: 54, confidence: 1000, observedMs: 300,
  });
});

test('no face and clipped-away faces leave no observation', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  let r = submit(ctx).request;
  answer(worker, r);
  assert.ok(ctx.tracker.observe(elapsed(ctx)));
  ctx.advance(300);
  r = submit(ctx).request;
  answer(worker, r, []);
  assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
  assert.equal(ctx.tracker.getStatus(), 'active');
});

test('staleness boundary: 500 ms is usable and 501 ms is not', () => {
  for (const [wait, usable] of [[500, true], [501, false]]) {
    const ctx = setup();
    const worker = ready(ctx);
    ctx.advance(300);
    const { request } = submit(ctx);
    answer(worker, request);
    const at = request.observedMs + wait;
    assert.equal(ctx.tracker.observe(at) !== null, usable, `age ${wait}`);
  }
});

test('results older than 500 ms on arrival are dropped, without a restamp', () => {
  for (const [wait, kept] of [[500, true], [501, false]]) {
    const ctx = setup();
    const worker = ready(ctx);
    ctx.advance(300);
    const { request } = submit(ctx);
    ctx.advance(wait);
    // Keep the clock anchor current without submitting a request.
    ctx.tracker.offer(frame(), W, H, elapsed(ctx));
    answer(worker, request);
    const seen = ctx.tracker.observe(elapsed(ctx));
    assert.equal(seen !== null, kept, `arrival age ${wait}`);
    if (seen) assert.equal(seen.observedMs, 300);
  }
});

test('expiry applies with no new result and the old box is not refreshed', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  answer(worker, request);
  assert.ok(ctx.tracker.observe(600));
  assert.equal(ctx.tracker.observe(801), null);
  assert.equal(ctx.tracker.observe(600), null, 'expired data is discarded');
});

test('observation reads reject invalid and earlier clocks without refreshing bounds', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  answer(worker, request);
  for (const now of [NaN, Infinity, -Infinity, -1, 299, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(ctx.tracker.observe(now), null, `clock ${now}`);
  }
  assert.equal(ctx.tracker.observe(300.5).observedMs, 300);
});

test('future, mismatched and invalid results are rejected', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  // Mismatched generation or id: ignored, request stays pending.
  answer(worker, request, FACE, { generation: request.generation + 1 });
  answer(worker, request, FACE, { id: request.id + 1 });
  assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
  assert.equal(ctx.tracker.getStatus(), 'active');
  assert.equal(ctx.timers.size, 1, 'inference timer still running');
  answer(worker, request);
  assert.ok(ctx.tracker.observe(elapsed(ctx)));
});

test('a result stamped in the future is dropped', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  // The shared clock moved backwards: the frame time is now in the future.
  ctx.advance(-100);
  answer(worker, request);
  assert.equal(ctx.tracker.observe(request.observedMs + 1), null);
  assert.equal(ctx.tracker.getStatus(), 'active');
});

test('regressing offers are dropped', () => {
  const ctx = setup();
  ready(ctx);
  ctx.advance(300);
  assert.ok(ctx.tracker.offer(frame(), W, H, 300));
  ctx.advance(300);
  assert.equal(ctx.tracker.offer(frame(), W, H, 299), false);
  assert.equal(ctx.tracker.offer(frame(), W, H, 1.5), false);
  assert.equal(ctx.tracker.offer(frame(), W, H, -1), false);
});

for (const [name, patch] of [
  ['wrong timestamp', { observedMs: 1 }],
  ['fractional timestamp', { observedMs: 300.5 }],
  ['four entries', { face: [1, 2, 3, 4] }],
  ['non-array', { face: 'face' }],
  ['negative', { face: [-1, 2, 30, 40, 900] }],
  ['out of image', { face: [150, 2, 30, 40, 900] }],
  ['zero size', { face: [10, 2, 0, 40, 900] }],
  ['low confidence', { face: [10, 2, 30, 40, 799] }],
  ['high confidence', { face: [10, 2, 30, 40, 1001] }],
  ['NaN', { face: [10, 2, 30, 40, NaN] }],
]) {
  test(`malformed result (${name}) disables the locator`, () => {
    const ctx = setup();
    const worker = ready(ctx);
    ctx.advance(300);
    const { request } = submit(ctx);
    answer(worker, request, FACE, patch);
    assert.equal(ctx.tracker.getStatus(), 'unavailable');
    assert.equal(worker.terminated, 1);
    assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
    assert.equal(ctx.timers.size, 0);
  });
}

test('worker errors, protocol garbage and post failures disable it once', () => {
  for (const trigger of [
    (w) => w.send({ type: 'error', generation: w.posted[0].generation, message: 'model' }),
    (w) => w.onerror({}),
    (w) => w.onmessageerror({}),
    (w) => w.send(null),
    (w) => w.send({ type: 'bogus', generation: w.posted[0].generation }),
  ]) {
    const ctx = setup();
    const worker = ready(ctx);
    trigger(worker);
    assert.equal(ctx.tracker.getStatus(), 'unavailable');
    assert.equal(worker.terminated, 1);
    ctx.advance(1000);
    assert.equal(ctx.tracker.offer(frame(), W, H, 5000), false);
    assert.equal(ctx.workers.length, 1, 'no restart loop');
  }
  const ctx = setup();
  ctx.tracker.offer(frame(), W, H, 0);
  ctx.workers[0].postMessage = () => { throw new Error('closed'); };
  ctx.workers[0].send({ type: 'ready', generation: ctx.workers[0].posted[0].generation });
  ctx.advance(300);
  assert.equal(ctx.tracker.offer(frame(), W, H, 300), false);
  assert.equal(ctx.tracker.getStatus(), 'unavailable');
  const bad = setup();
  bad.tracker = new HeadTracker({ ...{}, createWorker() { throw new Error('no worker'); }, now: () => 0, setTimer: () => 1, clearTimer() {} });
  assert.equal(bad.tracker.offer(frame(), W, H, 0), false);
  assert.equal(bad.tracker.getStatus(), 'unavailable');
});

test('initialization times out at 8000 ms and inference at 1500 ms', () => {
  let ctx = setup();
  ctx.tracker.offer(frame(), W, H, 0);
  ctx.advance(INIT_TIMEOUT_MS - 1);
  assert.equal(ctx.tracker.getStatus(), 'initializing');
  ctx.advance(1);
  assert.equal(ctx.tracker.getStatus(), 'unavailable');
  assert.equal(ctx.workers[0].terminated, 1);
  assert.equal(ctx.timers.size, 0);

  ctx = setup();
  const worker = ready(ctx);
  assert.equal(ctx.timers.size, 0, 'init timer cleared on ready');
  ctx.advance(300);
  submit(ctx);
  ctx.advance(INFERENCE_TIMEOUT_MS - 1);
  assert.equal(ctx.tracker.getStatus(), 'active');
  ctx.advance(1);
  assert.equal(ctx.tracker.getStatus(), 'unavailable');
  assert.equal(worker.terminated, 1);
});

test('a failed locator retries only after release with retry, then starts fresh', () => {
  const ctx = setup();
  const worker = ready(ctx);
  worker.onerror({});
  ctx.tracker.release(false);
  ctx.tracker.offer(frame(), W, H, 0);
  assert.equal(ctx.workers.length, 1);
  assert.equal(ctx.tracker.getStatus(), 'unavailable');
  ctx.tracker.release(true);
  assert.equal(ctx.tracker.getStatus(), 'off');
  ctx.tracker.offer(frame(), W, H, 0);
  assert.equal(ctx.workers.length, 2);
  assert.equal(ctx.workers[1].posted[0].generation > worker.posted[0].generation, true);
});

test('release clears worker, timers, observation and storage; old callbacks are inert', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  const oldMessage = worker.onmessage;
  const oldError = worker.onerror;
  ctx.tracker.release(true);
  assert.equal(worker.terminated, 1);
  assert.equal(ctx.timers.size, 0);
  assert.equal(ctx.tracker.hasWorker(), false);
  // Callbacks retained by the old worker cannot revive anything.
  oldMessage?.({ data: { type: 'result', generation: request.generation, id: request.id, observedMs: 300, face: FACE } });
  oldError?.({});
  worker.send({ type: 'ready', generation: request.generation });
  assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
  assert.equal(ctx.tracker.getStatus(), 'off');
  // A fresh generation cannot be answered by the old worker.
  ctx.advance(10);
  ctx.tracker.offer(frame(), W, H, elapsed(ctx));
  const fresh = ctx.workers[1];
  fresh.send({ type: 'ready', generation: fresh.posted[0].generation });
  ctx.advance(300);
  const next = submit(ctx).request;
  worker.onmessage?.({ data: { type: 'result', generation: next.generation, id: next.id, observedMs: next.observedMs, face: FACE } });
  assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
});

test('a late result for a released request id cannot appear in a new generation', () => {
  const ctx = setup();
  const first = ready(ctx);
  ctx.advance(300);
  const old = submit(ctx).request;
  ctx.tracker.release(true);
  ctx.tracker.offer(frame(), W, H, 0);
  const second = ctx.workers[1];
  second.send({ type: 'ready', generation: second.posted[0].generation });
  ctx.advance(300);
  const request = submit(ctx).request;
  // Same id and timestamp shape, old generation number.
  second.send({ type: 'result', generation: old.generation, id: request.id, observedMs: request.observedMs, face: FACE });
  assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
  assert.notEqual(first, second);
});

test('hiding terminates the worker; unhiding allows one fresh generation', () => {
  const ctx = setup();
  const worker = ready(ctx);
  ctx.advance(300);
  const { request } = submit(ctx);
  answer(worker, request);
  assert.ok(ctx.tracker.observe(elapsed(ctx)));
  ctx.tracker.setHidden(true);
  assert.equal(worker.terminated, 1);
  assert.equal(ctx.tracker.observe(elapsed(ctx)), null);
  assert.equal(ctx.timers.size, 0);
  assert.equal(ctx.tracker.offer(frame(), W, H, 400), false);
  assert.equal(ctx.workers.length, 1, 'no start while hidden');
  ctx.tracker.setHidden(false);
  assert.equal(ctx.workers.length, 1, 'unhiding alone starts nothing');
  ctx.tracker.offer(frame(), W, H, 500);
  assert.equal(ctx.workers.length, 2);
  ctx.tracker.offer(frame(), W, H, 510);
  assert.equal(ctx.workers.length, 2, 'only one fresh generation');
});

test('hiding does not clear a failure latch', () => {
  const ctx = setup();
  const worker = ready(ctx);
  worker.onerror({});
  ctx.tracker.setHidden(true);
  ctx.tracker.setHidden(false);
  ctx.tracker.offer(frame(), W, H, 100);
  assert.equal(ctx.workers.length, 1);
});
