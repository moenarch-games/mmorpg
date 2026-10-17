import type { ClassChoice, EntityState } from "../../replication";
import type { ContentCatalog } from "../catalog";

/**
 * Which bundled Quaternius CC0 model (Atlas 6.3.6) presents a unit, its clip names and the
 * per-unit clip clock. Mirrors the native client's `presentation/skinned.rs` registry and
 * `imported_asset` mapping exactly; presentation only, never read by gameplay.
 */
export type QuaterniusKind =
  | "KnightMale" | "KnightFemale" | "Elf" | "NinjaFemale" | "Wizard" | "Witch"
  | "VikingMale" | "VikingFemale" | "GoblinMale" | "GoblinFemale" | "PirateMale" | "PirateFemale"
  | "SoldierMale" | "SoldierFemale" | "WorkerMale" | "WorkerFemale" | "CasualMale" | "CasualFemale"
  | "Wolf" | "Rat" | "Pig";

export type QuaterniusClips = { idle: string; walk: string; run: string; attack: string; death: string };

const humanoid = (attack: string): QuaterniusClips => ({ idle: "Idle", walk: "Walk", run: "Run", attack, death: "Death" });
const SLASH = humanoid("SwordSlash");
const SHOOT = humanoid("Shoot_OneHanded");
const PUNCH = humanoid("Punch");

/** File under `assets/quaternius/` and clips of every model. */
export const QUATERNIUS_MODELS: Readonly<Record<QuaterniusKind, { file: string; clips: QuaterniusClips }>> = {
  KnightMale: { file: "Knight_Male.glb", clips: SLASH },
  KnightFemale: { file: "Knight_Golden_Female.glb", clips: SLASH },
  Elf: { file: "Elf.glb", clips: SHOOT },
  NinjaFemale: { file: "Ninja_Female.glb", clips: SHOOT },
  Wizard: { file: "Wizard.glb", clips: SHOOT },
  Witch: { file: "Witch.glb", clips: SHOOT },
  VikingMale: { file: "Viking_Male.glb", clips: SLASH },
  VikingFemale: { file: "Viking_Female.glb", clips: SLASH },
  GoblinMale: { file: "Goblin_Male.glb", clips: PUNCH },
  GoblinFemale: { file: "Goblin_Female.glb", clips: PUNCH },
  PirateMale: { file: "Pirate_Male.glb", clips: SLASH },
  PirateFemale: { file: "Pirate_Female.glb", clips: SLASH },
  SoldierMale: { file: "Soldier_Male.glb", clips: SLASH },
  SoldierFemale: { file: "Soldier_Female.glb", clips: SLASH },
  WorkerMale: { file: "Worker_Male.glb", clips: PUNCH },
  WorkerFemale: { file: "Worker_Female.glb", clips: PUNCH },
  CasualMale: { file: "Casual_Male.glb", clips: PUNCH },
  CasualFemale: { file: "Casual_Female.glb", clips: PUNCH },
  Wolf: { file: "Wolf.glb", clips: { idle: "Idle", walk: "Walk", run: "Gallop", attack: "Attack", death: "Death" } },
  Rat: { file: "Rat.glb", clips: { idle: "Rat_Idle", walk: "Rat_Walk", run: "Rat_Run", attack: "Rat_Attack", death: "Rat_Death" } },
  // The farm pig has no attack clip; its jump reads as a charge.
  Pig: { file: "Pig.glb", clips: { idle: "Idle", walk: "Walk", run: "Run", attack: "Jump", death: "Death" } },
};

const CLASS_ORDER = ["warden", "ranger", "arcanist"] as const;

/** A player's appearance code `1 + class × 2 + sex` (sex 0 female), or null for 0 and unknown codes. */
export function classFromAppearance(code: number): ClassChoice | null {
  if (!Number.isInteger(code) || code < 1 || code > 6) { return null; }
  const classId = CLASS_ORDER[Math.floor((code - 1) / 2)];
  return classId ? { classId, sex: (code - 1) % 2 === 0 ? "female" : "male" } : null;
}

/**
 * The model presenting a projected unit; null when its template or NPC is unknown (the
 * procedural fallback stays). The viewer's own class sheet wins over its projected appearance.
 */
export function quaterniusKind(
  entity: Pick<EntityState, "kind" | "entityId" | "appearance">,
  catalog: ContentCatalog,
  viewer: { id: number; classChoice: ClassChoice | null },
): QuaterniusKind | null {
  const female = entity.entityId % 2 === 1;
  switch (entity.kind) {
    case "player": {
      const choice = (entity.entityId === viewer.id ? viewer.classChoice : null) ?? classFromAppearance(entity.appearance);
      const isFemale = choice?.sex === "female";
      switch (choice?.classId) {
        case "ranger": return isFemale ? "NinjaFemale" : "Elf";
        case "arcanist": return isFemale ? "Witch" : "Wizard";
        default: return isFemale ? "KnightFemale" : "KnightMale";
      }
    }
    case "creature": {
      const family = catalog.creatureTemplates.get(entity.appearance)?.family;
      switch (family) {
        case undefined: return null;
        case "wolf": return "Wolf";
        case "vermin": return "Rat";
        case "boar": return "Pig";
        case "marauder": return female ? "VikingFemale" : "VikingMale";
        case "mirefin": return female ? "GoblinFemale" : "GoblinMale";
        case "redbrand": return female ? "PirateFemale" : "PirateMale";
      }
      return null;
    }
    case "npc": {
      const role = catalog.npcs.get(entity.entityId)?.role;
      switch (role) {
        case undefined: return null;
        case "guard": return female ? "SoldierFemale" : "SoldierMale";
        case "vendor": return female ? "WorkerFemale" : "WorkerMale";
        case "spirit_healer": return "Wizard";
        case "quest_giver": return female ? "CasualFemale" : "CasualMale";
      }
      return null;
    }
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
      clip = clips.idle;
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
