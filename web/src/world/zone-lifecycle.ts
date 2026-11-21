import * as THREE from "three";

/** A zone presentation component that exclusively owns GPU buffers, materials, mixers or WASM handles. */
export type ZoneOwned = { dispose(): void };

/**
 * The one lifecycle owner of a presented zone's resources (#1005): the world view gives every component it builds for
 * a zone (ground, placed kits, buildings, modules, ambient animals, grass, scenery batches) to the zone's lifecycle,
 * and retiring the lifecycle on the next zone disposes each exactly once, newest first. A component handed to a
 * retired lifecycle is disposed at once. Retained bundle resources ([`retainedBundle`]) are not owned by a zone and
 * survive its retirement, so a revisit reuses them.
 */
export class ZoneLifecycle {
  readonly #owned: ZoneOwned[] = [];
  #retired = false;

  /** Whether the zone was replaced; a late load must not attach resources to it. */
  get retired(): boolean {
    return this.#retired;
  }

  /** Takes ownership of `component`, returning it. */
  own<T extends ZoneOwned>(component: T): T {
    if (this.#retired) { component.dispose(); } else { this.#owned.push(component); }
    return component;
  }

  /** Disposes every owned component once; later calls do nothing. */
  retire(): void {
    if (this.#retired) { return; }
    this.#retired = true;
    for (const component of this.#owned.splice(0).reverse()) { component.dispose(); }
  }
}

/** Frees a wasm-bindgen handle (anything with `free`); a plain object is left alone. */
export function freeHandle(handle: unknown): void {
  (handle as { free?: () => void } | null | undefined)?.free?.();
}

/** Ends an animation mixer: its actions stop and its cached bindings to `root` are dropped. */
export function releaseMixer(mixer: THREE.AnimationMixer): void {
  mixer.stopAllAction();
  mixer.uncacheRoot(mixer.getRoot());
}

const retained = new Map<string, Promise<unknown>>();

/**
 * Immutable parsed bundle resources (glTF scenes, their shared geometries, materials and textures, descriptor text),
 * built once per `key` and kept for the page (#1005): a revisited zone takes them again instead of parsing its bundles
 * anew. No zone disposes them; each zone clones or instances them into resources of its own. A build that fails is
 * forgotten, so a later visit tries again.
 */
export function retainedBundle<T>(key: string, build: () => Promise<T>): Promise<T> {
  let entry = retained.get(key) as Promise<T> | undefined;
  if (!entry) {
    const started = build();
    entry = started;
    retained.set(key, started);
    started.catch(() => { if (retained.get(key) === started) { retained.delete(key); } });
  }
  return entry;
}

/** How many retained bundle builds the page holds, for acceptance. */
export function retainedBundleCount(): number {
  return retained.size;
}
