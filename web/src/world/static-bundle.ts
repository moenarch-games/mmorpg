import type { Object3D } from "three";
import { GLTFLoader, type GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";

/** The `default` glTF of each of `keys` in the gltf-resources bundle at `base`, loaded with plain three.js. */
export async function loadBundleGltfs(base: string, keys: readonly string[]): Promise<GLTF[]> {
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
    return loader.loadAsync(url(entry.path));
  }));
}

/** Existing static glTF consumer shared by the inn and opt-in nature review. */
export async function loadStaticBundle(base: string, keys: readonly string[]): Promise<Object3D[]> {
  return (await loadBundleGltfs(base, keys)).map((gltf, index) => {
    const root = gltf.scene;
    root.name = keys[index]!;
    root.matrixAutoUpdate = false;
    return root;
  });
}
