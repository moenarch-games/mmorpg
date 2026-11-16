import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { WorldDetail } from "../settings/preferences";

/** Where grass grows, from the WASM owner (`mmorpg_presentation::grass`). */
export type GrassSource = {
  /** The clumps of cell `(column, row)`: position (m), yaw, height (m), linear RGB, wildflower; any prefix thins evenly. */
  cell(column: number, row: number): Float32Array;
  /** The side of one cell, in metres. */
  cellMetres: number;
};

const CLUMP_FLOATS = 9;
/** `mmorpg_presentation::grass::FLOWERS` (sRGB): buttercup, daisy, harebell and poppy, for flower 1–4. */
const FLOWERS = [0xf2cf3a, 0xf3efe2, 0x8f7ad6, 0xd9493a] as const;
const BUILDS_PER_FRAME = 3;

/**
 * The field at each world detail (`graphics.world_detail`; `low` draws none). A `density` share of a cell's clumps is drawn within
 * `full` metres of the eye; beyond, the cells thin out until the field sinks away at `fade`. Cells within `near` metres
 * draw the full blade clump; further ones the light one, which costs about a fifth of it.
 */
const PROFILES = {
  medium: { full: 6, fade: 30, near: 8, falloff: 2.4, density: 0.6 },
  high: { full: 10, fade: 44, near: 12, falloff: 2.2, density: 1 },
} as const;
type Profile = (typeof PROFILES)[keyof typeof PROFILES];

/**
 * One clump of thin, tapered, curved blades, a metre tall and about 0.5 m across: `uv.y` runs up each blade. `rows`
 * are the heights each blade bends at below its tip; the light clump has fewer, wider blades of one bend.
 */
export function clumpGeometry(blades: number, rows: readonly number[], widen = 1): THREE.BufferGeometry {
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
  for (let blade = 0; blade < blades; blade += 1) {
    const reach = Math.sqrt((blade + 0.5) / blades) * 0.24;
    const around = blade * 2.399963 + random() * 0.4;
    const root = [Math.sin(around) * reach, Math.cos(around) * reach];
    const facing = random() * Math.PI * 2;
    const across = [Math.cos(facing), -Math.sin(facing)];
    const lean = facing + (random() - 0.5) * 1.2;
    const leanBy = 0.12 + random() * 0.28;
    const height = 0.55 + random() * 0.45;
    const width = (0.022 + random() * 0.014) * widen;
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
    for (const t of rows) { point(t, -1); point(t, 1); }
    point(1, 0);
    for (let row = 0; row < rows.length - 1; row += 1) {
      const a = first + row * 2;
      indices.push(a, a + 1, a + 3, a, a + 3, a + 2);
    }
    const top = first + (rows.length - 1) * 2;
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
 * Five five-petalled heads just above a clump's blade tips (a clump is a metre tall before its height scales it), each
 * cupped slightly upward with a darker heart; `uv.y` is 1, so they sway with the blade tips.
 */
export function flowerGeometry(): THREE.BufferGeometry {
  const positions: number[] = [];
  const colors: number[] = [];
  const indices: number[] = [];
  const HEADS: readonly [number, number, number, number][] = [
    [0.05, 0.02, 1.08, 0.075], [-0.1, 0.07, 1.0, 0.065], [0.03, -0.12, 1.04, 0.07], [0.13, -0.04, 0.96, 0.06], [-0.06, -0.05, 1.12, 0.07],
  ];
  for (const [x, z, y, radius] of HEADS) {
    const heart = positions.length / 3;
    positions.push(x, y + 0.01, z);
    colors.push(0.55, 0.45, 0.2, 1);
    for (let petal = 0; petal < 10; petal += 1) {
      // Petal tips and the notches between them, alternately.
      const angle = (petal / 10) * Math.PI * 2;
      const reach = petal % 2 === 0 ? radius : radius * 0.45;
      positions.push(x + Math.sin(angle) * reach, y + (petal % 2 === 0 ? 0.012 : 0), z + Math.cos(angle) * reach);
      colors.push(1, 1, 1, 1);
    }
    for (let petal = 0; petal < 10; petal += 1) {
      indices.push(heart, heart + 1 + petal, heart + 1 + ((petal + 1) % 10));
    }
  }
  const count = positions.length / 3;
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(Array.from({ length: count }, () => [0, 1, 0]).flat(), 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 4));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute(Array.from({ length: count }, () => [0.5, 1]).flat(), 2));
  geometry.setIndex(indices);
  geometry.computeBoundingSphere();
  return geometry;
}

type Uniforms = {
  grassTime: { value: number };
  grassEye: { value: THREE.Vector3 };
  grassPush: { value: THREE.Vector3 };
  /** The profile's `full` and `fade` distances. */
  grassReach: { value: THREE.Vector2 };
};

/**
 * The blades' material: render-engine's lit material with its vertex stage bent by the wind, pushed aside around the
 * player's feet, spread wider with distance and sunk away at the field's edge.
 */
function grassMaterial(uniforms: Uniforms): LitMaterial {
  const material = new LitMaterial({ perceptualRoughness: 0.82, metallic: 0, reflectance: 0.35 });
  material.side = THREE.DoubleSide;
  Object.assign(material.uniforms, uniforms);
  const anchor = "vec4 world = model * vec4(position, 1.0);";
  if (!material.vertexShader.includes(anchor)) { throw new Error("The lit vertex stage has no world position to bend"); }
  material.vertexShader = material.vertexShader
    .replace("void main() {", "uniform float grassTime;\nuniform vec3 grassEye;\nuniform vec3 grassPush;\nuniform vec2 grassReach;\nvoid main() {")
    .replace(anchor, `${anchor}
    {
        vec3 origin = (model * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
        float d = distance(origin.xz, grassEye.xz);
        float far = clamp((d - grassReach.x) / (grassReach.y - grassReach.x), 0.0, 1.0);
        world.xz = origin.xz + (world.xz - origin.xz) * (1.0 + 1.4 * far);
        world.y = origin.y + (world.y - origin.y) * (1.0 - smoothstep(grassReach.y - 8.0, grassReach.y, d));
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

/** One cell's clumps as the full and the light clump, sharing their instances; one of them draws. */
/** `flowers` draws the cell's flowering clumps; `flowering[i]` is the clump index of its i-th, ascending. */
type Cell = { full: THREE.InstancedMesh; light: THREE.InstancedMesh; flowers: THREE.InstancedMesh | null; flowering: number[]; centre: [number, number]; clumps: number };

/**
 * Grass blades over the ground around the camera (presentation only): clumps of thin blades on the cells the shared
 * owner grows them in, every clump drawn near the eye and fewer, wider, lighter ones further out, as far as the world
 * detail setting reaches. At `low` it draws nothing and keeps no cells.
 */
export class Grass {
  readonly root = new THREE.Group();
  readonly #source: GrassSource;
  /** 15 blades of five triangles near the eye; 7 wider blades of three further out. */
  readonly #full = clumpGeometry(15, [0, 0.4, 0.75]);
  readonly #light = clumpGeometry(7, [0, 0.5], 1.5);
  readonly #flower = flowerGeometry();
  readonly #uniforms: Uniforms = {
    grassTime: { value: 0 }, grassEye: { value: new THREE.Vector3() }, grassPush: { value: new THREE.Vector3(0, -1e4, 0) },
    grassReach: { value: new THREE.Vector2(PROFILES.medium.full, PROFILES.medium.fade) },
  };
  readonly #material = grassMaterial(this.#uniforms);
  readonly #cells = new Map<string, Cell | null>();
  #profile: Profile | null = PROFILES.medium;
  #changed = true;

  constructor(source: GrassSource, detail: WorldDetail = "medium") {
    this.#source = source;
    this.root.name = "greyhaven.grass";
    this.setDetail(detail);
  }

  /** Applies the world detail setting: `low` drops every cell, another level reaches as far as its profile. */
  setDetail(detail: WorldDetail): void {
    const profile = detail === "low" ? null : PROFILES[detail];
    if (profile === this.#profile) { return; }
    this.#profile = profile;
    if (profile) { this.#uniforms.grassReach.value.set(profile.full, profile.fade); }
    if (!profile) { this.#clear(); }
    this.#changed = true;
  }

  /** Grows and drops cells around `eye`, thins them by distance and moves the wind; `feet` push the blades aside. */
  update(eye: readonly [number, number, number], feet: readonly [number, number, number] | null, seconds: number): void {
    const profile = this.#profile;
    if (!profile) { return; }
    this.#uniforms.grassTime.value = seconds;
    this.#uniforms.grassEye.value.set(eye[0], eye[1], eye[2]);
    if (feet) { this.#uniforms.grassPush.value.set(feet[0], feet[1], feet[2]); }
    const size = this.#source.cellMetres;
    const keep = profile.fade + 6;
    const distance = (column: number, row: number) => {
      const dx = Math.max(column * size - eye[0], 0, eye[0] - (column + 1) * size);
      const dz = Math.max(row * size - eye[2], 0, eye[2] - (row + 1) * size);
      return Math.hypot(dx, dz);
    };
    for (const [key, cell] of this.#cells) {
      const [column, row] = key.split(",").map(Number) as [number, number];
      if (distance(column, row) > keep + size * 1.5) {
        this.#drop(cell);
        this.#cells.delete(key);
      }
    }
    const reach = Math.ceil(keep / size);
    const [ec, er] = [Math.floor(eye[0] / size), Math.floor(eye[2] / size)];
    const wanted: [number, number, number][] = [];
    for (let row = er - reach; row <= er + reach; row += 1) {
      for (let column = ec - reach; column <= ec + reach; column += 1) {
        const d = distance(column, row);
        if (d <= keep && !this.#cells.has(`${column},${row}`)) { wanted.push([d, column, row]); }
      }
    }
    wanted.sort((a, b) => a[0] - b[0]);
    for (const [, column, row] of wanted.slice(0, BUILDS_PER_FRAME)) { this.#build(column, row); }
    for (const [key, cell] of this.#cells) {
      if (!cell) { continue; }
      const [column, row] = key.split(",").map(Number) as [number, number];
      const d = distance(column, row);
      const share = d <= profile.full ? 1 : Math.max(0, 1 - (d - profile.full) / (profile.fade - profile.full)) ** profile.falloff;
      const count = d > profile.fade ? 0 : Math.max(1, Math.ceil(cell.clumps * profile.density * Math.max(share, 0.05)));
      const nearby = d < profile.near;
      for (const [mesh, drawn] of [[cell.full, nearby], [cell.light, !nearby]] as const) {
        const visible = drawn && count > 0;
        if (mesh.visible !== visible) { mesh.visible = visible; }
        if (visible && mesh.count !== count) { mesh.count = count; }
      }
      if (cell.flowers) {
        // The flowering clumps among the first `count`, which are the drawn ones.
        let flowers = 0;
        while (flowers < cell.flowering.length && cell.flowering[flowers]! < count) { flowers += 1; }
        const visible = flowers > 0;
        if (cell.flowers.visible !== visible) { cell.flowers.visible = visible; }
        if (visible && cell.flowers.count !== flowers) { cell.flowers.count = flowers; }
      }
    }
  }

  #build(column: number, row: number): void {
    const clumps = this.#source.cell(column, row);
    const count = clumps.length / CLUMP_FLOATS;
    if (count === 0) {
      this.#cells.set(`${column},${row}`, null);
      return;
    }
    const full = new THREE.InstancedMesh(this.#full, this.#material, count);
    const matrix = new THREE.Matrix4();
    const rotation = new THREE.Matrix4();
    const color = new THREE.Color();
    for (let index = 0; index < count; index += 1) {
      const at = index * CLUMP_FLOATS;
      matrix.makeScale(1, clumps[at + 4]!, 1).premultiply(rotation.makeRotationY(clumps[at + 3]!));
      matrix.setPosition(clumps[at]!, clumps[at + 1]! - 0.03, clumps[at + 2]!);
      full.setMatrixAt(index, matrix);
      full.setColorAt(index, color.setRGB(clumps[at + 5]!, clumps[at + 6]!, clumps[at + 7]!, THREE.LinearSRGBColorSpace));
    }
    const light = new THREE.InstancedMesh(this.#light, this.#material, count);
    light.instanceMatrix = full.instanceMatrix;
    light.instanceColor = full.instanceColor;
    const flowering: number[] = [];
    for (let index = 0; index < count; index += 1) {
      if (clumps[index * CLUMP_FLOATS + 8]! > 0) { flowering.push(index); }
    }
    let flowers: THREE.InstancedMesh | null = null;
    if (flowering.length > 0) {
      flowers = new THREE.InstancedMesh(this.#flower, this.#material, flowering.length);
      flowering.forEach((index, slot) => {
        const at = index * CLUMP_FLOATS;
        full.getMatrixAt(index, matrix);
        flowers!.setMatrixAt(slot, matrix);
        const hue = FLOWERS[Math.min(FLOWERS.length, Math.max(1, clumps[at + 8]!)) - 1]!;
        flowers!.setColorAt(slot, color.setHex(hue, THREE.SRGBColorSpace));
      });
    }
    for (const mesh of flowers ? [full, light, flowers] : [full, light]) {
      mesh.name = `grass:${column},${row}`;
      mesh.userData.notShadowCaster = true;
      mesh.visible = false;
      mesh.computeBoundingSphere();
      // The field widens its far clumps in the vertex stage.
      if (mesh.boundingSphere) { mesh.boundingSphere.radius += 1; }
      this.root.add(mesh);
    }
    const size = this.#source.cellMetres;
    this.#cells.set(`${column},${row}`, { full, light, flowers, flowering, centre: [(column + 0.5) * size, (row + 0.5) * size], clumps: count });
    this.#changed = true;
  }

  #drop(cell: Cell | null): void {
    if (!cell) { return; }
    for (const mesh of [cell.full, cell.light, ...(cell.flowers ? [cell.flowers] : [])]) {
      this.root.remove(mesh);
      mesh.dispose();
    }
    this.#changed = true;
  }

  #clear(): void {
    for (const cell of this.#cells.values()) { this.#drop(cell); }
    this.#cells.clear();
  }

  /** The drawn root, and whether its cells changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: [this.root], changed };
  }

  /** Acceptance state: resident cells, the clumps drawn this frame, their triangles (before frustum culling) and flowers. */
  observe(): { cells: number; drawn: number; triangles: number; flowers: number } {
    let drawn = 0;
    let triangles = 0;
    let flowers = 0;
    for (const cell of this.#cells.values()) {
      for (const mesh of cell ? [cell.full, cell.light, ...(cell.flowers ? [cell.flowers] : [])] : []) {
        if (!mesh.visible) { continue; }
        drawn += mesh.count;
        if (mesh === cell!.flowers) { flowers += mesh.count; }
        triangles += mesh.count * mesh.geometry.index!.count / 3;
      }
    }
    return { cells: this.#cells.size, drawn, triangles, flowers };
  }

  dispose(): void {
    this.#clear();
    this.#full.dispose();
    this.#light.dispose();
    this.#flower.dispose();
    this.#material.dispose();
  }
}
