import type { Object3D } from "three";
import { GLTFLoader, type GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { litObject } from "./lighting";

/**
 * The glTF of each of `keys` in the gltf-resources bundle at `base`, loaded with plain three.js: a
 * key names its `default` variant, `<key>/<variant>` another one. At most `concurrency` glTFs load at once (all by
 * default), so a kit of many pieces does not exhaust the browser's connections.
 */
export async function loadBundleGltfs(base: string, keys: readonly string[], concurrency = Infinity): Promise<GLTF[]> {
  const url = (path: string) => new URL(path, new URL(base, window.location.href)).href;
  const response = await fetch(url("manifest.json"));
  if (!response.ok) {
    throw new Error(`Static bundle manifest is missing (${response.status}): ${base}`);
  }
  const assets = ((await response.json()) as { assets?: { key: string; variant: string; path: string }[] }).assets ?? [];
  const loader = new GLTFLoader();
  const paths = keys.map((key) => {
    const [name, variant = "default"] = key.split("/");
    const entry = assets.find((asset) => asset.key === name && asset.variant === variant);
    if (!entry) {
      throw new Error(`The static bundle has no ${name}/${variant} entry.`);
    }
    return url(entry.path);
  });
  const gltfs: GLTF[] = new Array(paths.length);
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < paths.length) {
      const index = next++;
      gltfs[index] = await loader.loadAsync(paths[index]!);
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(concurrency, paths.length)) }, worker));
  return gltfs;
}

/** The text of attachment `name` on entry `key` (its `variant`) in the gltf-resources bundle at `base`. */
export async function readBundleAttachment(base: string, key: string, variant: string, name: string): Promise<string> {
  const url = (path: string) => new URL(path, new URL(base, window.location.href)).href;
  const response = await fetch(url("manifest.json"));
  if (!response.ok) {
    throw new Error(`Static bundle manifest is missing (${response.status}): ${base}`);
  }
  type Asset = { key: string; variant: string; attachments?: { name: string; path: string }[] };
  const assets = ((await response.json()) as { assets?: Asset[] }).assets ?? [];
  const attachment = assets.find((asset) => asset.key === key && asset.variant === variant)
    ?.attachments?.find((candidate) => candidate.name === name);
  if (!attachment) {
    throw new Error(`The static bundle has no ${name} on ${key}/${variant}.`);
  }
  const text = await fetch(url(attachment.path));
  if (!text.ok) {
    throw new Error(`The ${name} attachment is missing (${text.status}): ${base}`);
  }
  return text.text();
}

/** Existing static glTF consumer shared by the inn and opt-in nature review. */
export async function loadStaticBundle(base: string, keys: readonly string[]): Promise<Object3D[]> {
  return (await loadBundleGltfs(base, keys)).map((gltf, index) => {
    // Lit like the rest of the world (#600).
    const root = litObject(gltf.scene);
    root.name = keys[index]!;
    root.matrixAutoUpdate = false;
    return root;
  });
}
