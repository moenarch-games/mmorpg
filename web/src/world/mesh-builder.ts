/**
 * Procedural low-poly geometry for static batching. Models are compositions
 * of a few unit shapes placed by affine transforms; a `MeshBuilder` appends
 * the transformed shape into one indexed mesh (positions, normals, indices)
 * that the renderer uploads once. Everything is plain arithmetic, so the same
 * scenery always yields the same vertices.
 */
export type Vec3 = [number, number, number];

/** An indexed triangle mesh; `normals` align one-to-one with `positions`. */
export type MeshData = { positions: Vec3[]; normals: Vec3[]; indices: number[] };

/** A shape in its own unit space. */
export type Shape = Readonly<MeshData>;

/** Row-major 3×3 linear part `m` plus translation `t`: `p' = m p + t`. */
export type Affine = { m: readonly number[]; t: Vec3 };

export const IDENTITY: Affine = { m: [1, 0, 0, 0, 1, 0, 0, 0, 1], t: [0, 0, 0] };

export function translate(x: number, y: number, z: number): Affine {
  return { m: IDENTITY.m, t: [x, y, z] };
}

export function scale(x: number, y = x, z = x): Affine {
  return { m: [x, 0, 0, 0, y, 0, 0, 0, z], t: [0, 0, 0] };
}

/** Turns local +Z toward +X by `radians`, like a positive `u16` yaw. */
export function rotateY(radians: number): Affine {
  const c = Math.cos(radians);
  const s = Math.sin(radians);
  return { m: [c, 0, s, 0, 1, 0, -s, 0, c], t: [0, 0, 0] };
}

/** Tilts local +Y toward -Z by `radians` (right-handed about +X). */
export function rotateX(radians: number): Affine {
  const c = Math.cos(radians);
  const s = Math.sin(radians);
  return { m: [1, 0, 0, 0, c, -s, 0, s, c], t: [0, 0, 0] };
}

/** Tilts local +Y toward -X by `radians` (right-handed about +Z). */
export function rotateZ(radians: number): Affine {
  const c = Math.cos(radians);
  const s = Math.sin(radians);
  return { m: [c, -s, 0, s, c, 0, 0, 0, 1], t: [0, 0, 0] };
}

/** `compose(a, b, c)` applies `c` first, then `b`, then `a`. */
export function compose(...transforms: readonly Affine[]): Affine {
  return transforms.reduce((outer, inner) => {
    const a = outer.m;
    const b = inner.m;
    const m = [0, 0, 0, 0, 0, 0, 0, 0, 0];
    for (let row = 0; row < 3; row += 1) {
      for (let column = 0; column < 3; column += 1) {
        m[row * 3 + column] = a[row * 3]! * b[column]! + a[row * 3 + 1]! * b[3 + column]! + a[row * 3 + 2]! * b[6 + column]!;
      }
    }
    return { m, t: applyPoint(outer, inner.t) };
  }, IDENTITY);
}

export function applyPoint(transform: Affine, [x, y, z]: readonly [number, number, number]): Vec3 {
  const { m, t } = transform;
  return [
    m[0]! * x + m[1]! * y + m[2]! * z + t[0],
    m[3]! * x + m[4]! * y + m[5]! * z + t[1],
    m[6]! * x + m[7]! * y + m[8]! * z + t[2],
  ];
}

/** The inverse transpose of the linear part, which keeps normals perpendicular under non-uniform scale. */
function normalMatrix(m: readonly number[]): number[] {
  const [a = 0, b = 0, c = 0, d = 0, e = 0, f = 0, g = 0, h = 0, i = 0] = m;
  const A = e * i - f * h;
  const B = -(d * i - f * g);
  const C = d * h - e * g;
  const determinant = a * A + b * B + c * C;
  const inverse = determinant === 0 ? 0 : 1 / determinant;
  // Cofactor matrix divided by the determinant is the inverse transpose.
  return [
    A * inverse, B * inverse, C * inverse,
    -(b * i - c * h) * inverse, (a * i - c * g) * inverse, -(a * h - b * g) * inverse,
    (b * f - c * e) * inverse, -(a * f - c * d) * inverse, (a * e - b * d) * inverse,
  ];
}

function normalize([x, y, z]: Vec3): Vec3 {
  const length = Math.hypot(x, y, z);
  return length > 0 ? [x / length, y / length, z / length] : [0, 1, 0];
}

/** Accumulates transformed shapes into one indexed mesh. */
export class MeshBuilder {
  readonly colors: Vec3[] = [];
  readonly positions: Vec3[] = [];
  readonly normals: Vec3[] = [];
  readonly indices: number[] = [];

  get vertexCount(): number {
    return this.positions.length;
  }

  add(shape: Shape, transform: Affine): void {
    const base = this.positions.length;
    const n = normalMatrix(transform.m);
    // A mirroring transform flips winding; swap two indices to keep faces outward.
    const mirrored = determinantOf(transform.m) < 0;
    for (const position of shape.positions) {
      this.positions.push(applyPoint(transform, position));
    }
    for (const [x, y, z] of shape.normals) {
      this.normals.push(normalize([
        n[0]! * x + n[1]! * y + n[2]! * z,
        n[3]! * x + n[4]! * y + n[5]! * z,
        n[6]! * x + n[7]! * y + n[8]! * z,
      ]));
    }
    for (let index = 0; index < shape.indices.length; index += 3) {
      const a = shape.indices[index]! + base;
      const b = shape.indices[index + 1]! + base;
      const c = shape.indices[index + 2]! + base;
      if (mirrored) {
        this.indices.push(a, c, b);
      } else {
        this.indices.push(a, b, c);
      }
    }
  }

  data(): MeshData {
    return { positions: this.positions, normals: this.normals, indices: this.indices };
  }
}

function determinantOf(m: readonly number[]): number {
  const [a = 0, b = 0, c = 0, d = 0, e = 0, f = 0, g = 0, h = 0, i = 0] = m;
  return a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
}

/** Flat-shaded polygon fan helper: appends one face with its own vertices and normal. */
function face(mesh: MeshData, corners: readonly Vec3[]): void {
  const base = mesh.positions.length;
  const [a, b, c] = corners as [Vec3, Vec3, Vec3];
  const u: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const v: Vec3 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  const normal = normalize([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]);
  for (const corner of corners) {
    mesh.positions.push([corner[0], corner[1], corner[2]]);
    mesh.normals.push(normal);
  }
  for (let index = 1; index + 1 < corners.length; index += 1) {
    mesh.indices.push(base, base + index, base + index + 1);
  }
}

function emptyMesh(): MeshData {
  return { positions: [], normals: [], indices: [] };
}

/** Axis-aligned cube spanning [-1, 1] on every axis, flat-shaded. */
export const BOX: Shape = (() => {
  const mesh = emptyMesh();
  const p = (x: number, y: number, z: number): Vec3 => [x, y, z];
  face(mesh, [p(1, -1, -1), p(1, 1, -1), p(1, 1, 1), p(1, -1, 1)]);
  face(mesh, [p(-1, -1, 1), p(-1, 1, 1), p(-1, 1, -1), p(-1, -1, -1)]);
  face(mesh, [p(-1, 1, -1), p(-1, 1, 1), p(1, 1, 1), p(1, 1, -1)]);
  face(mesh, [p(-1, -1, 1), p(-1, -1, -1), p(1, -1, -1), p(1, -1, 1)]);
  face(mesh, [p(-1, -1, 1), p(1, -1, 1), p(1, 1, 1), p(-1, 1, 1)]);
  face(mesh, [p(1, -1, -1), p(-1, -1, -1), p(-1, 1, -1), p(1, 1, -1)]);
  return mesh;
})();

const frustums = new Map<string, Shape>();

/**
 * A prism or frustum standing on y = 0 up to y = 1: bottom radius 1, top
 * radius `top` (0 makes a cone). Smooth sides share radial normals; flat
 * sides get one normal per face. Caps are optional.
 */
export function frustum(sides: number, top = 1, options: { smooth?: boolean; caps?: boolean; offset?: number } = {}): Shape {
  const smooth = options.smooth ?? false;
  const caps = options.caps ?? true;
  const offset = options.offset ?? 0;
  const key = `${sides}:${top}:${smooth}:${caps}:${offset}`;
  const cached = frustums.get(key);
  if (cached) {
    return cached;
  }
  const mesh = emptyMesh();
  const ring = (radius: number, y: number): Vec3[] => Array.from({ length: sides }, (_, index) => {
    const angle = ((index + offset) / sides) * 2 * Math.PI;
    return [Math.sin(angle) * radius, y, Math.cos(angle) * radius];
  });
  const bottom = ring(1, 0);
  const upper = ring(top, 1);
  if (smooth) {
    // The side normal leans outward by the slope: (cos, 1 - top, sin) before normalising.
    const base = mesh.positions.length;
    for (let index = 0; index < sides; index += 1) {
      const angle = ((index + offset) / sides) * 2 * Math.PI;
      const normal = normalize([Math.sin(angle), 1 - top, Math.cos(angle)]);
      mesh.positions.push(bottom[index]!, upper[index]!);
      mesh.normals.push(normal, normal);
    }
    for (let index = 0; index < sides; index += 1) {
      const next = (index + 1) % sides;
      const b0 = base + index * 2;
      const b1 = base + next * 2;
      mesh.indices.push(b0, b1, b0 + 1, b0 + 1, b1, b1 + 1);
    }
  } else {
    for (let index = 0; index < sides; index += 1) {
      const next = (index + 1) % sides;
      if (top === 0) {
        face(mesh, [bottom[index]!, bottom[next]!, upper[index]!]);
      } else {
        face(mesh, [bottom[index]!, bottom[next]!, upper[next]!, upper[index]!]);
      }
    }
  }
  if (caps) {
    face(mesh, [...bottom].reverse());
    if (top > 0) {
      face(mesh, upper);
    }
  }
  frustums.set(key, mesh);
  return mesh;
}

/**
 * A gable roof spanning x, z in [-1, 1] from y = 0 to a ridge along X at
 * y = 1: two slopes and two gable ends, flat-shaded, without a floor.
 */
export const GABLE: Shape = (() => {
  const mesh = emptyMesh();
  face(mesh, [[-1, 0, 1], [1, 0, 1], [1, 1, 0], [-1, 1, 0]]);
  face(mesh, [[1, 0, -1], [-1, 0, -1], [-1, 1, 0], [1, 1, 0]]);
  face(mesh, [[1, 0, 1], [1, 0, -1], [1, 1, 0]]);
  face(mesh, [[-1, 0, -1], [-1, 0, 1], [-1, 1, 0]]);
  return mesh;
})();

/** A four-sided pyramid over x, z in [-1, 1] with its apex at y = 1. */
export const PYRAMID: Shape = (() => {
  const mesh = emptyMesh();
  const apex: Vec3 = [0, 1, 0];
  const corners: Vec3[] = [[-1, 0, -1], [-1, 0, 1], [1, 0, 1], [1, 0, -1]];
  for (let index = 0; index < 4; index += 1) {
    face(mesh, [corners[index]!, corners[(index + 1) % 4]!, apex]);
  }
  return mesh;
})();

/**
 * A grass blade: a three-sided spike from a small triangle at y = 0 to an
 * apex at y = 1, with upward-leaning normals so clumps light like foliage.
 */
export const BLADE: Shape = (() => {
  const base: Vec3[] = [[0.35, 0, 0], [-0.2, 0, 0.3], [-0.2, 0, -0.3]];
  const positions: Vec3[] = [...base, [0, 1, 0]];
  const normals: Vec3[] = [...base.map(([x, , z]) => normalize([x, 1.4, z])), [0, 1, 0]];
  return { positions, normals, indices: [1, 0, 3, 2, 1, 3, 0, 2, 3] };
})();

/** A flat disc of `segments` sides on y = 0 facing up, radius 1. */
export function disc(segments: number): Shape {
  const positions: Vec3[] = [[0, 0, 0]];
  const normals: Vec3[] = [[0, 1, 0]];
  const indices: number[] = [];
  for (let index = 0; index < segments; index += 1) {
    const angle = (index / segments) * 2 * Math.PI;
    positions.push([Math.sin(angle), 0, Math.cos(angle)]);
    normals.push([0, 1, 0]);
  }
  for (let index = 0; index < segments; index += 1) {
    // Counter-clockwise seen from above, so the face points up.
    indices.push(0, 1 + index, 1 + ((index + 1) % segments));
  }
  return { positions, normals, indices };
}

const ICOSAHEDRON_VERTICES: Vec3[] = (() => {
  const t = (1 + Math.sqrt(5)) / 2;
  return ([
    [-1, t, 0], [1, t, 0], [-1, -t, 0], [1, -t, 0],
    [0, -1, t], [0, 1, t], [0, -1, -t], [0, 1, -t],
    [t, 0, -1], [t, 0, 1], [-t, 0, -1], [-t, 0, 1],
  ] as Vec3[]).map(normalize);
})();

const ICOSAHEDRON_FACES: [number, number, number][] = [
  [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
  [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
  [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
  [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
];

/** A deterministic hash in [0, 1) of integers; presentation jitter only. */
export function hash01(...values: readonly number[]): number {
  let state = 0x9e3779b9;
  for (const value of values) {
    state = Math.imul(state ^ Math.round(value), 0x85ebca6b);
    state ^= state >>> 13;
    state = Math.imul(state, 0xc2b2ae35);
    state ^= state >>> 16;
  }
  return (state >>> 0) / 4_294_967_296;
}

const spheres = new Map<string, Shape>();

/**
 * A unit sphere from a subdivided icosahedron (`detail` 0 or 1). `jitter`
 * displaces vertices radially by a seeded amount for organic blobs and
 * rocks; `smooth` shares vertices with radial normals, otherwise every face
 * is flat-shaded.
 */
export function blob(detail: 0 | 1, options: { smooth?: boolean; jitter?: number; seed?: number } = {}): Shape {
  const smooth = options.smooth ?? true;
  const jitter = options.jitter ?? 0;
  const seed = options.seed ?? 0;
  const key = `${detail}:${smooth}:${jitter}:${seed}`;
  const cached = spheres.get(key);
  if (cached) {
    return cached;
  }
  const vertices = ICOSAHEDRON_VERTICES.map((vertex) => [...vertex] as Vec3);
  let faces = ICOSAHEDRON_FACES;
  if (detail === 1) {
    const midpoints = new Map<string, number>();
    const midpoint = (a: number, b: number): number => {
      const key = a < b ? `${a}:${b}` : `${b}:${a}`;
      const existing = midpoints.get(key);
      if (existing !== undefined) {
        return existing;
      }
      const [pa, pb] = [vertices[a]!, vertices[b]!];
      vertices.push(normalize([(pa[0] + pb[0]) / 2, (pa[1] + pb[1]) / 2, (pa[2] + pb[2]) / 2]));
      midpoints.set(key, vertices.length - 1);
      return vertices.length - 1;
    };
    faces = faces.flatMap(([a, b, c]) => {
      const ab = midpoint(a, b);
      const bc = midpoint(b, c);
      const ca = midpoint(c, a);
      return [[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]] as [number, number, number][];
    });
  }
  const displaced = vertices.map((vertex, index): Vec3 => {
    const radius = 1 + (hash01(seed, index) * 2 - 1) * jitter;
    return [vertex[0] * radius, vertex[1] * radius, vertex[2] * radius];
  });
  const mesh = emptyMesh();
  if (smooth) {
    mesh.positions.push(...displaced);
    mesh.normals.push(...vertices);
    for (const [a, b, c] of faces) {
      mesh.indices.push(a, b, c);
    }
  } else {
    for (const [a, b, c] of faces) {
      face(mesh, [displaced[a]!, displaced[b]!, displaced[c]!]);
    }
  }
  spheres.set(key, mesh);
  return mesh;
}
