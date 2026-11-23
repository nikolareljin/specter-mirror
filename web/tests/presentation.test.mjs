import test from 'node:test';
import assert from 'node:assert/strict';
import { samplePose } from '../src/sample.ts';
import { FullscreenSession } from '../src/fullscreen.ts';

test('generated visitor enters, moves, stays still and leaves', () => {
  assert.ok(samplePose(0).x < 0);
  assert.ok(samplePose(2_000).x > samplePose(0).x);
  assert.notDeepEqual(samplePose(4_000), samplePose(5_000));
  assert.deepEqual(samplePose(7_000), samplePose(10_999));
  assert.ok(samplePose(13_000).x > samplePose(11_000).x);
  assert.equal(samplePose(14_000).visible, false);
  assert.deepEqual(samplePose(0), samplePose(18_000));
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
