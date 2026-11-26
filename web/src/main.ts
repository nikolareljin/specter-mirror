/// <reference lib="esnext.disposable" />
import './style.css';
import './fullscreen.css';
import mirrorArtwork from '../../assets/illustrations/antique-mirror.svg';
import { drawSampleScene, sampleHeadBounds } from './sample';
import { FullscreenSession } from './fullscreen';
import { HeadTracker, type HeadTrackerStatus, type WorkerLike } from './head-tracker';
import {
  CAMERA_CONSTRAINTS,
  FRAME_HEIGHT,
  FRAME_WIDTH,
  MirrorController,
  type EngineLike,
  type MirrorStatus,
  type SourceKind,
  type StreamLike,
} from './mirror';

const mirror = document.querySelector<HTMLImageElement>('.mirror img');
if (mirror) mirror.src = mirrorArtwork;
const reduced = document.querySelector<HTMLInputElement>('#reduced-motion');
const reducedMotion = () => document.documentElement.classList.contains('reduced-motion');
if (reduced) {
  reduced.checked = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  document.documentElement.classList.toggle('reduced-motion', reduced.checked);
  reduced.addEventListener('change', () => {
    document.documentElement.classList.toggle('reduced-motion', reduced.checked);
  });
}

const canvas = document.querySelector<HTMLCanvasElement>('#mirror-canvas');
if (canvas) setupMirror(canvas);

type WasmModule = typeof import('../generated/spook_web.js');
let wasmReady: Promise<WasmModule> | null = null;

/** Loads and initializes the generated WASM module once; a failure can be retried. */
function loadWasm(): Promise<WasmModule> {
  if (!wasmReady) {
    wasmReady = import('../generated/spook_web.js').then(async (module) => {
      await module.default();
      return module;
    });
    wasmReady.catch(() => {
      wasmReady = null;
    });
  }
  return wasmReady;
}

function setupMirror(canvas: HTMLCanvasElement) {
  const need = <T extends HTMLElement>(selector: string): T => {
    const el = document.querySelector<T>(selector);
    if (!el) throw new Error(`Missing ${selector}`);
    return el;
  };
  const video = need<HTMLVideoElement>('#camera');
  const frame = need<HTMLElement>('#mirror-frame');
  const art = need<HTMLImageElement>('#mirror-art');
  const figure = need<HTMLElement>('#mirror-figure');
  const captionTitle = need<HTMLElement>('#caption-title');
  const captionNote = need<HTMLElement>('#caption-note');
  const startCamera = need<HTMLButtonElement>('#start-camera');
  const startSample = need<HTMLButtonElement>('#start-sample');
  const stop = need<HTMLButtonElement>('#stop');
  const effect = need<HTMLSelectElement>('#effect');
  const intensity = need<HTMLInputElement>('#intensity');
  const intensityValue = need<HTMLOutputElement>('#intensity-value');
  const delay = need<HTMLInputElement>('#delay');
  const delayValue = need<HTMLOutputElement>('#delay-value');
  const trails = need<HTMLInputElement>('#trails');
  const trailsValue = need<HTMLOutputElement>('#trails-value');
  const profile = need<HTMLSelectElement>('#haunt-profile');
  const automatic = need<HTMLInputElement>('#automatic');
  const skull = need<HTMLInputElement>('#skull');
  const skullNote = need<HTMLElement>('#skull-note');
  const haunting = need<HTMLElement>('#haunt-status');
  const temporalNote = need<HTMLElement>('#temporal-note');
  const statusEl = need<HTMLElement>('#status');
  const metrics = need<HTMLElement>('#metrics');
  const fullscreenButton = need<HTMLButtonElement>('#fullscreen');
  const exitFullscreen = need<HTMLButtonElement>('#exit-fullscreen');
  const fullscreenStop = need<HTMLButtonElement>('#stop-fullscreen');
  const fullscreenNote = need<HTMLElement>('#fullscreen-note');
  const fullscreenSupported = typeof figure.requestFullscreen === 'function';
  let live = false;
  let fullscreenBusy = false;
  const fullscreen = new FullscreenSession({
    supported: () => fullscreenSupported,
    active: () => document.fullscreenElement === figure,
    request: () => figure.requestFullscreen(),
    exit: () => document.exitFullscreen(),
  });

  function updateFullscreenControls() {
    fullscreenButton.disabled = !live || fullscreenBusy || !fullscreenSupported
      || document.fullscreenElement === figure;
  }
  if (!fullscreenSupported) fullscreenNote.textContent = 'Full screen is unavailable in this browser.';

  const view = canvas.getContext('2d');
  const scratch = document.createElement('canvas');
  scratch.width = FRAME_WIDTH;
  scratch.height = FRAME_HEIGHT;
  const scratchCtx = scratch.getContext('2d', { willReadFrequently: true });
  if (!view || !scratchCtx) {
    statusEl.textContent = 'Canvas 2D is unavailable in this browser.';
    startCamera.disabled = true;
    startSample.disabled = true;
    return;
  }
  const output = view.createImageData(FRAME_WIDTH, FRAME_HEIGHT);
  const skullBase = 'Sample placement is demonstrated. With the camera, a local face locator may place the skull; it is optional and can miss faces that are not frontal.';
  const showHeadStatus = (status: HeadTrackerStatus) => {
    skullNote.textContent = status.state === 'off' ? skullBase : `${skullBase} ${status.message}`;
  };
  skullNote.textContent = skullBase;
  // Face inference runs in a module worker with its own WASM instance and is
  // started lazily by the first eligible camera frame.
  const headTracker = new HeadTracker({
    createWorker: () => new Worker(new URL('./head-worker.ts', import.meta.url), {
      type: 'module',
    }) as unknown as WorkerLike,
    now: () => performance.now(),
    setTimer: (callback, ms) => setTimeout(callback, ms),
    clearTimer: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
    onStatus: showHeadStatus,
  });
  let shownMessage = '';
  // The visitor's visit plan follows the engine's timing seed; none means the
  // original 28 second loop.
  const sampleSeed = () => (controller.seed === null ? null : (controller.seed ^ 0xa5a5a5a5) >>> 0);

  const controller = new MirrorController(
    {
      async getUserMedia(): Promise<StreamLike> {
        if (!navigator.mediaDevices?.getUserMedia) {
          throw Object.assign(new Error('Camera access is not available here.'), {
            name: 'NotReadableError',
          });
        }
        return navigator.mediaDevices.getUserMedia(CAMERA_CONSTRAINTS);
      },
      async createEngine(width, height): Promise<EngineLike> {
        const module = await loadWasm();
        return new module.MirrorEngine(width, height);
      },
      async attachStream(stream) {
        video.srcObject = stream as MediaStream;
        await video.play();
      },
      detachStream() {
        video.pause();
        video.srcObject = null;
      },
      captureFrame(source: SourceKind, elapsedMs: number) {
        if (source === 'sample') {
          drawSampleScene(
            scratchCtx,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            elapsedMs,
            reducedMotion(),
            sampleSeed(),
          );
        } else {
          if (video.readyState < 2 || !video.videoWidth || !video.videoHeight) return null;
          // Crop to 4:3 instead of stretching the camera image.
          const aspect = FRAME_WIDTH / FRAME_HEIGHT;
          let sw = video.videoWidth;
          let sh = video.videoHeight;
          if (sw / sh > aspect) sw = Math.round(sh * aspect);
          else sh = Math.round(sw / aspect);
          const sx = Math.floor((video.videoWidth - sw) / 2);
          const sy = Math.floor((video.videoHeight - sh) / 2);
          scratchCtx.drawImage(video, sx, sy, sw, sh, 0, 0, FRAME_WIDTH, FRAME_HEIGHT);
        }
        const pixels = scratchCtx.getImageData(0, 0, FRAME_WIDTH, FRAME_HEIGHT).data;
        return new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength);
      },
      randomSeed() {
        const words = new Uint32Array(1);
        crypto.getRandomValues(words);
        return words[0];
      },
      observeHead(source: SourceKind, elapsedMs: number) {
        // The generated visitor has known geometry. A camera reports only the
        // latest fresh result of the local face locator, never a guess.
        if (source !== 'sample') return headTracker.observe(elapsedMs);
        const bounds = sampleHeadBounds(
          FRAME_WIDTH, FRAME_HEIGHT, elapsedMs, reducedMotion(), sampleSeed(),
        );
        return bounds ? { ...bounds, confidence: 1000 } : null;
      },
      offerHeadFrame(frame, width, height, elapsedMs) {
        headTracker.offer(frame, width, height, elapsedMs);
      },
      releaseHeadTracking(retry) {
        headTracker.release(retry);
      },
      render(rgba) {
        output.data.set(rgba);
        view.putImageData(output, 0, 0);
      },
      schedule: (callback) => requestAnimationFrame(callback),
      cancel: (handle) => cancelAnimationFrame(handle),
      now: () => performance.now(),
    },
    showStatus,
  );

  function showStatus(status: MirrorStatus) {
    const phase = status.hauntState ?? (automatic.checked ? 'Waiting for a source' : 'Manual reflection');
    if (haunting.textContent !== phase) haunting.textContent = phase;
    if (status.message !== shownMessage) {
      shownMessage = status.message;
      statusEl.textContent = status.message;
    }
    const wasLive = live;
    live = status.state === 'camera' || status.state === 'sample';
    if (wasLive && !live) void fullscreen.stop();
    updateFullscreenControls();
    frame.hidden = !live;
    art.hidden = live;
    figure.classList.toggle('live', live);
    figure.closest('.stage')?.classList.toggle('mirror-running', live);
    stop.disabled = status.state === 'idle' || status.state === 'error';
    if (live) {
      captionTitle.textContent = status.state === 'camera' ? 'Live camera' : 'Generated scene';
      captionNote.textContent =
        status.state === 'camera'
          ? 'Processed locally in your browser'
          : 'Generated in this page, not webcam footage';
    } else {
      captionTitle.textContent = 'Come closer...';
      captionNote.textContent = 'Original concept artwork';
    }
    if (status.processMs === null) {
      if (!live) metrics.textContent = 'No frames yet.';
      return;
    }
    const fps = status.fps === null ? 'measuring' : `${status.fps.toFixed(1)} fps`;
    metrics.textContent =
      `${fps} | capture ${status.captureMs?.toFixed(1)} ms | ` +
      `Rust/WASM ${status.processMs.toFixed(1)} ms | draw ${status.renderMs?.toFixed(1)} ms` +
      (status.hauntStrength === null ? '' : ` | effect ${Math.round(status.hauntStrength / 60)}%`);
  }

  const applyControls = (custom = true) => {
    controller.setAutomatic(automatic.checked);
    controller.setSkull(skull.checked);
    skull.disabled = !automatic.checked;
    for (const control of [profile, effect, intensity, delay, trails]) control.disabled = automatic.checked;
    if (custom) profile.value = 'custom';
    controller.setMode(Number(effect.value));
    const percent = Number(intensity.value);
    intensityValue.textContent = `${percent}%`;
    controller.setIntensity((percent * 255) / 100);
    const delayMs = Number(delay.value);
    const trailPercent = Number(trails.value);
    controller.setReducedMotion(reducedMotion());
    controller.setTemporal(delayMs, Math.round(trailPercent * 255 / 100));
    delayValue.textContent = `${delayMs} ms`;
    trailsValue.textContent = `${trailPercent}%`;
    temporalNote.textContent = automatic.checked && !reducedMotion()
      ? 'Starts clear. Movement gradually wakes the reflection; after stillness, it settles. Manual controls return when automatic haunting is off.'
      : reducedMotion()
      ? 'Reduced motion: delay and ghost trails are off. The reflection stays live.'
      : 'Delay and trails combine with any reflection. History warms up as frames arrive.';
  };
  effect.addEventListener('change', () => applyControls());
  intensity.addEventListener('input', () => applyControls());
  delay.addEventListener('input', () => applyControls());
  trails.addEventListener('input', () => applyControls());
  reduced?.addEventListener('change', () => applyControls(false));
  automatic.addEventListener('change', () => applyControls(false));
  skull.addEventListener('change', () => applyControls(false));
  profile.addEventListener('change', () => {
    const presets: Record<string, [string, string, string, string]> = {
      restless: ['1', '55', '250', '45'],
      trails: ['1', '70', '0', '65'],
      portrait: ['2', '75', '300', '35'],
      clear: ['0', '0', '0', '0'],
    };
    const settings = presets[profile.value];
    if (settings) [effect.value, intensity.value, delay.value, trails.value] = settings;
    applyControls(false);
  });
  applyControls(false);

  startCamera.addEventListener('click', () => void controller.startCamera());
  startSample.addEventListener('click', () => void controller.startSample());
  stop.addEventListener('click', () => controller.stop());
  fullscreenStop.addEventListener('click', () => controller.stop());
  exitFullscreen.addEventListener('click', () => void fullscreen.stop());
  fullscreenButton.addEventListener('click', async () => {
    if (!live || fullscreenBusy) return;
    fullscreenBusy = true;
    updateFullscreenControls();
    const entered = await fullscreen.enter();
    fullscreenBusy = false;
    if (live) fullscreenNote.textContent = entered
      ? 'Press Escape or Exit full screen to return. Stop mirror releases the camera.'
      : 'Full screen could not be opened. The mirror can continue in this page.';
    updateFullscreenControls();
  });
  document.addEventListener('fullscreenchange', updateFullscreenControls);
  window.addEventListener('pagehide', () => {
    controller.dispose();
    void fullscreen.stop();
  });
  document.addEventListener('visibilitychange', () => {
    headTracker.setHidden(document.hidden);
    if (document.hidden) controller.stop();
  });
}
