import * as THREE from "three";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { loadBundleGltfs } from "./static-bundle";
import type { PresentedEffect } from "./unit-presenter";

/** The rolling `strike-effects` and `spell-effects` bundles (#894) `scripts/fetch-bundles.sh` places in `web/public`. */
const STRIKE_BUNDLE = `${import.meta.env.BASE_URL}effects/strike.effects/`;
const SPELL_BUNDLE = `${import.meta.env.BASE_URL}effects/spell.effects/`;

/** One pooled copy of an entry: its own materials (a tint recolours them) and clip action. */
type Instance = {
  root: THREE.Object3D;
  mixer: THREE.AnimationMixer;
  actions: Map<string, THREE.AnimationAction>;
  materials: { material: THREE.MeshBasicMaterial; color: THREE.Color }[];
  tinted: boolean;
};

/**
 * The strike and spell effects (#894): asset-tooling's strike arcs, impacts, ground slam and dust, school bolts and
 * impacts, Frost Nova, beam segments, heal motes and shots, drawn where the shared Rust presenter places them, as
 * the native client does. Each entry loads once; its instances are pooled copies with their own materials and
 * mixer, reused frame to frame and hidden while unused. Until both bundles load, or if they fail, the presenter keeps
 * drawing its primitive stand-ins.
 */
export class KitEffects {
  readonly root = new THREE.Group();
  #entries = new Map<string, GLTF>();
  #pools = new Map<string, Instance[]>();
  #loading: Promise<boolean> | null = null;
  #loaded = false;
  #shown = 0;

  constructor() {
    this.root.name = "kit-effects";
  }

  /** Whether both bundles loaded: the presenter then hands their effects over. */
  get loaded(): boolean {
    return this.#loaded;
  }

  /** Loads both bundles once; settles drawable (true) or failed (false), never fatal to the world. */
  load(): Promise<boolean> {
    this.#loading ??= this.#load().then(() => {
      this.#loaded = true;
      return true;
    }, (error: unknown) => {
      console.error(`The strike and spell effect bundles did not load: ${error instanceof Error ? error.message : String(error)}`);
      return false;
    });
    return this.#loading;
  }

  async #load(): Promise<void> {
    for (const bundle of [STRIKE_BUNDLE, SPELL_BUNDLE]) {
      const response = await fetch(new URL("manifest.json", new URL(bundle, window.location.href)).href);
      if (!response.ok) { throw new Error(`${bundle} manifest is missing (${response.status})`); }
      const keys = (((await response.json()) as { assets?: { key: string; variant: string }[] }).assets ?? [])
        .filter((asset) => asset.variant === "default")
        .map((asset) => asset.key);
      (await loadBundleGltfs(bundle, keys)).forEach((gltf, index) => this.#entries.set(keys[index]!, gltf));
    }
  }

  #instance(key: string): Instance | null {
    const gltf = this.#entries.get(key);
    if (!gltf) { return null; }
    const root = gltf.scene.clone(true);
    root.matrixAutoUpdate = false;
    const materials: Instance["materials"] = [];
    root.traverse((object) => {
      if (!(object instanceof THREE.Mesh)) { return; }
      // Effects glow and fade rather than take the world's light (lighting.ts): an unlit copy shows the authored
      // colour, its emissive where it glows, times COLOR_0's colour and alpha.
      const own = (source: THREE.Material) => {
        const standard = source instanceof THREE.MeshStandardMaterial ? source : null;
        const glows = standard !== null && standard.emissive.getHex() !== 0;
        const color = (glows ? standard.emissive : standard?.color ?? new THREE.Color(1, 1, 1)).clone();
        const material = new THREE.MeshBasicMaterial({
          color, vertexColors: object.geometry.hasAttribute("color"), transparent: source.transparent,
          opacity: source.opacity, side: source.side, depthWrite: !source.transparent,
        });
        materials.push({ material, color: color.clone() });
        return material;
      };
      object.material = Array.isArray(object.material) ? object.material.map(own) : own(object.material);
      object.frustumCulled = false;
    });
    const mixer = new THREE.AnimationMixer(root);
    const actions = new Map(gltf.animations.map((clip) => [clip.name, mixer.clipAction(clip)]));
    this.root.add(root);
    return { root, mixer, actions, materials, tinted: false };
  }

  /** Poses this frame's effects; pooled copies left over are hidden. */
  update(effects: readonly PresentedEffect[]): void {
    if (!this.#loaded) { return; }
    const used = new Map<string, number>();
    for (const effect of effects) {
      const index = used.get(effect.key) ?? 0;
      used.set(effect.key, index + 1);
      const pool = this.#pools.get(effect.key) ?? [];
      this.#pools.set(effect.key, pool);
      const instance = pool[index] ?? this.#instance(effect.key);
      if (!instance) { continue; }
      pool[index] = instance;
      instance.root.visible = true;
      instance.root.matrix.fromArray(effect.model);
      instance.root.matrixWorldNeedsUpdate = true;
      if (effect.tint) {
        const tint = new THREE.Color().setRGB(effect.tint[0], effect.tint[1], effect.tint[2], THREE.SRGBColorSpace);
        for (const { material } of instance.materials) { material.color.copy(tint); }
        instance.tinted = true;
      } else if (instance.tinted) {
        for (const { material, color } of instance.materials) { material.color.copy(color); }
        instance.tinted = false;
      }
      const action = effect.clip ? instance.actions.get(effect.clip) : undefined;
      if (action) {
        if (!action.isRunning()) { instance.mixer.stopAllAction(); action.play(); }
        const duration = action.getClip().duration;
        action.time = duration > 0 ? effect.time % duration : 0;
        instance.mixer.update(0);
      }
    }
    for (const [key, pool] of this.#pools) {
      pool.slice(used.get(key) ?? 0).forEach((instance) => { instance.root.visible = false; });
    }
    this.#shown = effects.length;
  }

  /** Acceptance state: whether the bundles loaded, their entries and how many effects the last frame showed. */
  observe(): { loaded: boolean; entries: string[]; shown: number } {
    return { loaded: this.#loaded, entries: [...this.#entries.keys()], shown: this.#shown };
  }
}
