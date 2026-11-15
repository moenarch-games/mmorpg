import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";

/** Where grass grows, from the WASM owner (`mmorpg_presentation::grass`). */
export type GrassSource = {
  /** The clumps of cell `(column, row)`: position (m), yaw, height (m), linear RGB; any prefix thins evenly. */
  cell(column: number, row: number): Float32Array;
  /** The side of one cell, in metres. */
  cellMetres: number;
};

const CLUMP_FLOATS = 8;
/** Every clump is drawn out to here; beyond it the field thins, then fades out by `FADE_METRES`. */
const FULL_METRES = 12;
const FADE_METRES = 48;
/** Cells within this distance of the eye are kept, beyond the next one dropped. */
const KEEP_METRES = 56;
const DROP_METRES = 80;
const BUILDS_PER_FRAME = 3;
const BLADES = 15;

/** One clump of thin, tapered, curved blades, a metre tall and about 0.4 m across: `uv.y` runs up each blade. */
export function clumpGeometry(): THREE.BufferGeometry {
  const positions: number[] = [];
  const normals: number[] = [];
  const colors: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  // A fixed sequence, so every clump is the same mesh.
  let seed = 7;
  const random = () => {
    seed = (seed * 16_807) % 2_147_483_647;
    return (seed - 1) / 2_147_483_646;
  };
  const ROWS = [0, 0.4, 0.75];
  for (let blade = 0; blade < BLADES; blade += 1) {
    const reach = Math.sqrt((blade + 0.5) / BLADES) * 0.24;
    const around = blade * 2.399963 + random() * 0.4;
    const root = [Math.sin(around) * reach, Math.cos(around) * reach];
    const facing = random() * Math.PI * 2;
    const across = [Math.cos(facing), -Math.sin(facing)];
    const lean = facing + (random() - 0.5) * 1.2;
    const leanBy = 0.12 + random() * 0.28;
    const height = 0.55 + random() * 0.45;
    const width = 0.022 + random() * 0.014;
    const first = positions.length / 3;
    const point = (t: number, side: number) => {
      const bend = leanBy * t * t;
      const w = width * (1 - t) ** 0.7 * side;
      positions.push(
        root[0]! + Math.sin(lean) * bend + across[0]! * w,
        height * t * (1 - 0.18 * t * leanBy),
        root[1]! + Math.cos(lean) * bend + across[1]! * w,
      );
      // Lit mostly from above, so both faces read alike against the light.
      const n = new THREE.Vector3(Math.sin(lean) * 0.35, 1, Math.cos(lean) * 0.35).normalize();
      normals.push(n.x, n.y, n.z);
      const shade = 0.34 + 0.66 * t ** 0.8;
      colors.push(shade, shade, shade * (0.92 - 0.1 * t), 1);
      uvs.push(side * 0.5 + 0.5, t);
    };
    for (const t of ROWS) { point(t, -1); point(t, 1); }
    point(1, 0);
    for (let row = 0; row < ROWS.length - 1; row += 1) {
      const a = first + row * 2;
      indices.push(a, a + 1, a + 3, a, a + 3, a + 2);
    }
    const top = first + (ROWS.length - 1) * 2;
    indices.push(top, top + 1, top + 2);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 4));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  geometry.computeBoundingSphere();
  return geometry;
}

/**
 * The blades' material: render-engine's lit material with its vertex stage bent by the wind, pushed aside around the
 * player's feet, spread wider and lower with distance and sunk away at the field's edge.
 */
function grassMaterial(uniforms: { grassTime: { value: number }; grassEye: { value: THREE.Vector3 }; grassPush: { value: THREE.Vector3 } }): LitMaterial {
  const material = new LitMaterial({ perceptualRoughness: 0.82, metallic: 0, reflectance: 0.35 });
  material.side = THREE.DoubleSide;
  Object.assign(material.uniforms, uniforms);
  const anchor = "vec4 world = model * vec4(position, 1.0);";
  if (!material.vertexShader.includes(anchor)) { throw new Error("The lit vertex stage has no world position to bend"); }
  material.vertexShader = material.vertexShader
    .replace("void main() {", "uniform float grassTime;\nuniform vec3 grassEye;\nuniform vec3 grassPush;\nvoid main() {")
    .replace(anchor, `${anchor}
    {
        vec3 origin = (model * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
        float d = distance(origin.xz, grassEye.xz);
        float far = clamp((d - ${FULL_METRES.toFixed(1)}) / ${(FADE_METRES - FULL_METRES).toFixed(1)}, 0.0, 1.0);
        world.xz = origin.xz + (world.xz - origin.xz) * (1.0 + 1.4 * far);
        world.y = origin.y + (world.y - origin.y) * (1.0 - smoothstep(${(FADE_METRES - 8).toFixed(1)}, ${FADE_METRES.toFixed(1)}, d));
        float up = max(world.y - origin.y, 0.0);
        float bend = uv.y * uv.y;
        float gust = 0.5 + 0.5 * sin(grassTime * 0.55 + origin.x * 0.045 + origin.z * 0.03);
        float sway = 0.6 * sin(grassTime * 1.8 + origin.x * 0.43 + origin.z * 0.27)
            + 0.25 * sin(grassTime * 3.1 + origin.x * 1.3 - origin.z * 0.9);
        world.xz += vec2(0.8, 0.6) * (sway * 0.6 + gust * 0.9) * bend * 0.32 * up;
        vec2 away = origin.xz - grassPush.xz;
        float near = (1.0 - smoothstep(0.25, 1.0, length(away))) * (1.0 - smoothstep(0.5, 1.5, abs(origin.y - grassPush.y)));
        world.xz += normalize(away + vec2(1e-4, 0.0)) * near * bend * 0.6 * up;
        world.y -= near * bend * 0.45 * up;
    }`);
  return material;
}

type Cell = { mesh: THREE.InstancedMesh; centre: [number, number]; clumps: number };

/**
 * Grass blades over the ground around the camera (presentation only): clumps of thin blades on the cells the shared
 * owner grows them in, every clump drawn near the eye and fewer, wider ones further out. The old upright tufts are gone.
 */
export class Grass {
  readonly root = new THREE.Group();
  readonly #source: GrassSource;
  readonly #geometry = clumpGeometry();
  readonly #uniforms = { grassTime: { value: 0 }, grassEye: { value: new THREE.Vector3() }, grassPush: { value: new THREE.Vector3(0, -1e4, 0) } };
  readonly #material = grassMaterial(this.#uniforms);
  readonly #cells = new Map<string, Cell | null>();
  #changed = true;

  constructor(source: GrassSource) {
    this.#source = source;
    this.root.name = "greyhaven.grass";
  }

  /** Grows and drops cells around `eye`, thins them by distance and moves the wind; `feet` push the blades aside. */
  update(eye: readonly [number, number, number], feet: readonly [number, number, number] | null, seconds: number): void {
    this.#uniforms.grassTime.value = seconds;
    this.#uniforms.grassEye.value.set(eye[0], eye[1], eye[2]);
    if (feet) { this.#uniforms.grassPush.value.set(feet[0], feet[1], feet[2]); }
    const size = this.#source.cellMetres;
    const distance = (column: number, row: number) => {
      const dx = Math.max(column * size - eye[0], 0, eye[0] - (column + 1) * size);
      const dz = Math.max(row * size - eye[2], 0, eye[2] - (row + 1) * size);
      return Math.hypot(dx, dz);
    };
    for (const [key, cell] of this.#cells) {
      const [column, row] = key.split(",").map(Number) as [number, number];
      if (distance(column, row) > DROP_METRES) {
        if (cell) { this.root.remove(cell.mesh); cell.mesh.dispose(); }
        this.#cells.delete(key);
        this.#changed = true;
      }
    }
    const reach = Math.ceil(KEEP_METRES / size);
    const [ec, er] = [Math.floor(eye[0] / size), Math.floor(eye[2] / size)];
    const wanted: [number, number, number][] = [];
    for (let row = er - reach; row <= er + reach; row += 1) {
      for (let column = ec - reach; column <= ec + reach; column += 1) {
        const d = distance(column, row);
        if (d <= KEEP_METRES && !this.#cells.has(`${column},${row}`)) { wanted.push([d, column, row]); }
      }
    }
    wanted.sort((a, b) => a[0] - b[0]);
    for (const [, column, row] of wanted.slice(0, BUILDS_PER_FRAME)) { this.#build(column, row); }
    for (const cell of this.#cells.values()) {
      if (!cell) { continue; }
      const d = Math.hypot(cell.centre[0] - eye[0], cell.centre[1] - eye[2]) - size * 0.5;
      const share = d <= FULL_METRES ? 1 : Math.max(0, 1 - (d - FULL_METRES) / (FADE_METRES - FULL_METRES)) ** 2.2;
      const count = d > FADE_METRES ? 0 : Math.max(1, Math.ceil(cell.clumps * Math.max(share, 0.06)));
      if (cell.mesh.count !== count) { cell.mesh.count = count; }
    }
  }

  #build(column: number, row: number): void {
    const clumps = this.#source.cell(column, row);
    const count = clumps.length / CLUMP_FLOATS;
    if (count === 0) {
      this.#cells.set(`${column},${row}`, null);
      return;
    }
    const mesh = new THREE.InstancedMesh(this.#geometry, this.#material, count);
    mesh.name = `grass:${column},${row}`;
    mesh.userData.notShadowCaster = true;
    const matrix = new THREE.Matrix4();
    const rotation = new THREE.Matrix4();
    const color = new THREE.Color();
    for (let index = 0; index < count; index += 1) {
      const at = index * CLUMP_FLOATS;
      const height = clumps[at + 4]!;
      matrix.makeScale(1, height, 1).premultiply(rotation.makeRotationY(clumps[at + 3]!));
      matrix.setPosition(clumps[at]!, clumps[at + 1]! - 0.03, clumps[at + 2]!);
      mesh.setMatrixAt(index, matrix);
      mesh.setColorAt(index, color.setRGB(clumps[at + 5]!, clumps[at + 6]!, clumps[at + 7]!, THREE.LinearSRGBColorSpace));
    }
    mesh.computeBoundingSphere();
    // The field widens its far clumps in the vertex stage.
    if (mesh.boundingSphere) { mesh.boundingSphere.radius += 1; }
    const size = this.#source.cellMetres;
    this.#cells.set(`${column},${row}`, { mesh, centre: [(column + 0.5) * size, (row + 0.5) * size], clumps: count });
    this.root.add(mesh);
    this.#changed = true;
  }

  /** The drawn root, and whether its cells changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: [this.root], changed };
  }

  /** Acceptance state: resident cells and the clumps drawn this frame. */
  observe(): { cells: number; drawn: number } {
    let drawn = 0;
    for (const cell of this.#cells.values()) { drawn += cell?.mesh.count ?? 0; }
    return { cells: this.#cells.size, drawn };
  }

  dispose(): void {
    for (const cell of this.#cells.values()) { cell?.mesh.dispose(); }
    this.#cells.clear();
    this.#geometry.dispose();
    this.#material.dispose();
  }
}
