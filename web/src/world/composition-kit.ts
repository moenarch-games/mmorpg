import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadBundleGltfs, readBundleAttachment } from "./static-bundle";

/** The rolling `fortress-kit` bundle `scripts/fetch-bundles.sh` places in `web/public` (#591). */
export const FORTRESS_BUNDLE = `${import.meta.env.BASE_URL}scenery/fortress.kit/`;

/** What the shared Rust presentation draws of a composition kit (`mmorpg_presentation::composition`). */
export type CompositionDressing = {
  /** Every entry key drawn, sorted. */
  entries: string[];
  /** Static pieces: entry key and 16 column-major entry-to-world floats (metres). */
  statics: { key: string; matrix: number[] }[];
  /** Lift pieces: entry key, the platform the unit presenter places, and the turn about +Y (radians). */
  lifts: { key: string; platform: number; yaw: number }[];
  /** Per entry its bounding-sphere centre (entry frame, metres) and `[variant, camera distance it starts at]`, finest first. */
  lods: { key: string; centre: [number, number, number]; levels: [string, number][] }[];
};

/** A composition kit the world draws over its graybox: the bundle, where its descriptor is, and the Rust dressing. */
export type CompositionKitSource = {
  bundle: string;
  name: string;
  descriptor: { key: string; variant: string; attachment: string };
  /** The pieces whose bodies the descriptor fits exactly, for a camera of vertical field of view `fovY` (radians). */
  dress(descriptor: string, fovY: number): CompositionDressing;
};

/** Where the unit presenter puts a platform the kit draws: its box's bottom centre, metres. */
export type PlatformPlacement = { platform: number; base: [number, number, number] };

/** A level switches only this fraction past its distance, so a piece at a boundary does not flicker. */
const LOD_HYSTERESIS = 0.08;

/**
 * Every piece and level of a kit uses the same few images, but each glTF loads its own textures and materials: one
 * texture per image and one material per name (with those textures) is kept, so the GPU holds each image once and
 * the pieces draw with a handful of programs.
 */
function shareMaterials(gltfs: readonly GLTF[]): void {
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

/**
 * An asset-tooling composition kit (#591, the Battle Royale fortress): every piece whose bodies the shared Rust
 * presentation matched is drawn at its placement, each a three.js `LOD` of the descriptor's levels switched at the
 * descriptor's screen sizes; lift pieces follow their platforms. Until the bundle loads (or when it fails) nothing is
 * drawn, and the static scene keeps the graybox. Presentation only.
 */
export class CompositionKit {
  readonly #source: CompositionKitSource;
  #roots: THREE.Object3D[] = [];
  #lifts = new Map<number, { root: THREE.Object3D; yaw: number; centre: THREE.Vector3 }>();
  #loading: Promise<boolean> | null = null;
  #changed = false;
  #dressing: CompositionDressing | null = null;
  /** Stands in for the renderer's camera when switching levels: only its position (and zoom 1) matter. */
  readonly #eye = new THREE.PerspectiveCamera();

  constructor(source: CompositionKitSource) {
    this.#source = source;
  }

  /** Loads the bundle once; settles drawable (true) or failed (false), never fatal to the world. */
  load(fovY: number): Promise<boolean> {
    this.#loading ??= this.#load(fovY).then(() => true, (error: unknown) => {
      console.error(`The ${this.#source.name} bundle did not load: ${error instanceof Error ? error.message : String(error)}`);
      this.#roots = [];
      this.#lifts.clear();
      return false;
    });
    return this.#loading;
  }

  async #load(fovY: number): Promise<void> {
    const { bundle, descriptor } = this.#source;
    const json = await readBundleAttachment(bundle, descriptor.key, descriptor.variant, descriptor.attachment);
    const dressing = this.#source.dress(json, fovY);
    const lods = new Map(dressing.lods.map((entry) => [entry.key, entry]));
    const levels = (key: string): [string, number][] => lods.get(key)?.levels ?? [["default", 0]];
    const files = dressing.entries.flatMap((key) => levels(key).map(([variant]) => variant === "default" ? key : `${key}/${variant}`));
    const gltfs = await loadBundleGltfs(bundle, files, 4);
    shareMaterials(gltfs);
    const scenes = new Map(files.map((file, index) => [file, gltfs[index]!.scene]));
    const piece = (key: string, name: string): { root: THREE.LOD; centre: THREE.Vector3 } => {
      const centre = new THREE.Vector3(...(lods.get(key)?.centre ?? [0, 0, 0]));
      const root = new THREE.LOD();
      root.name = name;
      root.matrixAutoUpdate = false;
      // `update` switches the levels (see there).
      root.autoUpdate = false;
      for (const [variant, distance] of levels(key)) {
        // Clones share the level's geometry and materials; each stands about the sphere centre the LOD measures from.
        const level = scenes.get(variant === "default" ? key : `${key}/${variant}`)!.clone();
        level.position.sub(centre);
        root.addLevel(level, distance, LOD_HYSTERESIS);
      }
      return { root, centre };
    };
    const at = new THREE.Matrix4();
    const roots = dressing.statics.map(({ key, matrix }, index) => {
      const { root, centre } = piece(key, `${key}#${index}`);
      root.matrix.fromArray(matrix).multiply(at.makeTranslation(centre));
      root.matrixWorldNeedsUpdate = true;
      return root;
    });
    this.#lifts = new Map(dressing.lifts.map(({ key, platform, yaw }) => {
      const { root, centre } = piece(key, `${key}@${platform}`);
      // Hidden until the unit presenter first places it.
      root.visible = false;
      return [platform, { root, yaw, centre }];
    }));
    this.#roots = [...roots, ...[...this.#lifts.values()].map((lift) => lift.root)];
    this.#dressing = dressing;
    this.#changed = true;
  }

  /** Whether the kit is drawn (loaded); the static scene hides the graybox of the bodies it dresses then. */
  get drawn(): boolean {
    return this.#roots.length > 0;
  }

  /** The platforms the kit's lift pieces draw, for the unit presenter (empty until drawn). */
  get platforms(): number[] {
    return [...this.#lifts.keys()];
  }

  /** Moves each lift piece to where the unit presenter puts its platform this frame. */
  place(platforms: readonly PlatformPlacement[]): void {
    const turn = new THREE.Matrix4();
    const offset = new THREE.Matrix4();
    for (const { platform, base } of platforms) {
      const lift = this.#lifts.get(platform);
      if (!lift) { continue; }
      lift.root.matrix.makeTranslation(base[0], base[1], base[2])
        .multiply(turn.makeRotationY(lift.yaw))
        .multiply(offset.makeTranslation(lift.centre));
      lift.root.matrixWorldNeedsUpdate = true;
      lift.root.visible = true;
    }
  }

  /**
   * Picks every piece's level for a camera at `eye` (metres). The shared renderer drives its own camera, not a
   * `PerspectiveCamera`, so the kit's `LOD`s are switched here each frame instead of by three's render loop.
   */
  update(eye: readonly [number, number, number]): void {
    this.#eye.matrixWorld.makeTranslation(eye[0], eye[1], eye[2]);
    for (const root of this.#roots) {
      if (root instanceof THREE.LOD && root.visible) {
        root.updateMatrixWorld();
        root.update(this.#eye);
      }
    }
  }

  /** The drawn roots, and whether the set changed since the last call. */
  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#roots, changed };
  }

  /** Acceptance state: whether the kit is drawn, its entries and how many static and lift pieces it places. */
  observe(): { loaded: boolean; entries: string[]; statics: number; lifts: number; levels: Record<string, number> } {
    const levels: Record<string, number> = {};
    for (const root of this.#roots) {
      if (root instanceof THREE.LOD && root.visible) {
        const level = root.getCurrentLevel();
        const name = root.levels[level] ? `level${level}` : "none";
        levels[name] = (levels[name] ?? 0) + 1;
      }
    }
    return {
      loaded: this.drawn,
      entries: this.#dressing?.entries ?? [],
      statics: this.#dressing?.statics.length ?? 0,
      lifts: this.#dressing?.lifts.length ?? 0,
      levels,
    };
  }
}
