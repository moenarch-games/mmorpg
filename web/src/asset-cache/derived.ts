import { DERIVED_CACHE_NAME, MANIFEST_FILE, parseManifest, type AssetManifest } from "./policy";

/**
 * Reuse of what the browser derives from a release (#526), beside the service worker's reuse of what it downloads
 * (#410): immutable bytes kept in Cache Storage per release, so a later visit skips the derivation. An entry is only
 * trusted under the exact release that wrote it, and every failure reads as a miss, so a cold or cleared cache, or one
 * without storage at all, is always the same as deriving again.
 */

/** The file whose hash names a release's derivations: every derived byte is a function of the WASM build. */
const RELEASE_WASM = /(^|\/)mmorpg_wasm_bg[^/]*\.wasm$/;

/** The release identity of `manifest`: the WASM build's content hash, or null when it lists no such file. */
export function releaseOf(manifest: AssetManifest): string | null {
  for (const [file, revision] of Object.entries(manifest.files)) {
    if (RELEASE_WASM.test(file)) { return revision; }
  }
  return null;
}

/** The running release's identity from the deployed manifest, or null (a development server has no manifest). */
export async function fetchRelease(base: string): Promise<string | null> {
  try {
    const response = await fetch(`${base}${MANIFEST_FILE}`, { cache: "no-cache" });
    if (!response.ok) { return null; }
    const manifest = parseManifest(await response.json());
    return manifest && releaseOf(manifest);
  } catch {
    return null;
  }
}

/** The Cache Storage URL of derived data `name` for `release`. */
export function derivedUrl(origin: string, release: string, name: string): string {
  return `${origin}/__derived/${release}/${encodeURIComponent(name)}`;
}

/** The release a derived entry was written for, or null when `url` is not a derived entry. */
export function derivedRelease(url: string): string | null {
  return /\/__derived\/([0-9a-f]{64})\//.exec(url)?.[1] ?? null;
}

/** Derived data `name` of `release` from Cache Storage, or null on any miss or failure. */
export async function readDerived(cacheStorage: CacheStorage | undefined, origin: string, release: string, name: string): Promise<Uint8Array | null> {
  if (!cacheStorage) { return null; }
  try {
    const cache = await cacheStorage.open(DERIVED_CACHE_NAME);
    const response = await cache.match(derivedUrl(origin, release, name));
    return response ? new Uint8Array(await response.arrayBuffer()) : null;
  } catch {
    return null;
  }
}

/** Stores derived data `name` for `release` and removes every other release's; failures are ignored. */
export async function writeDerived(cacheStorage: CacheStorage | undefined, origin: string, release: string, name: string, bytes: Uint8Array): Promise<void> {
  if (!cacheStorage) { return; }
  try {
    const cache = await cacheStorage.open(DERIVED_CACHE_NAME);
    for (const key of await cache.keys()) {
      if (derivedRelease(key.url) !== release) { await cache.delete(key); }
    }
    await cache.put(derivedUrl(origin, release, name), new Response(new Blob([bytes as BlobPart]), { headers: { "content-type": "application/octet-stream" } }));
  } catch {
    // Reuse is an optimisation: without it the next visit derives again.
  }
}
