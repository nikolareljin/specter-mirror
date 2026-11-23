// Mirror lifecycle: source selection, frame loop and cleanup.
// No DOM or WASM imports; everything environment-specific is injected.
// Keep this file free of parameter properties, enums and namespaces so Node's
// type stripping can run it directly in tests.

export const FRAME_WIDTH = 320;
export const FRAME_HEIGHT = 240;

export const MODES = [
  { id: 0, label: 'Mirror' },
  { id: 1, label: 'Spectral Mirror' },
  { id: 2, label: 'Cursed Portrait' },
] as const;

export const CAMERA_CONSTRAINTS = {
  audio: false,
  video: {
    width: { ideal: FRAME_WIDTH },
    height: { ideal: FRAME_HEIGHT },
    facingMode: { ideal: 'user' },
  },
};

export type SourceKind = 'camera' | 'sample';
export type MirrorState = 'idle' | 'starting' | 'camera' | 'sample' | 'error';

export interface TrackLike {
  readonly readyState?: string;
  stop(): void;
  addEventListener(type: 'ended', listener: () => void): void;
}
export interface StreamLike {
  getTracks(): TrackLike[];
}
export interface EngineLike {
  process_rgba(input: Uint8Array, mode: number, intensity: number): Uint8Array;
  free(): void;
}

export interface MirrorDeps {
  /** Called only from startCamera, which the UI calls from a button click. */
  getUserMedia(): Promise<StreamLike>;
  createEngine(width: number, height: number): Promise<EngineLike> | EngineLike;
  /** Bind the stream to the video element and start playback. */
  attachStream(stream: StreamLike): Promise<void>;
  detachStream(): void;
  /** Returns RGBA of exactly FRAME_WIDTH x FRAME_HEIGHT, or null if not ready. */
  captureFrame(source: SourceKind, elapsedMs: number): Uint8Array | null;
  render(rgba: Uint8Array): void;
  schedule(callback: () => void): number;
  cancel(handle: number): void;
  now(): number;
}

export interface MirrorStatus {
  state: MirrorState;
  message: string;
  fps: number | null;
  captureMs: number | null;
  processMs: number | null;
  renderMs: number | null;
}

const FPS_WINDOW = 30;

export function describeCameraError(error: unknown): string {
  const name = error instanceof Error || (error && typeof error === 'object')
    ? String((error as { name?: unknown }).name ?? '')
    : '';
  switch (name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return 'Camera permission was denied. You can try again or use the generated sample scene.';
    case 'NotFoundError':
    case 'OverconstrainedError':
      return 'No camera was found. Use the generated sample scene instead.';
    case 'NotReadableError':
    case 'AbortError':
      return 'The camera is unavailable or in use by another application. Use the generated sample scene instead.';
    default:
      return 'The camera could not be started. Use the generated sample scene instead.';
  }
}

export class MirrorController {
  private deps: MirrorDeps;
  private onStatus: (status: MirrorStatus) => void;
  private epoch = 0;
  private state: MirrorState = 'idle';
  private source: SourceKind | null = null;
  private stream: StreamLike | null = null;
  private engine: EngineLike | null = null;
  private handle: number | null = null;
  private busy = false;
  private startedAt = 0;
  private frameTimes: number[] = [];
  private mode = 1;
  private intensity = 200;

  constructor(deps: MirrorDeps, onStatus: (status: MirrorStatus) => void) {
    this.deps = deps;
    this.onStatus = onStatus;
    this.emit('Choose the camera or the generated sample scene.');
  }

  setMode(mode: number): void {
    if (MODES.some((m) => m.id === mode)) this.mode = mode;
  }

  setIntensity(intensity: number): void {
    this.intensity = Math.max(0, Math.min(255, Math.round(intensity)));
  }

  getState(): MirrorState {
    return this.state;
  }

  async startCamera(): Promise<void> {
    const epoch = this.begin('starting', 'Waiting for camera permission...');
    let stream: StreamLike;
    try {
      stream = await this.deps.getUserMedia();
    } catch (error) {
      if (epoch === this.epoch) this.fail(describeCameraError(error));
      return;
    }
    if (epoch !== this.epoch) {
      stopStream(stream);
      return;
    }
    this.stream = stream;
    try {
      for (const track of stream.getTracks()) {
        track.addEventListener('ended', () => {
          if (epoch === this.epoch) this.fail('The camera stopped or was disconnected.');
        });
        if (track.readyState === 'ended') {
          this.fail('The camera stopped or was disconnected.');
          return;
        }
      }
      const engine = await this.deps.createEngine(FRAME_WIDTH, FRAME_HEIGHT);
      if (epoch !== this.epoch) {
        engine.free();
        stopStream(stream);
        if (this.stream === stream) this.stream = null;
        return;
      }
      this.engine = engine;
      await this.deps.attachStream(stream);
      if (epoch !== this.epoch) return;
    } catch (error) {
      if (epoch === this.epoch) {
        this.fail(
          error instanceof Error && error.message
            ? `Could not start the mirror: ${error.message}`
            : 'Could not start the mirror.',
        );
      } else {
        stopStream(stream);
      }
      return;
    }
    this.run(epoch, 'camera', 'Camera mirror running. Frames stay on this device.');
  }

  async startSample(): Promise<void> {
    const epoch = this.begin('starting', 'Preparing the generated sample scene...');
    try {
      const engine = await this.deps.createEngine(FRAME_WIDTH, FRAME_HEIGHT);
      if (epoch !== this.epoch) {
        engine.free();
        return;
      }
      this.engine = engine;
    } catch (error) {
      if (epoch === this.epoch) {
        this.fail(
          error instanceof Error && error.message
            ? `Could not start the mirror: ${error.message}`
            : 'Could not start the mirror.',
        );
      }
      return;
    }
    this.run(epoch, 'sample', 'Generated sample scene running. This is not webcam footage.');
  }

  stop(): void {
    this.epoch += 1;
    this.teardown();
    this.state = 'idle';
    this.emit('Stopped. The camera is released.');
  }

  /** For pagehide or page teardown. */
  dispose(): void {
    this.stop();
  }

  private begin(state: MirrorState, message: string): number {
    this.epoch += 1;
    this.teardown();
    this.state = state;
    this.emit(message);
    return this.epoch;
  }

  private run(epoch: number, source: SourceKind, message: string): void {
    if (epoch !== this.epoch) return;
    this.source = source;
    this.state = source;
    this.startedAt = this.deps.now();
    this.frameTimes = [];
    this.emit(message);
    this.queue(epoch);
  }

  private queue(epoch: number): void {
    if (epoch !== this.epoch || this.handle !== null) return;
    this.handle = this.deps.schedule(() => {
      this.handle = null;
      this.tick(epoch);
    });
  }

  private tick(epoch: number): void {
    if (epoch !== this.epoch || this.busy || !this.engine || !this.source) return;
    this.busy = true;
    let captureMs = 0;
    let processMs = 0;
    let renderMs = 0;
    try {
      const t0 = this.deps.now();
      const frame = this.deps.captureFrame(this.source, t0 - this.startedAt);
      const t1 = this.deps.now();
      captureMs = t1 - t0;
      if (frame) {
        const output = this.engine.process_rgba(frame, this.mode, this.intensity);
        const t2 = this.deps.now();
        processMs = t2 - t1;
        this.deps.render(output);
        const t3 = this.deps.now();
        renderMs = t3 - t2;
        this.frameTimes.push(t3);
        if (this.frameTimes.length > FPS_WINDOW) this.frameTimes.shift();
        this.emit(undefined, captureMs, processMs, renderMs);
      }
    } catch (error) {
      this.busy = false;
      this.fail(
        error instanceof Error && error.message
          ? `Processing stopped: ${error.message}`
          : 'Processing stopped.',
      );
      return;
    }
    this.busy = false;
    this.queue(epoch);
  }

  private fail(message: string): void {
    this.epoch += 1;
    this.teardown();
    this.state = 'error';
    this.emit(message);
  }

  private teardown(): void {
    if (this.handle !== null) {
      this.deps.cancel(this.handle);
      this.handle = null;
    }
    if (this.stream) {
      stopStream(this.stream);
      this.stream = null;
    }
    try {
      this.deps.detachStream();
    } catch {
      // Detaching must never block cleanup.
    }
    if (this.engine) {
      try {
        this.engine.free();
      } catch {
        // Freeing must never block cleanup.
      }
      this.engine = null;
    }
    this.source = null;
    this.busy = false;
    this.frameTimes = [];
  }

  private lastMessage = '';

  private emit(message?: string, captureMs?: number, processMs?: number, renderMs?: number): void {
    if (message !== undefined) this.lastMessage = message;
    const times = this.frameTimes;
    let fps: number | null = null;
    if (times.length >= 2) {
      const span = times[times.length - 1] - times[0];
      if (span > 0) fps = ((times.length - 1) * 1000) / span;
    }
    this.onStatus({
      state: this.state,
      message: this.lastMessage,
      fps,
      captureMs: captureMs ?? null,
      processMs: processMs ?? null,
      renderMs: renderMs ?? null,
    });
  }
}

function stopStream(stream: StreamLike): void {
  for (const track of stream.getTracks()) {
    try {
      track.stop();
    } catch {
      // Keep stopping the remaining tracks.
    }
  }
}

/**
 * Deterministic animation phase for the generated sample scene, in radians.
 * Reduced motion returns a fixed phase so the scene stays static.
 */
export function samplePhase(elapsedMs: number, reducedMotion: boolean): number {
  if (reducedMotion) return 0.6;
  return ((elapsedMs / 1000) * 0.9) % (Math.PI * 2);
}
