import type * as THREE from "three";

/**
 * Opt-in gameplay frame profile (#546), enabled by `?profile` and read by
 * `scripts/profile-browser.py`. Each world frame is split into CPU phases timed with
 * `performance.now()`; the zone advance is also a `runtime-profiler:js-to-wasm:zone-advance`
 * User Timing measure so a Chromium trace attributes it. Without `?profile` every call is one
 * null check, and profiling only reads clocks and three's draw counters: it never changes
 * input, ticks or drawing. Each render pass's draw calls and triangles are counted, and the
 * main pass's by contributor (`profileRenderPasses`). Where the browser exposes
 * `EXT_disjoint_timer_query_webgl2`, each render pass's GPU time is measured too (#1106,
 * `GpuPassTimer`): timer queries in sequence, never nested.
 *
 * The phases are main-thread CPU times. Compositor presentation happens after `render`
 * returns and is not visible here; the harness reads it from the trace.
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
  /**
   * GPU milliseconds per render pass (#1106), labelled like `passes` and, in the main passes, by what draws
   * (`main-opaque:grass`, …); their sum is the lighting's whole frame. `null` when not measured (no
   * `EXT_disjoint_timer_query_webgl2`, or no lit frame), `"disjoint"` when the GPU timer reported a disjoint period.
   */
  gpu: Record<string, number> | "pending" | "disjoint" | null;
  /**
   * Main-thread milliseconds while each `gpu` label was being timed: the time to issue its commands. A timer query
   * measures from when the GPU reaches its begin to when it reaches its end, including any wait for the commands
   * between, so a label whose GPU time is about its issue time is bound by issuing (the main thread), one well above
   * it by the GPU.
   */
  gpuIssue: Record<string, number> | null;
  /** Timer queries the frame's GPU passes took. */
  gpuSpans: number;
};

/** One timer query of a frame: its label, and the main-thread time it was open. */
type Span = { label: string; query: WebGLQuery; cpu: number };

/**
 * GPU time per render pass (#1106), from `EXT_disjoint_timer_query_webgl2`. Timer queries cannot nest, so a frame's
 * passes are one chain of back-to-back spans, each one query: a span starts unlabelled at a render-target change (a
 * pass boundary) and takes the label of the first draw after it, so a pass's target switch (and clear) count towards
 * it; a draw with a different label ends the span and starts the next. The labels' sum is the GPU time of
 * `Lighting.render`, including any time the GPU waited for the main thread to issue the commands (`gpuIssue`).
 * Results arrive a few frames later; until then the frame's `gpu` is `"pending"`.
 */
class GpuPassTimer {
  readonly #gl: WebGL2RenderingContext;
  readonly #timeElapsed: number;
  readonly #disjoint: number;
  readonly #free: WebGLQuery[] = [];
  readonly #pending: { frame: ProfiledFrame; spans: Span[] }[] = [];
  #spans: Span[] = [];
  #open: { label: string | null; query: WebGLQuery; start: number } | null = null;

  constructor(gl: WebGL2RenderingContext, ext: { TIME_ELAPSED_EXT: number; GPU_DISJOINT_EXT: number }) {
    this.#gl = gl;
    this.#timeElapsed = ext.TIME_ELAPSED_EXT;
    this.#disjoint = ext.GPU_DISJOINT_EXT;
  }

  /** Starts a frame's chain. */
  start(): void {
    this.#spans = [];
    this.#open = this.#begin(null);
  }

  /** A pass boundary: a labelled span ends, and the next span takes the next draw's label. */
  boundary(): void {
    if (this.#open?.label) { this.#end(); this.#open = this.#begin(null); }
  }

  /** The GPU work from here on is `label`'s. */
  enter(label: string): void {
    const open = this.#open;
    if (!open || open.label === label) { return; }
    if (open.label === null) { open.label = label; return; }
    this.#end();
    this.#open = this.#begin(label);
  }

  /** Ends the chain; `frame.gpu` is filled once every span's result is available. */
  finish(frame: ProfiledFrame): void {
    if (!this.#open) { return; }
    this.#end();
    frame.gpu = "pending";
    frame.gpuSpans = this.#spans.length;
    this.#pending.push({ frame, spans: this.#spans });
    this.#spans = [];
  }

  /** Resolves the frames whose spans have all finished, in order. */
  poll(): void {
    const gl = this.#gl;
    for (let next = this.#pending[0]; next; next = this.#pending[0]) {
      const { frame, spans } = next;
      const last = spans[spans.length - 1]?.query;
      if (last && !gl.getQueryParameter(last, gl.QUERY_RESULT_AVAILABLE)) { return; }
      this.#pending.shift();
      // A disjoint operation (a clock change, a context loss) voids every result still in flight.
      const disjoint = Boolean(gl.getParameter(this.#disjoint));
      const gpu: Record<string, number> = {};
      const issue: Record<string, number> = {};
      for (const { label, query, cpu } of spans) {
        if (!disjoint) { gpu[label] = (gpu[label] ?? 0) + (gl.getQueryParameter(query, gl.QUERY_RESULT) as number) / 1e6; }
        issue[label] = (issue[label] ?? 0) + cpu;
        this.#free.push(query);
      }
      frame.gpu = disjoint ? "disjoint" : gpu;
      frame.gpuIssue = issue;
    }
  }

  #begin(label: string | null): { label: string | null; query: WebGLQuery; start: number } {
    const query = this.#free.pop() ?? this.#gl.createQuery();
    this.#gl.beginQuery(this.#timeElapsed, query);
    return { label, query, start: performance.now() };
  }

  #end(): void {
    const open = this.#open;
    if (!open) { return; }
    this.#gl.endQuery(this.#timeElapsed);
    // Work after the last labelled draw (state restored after the output pass) is `other`.
    this.#spans.push({ label: open.label ?? "other", query: open.query, cpu: performance.now() - open.start });
    this.#open = null;
  }
}

const CAPACITY = 36_000;
const ZONE_ADVANCE_MEASURE = "runtime-profiler:js-to-wasm:zone-advance";

class FrameProfile {
  readonly #frames: ProfiledFrame[] = [];
  #current: ProfiledFrame | null = null;
  #last = 0;
  /** The GPU pass timer, once a lit frame looked for it; `null` without the timer-query extension. */
  #gpu: GpuPassTimer | null | undefined;

  begin(raf: number): void {
    this.#gpu?.poll();
    const start = performance.now();
    this.#last = start;
    this.#current = {
      raf, start, end: start, passes: [], mainDraws: [], shadowDraws: [], ticks: 0, viewer: [0, 0], heading: 0, nodes: 0, instanceBatches: 0,
      created: { objects: 0, geometries: 0, materials: 0, instanceUploads: 0 },
      phases: { input: 0, simulation: 0, projection: 0, units: 0, scene: 0, render: 0, hud: 0 },
      gpu: null, gpuIssue: null, gpuSpans: 0,
    };
  }

  /** The current frame's GPU pass timer for `renderer`'s context, or null when the frame is not profiled or untimed. */
  gpuTimer(renderer: THREE.WebGLRenderer): GpuPassTimer | null {
    if (!this.#current) { return null; }
    if (this.#gpu === undefined) {
      const gl = renderer.getContext();
      const ext = gl instanceof WebGL2RenderingContext ? gl.getExtension("EXT_disjoint_timer_query_webgl2") : null;
      this.#gpu = ext ? new GpuPassTimer(gl as WebGL2RenderingContext, ext) : null;
    }
    return this.#gpu;
  }

  /** Whether GPU pass times are measured: null until a lit frame looked for the timer-query extension. */
  gpuSupported(): boolean | null {
    return this.#gpu === undefined ? null : this.#gpu !== null;
  }

  /** The current frame, for the GPU pass timer to fill in once its results arrive. */
  current(): ProfiledFrame | null {
    return this.#current;
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

  /** The recorded frames, up to the first whose GPU times are still in flight (they follow in a later take). */
  take(): ProfiledFrame[] {
    this.#gpu?.poll();
    const pending = this.#frames.findIndex((frame) => frame.gpu === "pending");
    return this.#frames.splice(0, pending < 0 ? this.#frames.length : pending);
  }
}

let active: FrameProfile | null = null;

/** Turns profiling on for this page and returns the reader the harness uses. */
export function enableFrameProfile(): { take(): ProfiledFrame[]; gpuSupported(): boolean | null } {
  active ??= new FrameProfile();
  const profile = active;
  return { take: () => profile.take(), gpuSupported: () => profile.gpuSupported() };
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
 * What a main-pass draw is, for its GPU time (#1106), from the scene child it hangs under: grass, terrain, the batched
 * static scene, water (rivers and lakes), units, vegetation (nature kits, ground detail), other kits, or other.
 */
function drawKind(object: THREE.Object3D): string {
  let root = object;
  while (root.parent && !(root.parent as THREE.Scene).isScene) { root = root.parent; }
  const name = root.name;
  switch (name) {
    case "greyhaven.grass": return "grass";
    case "greyhaven.terrain": return "terrain";
    case "static-scene": return "static-scene";
    case "greyhaven.nature": return "vegetation";
  }
  if (/^river-|water|lake/i.test(name)) { return "water"; }
  if (name.startsWith("unit-")) { return "units"; }
  if (/^scenery\..*\.detail$/.test(name)) { return "vegetation"; }
  if (name.endsWith(".kit") || name.startsWith("greyhaven.")) { return "kits"; }
  return "other";
}

/**
 * The pass of a render of anything but the scene (a fullscreen quad): the sky's LUTs before the main pass, its
 * composite between the opaque and blended main draws, then tone mapping (reads the HDR frame) and the output pass.
 */
function quadPass(drawn: THREE.Object3D, mainDone: boolean, mainStarted: boolean): string {
  if (!mainStarted) { return "sky"; }
  if (!mainDone) { return "sky-composite"; }
  const uniforms = ((drawn as THREE.Mesh).material as THREE.ShaderMaterial | undefined)?.uniforms ?? {};
  return "hdrTexture" in uniforms ? "tone-mapping" : "postTexture" in uniforms ? "output" : "post";
}

/**
 * Runs `draw` (the lighting's passes) counting each `renderer.render` of `scene` as a pass: an orthographic camera is a
 * sun cascade, `camera` the main pass (opaque, then transparent when the sky splits it), any other camera a local
 * shadow view; renders of other scenes are the sky's LUTs, its composite, tone mapping and output (`quadPass`). The
 * main and shadow passes' draws are grouped by contributor. With `EXT_disjoint_timer_query_webgl2` each pass's GPU
 * time is measured too (`GpuPassTimer`), the main passes' by what draws (`drawKind`). Only three's counters and timer
 * queries are read: what is drawn does not change.
 */
export function profileRenderPasses(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, split: boolean, draw: () => void): void {
  const profile = active;
  if (!profile) { draw(); return; }
  const info = renderer.info;
  const timer = profile.gpuTimer(renderer);
  const frame = profile.current();
  const passes: PassDraws[] = [];
  const contributors = { main: new Map<string, DrawContributor>(), shadow: new Map<string, DrawContributor>() };
  let cascade = 0;
  let mains = 0;
  const mainsInFrame = split ? 2 : 1;
  let grouped: Map<string, DrawContributor> | null = null;
  // The main pass a draw belongs to, for its GPU label, and the pass drawing (for its MSAA resolve).
  let mainPass: string | null = null;
  let drawing = "";
  const render = renderer.render;
  const direct = renderer.renderBufferDirect;
  const setRenderTarget = renderer.setRenderTarget;
  const gl = renderer.getContext() as WebGL2RenderingContext;
  const blit = gl.blitFramebuffer;
  renderer.render = function profiledRender(this: THREE.WebGLRenderer, drawn: THREE.Object3D, view: THREE.Camera): void {
    const isMain = drawn === scene && view === camera;
    const pass = drawn !== scene ? quadPass(drawn, mains >= mainsInFrame, mains > 0)
      : (view as THREE.OrthographicCamera).isOrthographicCamera ? `sun-cascade-${cascade++}`
        : isMain ? (split ? (mains === 0 ? "main-opaque" : "main-transparent") : "main")
          : "local-shadows";
    if (isMain) { mains++; }
    const calls = info.autoReset ? 0 : info.render.calls;
    const triangles = info.autoReset ? 0 : info.render.triangles;
    grouped = drawn !== scene ? null : isMain ? contributors.main : contributors.shadow;
    mainPass = isMain ? pass : null;
    drawing = pass;
    // A main pass's clear has its own label; its draws label themselves (`renderBufferDirect`).
    timer?.enter(isMain ? `${pass}:clear` : pass);
    try {
      render.call(this, drawn, view);
    } finally {
      grouped = null;
      mainPass = null;
    }
    const counted = { pass, calls: info.render.calls - calls, triangles: info.render.triangles - triangles };
    const last = passes[passes.length - 1];
    if (last?.pass === pass) { last.calls += counted.calls; last.triangles += counted.triangles; } else { passes.push(counted); }
  };
  renderer.renderBufferDirect = function profiledDraw(this: THREE.WebGLRenderer, view, drawnScene, geometry, material, object, group): void {
    if (mainPass !== null) { timer?.enter(`${mainPass}:${drawKind(object)}`); }
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
  if (timer) {
    // Every pass binds its target first: the GPU span ends there, so the next pass's clear counts towards it.
    renderer.setRenderTarget = function profiledTarget(this: THREE.WebGLRenderer, ...target: Parameters<THREE.WebGLRenderer["setRenderTarget"]>): void {
      timer.boundary();
      setRenderTarget.apply(this, target);
    };
    // three resolves a multisampled target (the HDR frame) with a blit at the end of the render that drew into it.
    gl.blitFramebuffer = function profiledBlit(this: WebGL2RenderingContext, ...args: Parameters<WebGL2RenderingContext["blitFramebuffer"]>): void {
      timer.enter(`${drawing}:resolve`);
      blit.apply(this, args);
    };
    timer.start();
  }
  try {
    draw();
  } finally {
    renderer.render = render;
    renderer.renderBufferDirect = direct;
    renderer.setRenderTarget = setRenderTarget;
    if (timer) { delete (gl as Partial<WebGL2RenderingContext>).blitFramebuffer; }
    if (timer && frame) { timer.finish(frame); }
  }
  profile.draws(passes, [...contributors.main.values()], [...contributors.shadow.values()]);
}
