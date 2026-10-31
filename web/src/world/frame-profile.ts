/**
 * Opt-in gameplay frame profile (#546), enabled by `?profile` and read by
 * `scripts/profile-browser.py`. Each world frame is split into CPU phases timed with
 * `performance.now()`; the zone advance is also a `runtime-profiler:js-to-wasm:zone-advance`
 * User Timing measure so a Chromium trace attributes it. Without `?profile` every call is one
 * null check, and profiling only reads clocks: it never changes input, ticks or drawing.
 *
 * These are main-thread CPU times. GPU and compositor presentation happen after `render`
 * returns and are not visible here; the harness reads them from the trace.
 */
export const FRAME_PHASES = ["input", "simulation", "projection", "units", "scene", "render", "hud"] as const;
export type FramePhase = (typeof FRAME_PHASES)[number];

export type FrameFacts = {
  /** Zone ticks the frame advanced (60 Hz fixed step). */
  ticks: number;
  /** The viewer's drawn feet position, to find the first frame that shows a keypress. */
  viewer: [number, number];
  /** Camera orbit heading in radians. */
  heading: number;
  nodes: number;
  instanceBatches: number;
  /** The renderer's resource churn this frame: meshes, geometries and materials created, instance batches uploaded. */
  created: { objects: number; geometries: number; materials: number; instanceUploads: number };
};

export type ProfiledFrame = FrameFacts & {
  /** The `requestAnimationFrame` timestamp. */
  raf: number;
  /** `performance.now()` when the frame callback started and finished. */
  start: number;
  end: number;
  /** Milliseconds per phase; their sum is `end - start`. */
  phases: Record<FramePhase, number>;
};

const CAPACITY = 36_000;
const ZONE_ADVANCE_MEASURE = "runtime-profiler:js-to-wasm:zone-advance";

class FrameProfile {
  readonly #frames: ProfiledFrame[] = [];
  #current: ProfiledFrame | null = null;
  #last = 0;

  begin(raf: number): void {
    const start = performance.now();
    this.#last = start;
    this.#current = {
      raf, start, end: start, ticks: 0, viewer: [0, 0], heading: 0, nodes: 0, instanceBatches: 0,
      created: { objects: 0, geometries: 0, materials: 0, instanceUploads: 0 },
      phases: { input: 0, simulation: 0, projection: 0, units: 0, scene: 0, render: 0, hud: 0 },
    };
  }

  /** Charges the time since the previous phase boundary to `phase`. */
  phase(phase: FramePhase): void {
    const current = this.#current;
    if (!current) { return; }
    const now = performance.now();
    current.phases[phase] += now - this.#last;
    if (phase === "simulation") {
      performance.measure(ZONE_ADVANCE_MEASURE, { start: this.#last, end: now });
      performance.clearMeasures(ZONE_ADVANCE_MEASURE);
    }
    this.#last = now;
  }

  facts(facts: FrameFacts): void {
    if (this.#current) { Object.assign(this.#current, facts); }
  }

  /** Ends a world frame; the remainder since the last boundary is HUD work. Selection frames are dropped. */
  end(): void {
    const current = this.#current;
    if (!current) { return; }
    this.phase("hud");
    this.#current = null;
    if (current.nodes === 0) { return; }
    current.end = this.#last;
    if (this.#frames.length >= CAPACITY) { this.#frames.shift(); }
    this.#frames.push(current);
  }

  take(): ProfiledFrame[] {
    return this.#frames.splice(0);
  }
}

let active: FrameProfile | null = null;

/** Turns profiling on for this page and returns the reader the harness uses. */
export function enableFrameProfile(): { take(): ProfiledFrame[] } {
  active ??= new FrameProfile();
  const profile = active;
  return { take: () => profile.take() };
}

/** Whether `?profile` is on; guard any per-frame allocation for the profile with it. */
export function profiling(): boolean { return active !== null; }
export function profileBegin(raf: number): void { active?.begin(raf); }
export function profilePhase(phase: FramePhase): void { active?.phase(phase); }
export function profileFacts(facts: FrameFacts): void { active?.facts(facts); }
export function profileEnd(): void { active?.end(); }
