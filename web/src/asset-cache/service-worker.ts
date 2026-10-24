/**
 * The Pages service worker (#410), bundled to `dist/sw.js` by
 * `scripts/build-asset-cache.ts`. Listed static files are cache-first by content
 * revision (see `policy.ts`); pages are network-first with a cached fallback;
 * everything else (other origins, queries, sessions) goes straight to the network.
 * Any storage failure falls back to ordinary fetching.
 */
import { CACHE_NAME, MANIFEST_FILE, assetRevision, hex, isCurrentKey, parseManifest, revisionKey, type AssetManifest } from "./policy";

interface ExtendableEvent extends Event { waitUntil(promise: Promise<unknown>): void }
interface FetchEvent extends ExtendableEvent { readonly request: Request; respondWith(response: Promise<Response>): void }
interface WorkerScope {
  readonly registration: { readonly scope: string };
  readonly clients: { claim(): Promise<void> };
  skipWaiting(): Promise<void>;
  addEventListener(type: "install" | "activate", listener: (event: ExtendableEvent) => void): void;
  addEventListener(type: "fetch", listener: (event: FetchEvent) => void): void;
}

const worker = self as unknown as WorkerScope;
const base = new URL(worker.registration.scope);
const manifestUrl = new URL(MANIFEST_FILE, base).href;
let manifest: Promise<AssetManifest | null> | null = null;

const open = () => caches.open(CACHE_NAME);

/** Fetch the deployed manifest (revalidated), remember it, and drop revisions it no longer names. */
async function refreshManifest(): Promise<AssetManifest | null> {
  try {
    const response = await fetch(manifestUrl, { cache: "no-cache" });
    const fresh = response.ok ? parseManifest(await response.clone().json()) : null;
    if (fresh === null) { return await cachedManifest(); }
    const cache = await open();
    await cache.put(manifestUrl, response);
    for (const key of await cache.keys()) {
      if (key.url !== manifestUrl && !isCurrentKey(fresh, base, key.url)) { await cache.delete(key); }
    }
    return fresh;
  } catch {
    return await cachedManifest();
  }
}

async function cachedManifest(): Promise<AssetManifest | null> {
  try {
    const response = await (await open()).match(manifestUrl);
    return response ? parseManifest(await response.json()) : null;
  } catch {
    return null;
  }
}

function currentManifest(): Promise<AssetManifest | null> {
  manifest ??= cachedManifest().then((cached) => cached ?? refreshManifest());
  return manifest;
}

/** Network-first page shell: revalidated so a new deploy's HTML (and its hashed chunks) wins. */
async function page(event: FetchEvent): Promise<Response> {
  const url = new URL(event.request.url);
  const key = new URL(url.pathname, url.origin).href;
  manifest = refreshManifest();
  event.waitUntil(manifest);
  try {
    const response = await fetch(event.request, { cache: "no-cache" });
    if (response.ok) {
      const copy = response.clone();
      event.waitUntil(open().then((cache) => cache.put(key, copy)).catch(() => undefined));
    }
    return response;
  } catch (error) {
    const cached = await open().then((cache) => cache.match(key)).catch(() => undefined);
    if (cached) { return cached; }
    throw error;
  }
}

/** Cache-first by revision; a miss is fetched, and stored only when its bytes hash to that revision. */
async function asset(event: FetchEvent, url: URL, revision: string): Promise<Response> {
  const key = revisionKey(url, revision);
  const cache = await open().catch(() => null);
  const hit = await cache?.match(key).catch(() => undefined);
  if (hit) { return hit; }
  const response = await fetch(event.request, { cache: "no-cache" });
  if (cache && response.status === 200) {
    const copy = response.clone();
    event.waitUntil((async () => {
      const bytes = await copy.arrayBuffer();
      if (hex(await crypto.subtle.digest("SHA-256", bytes)) !== revision) { return; }
      await cache.put(key, new Response(bytes, { status: 200, statusText: copy.statusText, headers: copy.headers }));
    })().catch(() => undefined));
  }
  return response;
}

async function listed(event: FetchEvent): Promise<Response> {
  const url = new URL(event.request.url);
  const current = await currentManifest().catch(() => null);
  const revision = current && assetRevision(current, base, url);
  return revision ? asset(event, url, revision) : fetch(event.request);
}

worker.addEventListener("install", (event) => {
  event.waitUntil(worker.skipWaiting());
});

worker.addEventListener("activate", (event) => {
  manifest = refreshManifest();
  event.waitUntil(Promise.all([manifest, worker.clients.claim()]));
});

worker.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET" || request.headers.has("range")) { return; }
  const url = new URL(request.url);
  if (url.origin !== base.origin || !url.pathname.startsWith(base.pathname)) { return; }
  if (request.mode === "navigate") { event.respondWith(page(event)); return; }
  if (url.search !== "" || url.href === manifestUrl) { return; }
  event.respondWith(listed(event));
});
