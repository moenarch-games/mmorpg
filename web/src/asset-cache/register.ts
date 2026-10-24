import { CACHE_NAME, RESET_PARAM } from "./policy";

/**
 * Install the Pages asset cache (#410) for later visits. `?reset-cache` instead
 * unregisters it, deletes its stored bytes and reloads without the parameter.
 * Unsupported or failing storage leaves the game on ordinary network fetching.
 */
export async function installAssetCache(base: string): Promise<void> {
  if (typeof navigator === "undefined" || !("serviceWorker" in navigator)) { return; }
  const url = new URL(location.href);
  try {
    if (url.searchParams.has(RESET_PARAM)) {
      for (const registration of await navigator.serviceWorker.getRegistrations()) { await registration.unregister(); }
      if (typeof caches !== "undefined") { await caches.delete(CACHE_NAME); }
      url.searchParams.delete(RESET_PARAM);
      location.replace(url.href);
      return;
    }
    await navigator.serviceWorker.register(`${base}sw.js`, { scope: base });
  } catch (error) {
    console.warn("Asset cache unavailable; using the network", error);
  }
}
