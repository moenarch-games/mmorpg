import * as THREE from "three";

/**
 * How often a placed cosmetic clip samples (#1095, Atlas 6.6.26): every frame within `nearMetres` of the eye, every
 * `every`th frame (staggered by index) out to `farMetres`, and never off-screen or beyond. Distances are to the clip's
 * bounding sphere.
 */
export type ClipDetailOptions = { nearMetres: number; farMetres: number; every: number };

export const CLIP_DETAIL: ClipDetailOptions = { nearMetres: 40, farMetres: 150, every: 3 };

/** One frame's clip work (`ambient-clips` in `.performance/contract.json`). */
export type ClipWork = {
  /** Clips placed, sampled or not. */
  placed: number;
  /** Clips in the view frustum within `nearMetres`. */
  visibleNear: number;
  /** Clips in the view frustum between `nearMetres` and `farMetres`. */
  visibleMiddle: number;
  /** Clips sampled this frame. */
  sampled: number;
};

/**
 * Which of a fixed set of placed cosmetic clips (ambient creatures, settlement prop clips) sample this frame. Clips
 * sample at absolute presentation time, so a clip that skipped frames resumes on its true phase; one that was skipped
 * longer than the middle stride (off-screen, far, or new) samples as soon as it is visible again, so nothing shows a
 * stale pose. Without `animate` (reduced motion) a clip holds one still pose, sampled once. Presentation only.
 */
export class ClipDetail {
  readonly #options: ClipDetailOptions;
  readonly #frustum = new THREE.Frustum();
  readonly #clip = new THREE.Matrix4();
  readonly #sphere = new THREE.Sphere();
  readonly #eye = new THREE.Vector3();
  /** Per clip the frame it last sampled; -Infinity when its pose is stale. */
  readonly #sampledAt: Float64Array;
  #frame = 0;
  #animate = true;
  readonly work: ClipWork;

  constructor(count: number, options: ClipDetailOptions = CLIP_DETAIL) {
    this.#options = options;
    this.#sampledAt = new Float64Array(count).fill(-Infinity);
    this.work = { placed: count, visibleNear: 0, visibleMiddle: 0, sampled: 0 };
  }

  /** Starts a frame seen by `camera` (its world matrix and projection current). */
  begin(camera: THREE.Camera, animate: boolean): void {
    this.#frame += 1;
    if (animate !== this.#animate) {
      this.#animate = animate;
      this.#sampledAt.fill(-Infinity);
    }
    this.#clip.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    this.#frustum.setFromProjectionMatrix(this.#clip);
    this.#eye.setFromMatrixPosition(camera.matrixWorld);
    this.work.visibleNear = 0;
    this.work.visibleMiddle = 0;
    this.work.sampled = 0;
  }

  /** Whether clip `index`, bounded by a sphere at `(x, y, z)` with `radius` (metres), is in the view frustum. */
  visible(x: number, y: number, z: number, radius: number): boolean {
    this.#sphere.center.set(x, y, z);
    this.#sphere.radius = radius;
    return this.#frustum.intersectsSphere(this.#sphere);
  }

  /**
   * Whether clip `index`, bounded by a sphere at `(x, y, z)` with `radius` (metres), samples this frame; a clip that
   * samples is counted as sampled.
   */
  sample(index: number, x: number, y: number, z: number, radius: number): boolean {
    const { nearMetres, farMetres, every } = this.#options;
    const distance = Math.max(0, this.#eye.distanceTo(this.#sphere.center.set(x, y, z)) - radius);
    if (distance > farMetres || !this.visible(x, y, z, radius)) { return false; }
    const near = distance <= nearMetres;
    if (near) { this.work.visibleNear += 1; } else { this.work.visibleMiddle += 1; }
    const last = this.#sampledAt[index]!;
    const due = !this.#animate
      ? last === -Infinity
      : near || this.#frame - last > every || (this.#frame + index) % every === 0;
    if (!due) { return false; }
    this.#sampledAt[index] = this.#frame;
    this.work.sampled += 1;
    return true;
  }
}
