import * as THREE from "three";
import { LitMaterial } from "@moritzbrantner/three-lighting";
import type { WorldDetail } from "../settings/preferences";

/** Where grass grows, from the WASM owner (`mmorpg_presentation::grass`). */
export type GrassSource = {
  /** The clumps of cell `(column, row)`: position (m), yaw, height (m), linear RGB, wildflower; any prefix thins evenly. */
  cell(column: number, row: number): Float32Array;
  /** The side of one cell, in metres. */
  cellMetres: number;
  /** The sparse far clumps of far cell `(column, row)`, laid out as `cell`'s; without them the field ends at the near one. */
  farCell?(column: number, row: number): Float32Array;
  farCellMetres?: number;
};

const CLUMP_FLOATS = 9;
/** `mmorpg_presentation::grass::FLOWERS` (sRGB): buttercup, daisy, harebell and poppy, for flower 1–4. */
const FLOWERS = [0xf2cf3a, 0xf3efe2, 0x8f7ad6, 0xd9493a] as const;
const BUILDS_PER_FRAME = 3;
/** How far either side of a profile's `swap` distance a clump changes from the full to the light clump. */
const SWAP_JITTER = 3;
/** Half-width of the dithered full/light cross-fade, in metres; both LODs contribute in this band. */
const SWAP_BLEND = 4;
/** The share of the rank range over which a clump dissolves in as its distance's share passes its rank. */
const GROW = 0.08;
/** How much wider than a near clump a far clump spreads, which stands for about ten of them. */
const FAR_SPREAD = 2;
/**
 * The flat shade of a distant blade over its clump's colour (#871): about the ground's own tone under it, so the field
 * thins into the turf rather than leaving darker or paler clumps standing on it.
 */
const FLAT_SHADE = 0.72;

/**
 * The field at each world detail (`graphics.world_detail`; `low` draws none). A `density` share of the near clumps is
 * drawn within `full` metres of the eye; beyond, they thin out to none at `fade`, each clump dissolving on
 * its own as its distance's share passes its rank, so no line shows where the field ends. Within `swap` metres a clump
 * has all its blades; beyond, the light clump. The far field's sparse wide clumps grow in from `farFrom` as the near
 * one thins, at most a `farDensity` share of them, and thin out again to none at `farTo`.
 */
const PROFILES = {
  medium: { full: 8, fade: 36, swap: 12, falloff: 1.1, density: 0.6, farFrom: 14, farTo: 92, farDensity: 0.52 },
  high: { full: 12, fade: 50, swap: 17, falloff: 1.2, density: 1, farFrom: 20, farTo: 120, farDensity: 0.62 },
} as const;
type Profile = (typeof PROFILES)[keyof typeof PROFILES];

/**
 * How a layer thins by distance `d`: `density × in(d) × out(d)`, rising linearly over `[a0, a1]` (none when `a1 <= a0`)
 * and falling over `[b0, b1]` with exponent `p`. The vertex stage evaluates the same per clump.
 */
type Share = { density: number; p: number; a0: number; a1: number; b0: number; b1: number };

function share({ density, p, a0, a1, b0, b1 }: Share, d: number): number {
  const rise = a1 > a0 ? Math.min(Math.max((d - a0) / (a1 - a0), 0), 1) : 1;
  const fall = (1 - Math.min(Math.max((d - b0) / (b1 - b0), 0), 1)) ** p;
  return density * rise * fall;
}

/** The most `s` reaches between `near` and `far` metres: at either end or where it levels. */
function mostShare(s: Share, near: number, far: number): number {
  return Math.max(...[near, far, s.a1, s.b0, (near + far) / 2].map((d) => share(s, Math.min(Math.max(d, near), far))));
}

/**
 * One clump of thin, tapered, curved blades, a metre tall and about 0.5 m across: `uv.y` runs up each blade. `rows`
 * are the heights each blade bends at below its tip; the light clump has fewer, wider blades of one bend, the far
 * clump a single triangle a blade.
 */
export function clumpGeometry(blades: number, rows: readonly number[], widen = 1, tone: readonly [number, number] = [0.34, 1]): THREE.BufferGeometry {
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
    // Each blade its own shade and hue, some fresher, some drier, so a clump never reads as one stamped colour.
    const dry = random() - 0.5;
    const light = 0.86 + random() * 0.24;
    const hue = [light * (1 + 0.22 * dry), light * (1 + 0.04 * (random() - 0.5)), light * (1 - 0.3 * dry)];
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
      const shade = tone[0] + (tone[1] - tone[0]) * t ** 0.8;
      colors.push(shade * hue[0]!, shade * hue[1]!, shade * hue[2]! * (0.97 - 0.06 * t), 1);
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
 * A clump's flowering plants (#868): `heads` small flowers, each on a thin bowed stem with a leaf, rising from among the
 * blades to about their tips (a clump is a metre tall before its height scales it). Every head differs: five or six
 * rounded petals cupped round a raised heart, its own size, spin and tilt (some nod), so a drift reads as plants, not
 * stamped stars. `flowerPart` is 0 on the stem and leaf (drawn in the clump's own green, `flowerStem`) and 1 on the head
 * (in the instance's flower colour, petals paling to their tips); `flowerHead` is the head's centre, about which the
 * vertex stage keeps the head its true shape however short the clump. `uv.y` runs up the stem, so heads sway with the
 * blade tips. The far clump has fewer, larger heads (`grow` times the radius) on plain stems.
 */
export function flowerGeometry(heads = 4, grow = 1, stemRows: readonly number[] = [0, 0.5, 1]): THREE.BufferGeometry {
  const positions: number[] = [];
  const normals: number[] = [];
  const colors: number[] = [];
  const uvs: number[] = [];
  const parts: number[] = [];
  const centres: number[] = [];
  const indices: number[] = [];
  // A fixed sequence, so every flowering clump is the same mesh.
  let seed = 11;
  const random = () => {
    seed = (seed * 16_807) % 2_147_483_647;
    return (seed - 1) / 2_147_483_646;
  };
  const vertex = (p: THREE.Vector3, n: THREE.Vector3, shade: readonly number[], v: number, part: number, centre: THREE.Vector3) => {
    positions.push(p.x, p.y, p.z);
    normals.push(n.x, n.y, n.z);
    colors.push(shade[0]!, shade[1]!, shade[2]!, 1);
    uvs.push(0.5, v);
    parts.push(part);
    centres.push(centre.x, centre.y, centre.z);
    return positions.length / 3 - 1;
  };
  const up = new THREE.Vector3(0, 1, 0);
  for (let head = 0; head < heads; head += 1) {
    const reach = Math.sqrt((head + 0.5) / heads) * 0.16;
    const around = head * 2.399963 + random() * 0.5;
    const root = new THREE.Vector3(Math.sin(around) * reach, 0, Math.cos(around) * reach);
    const leanAt = random() * Math.PI * 2;
    const leanBy = 0.02 + random() * 0.05;
    const centre = new THREE.Vector3(root.x + Math.sin(leanAt) * leanBy, 0.72 + random() * 0.42, root.z + Math.cos(leanAt) * leanBy);
    // The stem: a tapered ribbon bowing towards its lean, darker at the root like a blade.
    const across = new THREE.Vector3(Math.cos(leanAt), 0, -Math.sin(leanAt));
    const stemNormal = new THREE.Vector3(Math.sin(leanAt) * 0.35, 1, Math.cos(leanAt) * 0.35).normalize();
    const stemAt = (t: number) => new THREE.Vector3(
      root.x + (centre.x - root.x) * t ** 1.8, centre.y * t, root.z + (centre.z - root.z) * t ** 1.8,
    );
    const stemShade = (t: number) => { const s = 0.4 + 0.45 * t; return [s, s, s * 0.9]; };
    const first = positions.length / 3;
    for (const t of stemRows) {
      const width = 0.008 * (1 - 0.5 * t);
      for (const side of [-1, 1]) {
        vertex(stemAt(t).addScaledVector(across, width * side), stemNormal, stemShade(t), t, 0, stemAt(t));
      }
    }
    for (let row = 0; row < stemRows.length - 1; row += 1) {
      const a = first + row * 2;
      indices.push(a, a + 1, a + 3, a, a + 3, a + 2);
    }
    // A leaf off the lower stem, pointing away from the lean.
    if (stemRows.length > 2) {
      const t = 0.3 + random() * 0.15;
      const base = stemAt(t);
      const out = new THREE.Vector3(-Math.sin(leanAt + 0.6), 0, -Math.cos(leanAt + 0.6));
      const leaf = [
        vertex(base.clone().addScaledVector(across, -0.008), stemNormal, stemShade(t), t, 0, base),
        vertex(base.clone().addScaledVector(across, 0.008), stemNormal, stemShade(t), t, 0, base),
        vertex(base.clone().addScaledVector(out, 0.06).setY(base.y + 0.1), stemNormal, stemShade(t + 0.2), t + 0.1, 0, base),
      ];
      indices.push(leaf[0]!, leaf[1]!, leaf[2]!);
    }
    // The head, in metres about its centre: a shallow cup of rounded petals spun and tilted as a whole.
    const radius = (0.035 + random() * 0.025) * grow;
    const cup = 0.25 + random() * 0.45;
    const petals = random() < 0.35 ? 6 : 5;
    const tilt = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(Math.cos(leanAt), 0, -Math.sin(leanAt)), 0.15 + random() * 0.65,
    ).multiply(new THREE.Quaternion().setFromAxisAngle(up, random() * Math.PI * 2));
    const at = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z).applyQuaternion(tilt).add(centre);
    const facing = (radial: number, angle: number) =>
      new THREE.Vector3(-Math.sin(angle) * radial, 1, -Math.cos(angle) * radial).normalize().applyQuaternion(tilt);
    const half = (Math.PI / petals) * 0.82;
    for (let petal = 0; petal < petals; petal += 1) {
      const angle = (petal / petals) * Math.PI * 2;
      const ring = (share: number, spread: number, rise: number, shade: number) => {
        const a = angle + spread;
        return vertex(
          at(Math.sin(a) * radius * share, radius * cup * rise, Math.cos(a) * radius * share),
          facing(cup * 0.9, angle), [shade, shade, shade], 1, 1, centre,
        );
      };
      const base = ring(0.15, 0, 0, 0.72);
      const edge = [ring(0.7, -half, 0.55, 0.92), ring(0.98, -half * 0.45, 1, 1), ring(0.98, half * 0.45, 1, 1), ring(0.7, half, 0.55, 0.92)];
      for (let k = 0; k < 3; k += 1) { indices.push(base, edge[k]!, edge[k + 1]!); }
    }
    // The heart: a small raised boss over the petals' bases, warm-toned.
    const heart = vertex(at(0, radius * 0.22, 0), facing(0, 0), [1, 0.8, 0.32], 1, 1, centre);
    const boss: number[] = [];
    for (let k = 0; k < 5; k += 1) {
      const a = (k / 5) * Math.PI * 2;
      boss.push(vertex(at(Math.sin(a) * radius * 0.28, radius * 0.1, Math.cos(a) * radius * 0.28), facing(-0.6, a), [0.85, 0.65, 0.25], 1, 1, centre));
    }
    for (let k = 0; k < 5; k += 1) { indices.push(heart, boss[k]!, boss[(k + 1) % 5]!); }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  geometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 4));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setAttribute("flowerPart", new THREE.Float32BufferAttribute(parts, 1));
  geometry.setAttribute("flowerHead", new THREE.Float32BufferAttribute(centres, 3));
  geometry.setIndex(indices);
  geometry.computeBoundingSphere();
  return geometry;
}

/** The uniforms every grass material shares. */
type Shared = {
  grassTime: { value: number };
  grassEye: { value: THREE.Vector3 };
  grassPush: { value: THREE.Vector3 };
};

/** One material's own: its layer's thinning, its band of the full/light swap and how far out it spreads. */
type Own = {
  /** `[a0, a1, b0, b1]` of its layer's `Share`. */
  grassRamp: { value: THREE.Vector4 };
  /** Its layer's `density` and `p`, and the band `[lo, hi)` of (jittered) distances it draws a clump in. */
  grassMix: { value: THREE.Vector4 };
  /** Spreads its clumps up to `1 + z` times as wide from `x` to `y` metres. */
  grassSpread: { value: THREE.Vector3 };
  /** Flattens its blades towards their clump's colour, by up to `z`, from `x` to `y` metres. */
  grassFlat: { value: THREE.Vector3 };
};

/**
 * The blades' material: render-engine's lit material with its vertex stage bent by the wind, pushed aside around the
 * player's feet, spread wider with distance and grown from or sunk into the ground clump by clump by its layer's share.
 * Every clump carries its rank in its cell's random order (`grassRank`). A `flower` material draws `flowerGeometry`.
 */
function grassMaterial(shared: Shared, flower = false): { material: LitMaterial; own: Own } {
  const material = new LitMaterial({ perceptualRoughness: 0.82, metallic: 0, reflectance: 0.35 });
  material.side = THREE.DoubleSide;
  const own: Own = {
    grassRamp: { value: new THREE.Vector4() },
    grassMix: { value: new THREE.Vector4() },
    grassSpread: { value: new THREE.Vector3() },
    grassFlat: { value: new THREE.Vector3() },
  };
  Object.assign(material.uniforms, shared, own);
  const anchor = "vec4 world = model * vec4(position, 1.0);";
  if (!material.vertexShader.includes(anchor)) { throw new Error("The lit vertex stage has no world position to bend"); }
  material.vertexShader = material.vertexShader
    .replace("void main() {", `uniform float grassTime;
uniform vec3 grassEye;
uniform vec3 grassPush;
uniform vec4 grassRamp;
uniform vec4 grassMix;
uniform vec3 grassSpread;
uniform vec3 grassFlat;
in float grassRank;
out float grassShown;
void main() {
    float grassFlatten = 0.0;`)
    .replace(anchor, `${anchor}
    {
        vec3 origin = (model * vec4(0.0, 0.0, 0.0, 1.0)).xyz;
        float d = distance(origin.xz, grassEye.xz);
        float rise = grassRamp.y > grassRamp.x ? clamp((d - grassRamp.x) / (grassRamp.y - grassRamp.x), 0.0, 1.0) : 1.0;
        float share = grassMix.x * rise * pow(1.0 - clamp((d - grassRamp.z) / (grassRamp.w - grassRamp.z), 0.0, 1.0), grassMix.y);
        float jittered = d + (fract(grassRank * 61.7) * 2.0 - 1.0) * ${SWAP_JITTER.toFixed(1)};
        // The full and light clumps overlap smoothly across the jittered LOD boundary.
        // An open bound (-/+1e4) keeps flowers and the independent far layer fully in-band.
        float band = smoothstep(grassMix.z - ${SWAP_BLEND.toFixed(1)}, grassMix.z + ${SWAP_BLEND.toFixed(1)}, jittered)
            * (1.0 - smoothstep(grassMix.w - ${SWAP_BLEND.toFixed(1)}, grassMix.w + ${SWAP_BLEND.toFixed(1)}, jittered));
        float shown = clamp((share - grassRank) / ${GROW.toFixed(2)}, 0.0, 1.0) * band;
        float spread = 1.0 + grassSpread.z * clamp((d - grassSpread.x) / (grassSpread.y - grassSpread.x), 0.0, 1.0);
        world.xz = origin.xz + (world.xz - origin.xz) * spread;
        grassShown = shown;
        grassFlatten = smoothstep(grassFlat.x, grassFlat.y, d) * grassFlat.z;
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
  // A clump never grows from or sinks into the ground with its share, which raised and lowered the field as the eye
  // moved (#868, #871): it keeps its height and dissolves in and out in a screen-door dither instead.
  const sampled = "MaterialSample m = sampleMaterial();";
  if (!material.fragmentShader.includes(sampled)) { throw new Error("The lit fragment stage has no material sample to dither after"); }
  material.fragmentShader = material.fragmentShader
    .replace("void main() {", "in float grassShown;\nvoid main() {")
    .replace(sampled, `${sampled}
    if (grassShown < 1.0 && grassShown <= fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))))) {
        discard;
    }`);
  const projected = "gl_Position = projectionMatrix";
  if (!material.vertexShader.includes(projected)) { throw new Error("The lit vertex stage has no projection to colour before"); }
  if (flower) {
    // A head keeps its shape however short its clump; the stem and leaf take the clump's own green.
    material.vertexShader = material.vertexShader
      .replace("in float grassRank;", "in float grassRank;\nin float flowerPart;\nin vec3 flowerHead;\nin vec3 flowerStem;")
      .replace(anchor, `float flowerKeep = length(model[0].xyz) / length(model[1].xyz);
    vec4 world = model * vec4(mix(position, flowerHead + (position - flowerHead) * vec3(1.0, flowerKeep, 1.0), flowerPart), 1.0);`)
      .replace("vec3 n = normal;", "vec3 n = normal * mix(vec3(1.0), vec3(1.0, 1.0 / flowerKeep, 1.0), flowerPart);")
      .replace(projected, `vColor.rgb = mix(color.rgb * flowerStem, vColor.rgb, flowerPart);\n    ${projected}`);
  } else {
    // With distance a blade loses its root-to-tip shading and takes its clump's flat colour, the turf's tone, so a far
    // or thinning clump melts into the ground under it instead of standing on it as a dark shard.
    material.vertexShader = material.vertexShader
      .replace(projected, `vColor.rgb = mix(vColor.rgb, instanceColor * ${FLAT_SHADE.toFixed(2)}, grassFlatten);\n    ${projected}`);
  }
  return { material, own };
}

/** A drawn kind of clump: its geometry, its material and that material's own uniforms. */
type Kind = { geometry: THREE.BufferGeometry; material: LitMaterial; own: Own };

/**
 * One cell's clumps: per kind of clump drawn from them a mesh, the blade kinds sharing their instances; `flowers` draws
 * its flowering clumps, `flowering[i]` the clump index of its i-th, ascending.
 */
type Cell = { meshes: { kind: Kind; mesh: THREE.InstancedMesh; blades: boolean }[]; flowering: number[]; clumps: number };

/** A layer of cells: the near field's or the far one's. */
type Layer = { size: number; cells: Map<string, Cell | null>; build: (column: number, row: number) => Float32Array };

/**
 * Grass blades around the character (or camera without one), presentation only: clumps on the cells the shared
 * owner grows them in, every clump drawn near the eye and fewer, wider, lighter ones further out, then the far field's
 * sparse wide clumps as far as the world detail setting reaches. At `low` it draws nothing and keeps no cells.
 */
export class Grass {
  readonly root = new THREE.Group();
  readonly #shared: Shared = {
    grassTime: { value: 0 }, grassEye: { value: new THREE.Vector3() }, grassPush: { value: new THREE.Vector3(0, -1e4, 0) },
  };
  /** 15 blades of five triangles near the eye, 7 wider blades of three beyond `swap`, and 8 of one in the far field. */
  readonly #full: Kind;
  readonly #light: Kind;
  readonly #flowers: Kind;
  readonly #far: Kind;
  readonly #farFlowers: Kind;
  readonly #near: Layer;
  readonly #distant: Layer | null;
  #profile: Profile | null = null;
  #changed = true;

  constructor(source: GrassSource, detail: WorldDetail = "medium") {
    this.root.name = "greyhaven.grass";
    const kind = (geometry: THREE.BufferGeometry, flower = false): Kind => ({ geometry, ...grassMaterial(this.#shared, flower) });
    this.#full = kind(clumpGeometry(15, [0, 0.4, 0.75], 1, [0.4, 1]));
    // The simplified blades retain the same per-clump colour gradient as the full blades during LOD overlap.
    this.#light = kind(clumpGeometry(7, [0, 0.5], 1.5, [0.4, 1]));
    this.#flowers = kind(flowerGeometry(), true);
    // The far layer samples the same terrain tint and blade shade as the near layer; distance flattening fades it into turf.
    this.#far = kind(clumpGeometry(8, [0], 1.4, [0.4, 1]));
    this.#farFlowers = kind(flowerGeometry(3, 1.2, [0, 1]), true);
    this.#near = { size: source.cellMetres, cells: new Map(), build: (column, row) => source.cell(column, row) };
    const { farCell, farCellMetres } = source;
    this.#distant = farCell && farCellMetres ? { size: farCellMetres, cells: new Map(), build: (column, row) => farCell(column, row) } : null;
    this.setDetail(detail);
  }

  /** The near and far layers' thinning under `profile`. */
  static #shares(profile: Profile): { near: Share; far: Share } {
    return {
      near: { density: profile.density, p: profile.falloff, a0: 0, a1: 0, b0: profile.full, b1: profile.fade },
      far: { density: profile.farDensity, p: 1.2, a0: profile.farFrom, a1: profile.fade, b0: profile.fade + 8, b1: profile.farTo },
    };
  }

  /** Applies the world detail setting: `low` drops every cell, another level reaches as far as its profile. */
  setDetail(detail: WorldDetail): void {
    const profile = detail === "low" ? null : PROFILES[detail];
    if (profile === this.#profile) { return; }
    this.#profile = profile;
    if (!profile) {
      this.#clear();
    } else {
      const { near, far } = Grass.#shares(profile);
      const set = (kind: Kind, s: Share, band: [number, number], spread: [number, number, number], flat: [number, number, number] = [0, 1, 0]) => {
        kind.own.grassFlat.value.set(...flat);
        kind.own.grassRamp.value.set(s.a0, s.a1, s.b0, s.b1);
        kind.own.grassMix.value.set(s.density, s.p, band[0], band[1]);
        kind.own.grassSpread.value.set(...spread);
      };
      const everywhere: [number, number] = [-1e4, 1e4];
      const nearSpread: [number, number, number] = [profile.full, profile.fade, 0.5];
      const nearFlat: [number, number, number] = [profile.full, profile.fade, 0.75];
      set(this.#full, near, [-1e4, profile.swap], nearSpread, nearFlat);
      set(this.#light, near, [profile.swap, 1e4], nearSpread, nearFlat);
      set(this.#flowers, near, everywhere, nearSpread);
      set(this.#far, far, everywhere, [profile.fade, profile.farTo, 0.3], [profile.farFrom, profile.fade + 10, 0.9]);
      set(this.#farFlowers, far, everywhere, [profile.fade, profile.farTo, 0.3]);
    }
    this.#changed = true;
  }

  /** Grows and drops cells around the player (or eye without one), thins them and moves the wind; `feet` push blades aside. */
  update(eye: readonly [number, number, number], feet: readonly [number, number, number] | null, seconds: number): void {
    const profile = this.#profile;
    if (!profile) { return; }
    this.#shared.grassTime.value = seconds;
    // Orbiting the camera must not move the vegetation-density field across the player's feet.
    // Use the character as the stable reference, falling back to the eye for views without one.
    const anchor = feet ?? eye;
    this.#shared.grassEye.value.set(anchor[0], anchor[1], anchor[2]);
    if (feet) { this.#shared.grassPush.value.set(feet[0], feet[1], feet[2]); }
    const { near, far } = Grass.#shares(profile);
    let builds = BUILDS_PER_FRAME;
    builds -= this.#stream(this.#near, anchor, profile.fade + 6, near, builds, false);
    if (this.#distant) { this.#stream(this.#distant, anchor, profile.farTo + 8, far, Math.max(builds, 1), true); }
  }

  /** Keeps `layer`'s cells within `keep` metres of `eye`, builds up to `builds` missing ones, nearest first; how many it built. */
  #stream(layer: Layer, eye: readonly [number, number, number], keep: number, thinning: Share, builds: number, far: boolean): number {
    const size = layer.size;
    // The nearest and furthest the cell's ground lies from the eye.
    const range = (column: number, row: number): [number, number] => {
      const dx = Math.max(column * size - eye[0], 0, eye[0] - (column + 1) * size);
      const dz = Math.max(row * size - eye[2], 0, eye[2] - (row + 1) * size);
      const fx = Math.max(Math.abs(column * size - eye[0]), Math.abs((column + 1) * size - eye[0]));
      const fz = Math.max(Math.abs(row * size - eye[2]), Math.abs((row + 1) * size - eye[2]));
      return [Math.sqrt(dx * dx + dz * dz), Math.sqrt(fx * fx + fz * fz)];
    };
    for (const [key, cell] of layer.cells) {
      const [column, row] = key.split(",").map(Number) as [number, number];
      if (range(column, row)[0] > keep + size) {
        this.#drop(cell);
        layer.cells.delete(key);
      }
    }
    const reach = Math.ceil(keep / size);
    const [ec, er] = [Math.floor(eye[0] / size), Math.floor(eye[2] / size)];
    const wanted: [number, number, number][] = [];
    for (let row = er - reach; row <= er + reach; row += 1) {
      for (let column = ec - reach; column <= ec + reach; column += 1) {
        const d = range(column, row)[0];
        if (d <= keep && !layer.cells.has(`${column},${row}`)) { wanted.push([d, column, row]); }
      }
    }
    wanted.sort((a, b) => a[0] - b[0]);
    const built = wanted.slice(0, builds);
    for (const [, column, row] of built) { this.#build(layer, column, row, far); }
    for (const [key, cell] of layer.cells) {
      if (!cell) { continue; }
      const [column, row] = key.split(",").map(Number) as [number, number];
      const [nearest, furthest] = range(column, row);
      // Every clump whose rank its share may pass somewhere on the cell; the vertex stage sinks the rest.
      const most = mostShare(thinning, nearest, furthest);
      const count = most <= 0 ? 0 : Math.min(cell.clumps, Math.ceil(cell.clumps * (most + GROW)));
      const flowers = cell.flowering.findIndex((index) => index >= count);
      const flowering = flowers < 0 ? cell.flowering.length : flowers;
      for (const { kind, mesh, blades } of cell.meshes) {
        const swapped = kind === this.#full ? nearest < this.#profile!.swap + SWAP_JITTER + SWAP_BLEND
          : kind === this.#light ? furthest > this.#profile!.swap - SWAP_JITTER - SWAP_BLEND : true;
        const drawn = blades ? count : flowering;
        const visible = swapped && drawn > 0;
        if (mesh.visible !== visible) { mesh.visible = visible; }
        if (visible && mesh.count !== drawn) { mesh.count = drawn; }
      }
    }
    return built.length;
  }

  #build(layer: Layer, column: number, row: number, far: boolean): void {
    const clumps = layer.build(column, row);
    const count = clumps.length / CLUMP_FLOATS;
    const key = `${column},${row}`;
    if (count === 0) {
      layer.cells.set(key, null);
      return;
    }
    const spread = far ? FAR_SPREAD : 1;
    const matrices = new Float32Array(count * 16);
    const colors = new Float32Array(count * 3);
    const ranks = new Float32Array(count);
    const matrix = new THREE.Matrix4();
    const rotation = new THREE.Matrix4();
    const flowering: number[] = [];
    for (let index = 0; index < count; index += 1) {
      const at = index * CLUMP_FLOATS;
      matrix.makeScale(spread, clumps[at + 4]!, spread).premultiply(rotation.makeRotationY(clumps[at + 3]!));
      matrix.setPosition(clumps[at]!, clumps[at + 1]! - 0.03, clumps[at + 2]!);
      matrix.toArray(matrices, index * 16);
      colors.set([clumps[at + 5]!, clumps[at + 6]!, clumps[at + 7]!], index * 3);
      // The clumps come in a random order: an index's share of them is an even random rank.
      ranks[index] = (index + 0.5) / count;
      if (clumps[at + 8]! > 0) { flowering.push(index); }
    }
    const instanceMatrix = new THREE.InstancedBufferAttribute(matrices, 16);
    const instanceColor = new THREE.InstancedBufferAttribute(colors, 3);
    const rank = new THREE.InstancedBufferAttribute(ranks, 1);
    // A copy of `kind`'s small clump with this cell's ranks: disposing a geometry frees its buffers, which a geometry
    // sharing them would go on binding.
    const ranked = (kind: Kind, ranksOf: THREE.InstancedBufferAttribute) => kind.geometry.clone().setAttribute("grassRank", ranksOf);
    const meshes: Cell["meshes"] = [];
    for (const kind of far ? [this.#far] : [this.#full, this.#light]) {
      const mesh = new THREE.InstancedMesh(ranked(kind, rank), kind.material, count);
      mesh.instanceMatrix = instanceMatrix;
      mesh.instanceColor = instanceColor;
      meshes.push({ kind, mesh, blades: true });
    }
    if (flowering.length > 0) {
      const kind = far ? this.#farFlowers : this.#flowers;
      const flowerRanks = new THREE.InstancedBufferAttribute(Float32Array.from(flowering, (index) => ranks[index]!), 1);
      // The stems' green: the clump's own, a shade deeper.
      const stems = new THREE.InstancedBufferAttribute(new Float32Array(flowering.length * 3), 3);
      const mesh = new THREE.InstancedMesh(ranked(kind, flowerRanks).setAttribute("flowerStem", stems), kind.material, flowering.length);
      const color = new THREE.Color();
      flowering.forEach((index, slot) => {
        const at = index * CLUMP_FLOATS;
        // Each plant its own turn, spread and height round its clump's, so no two drifts repeat one stamp.
        const vary = (salt: number) => {
          const v = Math.sin(clumps[at]! * 12.9898 + clumps[at + 2]! * 78.233 + salt * 37.719) * 43_758.545;
          return v - Math.floor(v);
        };
        const across = spread * (0.8 + 0.4 * vary(1));
        matrix.makeScale(across, clumps[at + 4]! * (0.85 + 0.35 * vary(2)), across)
          .premultiply(rotation.makeRotationY(clumps[at + 3]! + vary(3) * Math.PI * 2));
        matrix.setPosition(clumps[at]!, clumps[at + 1]! - 0.03, clumps[at + 2]!);
        matrix.toArray(mesh.instanceMatrix.array, slot * 16);
        stems.setXYZ(slot, colors[index * 3]! * 0.8, colors[index * 3 + 1]! * 0.8, colors[index * 3 + 2]! * 0.8);
        const hue = FLOWERS[Math.min(FLOWERS.length, Math.max(1, clumps[at + 8]!)) - 1]!;
        mesh.setColorAt(slot, color.setHex(hue, THREE.SRGBColorSpace));
      });
      meshes.push({ kind, mesh, blades: false });
    }
    for (const { mesh } of meshes) {
      mesh.name = `grass:${far ? "far:" : ""}${key}`;
      mesh.userData.notShadowCaster = true;
      mesh.visible = false;
      mesh.computeBoundingSphere();
      // The field spreads its far clumps in the vertex stage.
      if (mesh.boundingSphere) { mesh.boundingSphere.radius += far ? 3 : 1.5; }
      this.root.add(mesh);
    }
    layer.cells.set(key, { meshes, flowering, clumps: count });
    this.#changed = true;
  }

  #drop(cell: Cell | null): void {
    if (!cell) { return; }
    for (const { mesh } of cell.meshes) {
      this.root.remove(mesh);
      mesh.geometry.dispose();
      mesh.dispose();
    }
    this.#changed = true;
  }

  #clear(): void {
    for (const layer of [this.#near, ...(this.#distant ? [this.#distant] : [])]) {
      for (const cell of layer.cells.values()) { this.#drop(cell); }
      layer.cells.clear();
    }
  }

  /** The drawn root, and whether its cells changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: [this.root], changed };
  }

  /**
   * Acceptance state: resident cells (near and far), the clumps drawn this frame (at most; the vertex stage sinks some),
   * their triangles (before frustum culling), the flowers among them and the far clumps.
   */
  observe(): { cells: number; drawn: number; triangles: number; flowers: number; far: number } {
    let cells = 0;
    let drawn = 0;
    let triangles = 0;
    let flowers = 0;
    let far = 0;
    for (const layer of [this.#near, ...(this.#distant ? [this.#distant] : [])]) {
      cells += layer.cells.size;
      for (const cell of layer.cells.values()) {
        for (const { kind, mesh, blades } of cell?.meshes ?? []) {
          if (!mesh.visible) { continue; }
          drawn += mesh.count;
          if (!blades) { flowers += mesh.count; }
          if (kind === this.#far) { far += mesh.count; }
          triangles += mesh.count * mesh.geometry.index!.count / 3;
        }
      }
    }
    return { cells, drawn, triangles, flowers, far };
  }

  dispose(): void {
    this.#clear();
    for (const kind of [this.#full, this.#light, this.#flowers, this.#far, this.#farFlowers]) {
      kind.geometry.dispose();
      kind.material.dispose();
    }
  }
}
