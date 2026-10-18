import * as THREE from "three";
import type { EntityRef } from "../entity-ref";
import type { ZoneSnapshot } from "../replication";

/**
 * Authored combat impact effects (#52): asset-tooling's one-shot puff/ring frame sequences
 * (`web/public/effects`, see its SOURCE) drawn as three.js sprites on zone-confirmed events.
 * Purely cosmetic: an effect starts only from a projection's events and never feeds back.
 */
export type ImpactFamily = "puff" | "ring";
export type ImpactIntensity = "strong" | "subtle";

/** One trimmed atlas frame: its pixel rect in the atlas and its slot in the sequence timeline. */
export type ImpactFrame = {
  empty: boolean;
  rect: { x: number; y: number; width: number; height: number };
  pivot: { x: number; y: number };
  trimOffset: { x: number; y: number };
  sourceSize: { width: number; height: number };
  timeMs: number;
  durationMs: number;
};

export type ImpactAtlas = { sequence: string; width: number; height: number; frames: readonly ImpactFrame[]; durationMs: number };

/** Reads a reduced asset-tooling sprite-atlas manifest; frames must tile the timeline contiguously. */
export function parseImpactAtlas(json: unknown): ImpactAtlas {
  const value = json as { sequence?: unknown; width?: unknown; height?: unknown; sprites?: unknown };
  if (typeof value?.sequence !== "string" || typeof value.width !== "number" || typeof value.height !== "number" || !Array.isArray(value.sprites) || value.sprites.length === 0) {
    throw new Error("impact atlas must name its sequence, size and sprites");
  }
  const frames = value.sprites as ImpactFrame[];
  let time = 0;
  for (const frame of frames) {
    if (frame.timeMs !== time || !(frame.durationMs > 0)) {
      throw new Error(`impact atlas ${value.sequence} frames must be contiguous from 0`);
    }
    time += frame.durationMs;
  }
  return { sequence: value.sequence, width: value.width, height: value.height, frames, durationMs: time };
}

/** The frame index shown `ageMs` after the effect started, or -1 before the start and once it expired. */
export function impactFrameAt(atlas: ImpactAtlas, ageMs: number): number {
  if (!(ageMs >= 0) || ageMs >= atlas.durationMs) {
    return -1;
  }
  return atlas.frames.findIndex((frame) => ageMs < frame.timeMs + frame.durationMs);
}

/** The impacts one projection's confirmed events produce: hits puff, blocks ring, on the struck unit. */
export function impactCues(snapshot: ZoneSnapshot): { family: ImpactFamily; target: EntityRef }[] {
  return snapshot.events.flatMap((event): { family: ImpactFamily; target: EntityRef }[] => {
    switch (event.kind) {
      case "damage-dealt":
      case "damage-taken":
      case "damage-observed":
        return [{ family: "puff", target: event.target }];
      case "blocked":
        return [{ family: "ring", target: event.target }];
      default:
        return [];
    }
  });
}

const EFFECTS = `${import.meta.env.BASE_URL}effects/`;
/** World size of a sequence's full source canvas, metres. */
const SIZE_METRES: Record<ImpactFamily, number> = { puff: 1.1, ring: 1.3 };
/** Bound on live effects; the oldest is retired first. */
const MAX_ACTIVE = 32;

type Sequence = { atlas: ImpactAtlas; materials: (THREE.SpriteMaterial | null)[] };
type Active = { sprite: THREE.Sprite; sequence: Sequence; family: ImpactFamily; startMs: number };

async function loadSequence(name: string): Promise<Sequence> {
  const response = await fetch(new URL(`${name}/atlas.json`, new URL(EFFECTS, window.location.href)));
  if (!response.ok) {
    throw new Error(`impact atlas ${name} is missing (${response.status})`);
  }
  const atlas = parseImpactAtlas(await response.json());
  const base = await new THREE.TextureLoader().loadAsync(new URL(`${name}/atlas.png`, new URL(EFFECTS, window.location.href)).href);
  base.colorSpace = THREE.SRGBColorSpace;
  // One immutable texture view and material per frame, shared by every burst of the sequence.
  const materials = atlas.frames.map((frame) => {
    if (frame.empty) {
      return null;
    }
    const texture = base.clone();
    texture.repeat.set(frame.rect.width / atlas.width, frame.rect.height / atlas.height);
    texture.offset.set(frame.rect.x / atlas.width, 1 - (frame.rect.y + frame.rect.height) / atlas.height);
    texture.needsUpdate = true;
    return new THREE.SpriteMaterial({ map: texture, transparent: true, depthWrite: false });
  });
  return { atlas, materials };
}

export class ImpactEffects {
  /** The one imported root every live burst hangs under. */
  readonly root = new THREE.Group();
  readonly #sequences = new Map<string, Sequence | "loading" | "failed">();
  readonly #active: Active[] = [];

  constructor() {
    this.root.name = "impact-effects";
  }

  #sequence(family: ImpactFamily, intensity: ImpactIntensity): Sequence | null {
    const name = `${family}.${intensity}`;
    const state = this.#sequences.get(name);
    if (state === undefined) {
      this.#sequences.set(name, "loading");
      loadSequence(name).then(
        (sequence) => this.#sequences.set(name, sequence),
        (error: unknown) => {
          console.warn(`Impact effect ${name} failed to load; impacts stay text-only.`, error);
          this.#sequences.set(name, "failed");
        },
      );
      return null;
    }
    return typeof state === "object" ? state : null;
  }

  /** Starts loading every sequence `intensity` uses. */
  preload(intensity: ImpactIntensity): void {
    this.#sequence("puff", intensity);
    this.#sequence("ring", intensity);
  }

  /** Starts one burst of `family` at `position` (metres); dropped while its atlas is still loading. */
  spawn(family: ImpactFamily, intensity: ImpactIntensity, position: readonly [number, number, number], nowMs: number): void {
    const sequence = this.#sequence(family, intensity);
    if (!sequence) {
      return;
    }
    if (this.#active.length >= MAX_ACTIVE) {
      this.#retire(0);
    }
    const sprite = new THREE.Sprite();
    sprite.position.set(...position);
    sprite.visible = false;
    this.root.add(sprite);
    this.#active.push({ sprite, sequence, family, startMs: nowMs });
  }

  /** Shows each burst's frame for `nowMs` and releases expired ones. */
  update(nowMs: number): void {
    for (let index = this.#active.length - 1; index >= 0; index -= 1) {
      const active = this.#active[index]!;
      const { atlas, materials } = active.sequence;
      const frameIndex = impactFrameAt(atlas, nowMs - active.startMs);
      if (frameIndex < 0) {
        if (nowMs >= active.startMs) { this.#retire(index); }
        continue;
      }
      const frame = atlas.frames[frameIndex]!;
      const material = materials[frameIndex];
      if (!material) {
        active.sprite.visible = false;
        continue;
      }
      const metresPerPixel = SIZE_METRES[active.family] / frame.sourceSize.width;
      active.sprite.material = material;
      active.sprite.scale.set(frame.rect.width * metresPerPixel, frame.rect.height * metresPerPixel, 1);
      active.sprite.center.set((frame.pivot.x - frame.trimOffset.x) / frame.rect.width, 1 - (frame.pivot.y - frame.trimOffset.y) / frame.rect.height);
      active.sprite.visible = true;
    }
  }

  #retire(index: number): void {
    const [active] = this.#active.splice(index, 1);
    if (active) { this.root.remove(active.sprite); }
  }

  /** Live bursts, for debug readouts. */
  get activeCount(): number {
    return this.#active.length;
  }

  clear(): void {
    while (this.#active.length > 0) { this.#retire(this.#active.length - 1); }
  }
}

