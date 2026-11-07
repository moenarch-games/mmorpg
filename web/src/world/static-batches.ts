import * as THREE from "three";
import type { RendererMaterialRequest, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { presentationMaterial } from "./lighting";
import type { SceneryScene, StaticBatch } from "./scenery-nodes";

/** One batched mesh: the `BatchedMesh` that draws it and its instance there, or the plain mesh drawing it alone. */
export type BatchedDraw = { mesh: THREE.BatchedMesh | THREE.Mesh | null; instance: number };

type Pending = { geometry: THREE.BufferGeometry; matrix: THREE.Matrix4; color: THREE.Color | THREE.Vector4 | null; draw: BatchedDraw };

/**
 * Whether a mesh drawn with `material` can join a `BatchedMesh` of lit scenery: one opaque or masked `LitMaterial`
 * (blended draws sort per object, back to front, so they stay plain meshes).
 */
export function batchable(material: THREE.Material | THREE.Material[]): material is THREE.Material {
  return !Array.isArray(material) && (material as { isLitMaterial?: boolean }).isLitMaterial === true
    && (material as { alphaMode?: string }).alphaMode !== "blend";
}

/** The attributes (name, size, normalization, array type) and indexing every geometry of one batch must share. */
function layout(geometry: THREE.BufferGeometry): string {
  const attributes = Object.entries(geometry.attributes)
    .map(([name, attribute]) => `${name}:${attribute.itemSize}:${attribute.normalized}:${attribute.array.constructor.name}`)
    .sort();
  return `${attributes.join(",")}|${geometry.index ? "indexed" : "flat"}`;
}

/**
 * Makes `mesh` draw every visible instance in every view, unsorted. Culling or sorting per view rebuilds the batch's
 * draw list and re-uploads its indirect texture in each sun cascade and the main pass, and each upload into a texture
 * the GPU still reads stalls it: in Greyhaven that cost about 2.5 ms of GPU time a frame, more than the instances it
 * culls. The list (and the upload) then changes only when an instance is shown or hidden.
 */
export function drawWhole(mesh: THREE.BatchedMesh): void {
  mesh.perObjectFrustumCulled = false;
  mesh.sortObjects = false;
}

/**
 * Static lit scenery drawn as one three `BatchedMesh` per material and vertex layout (#615): Greyhaven's frame is bound
 * by draw calls, each paid in the main pass and again in every sun cascade, and a batch is one multi-draw call per
 * pass however many meshes it holds. Each batch draws all its shown instances in every view ([`drawWhole`]); the
 * static scene hides far groups. [`add`] collects meshes, [`build`] creates the batches once; afterwards each draw can
 * be moved ([`setMatrix`]) or shown and hidden ([`setVisible`]).
 */
export class StaticBatches {
  readonly root = new THREE.Group();
  readonly #groups = new Map<string, { material: THREE.Material; colored: boolean; pending: Pending[] }>();

  constructor(name: string) {
    this.root.name = name;
    // Never moved: three would otherwise recompute every batch's world matrix in every pass.
    this.root.matrixAutoUpdate = false;
  }

  /**
   * Adds `geometry` drawn with `material` (see [`batchable`]) at `matrix`, times `color` (linear) when given. A `Vector4`
   * colour also sets the instance colour's alpha, which three ignores and the Outpost kit's wear shader reads as the draw's
   * wear ([`applyOutpostWear`]). Draws of another `part` go to other batches: moving one re-uploads only its own batch's
   * matrices.
   */
  add(geometry: THREE.BufferGeometry, material: THREE.Material, matrix: THREE.Matrix4, color: THREE.Color | THREE.Vector4 | null = null, part = ""): BatchedDraw {
    const key = `${part}|${material.uuid}|${layout(geometry)}`;
    let group = this.#groups.get(key);
    if (!group) {
      group = { material, colored: false, pending: [] };
      this.#groups.set(key, group);
    }
    group.colored ||= color !== null;
    const draw: BatchedDraw = { mesh: null, instance: -1 };
    group.pending.push({ geometry, matrix: matrix.clone(), color, draw });
    return draw;
  }

  /** Creates the batches of everything added so far. */
  build(): void {
    const white = new THREE.Color(1, 1, 1);
    for (const { material, colored, pending } of this.#groups.values()) {
      if (pending.length === 1 && !colored) {
        // A batch of one would cull and re-upload its draw list in every pass for nothing: three culls a mesh itself.
        const { geometry, matrix, draw } = pending[0]!;
        const mesh = new THREE.Mesh(geometry, material);
        mesh.name = `${this.root.name}:${material.name || material.uuid}`;
        mesh.matrixAutoUpdate = false;
        mesh.matrix.copy(matrix);
        mesh.matrixWorldNeedsUpdate = true;
        draw.mesh = mesh;
        this.root.add(mesh);
        continue;
      }
      const geometries = [...new Set(pending.map((entry) => entry.geometry))];
      const vertices = geometries.reduce((sum, geometry) => sum + geometry.attributes.position!.count, 0);
      const indices = geometries.reduce((sum, geometry) => sum + (geometry.index?.count ?? 0), 0);
      const mesh = new THREE.BatchedMesh(pending.length, vertices, indices, material);
      mesh.name = `${this.root.name}:${material.name || material.uuid}`;
      // The batch's own sphere would go stale as draws move.
      mesh.frustumCulled = false;
      drawWhole(mesh);
      mesh.matrixAutoUpdate = false;
      const ids = new Map(geometries.map((geometry) => [geometry, mesh.addGeometry(geometry)]));
      for (const { geometry, matrix, color, draw } of pending) {
        draw.mesh = mesh;
        draw.instance = mesh.addInstance(ids.get(geometry)!);
        mesh.setMatrixAt(draw.instance, matrix);
        if (colored) { mesh.setColorAt(draw.instance, color ?? white); }
      }
      this.root.add(mesh);
    }
    this.#groups.clear();
  }

  /** Moves `draw` to `matrix` (world). */
  setMatrix(draw: BatchedDraw, matrix: THREE.Matrix4): void {
    const mesh = draw.mesh;
    if (mesh instanceof THREE.BatchedMesh) {
      mesh.setMatrixAt(draw.instance, matrix);
    } else if (mesh) {
      mesh.matrix.copy(matrix);
      mesh.matrixWorldNeedsUpdate = true;
    }
  }

  /** Shows or hides `draw`. */
  setVisible(draw: BatchedDraw, visible: boolean): void {
    const mesh = draw.mesh;
    if (mesh instanceof THREE.BatchedMesh) {
      if (mesh.getVisibleAt(draw.instance) !== visible) { mesh.setVisibleAt(draw.instance, visible); }
    } else if (mesh) {
      mesh.visible = visible;
    }
  }
}

type Mirrored = { mesh: THREE.Mesh; draw: BatchedDraw | null; proxy: THREE.Mesh | null };

/**
 * Asset-tooling glTF roots (kit modules, walk-in buildings) drawn through [`StaticBatches`]: every batchable mesh is an
 * instance, any other a plain copy, both following their source. The sources stay out of the scene (they keep their
 * meshes for picking); after a source root moves, or is shown or hidden, [`sync`] it.
 */
export class BatchedRoots {
  readonly #batches: StaticBatches;
  readonly #sources: readonly THREE.Object3D[];
  readonly #meshes: Mirrored[][];

  /** `wear`, when given, is each source root's wear in [0, 1], its instances' colour alpha ([`StaticBatches.add`]). */
  constructor(name: string, sources: readonly THREE.Object3D[], wear: readonly number[] | null = null) {
    this.#batches = new StaticBatches(name);
    this.#sources = sources;
    this.#meshes = sources.map((source, index) => {
      source.updateMatrixWorld(true);
      const meshes: Mirrored[] = [];
      const color = wear ? new THREE.Vector4(1, 1, 1, wear[index] ?? 0) : null;
      source.traverse((object) => {
        if (!(object instanceof THREE.Mesh)) { return; }
        if (batchable(object.material) && !(object as THREE.SkinnedMesh).isSkinnedMesh && !(object as THREE.InstancedMesh).isInstancedMesh) {
          meshes.push({ mesh: object, draw: this.#batches.add(object.geometry, object.material, object.matrixWorld, color), proxy: null });
          return;
        }
        const proxy = new THREE.Mesh(object.geometry, object.material);
        proxy.name = object.name;
        proxy.matrixAutoUpdate = false;
        meshes.push({ mesh: object, draw: null, proxy });
      });
      return meshes;
    });
    this.#batches.build();
    for (const meshes of this.#meshes) {
      for (const { proxy } of meshes) { if (proxy) { this.#batches.root.add(proxy); } }
    }
    sources.forEach((_, index) => this.sync(index));
  }

  /** The drawn root: the batches and the copies. */
  get root(): THREE.Group {
    return this.#batches.root;
  }

  /** Re-reads source root `index`: its meshes' world matrices and whether each (and every ancestor) is visible. */
  sync(index: number): void {
    const source = this.#sources[index];
    if (!source) { return; }
    source.updateMatrixWorld(true);
    for (const { mesh, draw, proxy } of this.#meshes[index]!) {
      let visible = true;
      for (let object: THREE.Object3D | null = mesh; object && visible; object = object === source ? null : object.parent) {
        visible = object.visible;
      }
      if (draw) {
        this.#batches.setMatrix(draw, mesh.matrixWorld);
        this.#batches.setVisible(draw, visible);
      } else if (proxy) {
        proxy.matrix.copy(mesh.matrixWorld);
        proxy.matrixWorldNeedsUpdate = true;
        proxy.visible = visible;
      }
    }
  }
}

/** A shared static-scene geometry as three draws it (3d-lab's `createIndexedMeshGeometry`: sRGB vertex colours made linear). */
function sceneGeometry(geometry: StaticBatch["node"]["geometry"]): THREE.BufferGeometry {
  const built = new THREE.BufferGeometry();
  built.setAttribute("position", new THREE.Float32BufferAttribute(geometry.positions.flat(), 3));
  built.setIndex([...geometry.indices]);
  if (geometry.normals) {
    built.setAttribute("normal", new THREE.Float32BufferAttribute(geometry.normals.flat(), 3));
  } else {
    built.computeVertexNormals();
  }
  if (geometry.colors) {
    const colors = new Float32Array(geometry.colors.length * 3);
    const color = new THREE.Color();
    geometry.colors.forEach(([r, g, b], index) => color.setRGB(r, g, b, THREE.SRGBColorSpace).toArray(colors, index * 3));
    built.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  }
  return built;
}

/**
 * The shared static world (`buildSceneryScene`) as [`StaticBatches`] (#615): every draw group (terrain, roads, merged
 * props) and every animated part's node is one instance, its colour an instance colour over a white
 * `presentationMaterial`, so the few materials left are the emissive and double-sided variants. Only water, which
 * blends, stays a renderer node. [`show`] follows a `SceneryFrame`'s nodes: its distance and kit culling, and where
 * the animated parts move.
 */
export class BatchedScenery {
  readonly #batches = new StaticBatches("static-scene");
  /** The draws of the static groups, then of the animated nodes, in `SceneryFrame` order. */
  readonly #statics: BatchedDraw[];
  readonly #animated: BatchedDraw[];
  readonly #water: number;
  /** Per animated node the model matrix it was last drawn at. */
  readonly #placed: Float32Array;
  readonly #matrix = new THREE.Matrix4();

  constructor(scene: Pick<SceneryScene, "batches" | "water" | "animated">) {
    const materials = new Map<string, THREE.Material>();
    const add = (node: Pick<RendererSceneNode, "geometry" | "color" | "emissive" | "doubleSided" | "unlit">, model: ArrayLike<number> | undefined, part: string): BatchedDraw => {
      const geometry = sceneGeometry(node.geometry as StaticBatch["node"]["geometry"]);
      const vertexColors = geometry.hasAttribute("color");
      const white = { ...node, color: "#ffffff" } as unknown as RendererSceneNode;
      const key = JSON.stringify([white.emissive ?? null, white.doubleSided ?? false, white.unlit ?? false, vertexColors]);
      let material = materials.get(key);
      if (!material) {
        material = presentationMaterial({ node: white, vertexColors } as RendererMaterialRequest);
        materials.set(key, material);
      }
      this.#matrix.identity();
      if (model) { this.#matrix.fromArray(model); }
      return this.#batches.add(geometry, material, this.#matrix, new THREE.Color(node.color ?? 0xffffff), part);
    };
    this.#statics = scene.batches.map(({ node }) => add(node, node.modelMatrix, "static"));
    this.#water = scene.water.length;
    // Their own batches: the parts move every frame, and only their matrices re-upload.
    this.#animated = scene.animated.flat().map((node) => add(node, undefined, "animated"));
    this.#placed = new Float32Array(this.#animated.length * 16).fill(Number.NaN);
    this.#batches.build();
  }

  /** The drawn root. */
  get root(): THREE.Group {
    return this.#batches.root;
  }

  /** How many of a `SceneryFrame`'s leading nodes the batches draw (the static groups); water and animated parts follow. */
  get statics(): number {
    return this.#statics.length;
  }

  /** The nodes of a frame the batches do not draw: the water. */
  undrawn(nodes: readonly RendererSceneNode[]): RendererSceneNode[] {
    return nodes.slice(this.#statics.length, this.#statics.length + this.#water);
  }

  /** Shows, hides and places the draws as a `SceneryFrame`'s `nodes` do (static groups, water, animated parts). */
  show(nodes: readonly RendererSceneNode[]): void {
    this.#statics.forEach((draw, index) => this.#batches.setVisible(draw, nodes[index]?.visible !== false));
    const first = this.#statics.length + this.#water;
    this.#animated.forEach((draw, index) => {
      const node = nodes[first + index];
      this.#batches.setVisible(draw, node !== undefined && node.visible !== false);
      const model = node?.modelMatrix;
      if (!model || sameMatrix(this.#placed, index * 16, model)) { return; }
      this.#placed.set(model, index * 16);
      this.#batches.setMatrix(draw, this.#matrix.fromArray(model));
    });
  }

  /** Releases the batches' GPU buffers. */
  dispose(): void {
    for (const mesh of this.#batches.root.children) { if (mesh instanceof THREE.BatchedMesh) { mesh.dispose(); } }
  }
}

function sameMatrix(kept: Float32Array, at: number, model: ArrayLike<number>): boolean {
  for (let index = 0; index < 16; index += 1) {
    if (kept[at + index] !== Math.fround(model[index]!)) { return false; }
  }
  return true;
}
