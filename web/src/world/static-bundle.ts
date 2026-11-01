import * as THREE from "three";
import type { Object3D } from "three";
import { GLTFLoader, type GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import type { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { litObject } from "./lighting";

/**
 * The glTF of each of `keys` in the gltf-resources bundle at `base`, loaded with plain three.js: a
 * key names its `default` variant, `<key>/<variant>` another one. At most `concurrency` glTFs load at once (all by
 * default), so a kit of many pieces does not exhaust the browser's connections. `ktx2` transcodes KTX2
 * (`KHR_texture_basisu`) textures, such as the Greyhaven terrain's (asset-tooling#475).
 */
export async function loadBundleGltfs(base: string, keys: readonly string[], concurrency = Infinity, ktx2?: KTX2Loader): Promise<GLTF[]> {
  const url = (path: string) => new URL(path, new URL(base, window.location.href)).href;
  const response = await fetch(url("manifest.json"));
  if (!response.ok) {
    throw new Error(`Static bundle manifest is missing (${response.status}): ${base}`);
  }
  const assets = ((await response.json()) as { assets?: { key: string; variant: string; path: string }[] }).assets ?? [];
  const loader = new GLTFLoader();
  if (ktx2) { loader.setKTX2Loader(ktx2); }
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

/**
 * The scenes of `keys` in the bundle at `base` for the walk-in buildings and the kit modules, lit like the rest of the
 * world, their materials shared across entries so the batches (#615) draw one call per material, not per entry.
 */
export async function loadStaticBundle(base: string, keys: readonly string[]): Promise<Object3D[]> {
  const gltfs = await loadBundleGltfs(base, keys);
  shareMaterials(gltfs);
  return gltfs.map((gltf, index) => {
    // Lit like the rest of the world (#600).
    const root = litObject(gltf.scene);
    root.name = keys[index]!;
    root.matrixAutoUpdate = false;
    return root;
  });
}

/**
 * Every piece and level of a kit uses the same few images, but each glTF loads its own textures and materials: one
 * texture per image and one material per name (with those textures) is kept, so the GPU holds each image once and
 * the pieces draw with a handful of programs.
 */
export function shareMaterials(gltfs: readonly GLTF[]): void {
  const textures = new Map<string, THREE.Texture>();
  const materials = new Map<string, THREE.Material>();
  for (const gltf of gltfs) {
    const json = gltf.parser.json as { textures?: { source?: number; sampler?: number }[]; images?: { uri?: string }[] };
    const texture = (value: THREE.Texture): THREE.Texture => {
      // Bundle images are content-addressed: equal URIs are equal images.
      const index = gltf.parser.associations.get(value)?.textures;
      const definition = index === undefined ? undefined : json.textures?.[index];
      const uri = definition?.source === undefined ? undefined : json.images?.[definition.source]?.uri;
      if (uri === undefined) { return value; }
      const key = `${uri}:${definition?.sampler ?? -1}:${value.channel}:${value.colorSpace}`;
      const kept = textures.get(key);
      if (kept) {
        if (kept !== value) { value.dispose(); }
        return kept;
      }
      textures.set(key, value);
      return value;
    };
    gltf.scene.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) { return; }
      const share = (material: THREE.Material): THREE.Material => {
        const record = material as unknown as Record<string, unknown>;
        for (const [name, value] of Object.entries(record)) {
          if (value instanceof THREE.Texture) { record[name] = texture(value); }
        }
        const maps = Object.values(record).filter((value) => value instanceof THREE.Texture).map((value) => (value as THREE.Texture).uuid);
        const standard = material as THREE.MeshStandardMaterial;
        const factors = [standard.color?.getHexString(), standard.emissive?.getHexString(), standard.roughness, standard.metalness,
          material.transparent, material.side, material.alphaTest, standard.vertexColors];
        const key = `${material.type}:${material.name}:${maps.join(",")}:${factors.join(",")}`;
        const kept = materials.get(key);
        if (kept) {
          if (kept !== material) { material.dispose(); }
          return kept;
        }
        materials.set(key, material);
        return material;
      };
      object.material = Array.isArray(object.material) ? object.material.map(share) : share(object.material);
    });
  }
}
