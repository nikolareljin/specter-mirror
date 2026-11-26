export interface SamplePose {
  visible: boolean;
  x: number;
  scale: number;
  arm: number;
  turn: number;
}

const CYCLE_MS = 28_000;
const ease = (t: number) => t * t * (3 - 2 * t);

// Seeded visits: the visitor arrives after a quiet gap, keeps moving for a
// while with short pauses, then leaves. A fixed number of visits make one
// loop of roughly four minutes, so sampling stays bounded for any clock value.
// Integer arithmetic only, mirrored by the Rust preview example.
const VISITS = 4;
const ENTER_MS = 3_000;
const LEAVE_MS = 3_000;

type SegmentKind = 'absent' | 'enter' | 'move' | 'pause' | 'leave';
interface Segment {
  kind: SegmentKind;
  start: number;
  duration: number;
  /** Whole sway periods and amplitude in thousandths of the frame width. */
  periods: number;
  amplitude: number;
}
interface Schedule {
  segments: Segment[];
  length: number;
}

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return (t ^ (t >>> 14)) >>> 0;
  };
}

let cachedSchedule: { seed: number; schedule: Schedule } | null = null;

/** The seeded visit plan: gaps 4-15 s, visits 36-75 s, pauses of 1-3 s. */
export function sampleSchedule(seed: number): Schedule {
  const normalized = seed >>> 0;
  if (cachedSchedule && cachedSchedule.seed === normalized) return cachedSchedule.schedule;
  const next = mulberry32((normalized ^ 0x5eed5eed) >>> 0);
  const between = (lo: number, hi: number) => lo + (next() % (hi - lo + 1));
  const segments: Segment[] = [];
  let at = 0;
  const push = (kind: SegmentKind, duration: number, periods = 0, amplitude = 0) => {
    segments.push({ kind, start: at, duration, periods, amplitude });
    at += duration;
  };
  for (let visit = 0; visit < VISITS; visit += 1) {
    push('absent', between(4_000, 15_000));
    push('enter', ENTER_MS);
    const body = between(30_000, 60_000);
    const bodyEnd = at + body;
    while (at < bodyEnd) {
      push('move', between(3_000, 6_000), 1 + (next() % 2), between(40, 120));
      push('pause', between(1_000, 3_000));
    }
    push('leave', LEAVE_MS);
  }
  const schedule = { segments, length: at };
  cachedSchedule = { seed: normalized, schedule };
  return schedule;
}

/**
 * Original generated visitor. Without a seed: enter, move, hold still, then
 * leave on a fixed 28 second loop. With a seed: the longer random schedule
 * from `sampleSchedule`. Reduced motion is always the static pose.
 */
export function samplePose(
  elapsedMs: number, reducedMotion = false, seed: number | null = null,
): SamplePose {
  const safeTime = Number.isFinite(elapsedMs) ? Math.max(0, elapsedMs) : 0;
  if (seed !== null && !reducedMotion) return seededPose(safeTime, seed);
  const t = reducedMotion ? 8_000 : safeTime % CYCLE_MS;
  if (t < 3_000) {
    const progress = ease(t / 3_000);
    return { visible: true, x: -0.2 + progress * 0.72, scale: 0.72 + progress * 0.28,
      arm: 0.2, turn: -0.06 };
  }
  if (t < 7_000) {
    const phase = ((t - 3_000) / 4_000) * Math.PI * 2;
    return { visible: true, x: 0.52 + Math.sin(phase) * 0.04, scale: 1,
      arm: 0.2 + (1 - Math.cos(phase)) * 0.35, turn: Math.sin(phase) * 0.1 };
  }
  if (t < 11_000) return { visible: true, x: 0.52, scale: 1, arm: 0.2, turn: 0 };
  if (t < 14_000) return { visible: true, x: 0.52 + ease((t - 11_000) / 3_000) * 0.78,
    scale: 1, arm: 0.2, turn: 0.08 };
  return { visible: false, x: 1.3, scale: 1, arm: 0.2, turn: 0.08 };
}

function seededPose(time: number, seed: number): SamplePose {
  const { segments, length } = sampleSchedule(seed);
  const t = Math.floor(time) % length;
  let index = segments.length - 1;
  for (let i = 0; i < segments.length; i += 1) {
    if (t < segments[i].start + segments[i].duration) { index = i; break; }
  }
  const segment = segments[index];
  const local = t - segment.start;
  const progress = local / segment.duration;
  switch (segment.kind) {
    case 'enter': {
      const p = ease(progress);
      return { visible: true, x: -0.2 + p * 0.72, scale: 0.72 + p * 0.28, arm: 0.2, turn: -0.06 };
    }
    case 'move': {
      const phase = progress * Math.PI * 2 * segment.periods;
      return { visible: true, x: 0.52 + Math.sin(phase) * (segment.amplitude / 1000), scale: 1,
        arm: 0.2 + (1 - Math.cos(phase)) * 0.35, turn: Math.sin(phase) * 0.1 };
    }
    case 'pause':
      return { visible: true, x: 0.52, scale: 1, arm: 0.2, turn: 0 };
    case 'leave':
      return { visible: true, x: 0.52 + ease(progress) * 0.78, scale: 1, arm: 0.2, turn: 0.08 };
    default:
      return { visible: false, x: 1.3, scale: 1, arm: 0.2, turn: 0.08 };
  }
}

export interface HeadBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

// Head ellipse in drawSampleScene's local space: center, radii (rotation is
// pose.turn). Keep these in step with the ellipse drawn below.
const HEAD_CENTER_Y = -0.69;
const HEAD_RADIUS_X = 0.25;
const HEAD_RADIUS_Y = 0.33;
const BODY_SCALE_X = 0.28;
const BODY_SCALE_Y = 0.38;
const BODY_ORIGIN_Y = 0.55;

/**
 * Integer, axis-aligned bounds of the generated visitor's head ellipse in
 * unmirrored frame pixels, from the same pose and transforms as the drawing.
 * This is known scene geometry, not detection. Returns null when the visitor
 * is absent or the head is not entirely inside the frame.
 */
export function sampleHeadBounds(
  width: number, height: number, elapsedMs: number, reducedMotion: boolean,
  seed: number | null = null,
): HeadBounds | null {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
  const pose = samplePose(elapsedMs, reducedMotion, seed);
  if (!pose.visible) return null;
  const cos = Math.cos(pose.turn);
  const sin = Math.sin(pose.turn);
  const halfX = Math.hypot(HEAD_RADIUS_X * cos, HEAD_RADIUS_Y * sin);
  const halfY = Math.hypot(HEAD_RADIUS_X * sin, HEAD_RADIUS_Y * cos);
  const scaleX = BODY_SCALE_X * pose.scale;
  const scaleY = BODY_SCALE_Y * pose.scale;
  const centerX = pose.x;
  const centerY = BODY_ORIGIN_Y + scaleY * HEAD_CENTER_Y;
  // Small epsilon keeps float noise from moving an exact pixel edge outward.
  const eps = 1e-9;
  const left = Math.floor((centerX - scaleX * halfX) * width + eps);
  const right = Math.ceil((centerX + scaleX * halfX) * width - eps);
  const top = Math.floor((centerY - scaleY * halfY) * height + eps);
  const bottom = Math.ceil((centerY + scaleY * halfY) * height - eps);
  if (left < 0 || top < 0 || right > width || bottom > height || right <= left || bottom <= top) {
    return null;
  }
  return { x: left, y: top, width: right - left, height: bottom - top };
}

export function drawSampleScene(
  ctx: CanvasRenderingContext2D, width: number, height: number,
  elapsedMs: number, reducedMotion: boolean, seed: number | null = null,
): void {
  const pose = samplePose(elapsedMs, reducedMotion, seed);
  ctx.save();
  ctx.scale(width, height);
  const room = ctx.createLinearGradient(0, 0, 0, 1);
  room.addColorStop(0, '#080d14');
  room.addColorStop(1, '#29313c');
  ctx.fillStyle = room;
  ctx.fillRect(0, 0, 1, 1);
  ctx.strokeStyle = '#667078';
  ctx.lineWidth = 0.008;
  ctx.beginPath();
  ctx.moveTo(0.22, 0.82);
  ctx.lineTo(0.22, 0.31);
  ctx.bezierCurveTo(0.22, 0.04, 0.78, 0.04, 0.78, 0.31);
  ctx.lineTo(0.78, 0.82);
  ctx.moveTo(0.5, 0.11);
  ctx.lineTo(0.5, 0.82);
  ctx.moveTo(0, 0.84);
  ctx.lineTo(1, 0.84);
  ctx.stroke();
  if (pose.visible) {
    ctx.save();
    ctx.translate(pose.x, 0.55);
    ctx.scale(0.28 * pose.scale, 0.38 * pose.scale);
    ctx.fillStyle = '#596877';
    ctx.beginPath();
    ctx.moveTo(-0.1, -0.27);
    ctx.bezierCurveTo(-0.6, -0.3, -0.53, 0.5, -0.48, 1.2);
    ctx.lineTo(0.48, 1.2);
    ctx.bezierCurveTo(0.53, 0.5, 0.6, -0.3, 0.1, -0.27);
    ctx.closePath();
    ctx.fill();
    const face = ctx.createLinearGradient(-0.25, 0, 0.25, 0);
    face.addColorStop(0, '#4c5864');
    face.addColorStop(0.6, '#b8b7b0');
    face.addColorStop(1, '#72808c');
    ctx.fillStyle = face;
    ctx.fillRect(-0.09, -0.45, 0.18, 0.25);
    ctx.beginPath();
    ctx.ellipse(0, -0.69, 0.25, 0.33, pose.turn, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#151d27';
    for (const eye of [-0.09, 0.09]) {
      ctx.beginPath();
      ctx.ellipse(eye + pose.turn * 0.2, -0.72, 0.055, 0.014,
        eye < 0 ? -0.22 : 0.22, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.strokeStyle = '#313a43';
    ctx.lineWidth = 0.025;
    ctx.beginPath();
    ctx.moveTo(0.01, -0.72);
    ctx.lineTo(-0.02, -0.58);
    ctx.lineTo(0.035, -0.58);
    ctx.moveTo(-0.07, -0.52);
    ctx.quadraticCurveTo(0, -0.56, 0.07, -0.52);
    ctx.stroke();
    ctx.strokeStyle = '#a0adb8';
    ctx.lineWidth = 0.018;
    ctx.beginPath();
    ctx.moveTo(0, -0.1);
    ctx.lineTo(0, 1.2);
    ctx.stroke();
    ctx.save();
    ctx.translate(0.39, -0.05);
    ctx.rotate(-pose.arm);
    ctx.strokeStyle = '#71808d';
    ctx.lineWidth = 0.16;
    ctx.lineCap = 'round';
    ctx.beginPath();
    ctx.moveTo(0, 0);
    ctx.lineTo(0.14, 0.64);
    ctx.stroke();
    ctx.fillStyle = '#b8b7b0';
    ctx.beginPath();
    ctx.ellipse(0.14, 0.7, 0.09, 0.12, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
    ctx.restore();
  }
  ctx.restore();
}
