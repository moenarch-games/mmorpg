import * as THREE from "three";

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
 * complementary pixels, so two levels of one placement blend without sorting. A per-instance attribute carries the fade.
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
  faded.customProgramCacheKey = () => "instanced-levels-cross-fade";
  return faded;
}

/**
 * One drawn primitive of one entry level: its settled instances through the glTF material, and its cross-fading
 * instances through the dithered copy (#593). The copy's `discard` would cost every settled pixel its early depth
 * test, so only the few fading instances pay for it. Both meshes share the geometry's buffers.
 */
type Part = { settled: THREE.InstancedMesh; fading: THREE.InstancedMesh; fade: THREE.InstancedBufferAttribute };

/** Where a placement's instance stands this frame: its level and slot in the settled or fading meshes. */
type Slot = { level: number; slot: number; fading: boolean };

/**
 * Placements of a few bundle entries drawn as one `InstancedMesh` per entry level and primitive, from the shared
 * Rust level selection (`mmorpg_presentation::lod::Lods`): Greyhaven's trees and rocks (#49) and the Battle Royale
 * fortress kit (#599). Every placement of an entry shares its geometry and materials; [`apply`] places the drawn
 * levels of a selection and [`move`] follows a moving placement (a lift piece) without re-placing the rest.
 */
export class InstancedLevels {
  readonly root = new THREE.Group();
  /** Per entry, per level its parts. */
  readonly #levels: Part[][][];
  /** Per placement its entry index and current model matrix. */
  readonly #entryOf: number[];
  readonly #matrices: Float32Array[];
  /** Per placement where it is drawn (one slot settled, two while cross-fading). */
  #slots: Slot[][];
  /** Per entry, per level how many placements it draws (settled and fading). */
  #counts: number[][];

  /**
   * `levels[entry][level]` is the level's glTF scene in the entry frame; `placements` name their entry and carry 16
   * column-major entry-to-world floats (metres).
   */
  constructor(name: string, entries: readonly string[], levels: readonly (readonly THREE.Object3D[])[], placements: readonly { key: string; matrix: ArrayLike<number> }[]) {
    this.root.name = name;
    this.#entryOf = placements.map((placement) => entries.indexOf(placement.key));
    if (this.#entryOf.some((entry) => entry < 0)) { throw new Error(`a ${name} placement names no entry`); }
    this.#matrices = placements.map((placement) => Float32Array.from(placement.matrix));
    const capacity = entries.map((_, entry) => this.#entryOf.filter((of) => of === entry).length);
    // Materials are shared across levels and entries where the glTFs share them.
    const materials = new Map<THREE.Material, THREE.Material>();
    this.#levels = levels.map((scenes, entry) => scenes.map((scene) => {
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
        for (const [attributeName, attribute] of Object.entries(geometry.attributes)) { fadingGeometry.setAttribute(attributeName, attribute); }
        const fade = new THREE.InstancedBufferAttribute(new Float32Array(size).fill(1), 1);
        fade.setUsage(THREE.DynamicDrawUsage);
        fadingGeometry.setAttribute("instanceFade", fade);
        const instanced = (meshGeometry: THREE.BufferGeometry, meshMaterial: THREE.Material, suffix: string) => {
          const mesh = new THREE.InstancedMesh(meshGeometry, meshMaterial, size);
          mesh.name = `${entries[entry]}#${parts.length}${suffix}`;
          // Placements are culled by the shared selection (by the descriptor bounds), not by one stale sphere.
          mesh.frustumCulled = false;
          mesh.count = 0;
          mesh.visible = false;
          mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
          this.root.add(mesh);
          return mesh;
        };
        parts.push({ settled: instanced(geometry, source, ""), fading: instanced(fadingGeometry, material, ".fading"), fade });
      });
      return parts;
    }));
    this.#slots = placements.map(() => []);
    this.#counts = this.#levels.map((entryLevels) => entryLevels.map(() => 0));
  }

  /** Places the drawn levels of a selection: per drawn level its placement, level and fade, three floats each. */
  apply(shown: Float32Array): void {
    const settled = this.#levels.map((entryLevels) => entryLevels.map(() => 0));
    const fading = this.#levels.map((entryLevels) => entryLevels.map(() => 0));
    for (const slots of this.#slots) { slots.length = 0; }
    for (let index = 0; index < shown.length; index += 3) {
      const placement: number = shown[index]!;
      const level: number = shown[index + 1]!;
      const t: number = shown[index + 2]!;
      const entry = this.#entryOf[placement]!;
      const parts = this.#levels[entry]?.[level];
      if (!parts) { continue; }
      const matrix = this.#matrices[placement]!;
      if (t >= 1) {
        const slot = settled[entry]![level]!++;
        for (const part of parts) { (part.settled.instanceMatrix.array as Float32Array).set(matrix, slot * 16); }
        this.#slots[placement]!.push({ level, slot, fading: false });
      } else {
        const slot = fading[entry]![level]!++;
        for (const part of parts) {
          (part.fading.instanceMatrix.array as Float32Array).set(matrix, slot * 16);
          (part.fade.array as Float32Array)[slot] = t;
        }
        this.#slots[placement]!.push({ level, slot, fading: true });
      }
    }
    this.#levels.forEach((entryLevels, entry) => entryLevels.forEach((parts, level) => {
      const settledCount = settled[entry]![level]!;
      const fadingCount = fading[entry]![level]!;
      this.#counts[entry]![level] = settledCount + fadingCount;
      for (const part of parts) {
        place(part.settled, settledCount);
        if (place(part.fading, fadingCount)) { upload(part.fade, fadingCount); }
      }
    }));
  }

  /** Moves `placement` to `matrix` (16 column-major floats), uploading only its own instances. */
  move(placement: number, matrix: ArrayLike<number>): void {
    const kept = this.#matrices[placement];
    if (!kept) { return; }
    kept.set(matrix);
    const parts = this.#levels[this.#entryOf[placement]!]!;
    for (const { level, slot, fading } of this.#slots[placement]!) {
      for (const part of parts[level]!) {
        const attribute = fading ? part.fading.instanceMatrix : part.settled.instanceMatrix;
        (attribute.array as Float32Array).set(kept, slot * 16);
        attribute.addUpdateRange(slot * 16, 16);
        attribute.needsUpdate = true;
      }
    }
  }

  /** Per entry, per level how many placements the last selection draws. */
  get counts(): readonly (readonly number[])[] {
    return this.#counts;
  }
}
