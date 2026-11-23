export interface FullscreenDeps {
  supported(): boolean;
  active(): boolean;
  request(): Promise<void>;
  exit(): Promise<void>;
}

/** Stops a late fullscreen request from reopening a stopped mirror. */
export class FullscreenSession {
  private deps: FullscreenDeps;
  private generation = 0;
  private entering = false;

  constructor(deps: FullscreenDeps) { this.deps = deps; }

  async enter(): Promise<boolean> {
    if (!this.deps.supported() || this.entering) return false;
    if (this.deps.active()) return true;
    const generation = this.generation;
    this.entering = true;
    try {
      await this.deps.request();
      if (generation !== this.generation) {
        if (this.deps.active()) await this.deps.exit();
        return false;
      }
      return this.deps.active();
    } catch {
      return false;
    } finally {
      this.entering = false;
    }
  }

  async stop(): Promise<void> {
    this.generation += 1;
    if (this.deps.active()) {
      try { await this.deps.exit(); } catch { /* Browser controls can still exit. */ }
    }
  }
}
