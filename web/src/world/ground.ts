import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { GroundDraw } from "./scenery-nodes";
import { loadBundleGltfs, readBundleAttachment } from "./static-bundle";

/** The `greyhaven-terrain` bundle `scripts/fetch-bundles.sh` places in `web/public` (#525). */
export const TERRAIN_BUNDLE = `${import.meta.env.BASE_URL}scenery/greyhaven.terrain/`;
/** `mmorpg_presentation::ground::MASK_CUTOFF`: an overlay texel shows where weight × height alpha reaches it. */
const MASK_CUTOFF = 0.5;
/**
 * `mmorpg_presentation::ground::OVERLAY_ALPHA`: an overlay material's alpha, below the cut-off, so its shadow (cut by
 * the material's alpha alone) is cut away; its vertex alpha is its weight over this.
 */
const OVERLAY_ALPHA = 0.25;
/** The presentation materials' flat surface (`standard_material`), for layers without textures. */
const ROUGHNESS = 0.86;
const METALLIC = 0.02;

type Descriptor = { layers?: { layer?: string; meanColor?: [number, number, number] }[] };

function linear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
}

function flatMaterial(overlay: boolean): LitMaterial {
  return new LitMaterial({
    perceptualRoughness: ROUGHNESS, metallic: METALLIC, alpha: overlay ? OVERLAY_ALPHA : 1, alphaMode: overlay ? "mask" : "opaque", alphaCutoff: MASK_CUTOFF,
  });
}

/**
 * The terrain's ground layers (#525): the opaque grass base and its masked overlays (forest floor, scree, mud, sand,
 * paths, rock), one mesh per layer built from the shared Rust presentation, as the native client draws them. Each
 * layer's material is its glTF material from asset-tooling's `greyhaven-terrain` bundle, its base colour divided by
 * the texture's mean colour so the vertex colours keep the palette; an overlay's weight is its vertex colour alpha
 * and its mask cuts by weight × the texture's height alpha. Until the bundle loads (or for a layer it lacks) the
 * meshes draw flat in their vertex colours: the explicit fallback. Presentation only.
 */
export class Ground {
  readonly root = new THREE.Group();
  readonly #meshes: { draw: GroundDraw; mesh: THREE.Mesh }[];
  #loading: Promise<boolean> | null = null;
  #changed = true;
  #textured = new Set<string>();

  constructor(draws: readonly GroundDraw[]) {
    this.root.name = "greyhaven.terrain";
    this.root.matrixAutoUpdate = false;
    this.#meshes = draws.map((draw) => {
      const geometry = new THREE.BufferGeometry();
      const count = draw.positions.length / 3;
      const colors = new Float32Array(count * 4);
      for (let index = 0; index < count; index += 1) {
        colors[index * 4] = linear(draw.colors[index * 3]!);
        colors[index * 4 + 1] = linear(draw.colors[index * 3 + 1]!);
        colors[index * 4 + 2] = linear(draw.colors[index * 3 + 2]!);
        colors[index * 4 + 3] = draw.alpha ? draw.alpha[index]! : 1;
      }
      geometry.setAttribute("position", new THREE.BufferAttribute(draw.positions, 3));
      geometry.setAttribute("normal", new THREE.BufferAttribute(draw.normals, 3));
      geometry.setAttribute("uv", new THREE.BufferAttribute(draw.uvs, 2));
      geometry.setAttribute("tangent", new THREE.BufferAttribute(draw.tangents, 4));
      geometry.setAttribute("color", new THREE.BufferAttribute(colors, 4));
      geometry.setIndex(new THREE.BufferAttribute(draw.indices, 1));
      geometry.computeBoundingSphere();
      const mesh = new THREE.Mesh(geometry, flatMaterial(draw.overlay));
      mesh.name = draw.id;
      mesh.matrixAutoUpdate = false;
      this.root.add(mesh);
      return { draw, mesh };
    });
  }

  /** Loads the layers' materials once; settles true when every drawn layer is textured, never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().catch((error: unknown) => {
      console.error(`The Greyhaven terrain bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<boolean> {
    const layers = [...new Set(this.#meshes.map(({ draw }) => draw.layer))];
    if (layers.length === 0) { return true; }
    const keys = layers.map((layer) => `greyhaven.terrain.${layer}`);
    const descriptor = JSON.parse(await readBundleAttachment(TERRAIN_BUNDLE, "greyhaven.terrain.grass", "default", "terrain-descriptor")) as Descriptor;
    const loaded = await Promise.allSettled(keys.map((key) => loadBundleGltfs(TERRAIN_BUNDLE, [key])));
    loaded.forEach((result, index) => {
      const layer = layers[index]!;
      if (result.status === "rejected") {
        console.error(`The Greyhaven terrain bundle has no ${layer} layer: ${String(result.reason)}`);
        return;
      }
      let source: THREE.MeshStandardMaterial | null = null;
      result.value[0]!.scene.traverse((object) => {
        if (!source && object instanceof THREE.Mesh && object.material instanceof THREE.MeshStandardMaterial) { source = object.material; }
      });
      const mean = descriptor.layers?.find((entry) => entry.layer === layer)?.meanColor;
      if (!source || !mean) { return; }
      const material = source as THREE.MeshStandardMaterial;
      for (const { draw, mesh } of this.#meshes) {
        if (draw.layer !== layer) { continue; }
        (mesh.material as THREE.Material).dispose();
        mesh.material = new LitMaterial({
          baseColor: mean.map((channel) => 1 / Math.max(channel, 1e-3)) as [number, number, number],
          alpha: draw.overlay ? OVERLAY_ALPHA : 1,
          baseColorTexture: material.map,
          normalMapTexture: material.normalMap,
          metallicRoughnessTexture: material.roughnessMap ?? material.metalnessMap,
          occlusionTexture: material.aoMap,
          metallic: 0,
          perceptualRoughness: 1,
          alphaMode: draw.overlay ? "mask" : "opaque",
          alphaCutoff: MASK_CUTOFF,
        });
      }
      this.#textured.add(layer);
    });
    this.#changed = true;
    return layers.every((layer) => this.#textured.has(layer));
  }

  /** The drawn root, and whether it changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#meshes.length > 0 ? [this.root] : [], changed };
  }

  /** Acceptance state: the ground layers and which of them are textured. */
  observe(): { layers: string[]; textured: string[] } {
    return { layers: this.#meshes.map(({ draw }) => `${draw.layer}${draw.overlay ? "-overlay" : ""}`), textured: [...this.#textured] };
  }
}
