import * as THREE from "three";
import { indexLevels, simplifiedIndex, simplifierReady, type ShadowLevel } from "./shadow-lod-indices";

/**
 * Shadow levels of detail (#1083): cheaper geometry the sun cascades draw in place of a caster's own (three-lighting's
 * `userData.shadowGeometry`). A shadow is a silhouette a few texels wide, so a simplified index over the same vertices
 * (meshoptimizer, which collapses edges onto existing vertices) casts the same shadow at a fraction of the triangles:
 * a skinned proxy keeps its joints and weights, a masked one its UVs. Units' proxies are simplified in a worker
 * (`shadow-bake.ts`, #1122); kit levels here, when their kit is built.
 */

export type { ShadowLevel } from "./shadow-lod-indices";

let ready = false;
/** Resolves once the simplifier's WebAssembly is compiled; until then {@link simplifiedShadow} returns null. */
export const shadowLodReady: Promise<void> = simplifierReady.then(() => { ready = true; });

/** Skinned units' proxies: in the near cascade about a third (within 0.5 % of the body), farther a tenth (2 %). */
export const UNIT_SHADOW_LEVELS: readonly ShadowLevel[] = [{ ratio: 0.3, error: 0.005 }, { ratio: 0.1, error: 0.02 }];
/** A kit entry's coarsest level in the cascades past it: about a third of its triangles, within 2 % of its extent. */
export const KIT_FAR_SHADOW: ShadowLevel = { ratio: 0.35, error: 0.02 };

/** `geometry`'s positions as tightly packed floats, as the simplifier reads them (its own array when already so). */
export function packedPositions(geometry: THREE.BufferGeometry): Float32Array {
  const position = geometry.getAttribute("position");
  if (position instanceof THREE.BufferAttribute && position.array instanceof Float32Array && position.itemSize === 3 && !position.normalized) {
    return position.array;
  }
  const out = new Float32Array(position.count * 3);
  for (let i = 0; i < position.count; i++) { out.set([position.getX(i), position.getY(i), position.getZ(i)], i * 3); }
  return out;
}

/** `geometry`'s triangle index as the simplifier reads it, or null when it is not one indexed triangle list. */
export function simplifiableIndex(geometry: THREE.BufferGeometry): Uint32Array | null {
  const index = geometry.index;
  if (!index || index.count < 3 || geometry.groups.length > 1) { return null; }
  return index.array instanceof Uint32Array ? index.array : Uint32Array.from(index.array);
}

/** A geometry sharing `geometry`'s attributes and bounds, drawing `index`. */
export function withShadowIndex(geometry: THREE.BufferGeometry, index: Uint32Array): THREE.BufferGeometry {
  const shadow = new THREE.BufferGeometry();
  for (const [name, attribute] of Object.entries(geometry.attributes)) { shadow.setAttribute(name, attribute); }
  shadow.setIndex(new THREE.BufferAttribute(index, 1));
  shadow.boundingSphere = geometry.boundingSphere;
  shadow.boundingBox = geometry.boundingBox;
  shadow.name = `${geometry.name}:shadow-lod`;
  return shadow;
}

/**
 * A geometry sharing `geometry`'s attributes with a simplified index (`level`), or null when the simplifier is not
 * ready, the geometry is not indexed triangles, or simplifying removes less than a tenth of its triangles.
 */
export function simplifiedShadow(geometry: THREE.BufferGeometry, level: ShadowLevel): THREE.BufferGeometry | null {
  const indices = simplifiableIndex(geometry);
  if (!ready || !indices) { return null; }
  const simplified = simplifiedIndex(indices, packedPositions(geometry), level.ratio, level.error);
  return simplified ? withShadowIndex(geometry, simplified) : null;
}

/** Per level the geometry drawing `indices` ({@link indexLevels}): a null level repeats the one before, the first `geometry`. */
export function levelGeometries(geometry: THREE.BufferGeometry, indices: readonly (Uint32Array | null)[]): THREE.BufferGeometry[] {
  let from = geometry;
  return indices.map((index) => (from = index ? withShadowIndex(geometry, index) : from));
}

/**
 * Per level (cascade, nearest first) the geometry a caster draws, each simplified from the one before (cheaper than
 * from the full mesh); a level that does not simplify repeats the previous one, the first `geometry` itself. On the
 * calling thread: units use `bakeShadowLevels` instead.
 */
export function shadowLevels(geometry: THREE.BufferGeometry, levels: readonly ShadowLevel[]): THREE.BufferGeometry[] {
  const indices = simplifiableIndex(geometry);
  if (!ready || !indices) { return levels.map(() => geometry); }
  return levelGeometries(geometry, indexLevels(indices, packedPositions(geometry), levels));
}

/** Triangles `geometry` draws. */
export function triangles(geometry: THREE.BufferGeometry): number {
  return Math.floor((geometry.index?.count ?? geometry.getAttribute("position")?.count ?? 0) / 3);
}

/**
 * Whether `geometry`'s surface is closed (each edge, with vertices welded by position, shared by two triangles that
 * wind it opposite ways), so drawing only its front faces casts the same shadow as drawing both. Runs as a unit is
 * built, so in typed arrays (#1122): vertices weld, and directed edges are counted, through numeric hash tables.
 */
export function closedSurface(geometry: THREE.BufferGeometry): boolean {
  const index = geometry.index;
  if (!index || index.count === 0) { return false; }
  const positions = packedPositions(geometry);
  const count = positions.length / 3;
  // Positions rounded to a tenth of a millimetre: equal ones weld into one id.
  const rounded = new Float64Array(positions.length);
  for (let i = 0; i < positions.length; i++) { rounded[i] = Math.round(positions[i]! * 1e4); }
  const size = 2 ** Math.ceil(Math.log2(Math.max(2, count * 2)));
  const slots = new Int32Array(size).fill(-1);
  const id = new Uint32Array(count);
  let welded = 0;
  for (let i = 0; i < count; i++) {
    const x = rounded[i * 3]!;
    const y = rounded[i * 3 + 1]!;
    const z = rounded[i * 3 + 2]!;
    let slot = (Math.imul(x | 0, 0x9e3779b1) ^ Math.imul(y | 0, 0x85ebca6b) ^ Math.imul(z | 0, 0xc2b2ae35)) & (size - 1);
    for (;;) {
      const at = slots[slot]!;
      if (at === -1) {
        slots[slot] = i;
        id[i] = welded++;
        break;
      }
      if (rounded[at * 3] === x && rounded[at * 3 + 1] === y && rounded[at * 3 + 2] === z) {
        id[i] = id[at]!;
        break;
      }
      slot = (slot + 1) & (size - 1);
    }
  }
  const order = index.array;
  const edges = new Float64Array(index.count);
  let edgeCount = 0;
  const add = (a: number, b: number) => {
    if (a !== b) { edges[edgeCount++] = a * welded + b; }
  };
  for (let t = 0; t + 2 < index.count; t += 3) {
    const a = id[order[t]!]!;
    const b = id[order[t + 1]!]!;
    const c = id[order[t + 2]!]!;
    add(a, b);
    add(b, c);
    add(c, a);
  }
  // Directed edges counted in a second hash table; closed when each count equals its reverse's.
  const edgeSize = 2 ** Math.ceil(Math.log2(Math.max(2, edgeCount * 2)));
  const keys = new Float64Array(edgeSize).fill(-1);
  const counts = new Int32Array(edgeSize);
  const slotOf = (edge: number): number => {
    let slot = (Math.imul(edge % 0x7fffffff, 0x9e3779b1) ^ Math.imul(Math.floor(edge / 0x7fffffff), 0x85ebca6b)) & (edgeSize - 1);
    while (keys[slot] !== -1 && keys[slot] !== edge) { slot = (slot + 1) & (edgeSize - 1); }
    return slot;
  };
  for (let e = 0; e < edgeCount; e++) {
    const edge = edges[e]!;
    const slot = slotOf(edge);
    keys[slot] = edge;
    counts[slot]! += 1;
  }
  for (let slot = 0; slot < edgeSize; slot++) {
    const edge = keys[slot]!;
    if (edge === -1) { continue; }
    const a = Math.floor(edge / welded);
    const reverse = slotOf((edge - a * welded) * welded + a);
    if (keys[reverse] === -1 || counts[reverse] !== counts[slot]) { return false; }
  }
  return true;
}
