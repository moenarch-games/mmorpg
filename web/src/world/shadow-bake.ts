import * as THREE from "three";
import { indexLevels, simplifierReady, type ShadowBakeAnswer, type ShadowBakeRequest, type ShadowLevel } from "./shadow-lod-indices";
import { levelGeometries, packedPositions, simplifiableIndex } from "./shadow-lod";

/**
 * Units' shadow levels off the page's thread (#1122). Simplifying a unit's shadow proxy (#1083) cost about 230 ms of
 * the first update after joining, so a worker simplifies each proxy once, cached by its content: units of the same
 * look share one simplification. Until a proxy's levels arrive it casts its full shape. Presentation only.
 */

/** Simplifies one geometry's levels somewhere other than the caller: per level an index, or null for "as before". */
export type ShadowBaker = (positions: Float32Array, indices: Uint32Array, levels: readonly ShadowLevel[]) => Promise<(Uint32Array | null)[]>;

/** How many proxies asked for levels and how many simplifications that cost (the rest were cached). */
export type ShadowBakeWork = { requests: number; simplified: number };

/** Simplifications kept for reuse, least recently used first; a unit look's levels are about 0.4 of its index. */
const CACHE_LIMIT = 64;
const cache = new Map<string, Promise<(Uint32Array | null)[] | null>>();
const work: ShadowBakeWork = { requests: 0, simplified: 0 };
let baker: ShadowBaker | null = null;

/** The worker baker, or the page's own in a later task where no worker starts or the worker fails. */
function defaultBaker(): ShadowBaker {
  const later: ShadowBaker = async (positions, indices, levels) => {
    await simplifierReady;
    await new Promise((resolve) => setTimeout(resolve, 0));
    return indexLevels(indices, positions, levels);
  };
  let worker: Worker | null = null;
  try {
    worker = typeof Worker === "function" ? new Worker(new URL("./shadow-lod.worker.ts", import.meta.url), { type: "module" }) : null;
  } catch {
    worker = null;
  }
  if (!worker) { return later; }
  // Bun and Node keep a process alive for a referenced worker; a browser has no `unref`.
  (worker as { unref?: () => void }).unref?.();
  const pending = new Map<number, { resolve: (levels: (Uint32Array | null)[]) => void; reject: (error: Error) => void }>();
  let next = 0;
  let failed = false;
  const fail = (message: string) => {
    if (!failed) { console.warn(`Simplifying shadow proxies in a worker failed; the page simplifies them instead: ${message}`); }
    failed = true;
    worker.terminate();
    for (const { reject } of pending.values()) { reject(new Error(message)); }
    pending.clear();
  };
  worker.onmessage = ({ data }: MessageEvent<ShadowBakeAnswer>) => {
    const request = pending.get(data.id);
    pending.delete(data.id);
    if ("error" in data) { request?.reject(new Error(data.error)); } else { request?.resolve(data.levels); }
  };
  worker.onerror = (event) => {
    event.preventDefault();
    fail(event.message || "worker error");
  };
  return (positions, indices, levels) => {
    if (failed) { return later(positions, indices, levels); }
    return new Promise<(Uint32Array | null)[]>((resolve, reject) => {
      const id = next++;
      pending.set(id, { resolve, reject });
      // Copies: the geometry keeps its own arrays, and the worker's are transferred to it.
      const request: ShadowBakeRequest = { id, positions: positions.slice(), indices: indices.slice(), levels: [...levels] };
      worker.postMessage(request, [request.positions.buffer, request.indices.buffer]);
    }).catch(() => later(positions, indices, levels));
  };
}

/** Two independent 32-bit hashes of `words` (multiply, xor-shift) in one tight pass: it runs as a unit is built. */
function hashWords(words: Uint32Array): string {
  let first = 0x811c9dc5;
  let second = 0x9747b28c;
  for (let i = 0; i < words.length; i++) {
    const word = words[i]!;
    first = Math.imul(first ^ word, 0x01000193);
    first ^= first >>> 15;
    second = Math.imul(second ^ word, 0x5bd1e995);
    second ^= second >>> 13;
  }
  return `${(first >>> 0).toString(36)}:${(second >>> 0).toString(36)}`;
}

/** A key for `positions`, `indices` and `levels` together: two 32-bit hashes of each array's words and their lengths. */
function contentKey(positions: Float32Array, indices: Uint32Array, levels: readonly ShadowLevel[]): string {
  const words = new Uint32Array(positions.buffer, positions.byteOffset, positions.length);
  const hashes = [hashWords(words), hashWords(indices)];
  const shape = levels.map((level) => `${level.ratio}/${level.error}`).join(",");
  return `${positions.length}:${indices.length}:${hashes.join(":")}:${shape}`;
}

/**
 * `geometry`'s shadow levels (`shadowLevels`' result) as soon as they are simplified away from the caller, or null
 * when `geometry` does not simplify (nothing to draw instead of it). The caller's thread only hashes the geometry.
 */
export function bakeShadowLevels(geometry: THREE.BufferGeometry, levels: readonly ShadowLevel[]): Promise<THREE.BufferGeometry[] | null> {
  const indices = simplifiableIndex(geometry);
  if (!indices) { return Promise.resolve(null); }
  work.requests += 1;
  const positions = packedPositions(geometry);
  const key = contentKey(positions, indices, levels);
  let baked = cache.get(key);
  if (baked) {
    cache.delete(key);
  } else {
    work.simplified += 1;
    baker ??= defaultBaker();
    baked = baker(positions, indices, levels).catch((error: unknown) => {
      console.warn("A unit's shadow proxy failed to simplify; it casts its full shape.", error);
      cache.delete(key);
      return null;
    });
    if (cache.size >= CACHE_LIMIT) { cache.delete(cache.keys().next().value!); }
  }
  cache.set(key, baked);
  return baked.then((indexes) => (indexes && indexes.some((index) => index !== null) ? levelGeometries(geometry, indexes) : null));
}

/** Proxies that asked for levels and the simplifications that cost, since the page loaded (or {@link useShadowBaker}). */
export function shadowBakeWork(): ShadowBakeWork {
  return { ...work };
}

/** Simplifies with `next` from now on, with an empty cache and zeroed {@link shadowBakeWork} (tests). */
export function useShadowBaker(next: ShadowBaker | null): void {
  baker = next;
  cache.clear();
  work.requests = 0;
  work.simplified = 0;
}
