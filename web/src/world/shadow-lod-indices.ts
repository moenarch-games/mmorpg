import { MeshoptSimplifier } from "meshoptimizer/simplifier";

/**
 * The simplification behind shadow levels of detail (#1083), over plain arrays so the shadow-proxy worker runs it
 * too (#1122): meshoptimizer collapses edges onto existing vertices, so a level is only a shorter index over the same
 * vertices. No three.js here, which keeps the worker small.
 */

/** A shadow level: about `ratio` of the triangles, within `error` of the extent (meshoptimizer's relative error). */
export type ShadowLevel = { ratio: number; error: number };

/** Resolves once the simplifier's WebAssembly is compiled in this thread. */
export const simplifierReady: Promise<void> = MeshoptSimplifier.ready;

/**
 * `indices` (triangles over `positions`, three floats a vertex) simplified to about `ratio` of its triangles within
 * `error`, or null when that removes less than a tenth of them. Needs {@link simplifierReady}.
 */
export function simplifiedIndex(indices: Uint32Array, positions: Float32Array, ratio: number, error: number): Uint32Array | null {
  const target = Math.max(3, Math.floor((indices.length * ratio) / 3) * 3);
  const [simplified] = MeshoptSimplifier.simplify(indices, positions, 3, target, error);
  return simplified.length === 0 || simplified.length > indices.length * 0.9 ? null : simplified;
}

/**
 * Per level (cascade, nearest first) its index, each simplified from the one before (cheaper than from the full
 * index); null where a level does not simplify and repeats the previous one (the first: `indices` itself).
 */
export function indexLevels(indices: Uint32Array, positions: Float32Array, levels: readonly ShadowLevel[]): (Uint32Array | null)[] {
  const full = Math.floor(indices.length / 3);
  const out: (Uint32Array | null)[] = [];
  let from = indices;
  for (const level of levels) {
    const simplified = simplifiedIndex(from, positions, (level.ratio * full) / Math.max(1, Math.floor(from.length / 3)), level.error);
    out.push(simplified);
    if (simplified) { from = simplified; }
  }
  return out;
}

/** What the page asks the shadow-proxy worker (#1122): one geometry's levels. */
export type ShadowBakeRequest = { id: number; positions: Float32Array; indices: Uint32Array; levels: ShadowLevel[] };
/** Its answer: the levels as {@link indexLevels} gives them, or why they failed. */
export type ShadowBakeAnswer = { id: number; levels: (Uint32Array | null)[] } | { id: number; error: string };
