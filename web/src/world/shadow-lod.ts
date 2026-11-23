import * as THREE from "three";
import { MeshoptSimplifier } from "meshoptimizer/simplifier";

/**
 * Shadow levels of detail (#1083): cheaper geometry the sun cascades draw in place of a caster's own (three-lighting's
 * `userData.shadowGeometry`). A shadow is a silhouette a few texels wide, so a simplified index over the same vertices
 * (meshoptimizer, which collapses edges onto existing vertices) casts the same shadow at a fraction of the triangles:
 * a skinned proxy keeps its joints and weights, a masked one its UVs.
 */

let ready = false;
/** Resolves once the simplifier's WebAssembly is compiled; until then {@link simplifiedShadow} returns null. */
export const shadowLodReady: Promise<void> = MeshoptSimplifier.ready.then(() => { ready = true; });

/** A shadow level: about `ratio` of the triangles, within `error` of the extent (meshoptimizer's relative error). */
export type ShadowLevel = { ratio: number; error: number };

/** Skinned units' proxies: in the near cascade about a third (within 0.5 % of the body), farther a tenth (2 %). */
export const UNIT_SHADOW_LEVELS: readonly ShadowLevel[] = [{ ratio: 0.3, error: 0.005 }, { ratio: 0.1, error: 0.02 }];
/** A kit entry's coarsest level in the cascades past it: about a third of its triangles, within 2 % of its extent. */
export const KIT_FAR_SHADOW: ShadowLevel = { ratio: 0.35, error: 0.02 };

/** `geometry`'s positions as tightly packed floats, as the simplifier reads them. */
function packedPositions(geometry: THREE.BufferGeometry): Float32Array {
  const position = geometry.getAttribute("position");
  if (position instanceof THREE.BufferAttribute && position.array instanceof Float32Array && position.itemSize === 3 && !position.normalized) {
    return position.array;
  }
  const out = new Float32Array(position.count * 3);
  for (let i = 0; i < position.count; i++) { out.set([position.getX(i), position.getY(i), position.getZ(i)], i * 3); }
  return out;
}

/**
 * A geometry sharing `geometry`'s attributes with a simplified index (`level`), or null when the simplifier is not
 * ready, the geometry is not indexed triangles, or simplifying removes less than a tenth of its triangles.
 */
export function simplifiedShadow(geometry: THREE.BufferGeometry, level: ShadowLevel): THREE.BufferGeometry | null {
  const index = geometry.index;
  if (!ready || !index || index.count < 3 || geometry.groups.length > 1) { return null; }
  const indices = index.array instanceof Uint32Array ? index.array : Uint32Array.from(index.array);
  const target = Math.max(3, Math.floor((index.count * level.ratio) / 3) * 3);
  const [simplified] = MeshoptSimplifier.simplify(indices, packedPositions(geometry), 3, target, level.error);
  if (simplified.length === 0 || simplified.length > index.count * 0.9) { return null; }
  const shadow = new THREE.BufferGeometry();
  for (const [name, attribute] of Object.entries(geometry.attributes)) { shadow.setAttribute(name, attribute); }
  shadow.setIndex(new THREE.BufferAttribute(simplified, 1));
  shadow.boundingSphere = geometry.boundingSphere;
  shadow.boundingBox = geometry.boundingBox;
  shadow.name = `${geometry.name}:shadow-lod`;
  return shadow;
}

/**
 * Per level (cascade, nearest first) the geometry a caster draws, each simplified from the one before (cheaper than
 * from the full mesh); a level that does not simplify repeats the previous one, the first `geometry` itself.
 */
export function shadowLevels(geometry: THREE.BufferGeometry, levels: readonly ShadowLevel[]): THREE.BufferGeometry[] {
  const out: THREE.BufferGeometry[] = [];
  let from = geometry;
  for (const level of levels) {
    from = simplifiedShadow(from, { ratio: (level.ratio * triangles(geometry)) / Math.max(1, triangles(from)), error: level.error }) ?? from;
    out.push(from);
  }
  return out;
}

/** Triangles `geometry` draws. */
export function triangles(geometry: THREE.BufferGeometry): number {
  return Math.floor((geometry.index?.count ?? geometry.getAttribute("position")?.count ?? 0) / 3);
}

/**
 * Whether `geometry`'s surface is closed (each edge, with vertices welded by position, shared by two triangles that
 * wind it opposite ways), so drawing only its front faces casts the same shadow as drawing both.
 */
export function closedSurface(geometry: THREE.BufferGeometry): boolean {
  const index = geometry.index;
  if (!index || index.count === 0) { return false; }
  const position = geometry.getAttribute("position");
  const weld = new Map<string, number>();
  const id = new Uint32Array(position.count);
  for (let i = 0; i < position.count; i++) {
    const key = `${Math.round(position.getX(i) * 1e4)},${Math.round(position.getY(i) * 1e4)},${Math.round(position.getZ(i) * 1e4)}`;
    let welded = weld.get(key);
    if (welded === undefined) { weld.set(key, welded = weld.size); }
    id[i] = welded;
  }
  const edges = new Map<number, number>();
  const n = weld.size;
  const add = (a: number, b: number) => {
    if (a === b) { return; }
    edges.set(a * n + b, (edges.get(a * n + b) ?? 0) + 1);
  };
  for (let t = 0; t + 2 < index.count; t += 3) {
    const a = id[index.getX(t)]!;
    const b = id[index.getX(t + 1)]!;
    const c = id[index.getX(t + 2)]!;
    add(a, b);
    add(b, c);
    add(c, a);
  }
  for (const [edge, count] of edges) {
    const a = Math.floor(edge / n);
    const b = edge % n;
    if ((edges.get(b * n + a) ?? 0) !== count) { return false; }
  }
  return true;
}
