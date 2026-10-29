import * as THREE from "three";
import type { RaidSignal } from "../replication";

/**
 * Smoke over a raided barn (#530, Atlas 5.1.53): while a night raid's assault runs, or while the
 * barn still recovers from one, soft grey puffs rise from its roof. The more integrity it has
 * lost, the thicker the smoke, and an ember glow tints it during the assault. This lets nearby
 * players see that something is happening without any announcement. It is purely cosmetic:
 * it follows the projection's raid signal and never feeds back.
 */

/** Puffs per second at full damage; an intact barn under attack still smokes a little. */
const MAX_RATE = 7;
const MIN_RATE = 1.5;
const LIFETIME_MS = 6_500;
const MAX_PUFFS = 64;
/** A puff rises this fast (m/s) and grows from `START` to `END` metres across. */
const RISE = 1.6;
const START = 2.2;
const END = 7;

/** Puffs per second for a signal: none without one. */
export function smokeRate(signal: RaidSignal | null): number {
  if (!signal) {
    return 0;
  }
  const lost = 1 - signal.integrity / 100;
  if (signal.phase === 0) {
    return lost > 0 ? MIN_RATE + (MAX_RATE - MIN_RATE) * lost * 0.6 : 0;
  }
  return MIN_RATE + (MAX_RATE - MIN_RATE) * lost;
}

function puffTexture(): THREE.Texture {
  const size = 64;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (context) {
    const gradient = context.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    gradient.addColorStop(0, "rgb(255 255 255 / 85%)");
    gradient.addColorStop(0.55, "rgb(255 255 255 / 35%)");
    gradient.addColorStop(1, "rgb(255 255 255 / 0%)");
    context.fillStyle = gradient;
    context.fillRect(0, 0, size, size);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

type Puff = { sprite: THREE.Sprite; material: THREE.SpriteMaterial; bornMs: number; drift: [number, number]; ember: boolean };

export class RaidSmoke {
  /** The one imported root every puff hangs under. */
  readonly root = new THREE.Group();
  readonly #puffs: Puff[] = [];
  #texture: THREE.Texture | null = null;
  #lastMs: number | null = null;
  #owed = 0;
  /** A deterministic jitter sequence, so frames do not depend on `Math.random`. */
  #seed = 1;

  constructor() {
    this.root.name = "raid-smoke";
  }

  #random(): number {
    this.#seed = (this.#seed * 1_103_515_245 + 12_345) % 2_147_483_648;
    return this.#seed / 2_147_483_648;
  }

  /**
   * Advances the smoke to `nowMs`. `roof` is the barn's roof centre in metres; `footprint` the
   * barn's half extents in metres, across which puffs start.
   */
  update(signal: RaidSignal | null, roof: readonly [number, number, number] | null, footprint: readonly [number, number], nowMs: number): void {
    const elapsed = this.#lastMs === null ? 0 : Math.min(250, Math.max(0, nowMs - this.#lastMs));
    this.#lastMs = nowMs;
    const rate = roof ? smokeRate(signal) : 0;
    this.#owed = rate > 0 ? this.#owed + (rate * elapsed) / 1_000 : 0;
    while (roof && this.#owed >= 1) {
      this.#owed -= 1;
      this.#spawn(roof, footprint, nowMs, (signal?.phase ?? 0) > 0);
    }
    for (let index = this.#puffs.length - 1; index >= 0; index -= 1) {
      const puff = this.#puffs[index]!;
      const age = (nowMs - puff.bornMs) / LIFETIME_MS;
      if (age >= 1) {
        this.#retire(index);
        continue;
      }
      const seconds = (nowMs - puff.bornMs) / 1_000;
      const base = puff.sprite.userData.base as [number, number, number];
      puff.sprite.position.set(base[0] + puff.drift[0] * seconds, base[1] + RISE * seconds, base[2] + puff.drift[1] * seconds);
      const size = START + (END - START) * age;
      puff.sprite.scale.set(size, size, 1);
      puff.material.opacity = 0.55 * Math.min(1, age * 6) * (1 - age);
      const shade = puff.ember ? 0.36 + 0.3 * age : 0.42 + 0.2 * age;
      puff.material.color.setRGB(puff.ember ? Math.min(1, shade + 0.35 * (1 - age) ** 3) : shade, shade, shade);
    }
  }

  #spawn(roof: readonly [number, number, number], footprint: readonly [number, number], nowMs: number, ember: boolean): void {
    if (this.#puffs.length >= MAX_PUFFS) {
      this.#retire(0);
    }
    this.#texture ??= puffTexture();
    const material = new THREE.SpriteMaterial({ map: this.#texture, transparent: true, depthWrite: false, opacity: 0 });
    const sprite = new THREE.Sprite(material);
    const base: [number, number, number] = [
      roof[0] + (this.#random() - 0.5) * footprint[0],
      roof[1],
      roof[2] + (this.#random() - 0.5) * footprint[1],
    ];
    sprite.userData.base = base;
    sprite.position.set(...base);
    this.root.add(sprite);
    this.#puffs.push({ sprite, material, bornMs: nowMs, drift: [0.35 + this.#random() * 0.4, -0.15 + this.#random() * 0.3], ember });
  }

  #retire(index: number): void {
    const [puff] = this.#puffs.splice(index, 1);
    if (puff) {
      this.root.remove(puff.sprite);
      puff.material.dispose();
    }
  }

  /** Live puffs, for tests and debug readouts. */
  get puffCount(): number {
    return this.#puffs.length;
  }

  clear(): void {
    while (this.#puffs.length > 0) { this.#retire(this.#puffs.length - 1); }
    this.#owed = 0;
    this.#lastMs = null;
  }
}
