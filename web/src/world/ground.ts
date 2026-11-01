import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import type { TerrainStreamSource } from "./scenery-nodes";
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
/** The presentation materials' flat surface (`standard_material`), for far tiles and layers without textures. */
const ROUGHNESS = 0.86;
const METALLIC = 0.02;
/** `mmorpg_presentation::ground::Layer`, in order. */
const LAYERS = ["grass", "forest", "gravel", "mud", "sand", "path", "rock"] as const;

type Descriptor = { layers?: { layer?: string; meanColor?: [number, number, number] }[] };

/** One unpacked tile draw (`static_scene::pack` in `mmorpg-wasm`); colours sRGB 0–1. */
export type TileDraw = {
  layer: string; overlay: boolean; far: boolean;
  positions: Float32Array; normals: Float32Array; colors: Float32Array; uvs: Float32Array; tangents: Float32Array;
  alpha: Float32Array; indices: Uint32Array;
};

/** Unpacks one tile level's draws; malformed data fails closed. */
export function unpackTile(data: Float32Array): TileDraw[] {
  const draws: TileDraw[] = [];
  let at = 1;
  const take = (count: number) => {
    if (at + count > data.length) { throw new Error("Invalid terrain tile: truncated"); }
    const out = data.subarray(at, at + count);
    at += count;
    return out;
  };
  for (let index = 0; index < data[0]!; index += 1) {
    const [layer, overlay, far, vertices, indices] = take(5);
    const name = LAYERS[layer!];
    if (name === undefined) { throw new Error(`Invalid terrain tile: layer ${layer}`); }
    const n = vertices!;
    draws.push({
      layer: name, overlay: overlay === 1, far: far === 1,
      positions: take(n * 3), normals: take(n * 3), colors: take(n * 3), uvs: take(n * 2), tangents: take(n * 4), alpha: take(n),
      indices: Uint32Array.from(take(indices!)),
    });
  }
  return draws;
}

function linear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
}

function flatMaterial(overlay: boolean): LitMaterial {
  return new LitMaterial({
    perceptualRoughness: ROUGHNESS, metallic: METALLIC, alpha: overlay ? OVERLAY_ALPHA : 1, alphaMode: overlay ? "mask" : "opaque", alphaCutoff: MASK_CUTOFF,
  });
}

/** A tile draw's geometry: textured attributes on the near level only, so each batch shares one vertex layout. */
function geometry(draw: TileDraw): THREE.BufferGeometry {
  const geometry = new THREE.BufferGeometry();
  const count = draw.positions.length / 3;
  const colors = new Float32Array(count * 4);
  for (let index = 0; index < count; index += 1) {
    colors[index * 4] = linear(draw.colors[index * 3]!);
    colors[index * 4 + 1] = linear(draw.colors[index * 3 + 1]!);
    colors[index * 4 + 2] = linear(draw.colors[index * 3 + 2]!);
    colors[index * 4 + 3] = draw.alpha[index]!;
  }
  geometry.setAttribute("position", new THREE.BufferAttribute(draw.positions, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(draw.normals, 3));
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 4));
  if (!draw.far) {
    geometry.setAttribute("uv", new THREE.BufferAttribute(draw.uvs, 2));
    geometry.setAttribute("tangent", new THREE.BufferAttribute(draw.tangents, 4));
  }
  geometry.setIndex(new THREE.BufferAttribute(draw.indices, 1));
  return geometry;
}

type Placed = { batch: Batch; geometry: number; instance: number };

/**
 * One `BatchedMesh` per material (#526, as #615 batches the static scenery): every tile drawn with that material is one
 * instance, culled per view by its bounding sphere, so the whole terrain costs one multi-draw call per material and pass.
 * Room grows as near levels stream in; evicted ones free theirs for reuse.
 */
class Batch {
  readonly mesh: THREE.BatchedMesh;
  #vertices: number;
  #indices: number;

  constructor(name: string, material: THREE.Material, vertices: number, indices: number, instances: number) {
    this.#vertices = vertices;
    this.#indices = indices;
    this.mesh = new THREE.BatchedMesh(instances, vertices, indices, material);
    this.mesh.name = name;
    this.mesh.matrixAutoUpdate = false;
    // The instances cull themselves per view.
    this.mesh.frustumCulled = false;
  }

  add(geometry: THREE.BufferGeometry, visible: boolean): Placed {
    const vertices = geometry.attributes.position!.count;
    const indices = geometry.index!.count;
    const mesh = this.mesh;
    if (mesh.unusedVertexCount < vertices || mesh.unusedIndexCount < indices) {
      mesh.optimize();
    }
    if (mesh.unusedVertexCount < vertices || mesh.unusedIndexCount < indices) {
      this.#vertices = Math.max(this.#vertices * 2, this.#vertices + vertices);
      this.#indices = Math.max(this.#indices * 2, this.#indices + indices);
      mesh.setGeometrySize(this.#vertices, this.#indices);
    }
    if (mesh.instanceCount >= mesh.maxInstanceCount) {
      mesh.setInstanceCount(mesh.maxInstanceCount * 2);
    }
    const id = mesh.addGeometry(geometry);
    const instance = mesh.addInstance(id);
    mesh.setVisibleAt(instance, visible);
    geometry.dispose();
    return { batch: this, geometry: id, instance };
  }

  remove(placed: Placed): void {
    this.mesh.deleteInstance(placed.instance);
    this.mesh.deleteGeometry(placed.geometry);
  }
}

/**
 * The terrain in visual tiles (#526) with the textured ground layers of #525. Every tile's far level (its relief at
 * eight metres, the ground layers baked into vertex colours, flat) is built once; near levels (the opaque grass base
 * and its masked overlays: forest floor, scree, mud, sand, paths, rock) stream around the camera as the shared Rust
 * `TerrainStream` says, a few per frame, and a tile shows its near level only once that is resident. Skirts under every
 * tile edge hide the cracks between levels, as natively. Each near layer's material is its glTF material from
 * asset-tooling's `greyhaven-terrain` bundle, its base colour divided by the texture's mean colour so the vertex
 * colours keep the palette; an overlay's weight is its vertex colour alpha and its mask cuts by weight × the texture's
 * height alpha. Until the bundle loads (or for a layer it lacks) near layers draw flat in their vertex colours: the
 * explicit fallback. Presentation only.
 */
export class Ground {
  readonly root = new THREE.Group();
  readonly #stream: TerrainStreamSource;
  readonly #batches = new Map<string, Batch>();
  readonly #materials = new Map<string, THREE.Material>();
  readonly #far: Placed[][];
  readonly #near = new Map<number, Placed[]>();
  /** Per tile, whether it last drew its near level. */
  readonly #levels: Uint8Array;
  #loading: Promise<boolean> | null = null;
  #changed = true;
  #textured = new Set<string>();

  constructor(stream: TerrainStreamSource) {
    this.#stream = stream;
    this.root.name = "greyhaven.terrain";
    this.root.matrixAutoUpdate = false;
    const count = stream.count();
    this.#levels = new Uint8Array(count);
    this.#far = Array.from({ length: count }, (_, index) => this.#place(unpackTile(stream.far(index)), true));
  }

  #batch(draw: TileDraw): Batch {
    const key = draw.far ? `far-${draw.overlay ? "overlay" : "base"}` : `${draw.layer}${draw.overlay ? "-overlay" : ""}`;
    let batch = this.#batches.get(key);
    if (!batch) {
      let material = this.#materials.get(key);
      if (!material) {
        material = flatMaterial(draw.overlay);
        this.#materials.set(key, material);
      }
      // Far levels are placed once (about 100 tiles of 430 vertices); near levels stream (about 15 resident).
      batch = draw.far ? new Batch(`terrain:${key}`, material, 65_536, 327_680, 512) : new Batch(`terrain:${key}`, material, 65_536, 196_608, 64);
      this.#batches.set(key, batch);
      this.root.add(batch.mesh);
      this.#changed = true;
    }
    return batch;
  }

  #place(draws: readonly TileDraw[], visible: boolean): Placed[] {
    return draws.map((draw) => this.#batch(draw).add(geometry(draw), visible));
  }

  /** Streams near levels for the camera `eye` (metres) and shows each tile at its level. */
  update(eye: readonly [number, number, number]): void {
    const update = this.#stream.update(eye[0], eye[1], eye[2]);
    let at = 0;
    const built = update[at++]!;
    for (let index = 0; index < built; index += 1) {
      const tile = update[at++]!;
      this.#near.set(tile, this.#place(unpackTile(this.#stream.near(tile)), false));
      // Shown below once its level says near.
      this.#levels[tile] = 0;
    }
    const evicted = update[at++]!;
    for (let index = 0; index < evicted; index += 1) {
      const tile = update[at++]!;
      for (const placed of this.#near.get(tile) ?? []) { placed.batch.remove(placed); }
      this.#near.delete(tile);
      if (this.#levels[tile] === 1) {
        for (const placed of this.#far[tile]!) { placed.batch.mesh.setVisibleAt(placed.instance, true); }
        this.#levels[tile] = 0;
      }
    }
    for (let tile = 0; tile < this.#levels.length; tile += 1) {
      const near = update[at + tile]! === 1 && this.#near.has(tile) ? 1 : 0;
      if (near === this.#levels[tile]) { continue; }
      for (const placed of this.#far[tile]!) { placed.batch.mesh.setVisibleAt(placed.instance, near === 0); }
      for (const placed of this.#near.get(tile) ?? []) { placed.batch.mesh.setVisibleAt(placed.instance, near === 1); }
      this.#levels[tile] = near;
    }
  }

  /**
   * Loads the near layers' materials once, their KTX2 textures transcoded by `ktx2` (asset-tooling#475); settles true
   * when every layer is textured, never fatal to the world.
   */
  load(ktx2: KTX2Loader): Promise<boolean> {
    this.#loading ??= this.#load(ktx2).catch((error: unknown) => {
      console.error(`The Greyhaven terrain bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(ktx2: KTX2Loader): Promise<boolean> {
    const descriptor = JSON.parse(await readBundleAttachment(TERRAIN_BUNDLE, "greyhaven.terrain.grass", "default", "terrain-descriptor")) as Descriptor;
    const loaded = await Promise.allSettled(LAYERS.map((layer) => loadBundleGltfs(TERRAIN_BUNDLE, [`greyhaven.terrain.${layer}`], Infinity, ktx2)));
    loaded.forEach((result, index) => {
      const layer = LAYERS[index]!;
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
      for (const overlay of [false, true]) {
        const key = `${layer}${overlay ? "-overlay" : ""}`;
        const textured = new LitMaterial({
          baseColor: mean.map((channel) => 1 / Math.max(channel, 1e-3)) as [number, number, number],
          alpha: overlay ? OVERLAY_ALPHA : 1,
          baseColorTexture: material.map,
          normalMapTexture: material.normalMap,
          metallicRoughnessTexture: material.roughnessMap ?? material.metalnessMap,
          occlusionTexture: material.aoMap,
          metallic: 0,
          perceptualRoughness: 1,
          alphaMode: overlay ? "mask" : "opaque",
          alphaCutoff: MASK_CUTOFF,
        });
        this.#materials.get(key)?.dispose();
        this.#materials.set(key, textured);
        const batch = this.#batches.get(key);
        if (batch) { batch.mesh.material = textured; }
      }
      this.#textured.add(layer);
    });
    this.#changed = true;
    return LAYERS.every((layer) => this.#textured.has(layer));
  }

  /** The drawn root, and whether it changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: [this.root], changed };
  }

  /** Acceptance state: the ground batches, which layers are textured, and the tiles' levels. */
  observe(): { layers: string[]; textured: string[]; tiles: number; near: number; resident: number } {
    return {
      layers: [...this.#batches.keys()], textured: [...this.#textured], tiles: this.#levels.length,
      near: this.#levels.reduce((sum, level) => sum + level, 0), resident: this.#near.size,
    };
  }
}
