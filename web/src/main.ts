/// <reference lib="esnext.disposable" />
import './style.css';
import mirrorArtwork from '../../assets/illustrations/antique-mirror.svg';
import {
  CAMERA_CONSTRAINTS,
  FRAME_HEIGHT,
  FRAME_WIDTH,
  MirrorController,
  samplePhase,
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

function drawSampleScene(ctx: CanvasRenderingContext2D, w: number, h: number, phase: number) {
  const bg = ctx.createLinearGradient(0, 0, w, h);
  bg.addColorStop(0, '#080d14');
  bg.addColorStop(1, '#151d27');
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, w, h);
  // Asymmetric moving contours: the left side is busier so mirroring is visible.
  const cx = w * 0.36;
  const cy = h * 0.52;
  for (let k = 1; k <= 6; k += 1) {
    const wobble = Math.sin(phase * 2 + k * 0.9) * 4;
    ctx.beginPath();
    ctx.ellipse(cx, cy, 14 * k + wobble, 11 * k - wobble * 0.5, 0, 0, Math.PI * 2);
    ctx.lineWidth = 3;
    ctx.strokeStyle = k % 2 ? '#e5dfd1' : '#8d99a6';
    ctx.stroke();
  }
  const glow = ctx.createRadialGradient(cx, cy, 2, cx, cy, 30);
  glow.addColorStop(0, '#e5dfd1');
  glow.addColorStop(1, '#3a4452');
  ctx.fillStyle = glow;
  ctx.beginPath();
  ctx.arc(cx, cy, 26, 0, Math.PI * 2);
  ctx.fill();
  // A drifting disc and a slanted bar give clear edges on the right.
  ctx.fillStyle = '#9fb0bb';
  ctx.beginPath();
  ctx.arc(w * 0.76 + Math.cos(phase) * 28, h * 0.3 + Math.sin(phase) * 14, 22, 0, Math.PI * 2);
  ctx.fill();
  ctx.save();
  ctx.translate(w * 0.72, h * 0.78);
  ctx.rotate(-0.35 + Math.sin(phase) * 0.12);
  ctx.fillStyle = '#6f7d8a';
  ctx.fillRect(-48, -7, 96, 14);
  ctx.restore();
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
  const statusEl = need<HTMLElement>('#status');
  const metrics = need<HTMLElement>('#metrics');

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
  let shownMessage = '';

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
            samplePhase(elapsedMs, reducedMotion()),
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
    if (status.message !== shownMessage) {
      shownMessage = status.message;
      statusEl.textContent = status.message;
    }
    const live = status.state === 'camera' || status.state === 'sample';
    frame.hidden = !live;
    art.hidden = live;
    figure.classList.toggle('live', live);
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
      `Rust/WASM ${status.processMs.toFixed(1)} ms | draw ${status.renderMs?.toFixed(1)} ms`;
  }

  const applyControls = () => {
    controller.setMode(Number(effect.value));
    const percent = Number(intensity.value);
    intensityValue.textContent = `${percent}%`;
    controller.setIntensity((percent * 255) / 100);
  };
  effect.addEventListener('change', applyControls);
  intensity.addEventListener('input', applyControls);
  applyControls();

  startCamera.addEventListener('click', () => void controller.startCamera());
  startSample.addEventListener('click', () => void controller.startSample());
  stop.addEventListener('click', () => controller.stop());
  window.addEventListener('pagehide', () => controller.dispose());
}
