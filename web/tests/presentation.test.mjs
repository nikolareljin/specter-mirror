import test from 'node:test';
import assert from 'node:assert/strict';
import { drawSampleScene, samplePose, sampleHeadBounds } from '../src/sample.ts';
import { FullscreenSession } from '../src/fullscreen.ts';

test('generated visitor enters, moves, stays still and leaves', () => {
  assert.ok(samplePose(0).x < 0);
  assert.ok(samplePose(2_000).x > samplePose(0).x);
  assert.notDeepEqual(samplePose(4_000), samplePose(5_000));
  assert.deepEqual(samplePose(7_000), samplePose(10_999));
  assert.ok(samplePose(13_000).x > samplePose(11_000).x);
  assert.equal(samplePose(14_000).visible, false);
  assert.deepEqual(samplePose(0), samplePose(28_000));
});

test('sample replay is deterministic, bounded and static with reduced motion', () => {
  for (const time of [0, 2_500, 6_000, 8_000, 12_000, 17_999, 1e12]) {
    const pose = samplePose(time);
    assert.deepEqual(pose, samplePose(time));
    assert.ok(pose.x >= -0.2 && pose.x <= 1.3);
    assert.ok(pose.scale >= 0.72 && pose.scale <= 1);
    assert.ok(pose.arm >= 0.2 && pose.arm <= 0.9);
    assert.deepEqual(samplePose(time, true), samplePose(8_000));
  }
  for (const invalid of [-1, NaN, Infinity, -Infinity]) {
    assert.deepEqual(samplePose(invalid), samplePose(0));
  }
});

function setup(overrides = {}) {
  let active = false;
  let requests = 0;
  let exits = 0;
  const session = new FullscreenSession({
    supported: () => true,
    active: () => active,
    request: async () => { requests += 1; active = true; },
    exit: async () => { exits += 1; active = false; },
    ...overrides,
  });
  return { session, get requests() { return requests; }, get exits() { return exits; } };
}

test('fullscreen request and stop release only the owned view', async () => {
  const view = setup();
  assert.equal(await view.session.enter(), true);
  assert.equal(await view.session.enter(), true);
  assert.equal(view.requests, 1);
  await view.session.stop();
  assert.equal(view.exits, 1);
  await view.session.stop();
  assert.equal(view.exits, 1);
});

test('unsupported and rejected fullscreen leave the mirror usable', async () => {
  const unsupported = setup({ supported: () => false });
  assert.equal(await unsupported.session.enter(), false);
  assert.equal(unsupported.requests, 0);
  let attempts = 0;
  const view = setup({ request: async () => { attempts += 1; throw new Error('denied'); } });
  assert.equal(await view.session.enter(), false);
  assert.equal(await view.session.enter(), false);
  assert.equal(attempts, 2);
});

test('stop cancels a late request and repeated clicks do not create new requests', async () => {
  let resolve;
  let active = false;
  let exits = 0;
  const ready = new Promise((done) => { resolve = done; });
  const view = setup({ active: () => active,
    request: async () => { await ready; active = true; },
    exit: async () => { exits += 1; active = false; } });
  const entering = view.session.enter();
  assert.equal(await view.session.enter(), false);
  await view.session.stop();
  resolve();
  assert.equal(await entering, false);
  assert.equal(active, false);
  assert.equal(exits, 1);
});

test('stale request cleanup never exits a different fullscreen element', async () => {
  let resolve;
  let exits = 0;
  const view = setup({ active: () => false,
    request: () => new Promise((done) => { resolve = done; }),
    exit: async () => { exits += 1; } });
  const entering = view.session.enter();
  await view.session.stop();
  resolve();
  assert.equal(await entering, false);
  assert.equal(exits, 0);
});

// Records the head ellipse drawSampleScene emits, with the active transform.
function recordHead(width, height, elapsed, reduced = false) {
  let m = { a: 1, d: 1, e: 0, f: 0 };
  const stack = [];
  const ellipses = [];
  const noop = () => {};
  const ctx = {
    save: () => stack.push({ ...m }),
    restore: () => { m = stack.pop(); },
    scale: (sx, sy) => { m = { ...m, a: m.a * sx, d: m.d * sy }; },
    translate: (tx, ty) => { m = { ...m, e: m.e + m.a * tx, f: m.f + m.d * ty }; },
    rotate: noop, fillRect: noop, beginPath: noop, moveTo: noop, lineTo: noop,
    bezierCurveTo: noop, quadraticCurveTo: noop, closePath: noop, fill: noop, stroke: noop,
    createLinearGradient: () => ({ addColorStop: noop }),
    ellipse: (x, y, rx, ry, rotation) => ellipses.push({ x, y, rx, ry, rotation, m: { ...m } }),
  };
  drawSampleScene(ctx, width, height, elapsed, reduced);
  return ellipses[0] ?? null;
}

// Independent oracle: sample the drawn ellipse outline in output pixels.
function outline(ellipse) {
  const { x, y, rx, ry, rotation, m } = ellipse;
  let minX = Infinity; let maxX = -Infinity; let minY = Infinity; let maxY = -Infinity;
  for (let i = 0; i < 20_000; i += 1) {
    const t = (i / 20_000) * Math.PI * 2;
    const lx = rx * Math.cos(t);
    const ly = ry * Math.sin(t);
    const px = x + lx * Math.cos(rotation) - ly * Math.sin(rotation);
    const py = y + lx * Math.sin(rotation) + ly * Math.cos(rotation);
    const ox = m.a * px + m.e;
    const oy = m.d * py + m.f;
    minX = Math.min(minX, ox); maxX = Math.max(maxX, ox);
    minY = Math.min(minY, oy); maxY = Math.max(maxY, oy);
  }
  return { minX, maxX, minY, maxY };
}

const TIMES = [3_000, 3_700, 4_500, 5_500, 6_500, 7_000, 9_000, 10_999, 11_000, 11_500];

test('sample head bounds enclose the drawn ellipse at both processing sizes', () => {
  for (const [w, h] of [[160, 120], [320, 240]]) {
    let usable = 0;
    for (const time of TIMES) {
      const ellipse = recordHead(w, h, time);
      const bounds = sampleHeadBounds(w, h, time, false);
      const { minX, maxX, minY, maxY } = outline(ellipse);
      const inside = minX >= 0 && minY >= 0 && maxX <= w && maxY <= h;
      if (!inside) { assert.equal(bounds, null, `${w}x${h} t=${time}`); continue; }
      usable += 1;
      assert.ok(bounds, `${w}x${h} t=${time}`);
      for (const value of Object.values(bounds)) assert.ok(Number.isInteger(value));
      // Enclosing, and tight to within one pixel on every side.
      assert.ok(bounds.x <= minX + 1e-6 && bounds.x > minX - 1 - 1e-6, `${time} left`);
      assert.ok(bounds.x + bounds.width >= maxX - 1e-6 && bounds.x + bounds.width < maxX + 1 + 1e-6);
      assert.ok(bounds.y <= minY + 1e-6 && bounds.y > minY - 1 - 1e-6);
      assert.ok(bounds.y + bounds.height >= maxY - 1e-6 && bounds.y + bounds.height < maxY + 1 + 1e-6);
      assert.ok(bounds.width > 0 && bounds.height > 0);
    }
    assert.ok(usable >= 6, `${w}x${h}`);
  }
});

test('sample head rotation widens the box beyond the unrotated ellipse', () => {
  const turned = samplePose(4_500);
  assert.ok(Math.abs(turned.turn) > 0.05);
  const w = 320;
  const h = 240;
  const bounds = sampleHeadBounds(w, h, 4_500, false);
  const unrotatedHalfY = 0.38 * turned.scale * 0.33 * h;
  assert.ok(bounds.height / 2 > unrotatedHalfY);
  const flat = sampleHeadBounds(w, h, 8_000, false);
  assert.equal(samplePose(8_000).turn, 0);
  assert.equal(flat.width, Math.ceil(0.52 * w + 0.28 * 0.25 * w) - Math.floor(0.52 * w - 0.28 * 0.25 * w));
});

test('the two processing sizes agree on the same head within rounding', () => {
  for (const time of [4_500, 8_000, 10_500]) {
    const small = sampleHeadBounds(160, 120, time, false);
    const large = sampleHeadBounds(320, 240, time, false);
    assert.ok(small && large);
    assert.ok(Math.abs(small.x * 2 - large.x) <= 2);
    assert.ok(Math.abs(small.y * 2 - large.y) <= 2);
    assert.ok(Math.abs(small.width * 2 - large.width) <= 3);
    assert.ok(Math.abs(small.height * 2 - large.height) <= 3);
  }
});

test('absent or clipped visitors report no head', () => {
  for (const [w, h] of [[160, 120], [320, 240]]) {
    for (const time of [0, 500, 14_000, 20_000, 27_999]) {
      assert.equal(sampleHeadBounds(w, h, time, false), null, `${w}x${h} t=${time}`);
    }
    // The visitor leaves through the right edge: clipped before invisible.
    assert.equal(sampleHeadBounds(w, h, 13_500, false), null);
    assert.ok(sampleHeadBounds(w, h, 8_000, false));
  }
  for (const [w, h] of [[0, 240], [320, 0], [NaN, 240], [320, Infinity], [-1, 5]]) {
    assert.equal(sampleHeadBounds(w, h, 8_000, false), null);
  }
  assert.equal(sampleHeadBounds(320, 240, Number.NaN, false), null);
});

test('reduced motion reports the static head and a loop replays it', () => {
  const still = sampleHeadBounds(320, 240, 8_000, false);
  assert.deepEqual(sampleHeadBounds(320, 240, 0, true), still);
  assert.deepEqual(sampleHeadBounds(320, 240, 12_345, true), still);
  assert.deepEqual(sampleHeadBounds(320, 240, 8_000 + 28_000, false), still);
});
