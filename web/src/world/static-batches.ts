import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { RendererMaterialRequest, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { presentationMaterial } from "./lighting";
import type { GroupBuffers, GroupStreamSource, SceneryScene, StaticBatch } from "./scenery-nodes";

/**
 * One batched mesh: the `BatchedMesh` that draws it and its instance there, or the plain mesh drawing it alone; and
 * its instance in its part's shadow batch ([`StaticBatches`]), -1 when it casts by itself.
 */
export type BatchedDraw = { mesh: THREE.BatchedMesh | THREE.Mesh | null; instance: number; shadow: number; caster: THREE.BatchedMesh | null };

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

/** One instance in a [`GrowingBatch`]. */
export type GrowingPlaced = { batch: GrowingBatch; geometry: number; instance: number };

/**
 * One `BatchedMesh` for one material (#526, as #615 batches the static scenery): every streamed tile or group drawn with
 * that material is one instance, drawn whole in every view ([`drawWhole`]), so a streamed layer costs one multi-draw call
 * per material and pass. Room grows as levels stream in; evicted ones free theirs for reuse.
 */
export class GrowingBatch {
  readonly mesh: THREE.BatchedMesh;
  #vertices: number;
  #indices: number;

  constructor(name: string, material: THREE.Material, vertices: number, indices: number, instances: number) {
    this.#vertices = vertices;
    this.#indices = indices;
    this.mesh = new THREE.BatchedMesh(instances, vertices, indices, material);
    this.mesh.name = name;
    this.mesh.matrixAutoUpdate = false;
    // Instances stream in and out, so the batch's own sphere would go stale.
    this.mesh.frustumCulled = false;
    drawWhole(this.mesh);
  }

  add(geometry: THREE.BufferGeometry, visible: boolean, matrix: THREE.Matrix4 | null = null, color: THREE.Color | null = null): GrowingPlaced {
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
    if (matrix) { mesh.setMatrixAt(instance, matrix); }
    if (color) { mesh.setColorAt(instance, color); }
    mesh.setVisibleAt(instance, visible);
    geometry.dispose();
    return { batch: this, geometry: id, instance };
  }

  remove(placed: GrowingPlaced): void {
    this.mesh.deleteInstance(placed.instance);
    this.mesh.deleteGeometry(placed.geometry);
  }
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
  readonly #groups = new Map<string, { material: THREE.Material; colored: boolean; part: string; pending: Pending[] }>();

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
      group = { material, colored: false, part, pending: [] };
      this.#groups.set(key, group);
    }
    group.colored ||= color !== null;
    const draw: BatchedDraw = { mesh: null, instance: -1, shadow: -1, caster: null };
    group.pending.push({ geometry, matrix: matrix.clone(), color, draw });
    return draw;
  }

  /**
   * Creates the batches of everything added so far. Every opaque single-sided draw also joins its part's shadow batch
   * and its own batch casts no shadow: the shadow views draw all such casters with one shared material, so a part
   * casts in one multi-draw per view instead of one per material.
   */
  build(): void {
    const white = new THREE.Color(1, 1, 1);
    const casters = new Map<string, { part: string; doubleSided: boolean; pending: Pending[] }>();
    for (const { material, colored, part, pending } of this.#groups.values()) {
      const casts = castsWithSharedShadow(material);
      if (casts) {
        const doubleSided = casts === "double";
        const caster = casters.get(`${part}|${doubleSided}`) ?? { part, doubleSided, pending: [] };
        caster.pending.push(...pending);
        casters.set(`${part}|${doubleSided}`, caster);
      }
      if (pending.length === 1 && !colored) {
        // A batch of one would cull and re-upload its draw list in every pass for nothing: three culls a mesh itself.
        const { geometry, matrix, draw } = pending[0]!;
        const mesh = new THREE.Mesh(geometry, material);
        mesh.name = `${this.root.name}:${material.name || material.uuid}`;
        mesh.matrixAutoUpdate = false;
        mesh.matrix.copy(matrix);
        mesh.matrixWorldNeedsUpdate = true;
        if (casts) { mesh.userData.notShadowCaster = true; }
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
      if (casts) { mesh.userData.notShadowCaster = true; }
      const ids = new Map(geometries.map((geometry) => [geometry, mesh.addGeometry(geometry)]));
      for (const { geometry, matrix, color, draw } of pending) {
        draw.mesh = mesh;
        draw.instance = mesh.addInstance(ids.get(geometry)!);
        mesh.setMatrixAt(draw.instance, matrix);
        if (colored) { mesh.setColorAt(draw.instance, color ?? white); }
      }
      this.root.add(mesh);
    }
    for (const { part, doubleSided, pending } of casters.values()) { this.#buildCaster(part, doubleSided, pending); }
    this.#groups.clear();
  }

  /** `pending`'s shadow batch: their positions only, drawn by nothing but the shadow views. */
  #buildCaster(part: string, doubleSided: boolean, pending: readonly Pending[]): void {
    const shapes = new Map<THREE.BufferGeometry, THREE.BufferGeometry>();
    for (const { geometry } of pending) {
      if (!shapes.has(geometry)) { shapes.set(geometry, shadowShape(geometry)); }
    }
    const vertices = [...shapes.values()].reduce((sum, shape) => sum + shape.attributes.position!.count, 0);
    const indices = [...shapes.values()].reduce((sum, shape) => sum + shape.index!.count, 0);
    const caster = new THREE.BatchedMesh(pending.length, vertices, indices, doubleSided ? SHADOW_ONLY.double : SHADOW_ONLY.single);
    caster.name = `${this.root.name}:shadow${part ? `:${part}` : ""}${doubleSided ? ":double" : ""}`;
    caster.frustumCulled = false;
    drawWhole(caster);
    caster.matrixAutoUpdate = false;
    const ids = new Map([...shapes].map(([geometry, shape]) => [geometry, caster.addGeometry(shape)]));
    for (const { geometry, matrix, draw } of pending) {
      draw.caster = caster;
      draw.shadow = caster.addInstance(ids.get(geometry)!);
      caster.setMatrixAt(draw.shadow, matrix);
    }
    this.root.add(caster);
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
    draw.caster?.setMatrixAt(draw.shadow, matrix);
  }

  /** Shows or hides `draw`. */
  setVisible(draw: BatchedDraw, visible: boolean): void {
    const mesh = draw.mesh;
    if (mesh instanceof THREE.BatchedMesh) {
      if (mesh.getVisibleAt(draw.instance) !== visible) { mesh.setVisibleAt(draw.instance, visible); }
    } else if (mesh) {
      mesh.visible = visible;
    }
    const caster = draw.caster;
    if (caster && caster.getVisibleAt(draw.shadow) !== visible) { caster.setVisibleAt(draw.shadow, visible); }
  }
}

/**
 * The materials of the shadow batches: `LitMaterial`s the main pass skips (`visible` false) and every shadow view
 * replaces with its shared shadow material, as it does each opaque `LitMaterial` (drawing both faces when it is
 * double-sided).
 */
export const SHADOW_ONLY = (() => {
  const material = (doubleSided: boolean) => {
    const shadow = new LitMaterial({ doubleSided });
    shadow.name = doubleSided ? "shadow-only:double" : "shadow-only";
    shadow.visible = false;
    return shadow;
  };
  return { single: material(false), double: material(true) };
})();

/**
 * Whether the shadow views draw `material`'s casters with their shared material, single- or double-sided: an opaque
 * `LitMaterial`. A masked or blended one cuts its shadow out by its own texture, so it casts by itself.
 */
export function castsWithSharedShadow(material: THREE.Material): "single" | "double" | null {
  const lit = material as { isLitMaterial?: boolean; alphaMode?: string; doubleSided?: boolean };
  if (lit.isLitMaterial !== true || lit.alphaMode !== "opaque") { return null; }
  return lit.doubleSided === true ? "double" : "single";
}

/** `geometry`'s positions and indices alone, indexed, as every geometry of one shadow batch must be. */
function shadowShape(geometry: THREE.BufferGeometry): THREE.BufferGeometry {
  const source = geometry.attributes.position!;
  const positions = new Float32Array(source.count * 3);
  for (let index = 0; index < source.count; index += 1) {
    positions[index * 3] = source.getX(index);
    positions[index * 3 + 1] = source.getY(index);
    positions[index * 3 + 2] = source.getZ(index);
  }
  const shape = new THREE.BufferGeometry();
  shape.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  const index = geometry.index;
  const indices = index ? Array.from({ length: index.count }, (_, at) => index.getX(at)) : Array.from({ length: source.count }, (_, at) => at);
  shape.setIndex(new THREE.Uint32BufferAttribute(indices, 1));
  return shape;
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

/** The kits a client draws (`SceneryMotion.kits`) with the stream's dressed props, for a group stream update. */
export type StreamKits = { outpost: boolean; settlementProps: boolean; nature: boolean; climbing: boolean; dressed: Uint32Array };

type ResidentGroup = { placed: GrowingPlaced; shadow: GrowingPlaced | null; visible: boolean };

/** A merged group's vertices (`GroupBuffers`) as three draws them: sRGB vertex colours made linear. */
function groupGeometry({ positions, normals, colors, indices }: GroupBuffers): THREE.BufferGeometry {
  const built = new THREE.BufferGeometry();
  built.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  built.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
  if (colors) {
    const linear = new Float32Array(colors.length);
    const color = new THREE.Color();
    for (let index = 0; index < colors.length; index += 3) {
      color.setRGB(colors[index]!, colors[index + 1]!, colors[index + 2]!, THREE.SRGBColorSpace).toArray(linear, index);
    }
    built.setAttribute("color", new THREE.BufferAttribute(linear, 3));
  }
  built.setIndex(new THREE.BufferAttribute(indices, 1));
  return built;
}

/**
 * The shared static world (`buildSceneryScene`) drawn through batches (#615, #526). Merged static groups stream through
 * [`GrowingBatch`]es as the shared Rust `GroupStream` says, a few per frame: a group's draw is one instance, its colour an
 * instance colour over a white `presentationMaterial`, its opaque casters in the shared shadow batches, so the few
 * materials left are the emissive and double-sided variants. Animated parts are built once in [`StaticBatches`] and only
 * their matrices move. Only water, which blends, stays a renderer node. [`show`] follows a `SceneryFrame`'s nodes: its
 * distance and kit culling, and where the animated parts move.
 */
export class BatchedScenery {
  readonly root = new THREE.Group();
  readonly #scene: Pick<SceneryScene, "batches" | "instanceBatches">;
  readonly #stream: GroupStreamSource;
  readonly #materials = new Map<string, THREE.Material>();
  readonly #batches = new Map<string, GrowingBatch>();
  /** Per static batch, its draw while resident. */
  readonly #resident: (ResidentGroup | null)[];
  /** Per layout group, its index in the scene's static batches (absent for instanced and textured groups). */
  readonly #batchOfGroup = new Map<number, number>();
  /** The layout groups (instance batches included) currently resident. */
  readonly #residentGroups = new Set<number>();
  readonly #animated = new StaticBatches("static-scene:animated");
  readonly #animatedDraws: BatchedDraw[];
  readonly #water: number;
  /** Per animated node the model matrix it was last drawn at. */
  readonly #placed: Float32Array;
  readonly #matrix = new THREE.Matrix4();

  constructor(scene: Pick<SceneryScene, "batches" | "instanceBatches" | "water" | "animated">, stream: GroupStreamSource) {
    this.#scene = scene;
    this.#stream = stream;
    this.root.name = "static-scene";
    this.root.matrixAutoUpdate = false;
    this.#resident = scene.batches.map(() => null);
    scene.batches.forEach((batch, index) => {
      this.#batchOfGroup.set(batch.group, index);
      // A batch per material now, so `prepare` compiles every streamed group's program before the first frame.
      const material = this.#material(batch.node, batch.buffers().colors !== null);
      this.#drawBatch(material);
      this.#shadowBatch(material);
    });
    this.#water = scene.water.length;
    // Their own batches: the parts move every frame, and only their matrices re-upload.
    this.#animatedDraws = scene.animated.flat().map((node) => {
      const geometry = sceneGeometry(node.geometry as StaticBatch["node"]["geometry"]);
      return this.#animated.add(geometry, this.#material(node, geometry.hasAttribute("color")), new THREE.Matrix4(), new THREE.Color(node.color ?? 0xffffff), "animated");
    });
    this.#placed = new Float32Array(this.#animatedDraws.length * 16).fill(Number.NaN);
    this.#animated.build();
    this.root.add(this.#animated.root);
  }

  /** The white material of `node`'s emissive, double-sided and vertex-colour variant; one per variant. */
  #material(node: Pick<RendererSceneNode, "emissive" | "doubleSided" | "unlit" | "color">, vertexColors: boolean): THREE.Material {
    const white = { ...node, color: "#ffffff" } as unknown as RendererSceneNode;
    const key = JSON.stringify([white.emissive ?? null, white.doubleSided ?? false, white.unlit ?? false, vertexColors]);
    let material = this.#materials.get(key);
    if (!material) {
      material = presentationMaterial({ node: white, vertexColors } as RendererMaterialRequest);
      this.#materials.set(key, material);
    }
    return material;
  }

  #batchFor(key: string, name: string, material: THREE.Material, casts: boolean): GrowingBatch {
    let batch = this.#batches.get(key);
    if (!batch) {
      batch = new GrowingBatch(name, material, 65_536, 196_608, 64);
      if (casts) { batch.mesh.userData.notShadowCaster = true; }
      this.#batches.set(key, batch);
      this.root.add(batch.mesh);
    }
    return batch;
  }

  #drawBatch(material: THREE.Material): GrowingBatch {
    return this.#batchFor(`static|${material.uuid}`, `static-scene:${material.name || material.uuid}`, material, castsWithSharedShadow(material) !== null);
  }

  #shadowBatch(material: THREE.Material): GrowingBatch | null {
    const casts = castsWithSharedShadow(material);
    return casts && this.#batchFor(`shadow|${casts}`, `static-scene:shadow:${casts}`, casts === "double" ? SHADOW_ONLY.double : SHADOW_ONLY.single, false);
  }

  /** Advances residency for the camera `eye` and builds or drops the groups the stream names (a few per frame). */
  stream(eye: readonly [number, number, number], kits: StreamKits): void {
    const update = this.#stream.update(eye[0], eye[1], eye[2], kits.outpost, kits.settlementProps, kits.nature, kits.climbing, kits.dressed);
    let at = 0;
    const built = update[at++]!;
    for (let index = 0; index < built; index += 1) {
      const group = update[at++]!;
      this.#residentGroups.add(group);
      const batch = this.#batchOfGroup.get(group);
      if (batch !== undefined) { this.#build(batch); }
    }
    const evicted = update[at++]!;
    for (let index = 0; index < evicted; index += 1) {
      const group = update[at++]!;
      this.#residentGroups.delete(group);
      const batch = this.#batchOfGroup.get(group);
      if (batch !== undefined) { this.#evict(batch); }
    }
  }

  /** Whether layout `group` is resident. */
  resident(group: number): boolean {
    return this.#residentGroups.has(group);
  }

  #build(index: number): void {
    const batch = this.#scene.batches[index]!;
    const geometry = groupGeometry(batch.buffers());
    const material = this.#material(batch.node, geometry.hasAttribute("color"));
    const color = new THREE.Color(batch.node.color ?? 0xffffff);
    const matrix = batch.node.modelMatrix ? this.#matrix.fromArray(batch.node.modelMatrix) : this.#matrix.identity();
    const shadow = this.#shadowBatch(material)?.add(shadowShape(geometry), false, matrix) ?? null;
    this.#resident[index] = { placed: this.#drawBatch(material).add(geometry, false, matrix, color), shadow, visible: false };
  }

  #evict(index: number): void {
    const resident = this.#resident[index];
    if (!resident) { return; }
    resident.placed.batch.remove(resident.placed);
    if (resident.shadow) { resident.shadow.batch.remove(resident.shadow); }
    this.#resident[index] = null;
  }

  /** How many of a `SceneryFrame`'s leading nodes the batches draw (the static groups); water and animated parts follow. */
  get statics(): number {
    return this.#resident.length;
  }

  /** The nodes of a frame the batches do not draw: the water. */
  undrawn(nodes: readonly RendererSceneNode[]): RendererSceneNode[] {
    return nodes.slice(this.#resident.length, this.#resident.length + this.#water);
  }

  /** Shows, hides and places the resident draws as a `SceneryFrame`'s `nodes` do (static groups, water, animated parts). */
  show(nodes: readonly RendererSceneNode[]): void {
    this.#resident.forEach((resident, index) => {
      if (!resident) { return; }
      const visible = nodes[index]?.visible !== false;
      if (visible === resident.visible) { return; }
      resident.visible = visible;
      resident.placed.batch.mesh.setVisibleAt(resident.placed.instance, visible);
      if (resident.shadow) { resident.shadow.batch.mesh.setVisibleAt(resident.shadow.instance, visible); }
    });
    const first = this.#resident.length + this.#water;
    this.#animatedDraws.forEach((draw, index) => {
      const node = nodes[first + index];
      this.#animated.setVisible(draw, node !== undefined && node.visible !== false);
      const model = node?.modelMatrix;
      if (!model || sameMatrix(this.#placed, index * 16, model)) { return; }
      this.#placed.set(model, index * 16);
      this.#animated.setMatrix(draw, this.#matrix.fromArray(model));
    });
  }

  /** Acceptance state: how many groups and merged draws are resident. */
  observe(): { groups: number; resident: number; residentBatches: number } {
    return {
      groups: this.#batchOfGroup.size + this.#scene.instanceBatches.length,
      resident: this.#residentGroups.size,
      residentBatches: this.#resident.filter((resident) => resident !== null).length,
    };
  }

  /** Releases the batches' GPU buffers. */
  dispose(): void {
    for (const batch of this.#batches.values()) { batch.mesh.dispose(); }
    for (const mesh of this.#animated.root.children) { if (mesh instanceof THREE.BatchedMesh) { mesh.dispose(); } }
  }
}

function sameMatrix(kept: Float32Array, at: number, model: ArrayLike<number>): boolean {
  for (let index = 0; index < 16; index += 1) {
    if (kept[at + index] !== Math.fround(model[index]!)) { return false; }
  }
  return true;
}
