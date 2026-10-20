import * as THREE from "three";
import type { SceneryProvider } from "./scenery";
import { buildSceneryScene, type SceneryScene } from "./scenery-nodes";
import { loadStaticBundle } from "./static-bundle";

const KEY = "greyhaven.nature.tree.broadleaf";

/** One opt-in oak at the existing woods camera. Never writes to the world source. */
export class NatureReview {
  readonly #provider: SceneryProvider;
  readonly #current: SceneryScene;
  readonly #index: number;
  #candidate: SceneryScene | null = null;
  #root: THREE.Object3D | null = null;
  #loading: Promise<void> | null = null;
  #enabled = false;
  #changed = false;

  constructor(provider: SceneryProvider, current: SceneryScene) {
    this.#provider = provider;
    this.#current = current;
    const { props, unitsPerMetre: unit } = provider.scenery;
    // Fixed semantic selection: the existing collidable oak nearest the woods camera foreground.
    this.#index = props.reduce((best, prop, index) => {
      if (prop.kind !== "tree-oak" || !prop.collider) { return best; }
      const distance = (i: number) => Math.hypot(props[i]!.position[0] / unit + 57, props[i]!.position[2] / unit - 8);
      return best < 0 || distance(index) < distance(best) ? index : best;
    }, -1);
  }

  async setEnabled(enabled: boolean): Promise<SceneryScene> {
    if (enabled) {
      this.#loading ??= this.#load().catch((error: unknown) => {
        this.#loading = null;
        throw error;
      });
      await this.#loading;
    }
    this.#enabled = enabled;
    if (this.#root) { this.#root.visible = enabled; }
    this.#changed = true;
    return enabled ? this.#candidate! : this.#current;
  }

  async #load(): Promise<void> {
    const provider = this.#provider;
    const prop = provider.scenery.props[this.#index];
    if (!prop || !provider.reviewWithoutOak) {
      throw new Error("The oak review presentation is unavailable in this WASM build.");
    }
    const [root] = await loadStaticBundle(`${import.meta.env.BASE_URL}review/greyhaven.nature/`, [KEY]);
    if (!root) { throw new Error("The broadleaf review root is missing."); }
    root.updateMatrixWorld(true);
    const bounds = new THREE.Box3().setFromObject(root);
    const height = bounds.max.y - bounds.min.y;
    if (!Number.isFinite(height) || height <= 0) { throw new Error("The broadleaf has no finite height."); }
    const unit = provider.scenery.unitsPerMetre;
    // Preserve the authored metre-sized shape, uniformly fitted to the placement's body height.
    const scale = prop.halfExtents[1] * 2 / unit / height;
    const x = prop.position[0] / unit;
    const z = prop.position[2] / unit;
    root.matrix.compose(
      new THREE.Vector3(x, this.#current.surfaceY(x, z), z),
      new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), prop.yaw / 65536 * Math.PI * 2),
      new THREE.Vector3(scale, scale, scale),
    );
    root.matrixWorldNeedsUpdate = true;
    this.#candidate = buildSceneryScene(provider.scenery, provider.reviewWithoutOak(this.#index));
    this.#root = root;
  }

  roots(): { roots: THREE.Object3D[]; changed: boolean } {
    const changed = this.#changed;
    this.#changed = false;
    return { roots: this.#root ? [this.#root] : [], changed };
  }

  observe() {
    return {
      enabled: this.#enabled,
      loaded: this.#root !== null,
      key: `${KEY}/default`,
      placementIndex: this.#index,
      placement: this.#provider.scenery.props[this.#index],
      matrix: this.#root?.matrix.toArray() ?? null,
    };
  }
}
