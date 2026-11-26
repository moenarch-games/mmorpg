import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { RendererMaterialRequest, RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { presentationMaterial } from "./lighting";
import type { GroupBuffers, GroupStreamSource, SceneryScene, StaticBatch } from "./scenery-nodes";
import { freeHandle } from "./zone-lifecycle";

/**
 * One batched mesh: the `BatchedMesh` that draws it and its instance there, or the plain mesh drawing it alone; and
 * its instance in its part's shadow batch ([`StaticBatches`]), -1 when it casts by itself.
 */
export type BatchedDraw = {
  mesh: THREE.BatchedMesh | THREE.Mesh | null;
  instance: number;
  shadow: number;
  caster: THREE.BatchedMesh | null;
  /** Its geometry's own bounding sphere, placed on a move ([`StaticBatches.setMatrix`]) to keep its cells' spheres around it. */
  bounds: THREE.Sphere;
};

type Pending = {
  geometry: THREE.BufferGeometry;
  matrix: THREE.Matrix4;
  color: THREE.Color | THREE.Vector4 | null;
  draw: BatchedDraw;
  /** Its geometry's sphere where `matrix` places it. */
  placed: THREE.Sphere;
  /** The ground cell ([`BATCH_CELL`]) holding its centre. */
  cell: string;
};

/**
 * The side, in metres, of the square ground cells (x and z) [`StaticBatches`] culls by (#1113). Unculled, every placed
 * kit module of the region was a sub-draw of every view, about 22k a frame in Greyhaven. Shadow batches split by cell,
 * each with the sphere around its casters, so a sun cascade skips the cells it cannot see as a whole: a shadow batch
 * holds every material's casters, so a cell costs one call per cascade. A drawn batch stays one per material (a cell
 * each would add a main-pass call per material and cell, about 200 at Greyhaven's hub) and hides the instances of the
 * cells outside the main view instead ([`CellCulledBatch`]).
 */
export const BATCH_CELL = 64;

/** The ground cell holding `point`. */
function cellOf(point: THREE.Vector3): string {
  return `${Math.floor(point.x / BATCH_CELL)},${Math.floor(point.z / BATCH_CELL)}`;
}

type Cell = { sphere: THREE.Sphere; instances: number[]; seen: boolean };

const CLIP_FROM_WORLD = new THREE.Matrix4();
/** The main view's frustum, shared by every [`CellCulledBatch`] and set again only when the view changes. */
const FRUSTUM = new THREE.Frustum();
const FRUSTUM_FOR = new THREE.Matrix4().set(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
let FRUSTUM_REVERSED = false;

/**
 * A drawn batch of [`StaticBatches`] that culls its instances by ground cell ([`BATCH_CELL`]) in the main view. Only the
 * main pass draws it (the shadow views give it an invisible material, so three never prepares it there), so before each
 * draw it tests each cell's sphere against that view and shows or hides the instances of the cells that came into or
 * left it. Unlike culling per instance in every view ([`drawWhole`]), the draw list is then rebuilt and re-uploaded
 * only in a frame a cell's view changes, at most once a frame.
 */
export class CellCulledBatch extends THREE.BatchedMesh {
  readonly #cells: Cell[] = [];
  readonly #cellIndex = new Map<string, number>();
  /** Per instance its cell's index. */
  readonly #cellOf: number[] = [];
  /** Per instance whether the caller shows it ([`show`]), whatever its cell. */
  readonly #shown: boolean[] = [];
  /** The clip-from-world matrix of the view it last culled for; zero until its first. */
  readonly #culledFor = new THREE.Matrix4().set(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);

  /** Puts `instance`, placed within `placed`, in `cell`; it starts shown and in view. */
  place(instance: number, cell: string, placed: THREE.Sphere): void {
    let index = this.#cellIndex.get(cell);
    if (index === undefined) {
      index = this.#cells.length;
      this.#cellIndex.set(cell, index);
      this.#cells.push({ sphere: new THREE.Sphere(), instances: [], seen: true });
    }
    const state = this.#cells[index]!;
    state.sphere.union(placed);
    state.instances.push(instance);
    this.#cellOf[instance] = index;
    this.#shown[instance] = true;
  }

  /** Whether the caller shows `instance`. */
  shown(instance: number): boolean {
    return this.#shown[instance] ?? false;
  }

  /** Shows or hides `instance`; a shown one draws while its cell is in view. */
  show(instance: number, visible: boolean): void {
    this.#shown[instance] = visible;
    this.setVisibleAt(instance, visible && this.#cells[this.#cellOf[instance]!]!.seen);
  }

  /** Grows `instance`'s cell's sphere to hold `placed` (world), where it moved; a grown cell is tested at the next draw. */
  moved(instance: number, placed: THREE.Sphere): void {
    if (enclose(this.#cells[this.#cellOf[instance]!]!.sphere, placed)) { this.#culledFor.elements.fill(0); }
  }

  /** How many cells it culls by. */
  get cellCount(): number {
    return this.#cells.length;
  }

  /**
   * Brings each cell's instances in line with whether `camera` sees the cell. A view unchanged since this batch's last
   * cull (a standing camera's) tests nothing, unless a move grew a cell since.
   */
  cull(camera: THREE.Camera): void {
    CLIP_FROM_WORLD.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    if (this.#culledFor.equals(CLIP_FROM_WORLD)) { return; }
    this.#culledFor.copy(CLIP_FROM_WORLD);
    if (!FRUSTUM_FOR.equals(CLIP_FROM_WORLD) || FRUSTUM_REVERSED !== camera.reversedDepth) {
      FRUSTUM.setFromProjectionMatrix(CLIP_FROM_WORLD, THREE.WebGLCoordinateSystem, camera.reversedDepth);
      FRUSTUM_FOR.copy(CLIP_FROM_WORLD);
      FRUSTUM_REVERSED = camera.reversedDepth;
    }
    for (const cell of this.#cells) {
      const seen = FRUSTUM.intersectsSphere(cell.sphere);
      if (seen === cell.seen) { continue; }
      cell.seen = seen;
      for (const instance of cell.instances) { this.setVisibleAt(instance, seen && this.#shown[instance]!); }
    }
  }

  override onBeforeRender(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera, geometry: THREE.BufferGeometry, material: THREE.Material, group: THREE.Group): void {
    this.cull(camera);
    super.onBeforeRender(renderer, scene, camera, geometry, material, group);
  }
}

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
 *
 * three never computes the batch's own sphere, which walks every vertex of every geometry on its first draw, a 100–500
 * ms frame each time a streamed batch is built mid-walk (#869). With `bounds` (the caller's sphere around every
 * instance, a shadow batch's cell's) a view culls the whole batch as it culls any mesh; without, the sphere is fixed
 * empty and nothing culls the batch.
 */
export function drawWhole(mesh: Pick<THREE.BatchedMesh, "perObjectFrustumCulled" | "sortObjects" | "boundingSphere" | "frustumCulled">, bounds: THREE.Sphere | null = null): void {
  mesh.perObjectFrustumCulled = false;
  mesh.sortObjects = false;
  mesh.boundingSphere = bounds ?? new THREE.Sphere();
  mesh.frustumCulled = bounds !== null;
}

/** One instance in a [`GrowingBatch`]. */
export type GrowingPlaced = { batch: GrowingBatch; geometry: number; instance: number };

/**
 * One `BatchedMesh` for one material (#526, as #615 batches the static scenery): every streamed tile or group drawn with
 * that material is one instance, drawn whole in every view ([`drawWhole`]), so a streamed layer costs one multi-draw call
 * per material and pass. Room grows as levels stream in; evicted ones leave their room for the next that fits it.
 *
 * Evicting never frees buffer space: `BatchedMesh` only appends, and reclaiming its holes (`optimize`) shifts and
 * re-uploads the whole buffer in one frame, a hitch every few metres of walking. Instead an evicted level's geometry
 * slot (reserved at a power of two, so slots fit many levels) waits on a free list, the smallest fitting slot takes
 * the next level, uploading only its own range, and a full buffer doubles.
 */
export class GrowingBatch {
  readonly mesh: THREE.BatchedMesh;
  #vertices: number;
  #indices: number;
  /** Geometry slots no instance uses, with their reserved room. */
  readonly #free: { id: number; vertices: number; indices: number }[] = [];

  constructor(name: string, material: THREE.Material, vertices: number, indices: number, instances: number) {
    this.#vertices = vertices;
    this.#indices = indices;
    this.mesh = new THREE.BatchedMesh(instances, vertices, indices, material);
    this.mesh.name = name;
    this.mesh.matrixAutoUpdate = false;
    // Instances stream in and out, so the batch's own sphere would go stale: nothing culls it.
    drawWhole(this.mesh);
  }

  add(geometry: THREE.BufferGeometry, visible: boolean, matrix: THREE.Matrix4 | null = null, color: THREE.Color | null = null): GrowingPlaced {
    const vertices = geometry.attributes.position!.count;
    const indices = geometry.index!.count;
    const mesh = this.mesh;
    let slot = -1;
    for (let index = 0; index < this.#free.length; index += 1) {
      const free = this.#free[index]!;
      if (free.vertices >= vertices && free.indices >= indices && (slot < 0 || free.indices < this.#free[slot]!.indices)) { slot = index; }
    }
    let id: number;
    if (slot >= 0) {
      id = this.#free.splice(slot, 1)[0]!.id;
      mesh.setGeometryAt(id, geometry);
    } else {
      const [reservedVertices, reservedIndices] = [roomFor(vertices), roomFor(indices)];
      if (mesh.unusedVertexCount < reservedVertices || mesh.unusedIndexCount < reservedIndices) {
        this.#vertices = Math.max(this.#vertices * 2, this.#vertices + reservedVertices);
        this.#indices = Math.max(this.#indices * 2, this.#indices + reservedIndices);
        mesh.setGeometrySize(this.#vertices, this.#indices);
      }
      id = mesh.addGeometry(geometry, reservedVertices, reservedIndices);
    }
    if (mesh.instanceCount >= mesh.maxInstanceCount) {
      mesh.setInstanceCount(mesh.maxInstanceCount * 2);
    }
    const instance = mesh.addInstance(id);
    if (matrix) { mesh.setMatrixAt(instance, matrix); }
    if (color) { mesh.setColorAt(instance, color); }
    mesh.setVisibleAt(instance, visible);
    geometry.dispose();
    return { batch: this, geometry: id, instance };
  }

  remove(placed: GrowingPlaced): void {
    this.mesh.deleteInstance(placed.instance);
    const room = this.mesh.getGeometryRangeAt(placed.geometry)!;
    this.#free.push({ id: placed.geometry, vertices: room.reservedVertexCount, indices: room.reservedIndexCount });
  }
}

/** A slot's reserved room for `count` vertices or indices: the next power of two. */
function roomFor(count: number): number {
  return 2 ** Math.ceil(Math.log2(Math.max(count, 1)));
}

/**
 * Static lit scenery drawn as one three `BatchedMesh` per material and vertex layout (#615): Greyhaven's frame is bound
 * by draw calls, each paid in the main pass and again in every sun cascade, and a batch is one multi-draw call per
 * pass however many meshes it holds. Each culls by ground cell ([`BATCH_CELL`]): a drawn batch hides the cells outside
 * the main view ([`CellCulledBatch`]), and shadow batches split by cell, so a sun cascade skips the cells outside it.
 * [`add`] collects meshes, [`build`] creates the batches once; afterwards each draw can be moved ([`setMatrix`]) or
 * shown and hidden ([`setVisible`]).
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
   * matrices. Its ground cell is where `matrix` places the geometry's centre.
   */
  add(geometry: THREE.BufferGeometry, material: THREE.Material, matrix: THREE.Matrix4, color: THREE.Color | THREE.Vector4 | null = null, part = ""): BatchedDraw {
    // Computed once per geometry: kit modules' clones share theirs.
    if (!geometry.boundingSphere) { geometry.computeBoundingSphere(); }
    const bounds = geometry.boundingSphere!;
    const placed = bounds.clone().applyMatrix4(matrix);
    const key = `${part}|${material.uuid}|${layout(geometry)}`;
    let group = this.#groups.get(key);
    if (!group) {
      group = { material, colored: false, part, pending: [] };
      this.#groups.set(key, group);
    }
    group.colored ||= color !== null;
    const draw: BatchedDraw = { mesh: null, instance: -1, shadow: -1, caster: null, bounds };
    group.pending.push({ geometry, matrix: matrix.clone(), color, draw, placed, cell: cellOf(placed.center) });
    return draw;
  }

  /**
   * Creates the batches of everything added so far. Every opaque single-sided draw also joins its part's shadow batch
   * and its own batch casts no shadow: the shadow views draw all such casters with one shared material, so a part
   * casts in one multi-draw per cell and view instead of one per material.
   */
  build(): void {
    const white = new THREE.Color(1, 1, 1);
    const casters = new Map<string, { part: string; doubleSided: boolean; pending: Pending[] }>();
    for (const { material, colored, part, pending } of this.#groups.values()) {
      const casts = castsWithSharedShadow(material);
      if (casts) {
        const doubleSided = casts === "double";
        for (const entry of pending) {
          const key = `${part}|${entry.cell}|${doubleSided}`;
          const caster = casters.get(key) ?? { part, doubleSided, pending: [] };
          caster.pending.push(entry);
          casters.set(key, caster);
        }
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
      const mesh = new CellCulledBatch(pending.length, vertices, indices, material);
      mesh.name = `${this.root.name}:${material.name || material.uuid}`;
      // Its cells cull its instances; the batch as a whole stays.
      drawWhole(mesh);
      mesh.matrixAutoUpdate = false;
      if (casts) { mesh.userData.notShadowCaster = true; }
      const ids = new Map(geometries.map((geometry) => [geometry, mesh.addGeometry(geometry)]));
      for (const { geometry, matrix, color, draw, placed, cell } of pending) {
        draw.mesh = mesh;
        draw.instance = mesh.addInstance(ids.get(geometry)!);
        mesh.place(draw.instance, cell, placed);
        mesh.setMatrixAt(draw.instance, matrix);
        if (colored) { mesh.setColorAt(draw.instance, color ?? white); }
      }
      this.root.add(mesh);
    }
    for (const { part, doubleSided, pending } of casters.values()) { this.#buildCaster(part, doubleSided, pending); }
    this.#groups.clear();
  }

  /** `pending`'s shadow batch (one cell's): their positions only, drawn by nothing but the shadow views. */
  #buildCaster(part: string, doubleSided: boolean, pending: readonly Pending[]): void {
    const shapes = new Map<THREE.BufferGeometry, THREE.BufferGeometry>();
    for (const { geometry } of pending) {
      if (!shapes.has(geometry)) { shapes.set(geometry, shadowShape(geometry)); }
    }
    const vertices = [...shapes.values()].reduce((sum, shape) => sum + shape.attributes.position!.count, 0);
    const indices = [...shapes.values()].reduce((sum, shape) => sum + shape.index!.count, 0);
    const caster = new THREE.BatchedMesh(pending.length, vertices, indices, doubleSided ? SHADOW_ONLY.double : SHADOW_ONLY.single);
    caster.name = `${this.root.name}:shadow${part ? `:${part}` : ""}${doubleSided ? ":double" : ""}`;
    // Its sphere grows as draws move ([`setMatrix`]).
    drawWhole(caster, enclosing(pending));
    caster.matrixAutoUpdate = false;
    const ids = new Map([...shapes].map(([geometry, shape]) => [geometry, caster.addGeometry(shape)]));
    for (const { geometry, matrix, draw } of pending) {
      draw.caster = caster;
      draw.shadow = caster.addInstance(ids.get(geometry)!);
      caster.setMatrixAt(draw.shadow, matrix);
    }
    this.root.add(caster);
  }

  /** Moves `draw` to `matrix` (world), growing its cells' spheres to keep it inside. */
  setMatrix(draw: BatchedDraw, matrix: THREE.Matrix4): void {
    const mesh = draw.mesh;
    const batched = mesh instanceof CellCulledBatch;
    const placed = batched || draw.caster ? MOVED.copy(draw.bounds).applyMatrix4(matrix) : null;
    if (batched) {
      mesh.setMatrixAt(draw.instance, matrix);
      mesh.moved(draw.instance, placed!);
    } else if (mesh) {
      mesh.matrix.copy(matrix);
      mesh.matrixWorldNeedsUpdate = true;
    }
    if (draw.caster) {
      draw.caster.setMatrixAt(draw.shadow, matrix);
      enclose(draw.caster.boundingSphere!, placed!);
    }
  }

  /**
   * Releases the batches' buffers (#1005), and the geometries of single-draw meshes when `ownGeometry` (the caller made
   * them for these batches; shared glTF geometry stays). Materials are the caller's.
   */
  dispose(ownGeometry: boolean): void {
    for (const object of this.root.children) {
      if (object instanceof THREE.BatchedMesh) { object.dispose(); }
      else if (ownGeometry && object instanceof THREE.Mesh) { (object.geometry as THREE.BufferGeometry).dispose(); }
    }
    this.root.clear();
  }

  /** Shows or hides `draw`. */
  setVisible(draw: BatchedDraw, visible: boolean): void {
    const mesh = draw.mesh;
    if (mesh instanceof CellCulledBatch) {
      if (mesh.shown(draw.instance) !== visible) { mesh.show(draw.instance, visible); }
    } else if (mesh) {
      mesh.visible = visible;
    }
    const caster = draw.caster;
    if (caster && caster.getVisibleAt(draw.shadow) !== visible) { caster.setVisibleAt(draw.shadow, visible); }
  }
}

const MOVED = new THREE.Sphere();

/** The sphere around every placed draw of `pending` (world). */
function enclosing(pending: readonly Pending[]): THREE.Sphere {
  const sphere = new THREE.Sphere();
  for (const { placed } of pending) { sphere.union(placed); }
  return sphere;
}

/** Grows `sphere` to hold `placed` when it does not yet; whether it grew. */
function enclose(sphere: THREE.Sphere, placed: THREE.Sphere): boolean {
  if (sphere.center.distanceTo(placed.center) + placed.radius <= sphere.radius) { return false; }
  sphere.union(placed);
  return true;
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

  /** Releases the batches (#1005); the sources' geometries and materials, and the copies sharing them, stay. */
  dispose(): void {
    this.#batches.dispose(false);
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
export type StreamKits = { outpost: boolean; settlementProps: boolean; nature: boolean; limestone: boolean; climbing: boolean; crag: boolean; dressed: Uint32Array };

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
  #disposed = false;

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

  /**
   * The white material of `node`'s emissive, double-sided and vertex-colour variant; one per variant. Only the fields
   * `presentationMaterial` reads are copied: spreading the node would decode its lazy geometry (#855).
   */
  #material(node: Pick<RendererSceneNode, "emissive" | "doubleSided" | "unlit" | "color" | "opacity">, vertexColors: boolean): THREE.Material {
    const { emissive, doubleSided, unlit, opacity } = node;
    const white = { emissive, doubleSided, unlit, opacity, color: "#ffffff" } as unknown as RendererSceneNode;
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
    const update = this.#stream.update(eye[0], eye[1], eye[2], kits.outpost, kits.settlementProps, kits.nature, kits.limestone, kits.climbing, kits.crag, kits.dressed);
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

  /**
   * Releases everything the static scene's batches own (#1005): their GPU buffers, the animated parts' batches and
   * geometries, the white materials and the group stream's WASM handle. Later calls do nothing.
   */
  dispose(): void {
    if (this.#disposed) { return; }
    this.#disposed = true;
    for (const batch of this.#batches.values()) { batch.mesh.dispose(); }
    this.#batches.clear();
    this.#animated.dispose(true);
    for (const material of this.#materials.values()) { material.dispose(); }
    this.#materials.clear();
    freeHandle(this.#stream);
    this.root.clear();
  }
}

function sameMatrix(kept: Float32Array, at: number, model: ArrayLike<number>): boolean {
  for (let index = 0; index < 16; index += 1) {
    if (kept[at + index] !== Math.fround(model[index]!)) { return false; }
  }
  return true;
}
