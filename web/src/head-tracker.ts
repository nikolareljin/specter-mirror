// Optional local head localization for camera frames.
// The detector runs in a module worker; this adapter owns the worker lifecycle,
// the resource bounds and the timestamp rules. No DOM or WASM imports, so it
// runs under Node's type stripping. Keep this file free of parameter
// properties, enums and namespaces.
//
// Protocol (main thread -> worker, then back). Every message carries the
// session generation; requests and results also carry a request id and the
// integer elapsed time of the captured frame, which the worker echoes.
//   init   { type, generation, width, height }
//   ready  { type, generation }
//   detect { type, generation, id, observedMs, pixels: Uint8Array(w*h) }
//   result { type, generation, id, observedMs, face: [] | [x, y, w, h, confidence] }
//   error  { type, generation, message }
// `face` is in detector-image pixels (unmirrored). This adapter expands it to
// a head box once and scales it into the processing frame. It never mirrors.

import type { HeadObservation } from './mirror.ts';

export const DETECT_WIDTH = 160;
export const DETECT_HEIGHT = 120;
export const MIN_SUBMIT_INTERVAL_MS = 200;
export const INIT_TIMEOUT_MS = 8000;
export const INFERENCE_TIMEOUT_MS = 1500;
export const MAX_RESULT_AGE_MS = 500;
const MAX_SAFE_MS = Number.MAX_SAFE_INTEGER;

export interface HeadInit { type: 'init'; generation: number; width: number; height: number }
export interface HeadDetect {
  type: 'detect';
  generation: number;
  id: number;
  observedMs: number;
  pixels: Uint8Array;
}
export type HeadRequest = HeadInit | HeadDetect;
export interface HeadReady { type: 'ready'; generation: number }
export interface HeadResult {
  type: 'result';
  generation: number;
  id: number;
  observedMs: number;
  face: number[];
}
export interface HeadError { type: 'error'; generation: number; message: string }
export type HeadReply = HeadReady | HeadResult | HeadError;

export interface WorkerLike {
  postMessage(message: HeadRequest): void;
  terminate(): void;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onmessageerror: ((event: unknown) => void) | null;
}

export type HeadTrackerState = 'off' | 'initializing' | 'active' | 'unavailable';
export interface HeadTrackerStatus { state: HeadTrackerState; message: string }

export interface HeadTrackerDeps {
  createWorker(): WorkerLike;
  /** Monotonic milliseconds, the same clock the controller derives elapsed time from. */
  now(): number;
  setTimer(callback: () => void, ms: number): unknown;
  clearTimer(handle: unknown): void;
  onStatus?(status: HeadTrackerStatus): void;
}

export const HEAD_STATUS_TEXT: Record<HeadTrackerState, string> = {
  off: 'Face locator is off. Detection is local and optional; frontal faces can be missed.',
  initializing: 'Face locator is starting. Detection is local and optional; frontal faces can be missed.',
  active: 'Face locator is active. Detection is local and optional; frontal faces can be missed.',
  unavailable: 'Face locator is unavailable. The mirror continues without skull placement.',
};

/**
 * Expands a detector face box into a head box, as the desktop preview does:
 * 10% wider on each side, 30% of the face height above and 5% below, all with
 * integer floor division, clipped to the detector image. Returns null if empty.
 */
export function expandFaceToHead(
  face: readonly number[], width: number, height: number,
): [number, number, number, number] | null {
  const [x, y, w, h] = face;
  const left = Math.max(0, x - Math.floor(w / 10));
  const right = Math.min(width, x + w + Math.floor(w / 10));
  const top = Math.max(0, y - Math.floor((h * 3) / 10));
  const bottom = Math.min(height, y + h + Math.floor(h / 20));
  return right > left && bottom > top ? [left, top, right - left, bottom - top] : null;
}

/**
 * Scales a box from a `from` sized image to a `to` sized frame. Edges round
 * outward (floor for left/top, ceil for right/bottom) using integers, then
 * clip to the frame. Returns null when nothing is left. No mirroring.
 */
export function scaleBox(
  box: readonly number[], fromW: number, fromH: number, toW: number, toH: number,
): [number, number, number, number] | null {
  const [x, y, w, h] = box;
  const left = Math.max(0, Math.floor((x * toW) / fromW));
  const top = Math.max(0, Math.floor((y * toH) / fromH));
  const right = Math.min(toW, Math.ceil(((x + w) * toW) / fromW));
  const bottom = Math.min(toH, Math.ceil(((y + h) * toH) / fromH));
  return right > left && bottom > top ? [left, top, right - left, bottom - top] : null;
}

/**
 * Downsamples RGBA (160x120 or 320x240) to 160x120 grayscale in `out`, using
 * the preview's weights (77, 150, 29) >> 8 and a floored 2x2 average. Reads
 * `rgba` only. Returns false for any other size or length.
 */
export function downsampleGray(rgba: Uint8Array, width: number, height: number, out: Uint8Array): boolean {
  if (out.length !== DETECT_WIDTH * DETECT_HEIGHT) return false;
  if (rgba.length !== width * height * 4) return false;
  const gray = (i: number) => (rgba[i] * 77 + rgba[i + 1] * 150 + rgba[i + 2] * 29) >> 8;
  if (width === DETECT_WIDTH && height === DETECT_HEIGHT) {
    for (let i = 0; i < out.length; i += 1) out[i] = gray(i * 4);
    return true;
  }
  if (width === DETECT_WIDTH * 2 && height === DETECT_HEIGHT * 2) {
    for (let y = 0; y < DETECT_HEIGHT; y += 1) {
      for (let x = 0; x < DETECT_WIDTH; x += 1) {
        const a = ((y * 2) * width + x * 2) * 4;
        const b = a + width * 4;
        out[y * DETECT_WIDTH + x] = (gray(a) + gray(a + 4) + gray(b) + gray(b + 4)) >> 2;
      }
    }
    return true;
  }
  return false;
}

function isUint(value: unknown, max = MAX_SAFE_MS): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max;
}

/** Validates a worker face array: empty, or five bounded integers. */
function validFace(face: unknown): face is number[] {
  if (!Array.isArray(face)) return false;
  if (face.length === 0) return true;
  if (face.length !== 5) return false;
  const [x, y, w, h, c] = face;
  return isUint(x, DETECT_WIDTH) && isUint(y, DETECT_HEIGHT)
    && isUint(w, DETECT_WIDTH) && isUint(h, DETECT_HEIGHT) && w > 0 && h > 0
    && x + w <= DETECT_WIDTH && y + h <= DETECT_HEIGHT
    && isUint(c, 1000) && c >= 800;
}

export class HeadTracker {
  private deps: HeadTrackerDeps;
  private generation = 0;
  private worker: WorkerLike | null = null;
  private phase: 'off' | 'initializing' | 'ready' | 'busy' = 'off';
  private failed = false;
  private hidden = false;
  private nextId = 1;
  private pending: { id: number; observedMs: number; width: number; height: number } | null = null;
  private initTimer: unknown = null;
  private inferenceTimer: unknown = null;
  private lastSubmitAt = Number.NEGATIVE_INFINITY;
  private lastOfferedMs: number | null = null;
  private lastAcceptedMs: number | null = null;
  /** Wall clock minus elapsed time, refreshed by every accepted offer. */
  private anchor = 0;
  private latest: HeadObservation | null = null;
  private staging: Uint8Array | null = null;
  private status: HeadTrackerState = 'off';

  constructor(deps: HeadTrackerDeps) {
    this.deps = deps;
  }

  getStatus(): HeadTrackerState {
    return this.status;
  }

  /** True while a worker exists, for tests and diagnostics. */
  hasWorker(): boolean {
    return this.worker !== null;
  }

  /**
   * Offers one unmirrored, cropped processing frame captured at `elapsedMs`
   * (an integer). The frame is only read. Returns true if it was submitted.
   * Starts the worker lazily; frames are dropped, never queued, while the
   * worker starts or is busy and between 200 ms submissions.
   */
  offer(rgba: Uint8Array, width: number, height: number, elapsedMs: number): boolean {
    if (this.failed || this.hidden || !isUint(elapsedMs)) return false;
    if (this.lastOfferedMs !== null && elapsedMs < this.lastOfferedMs) return false;
    this.lastOfferedMs = elapsedMs;
    this.anchor = this.deps.now() - elapsedMs;
    if (!this.worker) {
      this.start();
      return false;
    }
    if (this.phase !== 'ready') return false;
    if (this.deps.now() - this.lastSubmitAt < MIN_SUBMIT_INTERVAL_MS) return false;
    if (!this.staging) this.staging = new Uint8Array(DETECT_WIDTH * DETECT_HEIGHT);
    if (!downsampleGray(rgba, width, height, this.staging)) return false;
    const request: HeadDetect = {
      type: 'detect',
      generation: this.generation,
      id: this.nextId,
      observedMs: elapsedMs,
      // Structured clone copies the staging bytes; the staging buffer and the
      // renderer's frame both stay attached.
      pixels: this.staging,
    };
    this.nextId += 1;
    try {
      this.worker.postMessage(request);
    } catch {
      this.fail();
      return false;
    }
    this.pending = { id: request.id, observedMs: elapsedMs, width, height };
    this.phase = 'busy';
    this.lastSubmitAt = this.deps.now();
    const generation = this.generation;
    this.inferenceTimer = this.deps.setTimer(() => {
      if (generation === this.generation && this.phase === 'busy') this.fail();
    }, INFERENCE_TIMEOUT_MS);
    return true;
  }

  /**
   * The latest observation that is at most 500 ms older than `elapsedMs`, or
   * null. Synchronous; never waits for inference. Expired data is discarded.
   */
  observe(elapsedMs: number): HeadObservation | null {
    const seen = this.latest;
    if (!seen || seen.observedMs === undefined) return null;
    if (!Number.isFinite(elapsedMs) || elapsedMs < seen.observedMs || elapsedMs > MAX_SAFE_MS) return null;
    if (elapsedMs - seen.observedMs > MAX_RESULT_AGE_MS) {
      this.latest = null;
      return null;
    }
    return seen;
  }

  /** Document hiding terminates the worker; unhiding allows one fresh start. */
  setHidden(hidden: boolean): void {
    if (hidden === this.hidden) return;
    this.hidden = hidden;
    if (hidden) this.release(false);
  }

  /**
   * Terminates the worker and clears the observation, timers and storage.
   * Late callbacks from the old worker are ignored. `clearFailure` re-allows a
   * start; use it for a fresh source or a deliberate skull toggle.
   */
  release(clearFailure: boolean): void {
    this.generation += 1;
    this.stopTimers();
    const worker = this.worker;
    this.worker = null;
    if (worker) {
      worker.onmessage = null;
      worker.onerror = null;
      worker.onmessageerror = null;
      try {
        worker.terminate();
      } catch {
        // Terminating must never block cleanup.
      }
    }
    this.phase = 'off';
    this.pending = null;
    this.latest = null;
    this.staging = null;
    this.lastOfferedMs = null;
    this.lastAcceptedMs = null;
    this.lastSubmitAt = Number.NEGATIVE_INFINITY;
    if (clearFailure) this.failed = false;
    this.setStatus(this.failed ? 'unavailable' : 'off');
  }

  private start(): void {
    const generation = this.generation;
    let worker: WorkerLike;
    try {
      worker = this.deps.createWorker();
    } catch {
      this.fail();
      return;
    }
    this.worker = worker;
    this.phase = 'initializing';
    this.setStatus('initializing');
    const live = () => generation === this.generation && this.worker === worker;
    worker.onmessage = (event) => {
      if (live()) this.onMessage(generation, event.data);
    };
    worker.onerror = () => {
      if (live()) this.fail();
    };
    worker.onmessageerror = () => {
      if (live()) this.fail();
    };
    this.initTimer = this.deps.setTimer(() => {
      if (live() && this.phase === 'initializing') this.fail();
    }, INIT_TIMEOUT_MS);
    try {
      worker.postMessage({ type: 'init', generation, width: DETECT_WIDTH, height: DETECT_HEIGHT });
    } catch {
      this.fail();
    }
  }

  private onMessage(generation: number, data: unknown): void {
    if (!data || typeof data !== 'object') return this.fail();
    const message = data as Record<string, unknown>;
    if (message.generation !== generation) return;
    if (message.type === 'ready') {
      if (this.phase !== 'initializing') return;
      this.clearInit();
      this.phase = 'ready';
      this.setStatus('active');
    } else if (message.type === 'error') {
      this.fail();
    } else if (message.type === 'result') {
      this.onResult(message);
    } else {
      this.fail();
    }
  }

  private onResult(message: Record<string, unknown>): void {
    const pending = this.pending;
    if (this.phase !== 'busy' || !pending || message.id !== pending.id) return;
    if (message.observedMs !== pending.observedMs || !validFace(message.face)) return this.fail();
    this.clearInference();
    this.pending = null;
    this.phase = 'ready';
    const observedMs = pending.observedMs;
    const age = this.deps.now() - this.anchor - observedMs;
    if (age < 0 || age > MAX_RESULT_AGE_MS) return;
    if (this.lastAcceptedMs !== null && observedMs < this.lastAcceptedMs) return;
    this.lastAcceptedMs = observedMs;
    this.latest = null;
    const face = message.face as number[];
    if (face.length === 0) return;
    const head = expandFaceToHead(face, DETECT_WIDTH, DETECT_HEIGHT);
    const box = head ? scaleBox(head, DETECT_WIDTH, DETECT_HEIGHT, pending.width, pending.height) : null;
    if (!box) return;
    this.latest = { x: box[0], y: box[1], width: box[2], height: box[3], confidence: face[4], observedMs };
  }

  private fail(): void {
    this.failed = true;
    this.release(false);
  }

  private setStatus(state: HeadTrackerState): void {
    if (state === this.status) return;
    this.status = state;
    try {
      this.deps.onStatus?.({ state, message: HEAD_STATUS_TEXT[state] });
    } catch {
      // A status listener must never break cleanup.
    }
  }

  private clearInit(): void {
    if (this.initTimer !== null) this.deps.clearTimer(this.initTimer);
    this.initTimer = null;
  }

  private clearInference(): void {
    if (this.inferenceTimer !== null) this.deps.clearTimer(this.inferenceTimer);
    this.inferenceTimer = null;
  }

  private stopTimers(): void {
    this.clearInit();
    this.clearInference();
  }
}
