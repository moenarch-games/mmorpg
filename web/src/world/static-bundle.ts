import type { Object3D } from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

/** Existing static glTF consumer shared by the inn and opt-in nature review. */
export async function loadStaticBundle(base: string, keys: readonly string[]): Promise<Object3D[]> {
  const url = (path: string) => new URL(path, new URL(base, window.location.href)).href;
  const response = await fetch(url("manifest.json"));
  if (!response.ok) {
    throw new Error(`Static bundle manifest is missing (${response.status}): ${base}`);
  }
  const assets = ((await response.json()) as { assets?: { key: string; variant: string; path: string }[] }).assets ?? [];
  const loader = new GLTFLoader();
  return Promise.all(keys.map(async (key) => {
    const entry = assets.find((asset) => asset.key === key && asset.variant === "default");
    if (!entry) {
      throw new Error(`The static bundle has no ${key}/default entry.`);
    }
    const root = (await loader.loadAsync(url(entry.path))).scene;
    root.name = key;
    root.matrixAutoUpdate = false;
    return root;
  }));
}
