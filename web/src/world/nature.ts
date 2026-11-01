import * as THREE from "three";
import { loadBundleGltfs } from "./static-bundle";

/** The pinned `greyhaven-nature` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const NATURE_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.nature/`;

/** The shared level-of-detail selection (`mmorpg_presentation::nature::Lods`, through `mmorpg-wasm`). */
export type NatureLodsHandle = {
  /** An entry's variants, finest first, as `entry/variant`. */
  variants(entry: string): string[];
  /**
   * Per drawn level its placement, level and fade (1 settled; `t` incoming, `-t` outgoing), three floats each, or
   * `undefined` when unchanged since the previous update (the camera rests and nothing fades).
   */
  update(x: number, y: number, z: number, fovY: number, clipFromWorld: Float32Array, seconds: number, bias: number, fade: boolean): Float32Array | undefined;
};

export type NatureSource = {
  entries: readonly string[];
  /** Every placed tree, rock and bush: its entry and 16 column-major model floats (metres). */
  placements: readonly { key: string; matrix: Float32Array }[];
  /** The shared selection for these placements from the bundle's `nature-descriptor` JSON. */
  lods(descriptor: string): NatureLodsHandle;
};

/** Uploads only the first `count` items of `attribute` (the drawn instances), not its whole capacity. */
function upload(attribute: THREE.BufferAttribute, count: number): void {
  attribute.clearUpdateRanges();
  attribute.addUpdateRange(0, count * attribute.itemSize);
  attribute.needsUpdate = true;
}

/** Draws the first `count` instances of `mesh` (hidden at none); whether it draws any. */
function place(mesh: THREE.InstancedMesh, count: number): boolean {
  mesh.count = count;
  mesh.visible = count > 0;
  if (count > 0) { upload(mesh.instanceMatrix, count); }
  return count > 0;
}

/**
 * Screen-door cross-fade: an instance with fade `t` in (0, 1) keeps the fraction `t` of its pixels, one with `-t` the
 * complementary pixels, so two levels of one tree blend without sorting. A per-instance attribute carries the fade.
 */
function crossFaded(material: THREE.Material): THREE.Material {
  const faded = material.clone();
  faded.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace("#include <common>", "#include <common>\nattribute float instanceFade;\nvarying float vInstanceFade;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\nvInstanceFade = instanceFade;");
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", "#include <common>\nvarying float vInstanceFade;")
      .replace("#include <clipping_planes_fragment>", `#include <clipping_planes_fragment>
if (vInstanceFade < 0.999) {
  float screenDoor = fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))));
  if (vInstanceFade >= 0.0 ? screenDoor >= vInstanceFade : screenDoor < -vInstanceFade) discard;
}`);
  };
  faded.customProgramCacheKey = () => "nature-cross-fade";
  return faded;
}

/**
 * One drawn primitive of one entry level: its settled instances through the glTF material, and its cross-fading
 * instances through the dithered copy (#593). The copy's `discard` would cost every settled pixel its early depth
 * test, so only the few fading instances pay for it. Both meshes share the geometry's buffers.
 */
type Part = { settled: THREE.InstancedMesh; fading: THREE.InstancedMesh; fade: THREE.InstancedBufferAttribute };

/**
 * Greyhaven's trees, rocks and bushes from asset-tooling's `greyhaven-nature` bundle (#49), placed where the shared Rust
 * presentation places them, as the native client does. Each entry level is one instanced mesh per primitive whose
 * geometry and materials every placement shares; the shared selection picks each placement's level per frame and
 * cross-fades switches. Until the bundle loads (or when it fails) none is drawn and the static scene keeps the
 * procedural trees, rocks and bushes. Presentation only.
 */
export class Nature {
  readonly #source: NatureSource;
  readonly #root = new THREE.Group();
  /** Per placement its entry index; per entry, per level its parts. */
  #entryOf: number[] = [];
  #levels: Part[][][] = [];
  #lods: NatureLodsHandle | null = null;
  #loading: Promise<boolean> | null = null;
  #changed = false;
  /** Per entry, per level how many placements it draws (settled and fading). */
  #counts: number[][] = [];
  readonly #clip = new THREE.Matrix4();
  readonly #clipFloats = new Float32Array(16);

  constructor(source: NatureSource) {
    this.#source = source;
    this.#root.name = "greyhaven.nature";
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => true, (error: unknown) => {
      console.error(`The Greyhaven nature bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    const placements = this.#source.placements;
    if (placements.length === 0) { return; }
    const base = new URL(NATURE_BUNDLE, window.location.href);
    const response = await fetch(new URL("manifest.json", base));
    if (!response.ok) { throw new Error(`manifest ${response.status}`); }
    const manifest = await response.json() as { assets?: { attachments?: { name: string; path: string }[] }[] };
    const attachment = manifest.assets?.flatMap((asset) => asset.attachments ?? []).find((entry) => entry.name === "nature-descriptor");
    if (!attachment) { throw new Error("the bundle has no nature-descriptor"); }
    const descriptor = await fetch(new URL(attachment.path, base));
    if (!descriptor.ok) { throw new Error(`nature-descriptor ${descriptor.status}`); }
    const lods = this.#source.lods(await descriptor.text());
    const entries = this.#source.entries;
    const variants = entries.map((entry) => lods.variants(entry));
    const gltfs = await loadBundleGltfs(NATURE_BUNDLE, variants.flat());
    const capacity = entries.map((entry) => placements.filter((placement) => placement.key === entry).length);
    let next = 0;
    // Materials are shared across levels and entries where glTF shares them.
    const materials = new Map<THREE.Material, THREE.Material>();
    this.#levels = variants.map((levels, entry) => levels.map(() => {
      const scene = gltfs[next++]!.scene;
      scene.updateMatrixWorld(true);
      const parts: Part[] = [];
      scene.traverse((object) => {
        if (!(object instanceof THREE.Mesh)) { return; }
        // The entry frame is the bundle's: bake the node transforms into the shared geometry.
        const geometry = (object.geometry as THREE.BufferGeometry).clone().applyMatrix4(object.matrixWorld);
        const source = object.material as THREE.Material;
        let material = materials.get(source);
        if (!material) {
          material = crossFaded(source);
          materials.set(source, material);
        }
        const size = Math.max(capacity[entry]!, 1);
        // The fading copy shares the geometry's buffers and adds the per-instance fade.
        const fadingGeometry = new THREE.BufferGeometry();
        fadingGeometry.setIndex(geometry.index);
        for (const [name, attribute] of Object.entries(geometry.attributes)) { fadingGeometry.setAttribute(name, attribute); }
        const fade = new THREE.InstancedBufferAttribute(new Float32Array(size).fill(1), 1);
        fade.setUsage(THREE.DynamicDrawUsage);
        fadingGeometry.setAttribute("instanceFade", fade);
        const instanced = (meshGeometry: THREE.BufferGeometry, meshMaterial: THREE.Material, suffix: string) => {
          const mesh = new THREE.InstancedMesh(meshGeometry, meshMaterial, size);
          mesh.name = `${entries[entry]}#${parts.length}${suffix}`;
          // Placements are culled by the shared selection, not by one stale bounding sphere.
          mesh.frustumCulled = false;
          mesh.count = 0;
          mesh.visible = false;
          mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
          this.#root.add(mesh);
          return mesh;
        };
        parts.push({ settled: instanced(geometry, source, ""), fading: instanced(fadingGeometry, material, ".fading"), fade });
      });
      return parts;
    }));
    this.#counts = this.#levels.map((levels) => levels.map(() => 0));
    this.#entryOf = placements.map((placement) => entries.indexOf(placement.key));
    if (this.#entryOf.some((entry) => entry < 0)) { throw new Error("a placement names no nature entry"); }
    this.#lods = lods;
    this.#changed = true;
  }

  /** Whether the trees, rocks and bushes are drawn (loaded); the static scene hides their fallback then. */
  get drawn(): boolean {
    return this.#lods !== null;
  }

  /** Picks each placement's level for `camera` and places the instances; `fade` false switches at once. */
  update(camera: THREE.PerspectiveCamera, seconds: number, fade: boolean, bias = 1): void {
    const lods = this.#lods;
    if (!lods) { return; }
    this.#clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.#clipFloats.set(this.#clip.elements);
    const p = camera.position;
    // Unchanged (camera at rest, nothing fading): the instances stay uploaded as they are.
    const shown = lods.update(p.x, p.y, p.z, THREE.MathUtils.degToRad(camera.fov), this.#clipFloats, seconds, bias, fade);
    if (!shown) { return; }
    const settled = this.#levels.map((levels) => levels.map(() => 0));
    const fading = this.#levels.map((levels) => levels.map(() => 0));
    const placements = this.#source.placements;
    for (let index = 0; index < shown.length; index += 3) {
      const placement: number = shown[index]!;
      const level: number = shown[index + 1]!;
      const t: number = shown[index + 2]!;
      const entry = this.#entryOf[placement]!;
      const parts = this.#levels[entry]?.[level];
      if (!parts) { continue; }
      const matrix = placements[placement]!.matrix;
      if (t >= 1) {
        const slot = settled[entry]![level]!++;
        for (const part of parts) { (part.settled.instanceMatrix.array as Float32Array).set(matrix, slot * 16); }
      } else {
        const slot = fading[entry]![level]!++;
        for (const part of parts) {
          (part.fading.instanceMatrix.array as Float32Array).set(matrix, slot * 16);
          (part.fade.array as Float32Array)[slot] = t;
        }
      }
    }
    this.#levels.forEach((levels, entry) => levels.forEach((parts, level) => {
      const settledCount = settled[entry]![level]!;
      const fadingCount = fading[entry]![level]!;
      this.#counts[entry]![level] = settledCount + fadingCount;
      for (const part of parts) {
        place(part.settled, settledCount);
        if (place(part.fading, fadingCount)) { upload(part.fade, fadingCount); }
      }
    }));
  }

  /** The drawn root, and whether it changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.drawn ? [this.#root] : [], changed };
  }

  /** Acceptance state: whether the bundle is drawn, how many placements and how many instances each entry level draws. */
  observe(): { loaded: boolean; placements: number; drawn: Record<string, number[]> } {
    const drawn: Record<string, number[]> = {};
    this.#source.entries.forEach((entry, index) => { drawn[entry] = [...(this.#counts[index] ?? [])]; });
    return { loaded: this.drawn, placements: this.#source.placements.length, drawn };
  }
}
