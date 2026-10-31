import * as THREE from "three";
import { loadBundleGltfs } from "./static-bundle";

/** The pinned `greyhaven-nature` bundle `scripts/fetch-bundles.sh` places in `web/public`. */
export const NATURE_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.nature/`;

/** The shared level-of-detail selection (`mmorpg_presentation::nature::Lods`, through `mmorpg-wasm`). */
export type NatureLodsHandle = {
  /** An entry's variants, finest first, as `entry/variant`. */
  variants(entry: string): string[];
  /** Per drawn level its placement, level and fade (1 settled; `t` incoming, `-t` outgoing), three floats each. */
  update(x: number, y: number, z: number, fovY: number, clipFromWorld: Float32Array, seconds: number, bias: number, fade: boolean): Float32Array;
};

export type NatureSource = {
  entries: readonly string[];
  /** Every placed tree and rock: its entry and 16 column-major model floats (metres). */
  placements: readonly { key: string; matrix: Float32Array }[];
  /** The shared selection for these placements from the bundle's `nature-descriptor` JSON. */
  lods(descriptor: string): NatureLodsHandle;
};

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

/** One drawn primitive of one entry level: its instances this frame. */
type Part = { mesh: THREE.InstancedMesh; fade: THREE.InstancedBufferAttribute };

/**
 * Greyhaven's trees and rocks from asset-tooling's `greyhaven-nature` bundle (#49), placed where the shared Rust
 * presentation places them, as the native client does. Each entry level is one instanced mesh per primitive whose
 * geometry and materials every placement shares; the shared selection picks each placement's level per frame and
 * cross-fades switches. Until the bundle loads (or when it fails) none is drawn and the static scene keeps the
 * procedural trees and rocks. Presentation only.
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
  #last: Float32Array | null = null;
  #counts: number[][] = [];
  readonly #clip = new THREE.Matrix4();

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
        const mesh = new THREE.InstancedMesh(geometry, material, Math.max(capacity[entry]!, 1));
        mesh.name = `${entries[entry]}#${parts.length}`;
        // Placements are culled by the shared selection, not by one stale bounding sphere.
        mesh.frustumCulled = false;
        mesh.count = 0;
        mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
        const fade = new THREE.InstancedBufferAttribute(new Float32Array(Math.max(capacity[entry]!, 1)).fill(1), 1);
        fade.setUsage(THREE.DynamicDrawUsage);
        geometry.setAttribute("instanceFade", fade);
        parts.push({ mesh, fade });
        this.#root.add(mesh);
      });
      return parts;
    }));
    this.#counts = this.#levels.map((levels) => levels.map(() => 0));
    this.#entryOf = placements.map((placement) => entries.indexOf(placement.key));
    if (this.#entryOf.some((entry) => entry < 0)) { throw new Error("a placement names no nature entry"); }
    this.#lods = lods;
    this.#changed = true;
  }

  /** Whether the trees and rocks are drawn (loaded); the static scene hides their fallback then. */
  get drawn(): boolean {
    return this.#lods !== null;
  }

  /** Picks each placement's level for `camera` and places the instances; `fade` false switches at once. */
  update(camera: THREE.PerspectiveCamera, seconds: number, fade: boolean, bias = 1): void {
    const lods = this.#lods;
    if (!lods) { return; }
    this.#clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    const clip = Float32Array.from(this.#clip.elements);
    const p = camera.position;
    const shown = lods.update(p.x, p.y, p.z, THREE.MathUtils.degToRad(camera.fov), clip, seconds, bias, fade);
    const last = this.#last;
    if (last && last.length === shown.length && last.every((value, index) => value === shown[index])) { return; }
    this.#last = shown;
    for (const counts of this.#counts) { counts.fill(0); }
    const placements = this.#source.placements;
    for (let index = 0; index < shown.length; index += 3) {
      const placement = shown[index]!;
      const level = shown[index + 1]!;
      const entry = this.#entryOf[placement]!;
      const parts = this.#levels[entry]?.[level];
      if (!parts) { continue; }
      const slot = this.#counts[entry]![level]!++;
      const matrix = placements[placement]!.matrix;
      for (const part of parts) {
        (part.mesh.instanceMatrix.array as Float32Array).set(matrix, slot * 16);
        (part.fade.array as Float32Array)[slot] = shown[index + 2]!;
      }
    }
    this.#levels.forEach((levels, entry) => levels.forEach((parts, level) => {
      const count = this.#counts[entry]![level]!;
      for (const part of parts) {
        part.mesh.count = count;
        part.mesh.visible = count > 0;
        part.mesh.instanceMatrix.needsUpdate = true;
        part.fade.needsUpdate = true;
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
