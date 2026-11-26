// Module worker: runs the local frontal-face detector off the main thread.
// It owns a separate WASM instance and never touches the DOM, the camera or
// the network beyond loading its own same-origin WASM asset. The protocol is
// documented in head-tracker.ts.
import wasmUrl from '../generated/spook_web_bg.wasm?url';
import type { HeadReply } from './head-tracker.ts';

type Detector = { detect_gray8(pixels: Uint8Array): Uint32Array | number[]; free(): void };

const scope = self as unknown as {
  postMessage(message: HeadReply): void;
  onmessage: ((event: { data: unknown }) => void) | null;
};

let detector: Detector | null = null;
let generation = -1;
let starting = false;

function reply(message: HeadReply): void {
  scope.postMessage(message);
}

async function initialize(width: number, height: number): Promise<void> {
  const module = await import('../generated/spook_web.js');
  await module.default({ module_or_path: wasmUrl });
  detector = new module.BrowserFaceDetector(width, height);
}

scope.onmessage = async (event) => {
  const data = event.data as Record<string, unknown> | null;
  if (!data || typeof data !== 'object') return;
  try {
    if (data.type === 'init' && !starting && !detector) {
      starting = true;
      generation = Number(data.generation);
      await initialize(Number(data.width), Number(data.height));
      reply({ type: 'ready', generation });
    } else if (data.type === 'detect' && detector && data.generation === generation) {
      const pixels = data.pixels;
      if (!(pixels instanceof Uint8Array)) throw new Error('Expected grayscale pixels.');
      const face = Array.from(detector.detect_gray8(pixels));
      reply({
        type: 'result',
        generation,
        id: Number(data.id),
        observedMs: Number(data.observedMs),
        face,
      });
    }
  } catch (error) {
    reply({
      type: 'error',
      generation,
      message: error instanceof Error ? error.message : 'Face locator failed.',
    });
  }
};
