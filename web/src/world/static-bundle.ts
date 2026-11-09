import * as THREE from "three";
import type { Object3D } from "three";
import { GLTFLoader, type GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { computeMikkTSpaceTangents, mergeVertices } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { litObject } from "./lighting";

let sharedKtx2: KTX2Loader | undefined;

/**
 * Sets the KTX2 transcoder every bundle load uses unless it is given its own: the world view's, which the renderer
 * configured. Any asset-tooling bundle may ship KTX2 (`KHR_texture_basisu`) textures (asset-tooling#511).
 */
export function useBundleKtx2Loader(loader: KTX2Loader): void {
  sharedKtx2 = loader;
}

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
  const textures = ktx2 ?? sharedKtx2;
  if (textures) { loader.setKTX2Loader(textures); }
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
export async function loadStaticBundle(base: string, keys: readonly string[], ktx2?: KTX2Loader): Promise<Object3D[]> {
  const gltfs = await loadBundleGltfs(base, keys, Infinity, ktx2);
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
    type TextureDefinition = { source?: number; sampler?: number; extensions?: { KHR_texture_basisu?: { source?: number } } };
    const json = gltf.parser.json as { textures?: TextureDefinition[]; images?: { uri?: string }[] };
    const texture = (value: THREE.Texture): THREE.Texture => {
      // Bundle images are content-addressed: equal URIs are equal images. A KTX2 image is the extension's source.
      const index = gltf.parser.associations.get(value)?.textures;
      const definition = index === undefined ? undefined : json.textures?.[index];
      const source = definition?.extensions?.KHR_texture_basisu?.source ?? definition?.source;
      const uri = source === undefined ? undefined : json.images?.[source]?.uri;
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

/**
 * Generates the tangents a glTF leaves to its consumer: a normal-mapped primitive without `TANGENT` gets them computed
 * in MikkTSpace, as glTF prescribes and as the native client does (`crate::nature`), since `LitMaterial` applies a
 * normal map only through the mesh's tangents. Every other primitive keeps what the producer shipped.
 */
export async function generateMissingTangents(gltfs: readonly GLTF[]): Promise<void> {
  const meshes: THREE.Mesh[] = [];
  for (const gltf of gltfs) {
    gltf.scene.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) { return; }
      const materials = (Array.isArray(object.material) ? object.material : [object.material]) as THREE.MeshStandardMaterial[];
      const geometry = object.geometry as THREE.BufferGeometry;
      if (materials.some((material) => material.normalMap) && !geometry.hasAttribute("tangent")
        && geometry.hasAttribute("uv") && geometry.hasAttribute("normal")) {
        meshes.push(object);
      }
    });
  }
  if (meshes.length === 0) { return; }
  const MikkTSpace = await import("three/examples/jsm/libs/mikktspace.module.js");
  await MikkTSpace.ready;
  const generated = new Map<THREE.BufferGeometry, THREE.BufferGeometry>();
  for (const mesh of meshes) {
    const source = mesh.geometry as THREE.BufferGeometry;
    let geometry = generated.get(source);
    if (!geometry) {
      // MikkTSpace works on a non-indexed copy; merging equal vertices indexes the result again.
      geometry = mergeVertices(computeMikkTSpaceTangents(source.clone(), MikkTSpace));
      generated.set(source, geometry);
    }
    mesh.geometry = geometry;
  }
}

/** The texture uniforms of a kit `LitMaterial`, and the wear mask (`applyOutpostWear`). */
const MATERIAL_TEXTURES = ["baseColorTexture", "emissiveTexture", "metallicRoughnessTexture", "normalMapTexture", "occlusionTexture", "wearMask"];

/** The distinct materials and textures the meshes under `root` draw with, for acceptance observations. */
export function drawnResources(root: Object3D | undefined): { materials: Set<THREE.Material>; textures: Set<THREE.Texture> } {
  const materials = new Set<THREE.Material>();
  const textures = new Set<THREE.Texture>();
  root?.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) { return; }
    for (const material of Array.isArray(object.material) ? object.material : [object.material]) {
      materials.add(material);
      // A LitMaterial's own textures, not its frame uniforms (shadow maps, the environment, LUTs).
      const values = material instanceof THREE.ShaderMaterial ? MATERIAL_TEXTURES.map((name) => material.uniforms[name]?.value) : Object.values(material);
      for (const value of values) { if (value instanceof THREE.Texture) { textures.add(value); } }
    }
  });
  return { materials, textures };
}
