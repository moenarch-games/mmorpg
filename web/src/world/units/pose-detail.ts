import { sameEntity } from "../../entity-ref";
import type { EntityRef } from "../../entity-ref";
import type { ImportedRequest } from "../unit-presenter";

/** F3-only tuning profiles; "off" reproduces the original every-frame poses. */
export const POSE_DETAIL_LEVELS = ["off", "light", "balanced", "aggressive"] as const;
export type PoseDetailLevel = typeof POSE_DETAIL_LEVELS[number];
export const POSE_DETAIL_PRESETS: Readonly<Record<PoseDetailLevel, { distanceMetres: number; rateHz: number; label: string }>> = {
  off: { distanceMetres: 0, rateHz: 0, label: "Full (no pose LOD)" },
  light: { distanceMetres: 35, rateHz: 20, label: "Light: 20 Hz beyond 35 m" },
  balanced: { distanceMetres: 25, rateHz: 12, label: "Balanced: 12 Hz beyond 25 m" },
  aggressive: { distanceMetres: 15, rateHz: 6, label: "Aggressive: 6 Hz beyond 15 m" },
};

// Only ordinary idle/walk layers may be delayed. Guard, casts, strikes, running creatures,
// traversal and special clips always pose on the presented frame (Atlas 6.6.26).
const BACKGROUND_CLIPS = new Set([
  "Idle", "Rat_Idle", "Idle_Loop", "Dagger_Idle_Loop", "Crossbow_Idle_Loop",
  "Walk", "Rat_Walk", "Walk_Loop",
]);

type Candidate = Pick<ImportedRequest, "id" | "entity" | "feet" | "pitch" | "layers" | "bow" | "crossbow" | "channel" | "throwable">;

function poseKey(layers: Candidate["layers"]): string {
  // Include weights, but not time: the latter is precisely what the scheduled sample advances.
  return layers.map(({ clip, mask, weight }) => `${clip}|${mask}|${weight}`).join(";");
}

/** Stable phase distributes background poses over frames rather than creating 10 Hz spikes. */
function phase(id: string): number {
  let hash = 2166136261;
  for (let i = 0; i < id.length; i++) { hash = Math.imul(hash ^ id.charCodeAt(i), 16777619); }
  return (hash >>> 0) / 0x1_0000_0000;
}

/** Schedules cosmetic poses only. Root positions, facing, material tint and equipment still update every frame. */
export class BackgroundPoseDetail {
  #level: PoseDetailLevel = "off";
  readonly #next = new Map<string, { key: string; at: number; onScreen: boolean }>();

  get level(): PoseDetailLevel { return this.#level; }

  setLevel(level: PoseDetailLevel): void {
    if (!(level in POSE_DETAIL_PRESETS)) { throw new Error(`Unknown pose detail level: ${level}`); }
    if (level === this.#level) { return; }
    this.#level = level;
    this.#next.clear(); // Switching quality must never preserve a stale schedule.
  }

  /** True if the expensive mixer pose is due. Important actors and actions always return true. */
  shouldPose(unit: Candidate, viewerFeet: readonly [number, number, number] | null,
    target: EntityRef | null, seconds: number, onScreen = true): boolean {
    const profile = POSE_DETAIL_PRESETS[this.#level];
    if (profile.rateHz === 0 || !viewerFeet || !Number.isFinite(seconds) ||
      (unit.entity.kind !== "npc" && unit.entity.kind !== "creature") ||
      sameEntity(unit.entity, target) || unit.channel || unit.bow || unit.crossbow || unit.throwable || unit.pitch !== 0 ||
      unit.layers.length === 0 || !unit.layers.every(({ clip }) => BACKGROUND_CLIPS.has(clip))) {
      this.#next.delete(unit.id);
      return true;
    }
    const dx = unit.feet[0] - viewerFeet[0];
    const dy = unit.feet[1] - viewerFeet[1];
    const dz = unit.feet[2] - viewerFeet[2];
    const prior = this.#next.get(unit.id);
    // Once a character becomes background detail, keep it there until it is safely back
    // inside the full-detail radius. This prevents rapid resampling at the boundary.
    const threshold = profile.distanceMetres * (prior ? 0.9 : 1);
    if (dx * dx + dy * dy + dz * dz < threshold ** 2) {
      this.#next.delete(unit.id);
      return true;
    }
    const key = poseKey(unit.layers);
    if (!onScreen) {
      // An off-camera background pose can sleep until it is visible again. We still sample
      // a newly created model once; action/target changes above always force full-rate work.
      this.#next.set(unit.id, { key, at: Infinity, onScreen: false });
      return !prior;
    }
    // Re-entering the view always refreshes before the first visible frame is drawn.
    if (prior?.onScreen && prior.key === key && seconds < prior.at) { return false; }
    const interval = 1 / profile.rateHz;
    this.#next.set(unit.id, { key, at: seconds + interval * (prior ? 1 : 0.1 + 0.9 * phase(unit.id)), onScreen: true });
    return true;
  }

  forget(id: string): void { this.#next.delete(id); }
}
