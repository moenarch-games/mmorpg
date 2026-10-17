import type { ClassChoice, EntityState } from "../../replication";
import type { ContentCatalog } from "../catalog";

/**
 * Which imported model presents a unit, its clip names and the per-unit clip clock. Players
 * use asset-tooling's Ranger humanoid (Universal Base Characters with the Ranger outfit,
 * Atlas 6.3.7); wolves, vermin and boars use bundled Quaternius CC0 animals (Atlas 6.3.6);
 * humanoid creatures and NPCs keep their procedural models. Mirrors the native client's
 * `presentation/skinned.rs`; presentation only, never read by gameplay.
 */
export type QuaterniusKind = "Ranger" | "Wolf" | "Rat" | "Pig";

export type QuaterniusClips = {
  idle: string; walk: string; run: string; attack: string; death: string;
  /** A standing raised shield, for a model that carries one (#156). */
  guard?: string;
};

/** What a player model carries: a Warden's sword and shield, or nothing (#156). */
export type Loadout = "sword-and-shield" | null;

/** The loadout of a player's class: only the Warden has equipment in the character bundle. */
export function loadoutFor(choice: ClassChoice | null): Loadout {
  return choice?.classId === "warden" ? "sword-and-shield" : null;
}

/** The clips of every model. */
export const QUATERNIUS_MODELS: Readonly<Record<QuaterniusKind, { clips: QuaterniusClips }>> = {
  Ranger: { clips: { idle: "Idle_Loop", walk: "Walk_Loop", run: "Jog_Fwd_Loop", attack: "Sword_Attack", death: "Death01", guard: "Idle_Shield_Loop" } },
  Wolf: { clips: { idle: "Idle", walk: "Walk", run: "Gallop", attack: "Attack", death: "Death" } },
  Rat: { clips: { idle: "Rat_Idle", walk: "Rat_Walk", run: "Rat_Run", attack: "Rat_Attack", death: "Rat_Death" } },
  // The farm pig has no attack clip; its jump reads as a charge.
  Pig: { clips: { idle: "Idle", walk: "Walk", run: "Run", attack: "Jump", death: "Death" } },
};

const CLASS_ORDER = ["warden", "ranger", "arcanist"] as const;

/** A player's appearance code `1 + class × 2 + sex` (sex 0 female), or null for 0 and unknown codes. */
export function classFromAppearance(code: number): ClassChoice | null {
  if (!Number.isInteger(code) || code < 1 || code > 6) { return null; }
  const classId = CLASS_ORDER[Math.floor((code - 1) / 2)];
  return classId ? { classId, sex: (code - 1) % 2 === 0 ? "female" : "male" } : null;
}

/**
 * The model presenting a projected unit; null when it keeps its procedural model. Every player
 * is the Ranger humanoid while it is the only character bundle (Atlas 6.3.7 pauses the
 * per-class and per-sex look).
 */
export function quaterniusKind(
  entity: Pick<EntityState, "kind" | "entityId" | "appearance">,
  catalog: ContentCatalog,
): QuaterniusKind | null {
  switch (entity.kind) {
    case "player":
      return "Ranger";
    case "creature": {
      switch (catalog.creatureTemplates.get(entity.appearance)?.family) {
        case "wolf": return "Wolf";
        case "vermin": return "Rat";
        case "boar": return "Pig";
        default: return null;
      }
    }
    case "npc":
      return null;
  }
}

/** Per-unit clip choice and clock, as native `Playback`. Presentation-owned and cosmetic. */
export class ClipPlayback {
  #clip = "";
  #time = 0;
  #attackUntil = -Infinity;

  /** Starts an observed attack clip at `now` (seconds). */
  attack(duration: number, now: number): void {
    this.#attackUntil = now + Math.max(duration, 0.3);
  }

  /** Advances and returns the clip and time to pose this frame; `reduced` freezes the clock. */
  advance(
    clips: QuaterniusClips,
    duration: (clip: string) => number,
    speed: number,
    dead: boolean,
    now: number,
    dt: number,
    reduced: boolean,
    guarding = false,
  ): { clip: string; time: number } {
    let clip: string;
    let rate = 1;
    let once = false;
    if (dead) {
      clip = clips.death; once = true;
    } else if (now < this.#attackUntil && !reduced) {
      clip = clips.attack; once = true;
    } else if (speed > 3.2) {
      clip = clips.run; rate = Math.min(1.5, Math.max(0.7, speed / 5));
    } else if (speed > 0.25) {
      clip = clips.walk; rate = Math.min(1.6, Math.max(0.6, speed / 1.8));
    } else {
      // A standing raised guard holds the shield up; walking keeps the walk (no layered blend).
      clip = guarding && clips.guard ? clips.guard : clips.idle;
    }
    if (clip !== this.#clip) {
      this.#clip = clip;
      this.#time = 0;
    } else if (!reduced) {
      this.#time += dt * rate;
    }
    const length = duration(clip);
    const time = length <= 0 ? 0 : once ? Math.min(this.#time, length) : this.#time % length;
    return { clip, time };
  }
}
