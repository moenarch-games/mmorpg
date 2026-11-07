import type * as THREE from "three";

/**
 * Opt-in gameplay frame profile (#546), enabled by `?profile` and read by
 * `scripts/profile-browser.py`. Each world frame is split into CPU phases timed with
 * `performance.now()`; the zone advance is also a `runtime-profiler:js-to-wasm:zone-advance`
 * User Timing measure so a Chromium trace attributes it. Without `?profile` every call is one
 * null check, and profiling only reads clocks and three's draw counters: it never changes
 * input, ticks or drawing. Each render pass's draw calls and triangles are counted, and the
 * main pass's by contributor (`profileRenderPasses`).
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

/** Draw calls and triangles of one render pass, as three's `renderer.info` counts them. */
export type PassDraws = { pass: string; calls: number; triangles: number };
/** One main-pass contributor: the drawn mesh's nearest name (digits folded) and its material. */
export type DrawContributor = { key: string; calls: number; triangles: number };

export type ProfiledFrame = FrameFacts & {
  /** The frame's render passes in draw order (sun cascades, local shadows, main, post). */
  passes: PassDraws[];
  /** The main pass's draws by contributor. */
  mainDraws: DrawContributor[];
  /** The sun cascades' and local shadow views' draws by contributor (without material: they draw the shadow one). */
  shadowDraws: DrawContributor[];
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
      raf, start, end: start, passes: [], mainDraws: [], shadowDraws: [], ticks: 0, viewer: [0, 0], heading: 0, nodes: 0, instanceBatches: 0,
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

  draws(passes: PassDraws[], mainDraws: DrawContributor[], shadowDraws: DrawContributor[]): void {
    if (this.#current) { Object.assign(this.#current, { passes, mainDraws, shadowDraws }); }
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

/**
 * A drawn object's contributor key: the scene child it hangs under, its nearest name (digits folded, so units group, or
 * its geometry when nothing up the tree is named) and, in the main pass, its material (shadow passes draw with the
 * shadow material).
 */
function contributor(object: THREE.Object3D, geometry: THREE.BufferGeometry, material: THREE.Material | null): string {
  let named: THREE.Object3D | null = object;
  while (named && !named.name) { named = named.parent; }
  let root = object;
  while (root.parent && !(root.parent as THREE.Scene).isScene) { root = root.parent; }
  const name = named?.name ?? `${object.type}:${geometry.name || geometry.type}`;
  const under = root !== object && root !== named ? `${root.name || root.type} › ` : "";
  const instanced = (object as THREE.InstancedMesh).isInstancedMesh ? " [instanced]" : "";
  return `${under}${name}`.replace(/\d+/g, "#") + `${material ? ` | ${material.name || material.type}` : ""}${instanced}`;
}

/**
 * Runs `draw` (the lighting's passes) counting each `renderer.render` of `scene` as a pass: an orthographic camera is a
 * sun cascade, `camera` the main pass (opaque, then transparent when the sky splits it), any other camera a local
 * shadow view; renders of other scenes (sky, tone mapping, output quads) are post passes. The main and shadow passes'
 * draws are grouped by contributor. Only three's counters are read: what is drawn does not change.
 */
export function profileRenderPasses(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, split: boolean, draw: () => void): void {
  const profile = active;
  if (!profile) { draw(); return; }
  const info = renderer.info;
  const passes: PassDraws[] = [];
  const contributors = { main: new Map<string, DrawContributor>(), shadow: new Map<string, DrawContributor>() };
  let cascade = 0;
  let mains = 0;
  let grouped: Map<string, DrawContributor> | null = null;
  const render = renderer.render;
  const direct = renderer.renderBufferDirect;
  renderer.render = function profiledRender(this: THREE.WebGLRenderer, drawn: THREE.Object3D, view: THREE.Camera): void {
    const isMain = drawn === scene && view === camera;
    const pass = drawn !== scene ? "post"
      : (view as THREE.OrthographicCamera).isOrthographicCamera ? `sun-cascade-${cascade++}`
        : isMain ? (split ? (mains++ === 0 ? "main-opaque" : "main-transparent") : "main")
          : "local-shadows";
    const calls = info.autoReset ? 0 : info.render.calls;
    const triangles = info.autoReset ? 0 : info.render.triangles;
    grouped = drawn !== scene ? null : isMain ? contributors.main : contributors.shadow;
    try {
      render.call(this, drawn, view);
    } finally {
      grouped = null;
    }
    const counted = { pass, calls: info.render.calls - calls, triangles: info.render.triangles - triangles };
    const last = passes[passes.length - 1];
    if (last?.pass === pass) { last.calls += counted.calls; last.triangles += counted.triangles; } else { passes.push(counted); }
  };
  renderer.renderBufferDirect = function profiledDraw(this: THREE.WebGLRenderer, view, drawnScene, geometry, material, object, group): void {
    const into = grouped;
    if (!into) { direct.call(this, view, drawnScene, geometry, material, object, group); return; }
    const calls = info.render.calls;
    const triangles = info.render.triangles;
    direct.call(this, view, drawnScene, geometry, material, object, group);
    const key = contributor(object, geometry, into === contributors.main ? material : null);
    const entry = into.get(key) ?? { key, calls: 0, triangles: 0 };
    entry.calls += info.render.calls - calls;
    entry.triangles += info.render.triangles - triangles;
    into.set(key, entry);
  };
  try {
    draw();
  } finally {
    renderer.render = render;
    renderer.renderBufferDirect = direct;
  }
  profile.draws(passes, [...contributors.main.values()], [...contributors.shadow.values()]);
}
