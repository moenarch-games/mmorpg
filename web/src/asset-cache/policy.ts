/**
 * Return-visit download reuse for the Pages build (#410). The build writes
 * `asset-cache.json`: every static file's SHA-256, keyed by its path below the
 * deployed base. The service worker keeps a fetched file in Cache Storage under
 * its content revision and serves it again only while the latest manifest still
 * names that revision, so a new deploy (including a rolling Ranger bundle) is
 * fetched afresh and stale bytes are never accepted. Pure helpers live here so
 * they are testable outside a worker.
 */
export const CACHE_NAME = "mmorpg-assets";
export const MANIFEST_FILE = "asset-cache.json";
export const RESET_PARAM = "reset-cache";
const REVISION_PARAM = "__rev";
/** Page shells are revalidated on every navigation and never listed in the manifest. */
const UNLISTED = /(^|\/)[^/]*\.html$|^sw\.js$|^asset-cache\.json$/;

export interface AssetManifest {
  readonly files: Readonly<Record<string, string>>;
}

/** Whether a dist-relative path belongs in the manifest. */
export function isCacheableFile(relative: string): boolean {
  return !UNLISTED.test(relative);
}

export function parseManifest(value: unknown): AssetManifest | null {
  if (typeof value !== "object" || value === null) { return null; }
  const files = (value as { files?: unknown }).files;
  if (typeof files !== "object" || files === null) { return null; }
  for (const revision of Object.values(files)) {
    if (typeof revision !== "string" || !/^[0-9a-f]{64}$/.test(revision)) { return null; }
  }
  return { files: files as Record<string, string> };
}

/** The revision a same-origin GET for `url` must have, or null when it is not a listed static file. */
export function assetRevision(manifest: AssetManifest, base: URL, url: URL): string | null {
  if (url.origin !== base.origin || url.search !== "" || !url.pathname.startsWith(base.pathname)) { return null; }
  const relative = decodeURIComponent(url.pathname.slice(base.pathname.length));
  return Object.hasOwn(manifest.files, relative) ? manifest.files[relative] ?? null : null;
}

/** The Cache Storage key for one revision of a file. */
export function revisionKey(url: URL, revision: string): string {
  const key = new URL(url.pathname, url.origin);
  key.searchParams.set(REVISION_PARAM, revision);
  return key.href;
}

/** Whether a cached key is still current: page shells are kept, file revisions must match the manifest. */
export function isCurrentKey(manifest: AssetManifest, base: URL, key: string): boolean {
  const url = new URL(key);
  const revision = url.searchParams.get(REVISION_PARAM);
  if (revision === null) { return url.search === ""; }
  return assetRevision(manifest, base, new URL(url.pathname, url.origin)) === revision;
}

export function hex(digest: ArrayBuffer): string {
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
