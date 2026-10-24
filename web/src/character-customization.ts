import type { CharacterClassId, CharacterPreview, EquipmentItem } from "./character-selection";

export const HAT_OPTIONS = [
  { id: "wayfarer-hood", name: "Wayfarer's Hood", accent: "#b7c6bd" },
  { id: "ranger-cap", name: "Ranger's Cap", accent: "#6f875f" },
  { id: "ironcrest-helm", name: "Ironcrest Helm", accent: "#858e91" },
  { id: "none", name: "No hat", accent: "#5b625f" },
] as const;

export type HatStyle = (typeof HAT_OPTIONS)[number]["id"];

/** Hairstyles (#376): the Ranger bundle's `hair.*` entries, or none. */
export const HAIR_OPTIONS = [
  { id: "buzzed", name: "Buzzed" },
  { id: "parted", name: "Parted" },
  { id: "long", name: "Long" },
  { id: "bald", name: "Bald" },
] as const;
export type HairStyle = (typeof HAIR_OPTIONS)[number]["id"];

/** Hair colours: the bundle's `tint.*` entries. */
export const HAIR_COLOR_OPTIONS = [
  { id: "black", name: "Black", swatch: "#1f1c1a" },
  { id: "brown", name: "Brown", swatch: "#6b4426" },
  { id: "blond", name: "Blond", swatch: "#c9a55e" },
] as const;
export type HairColor = (typeof HAIR_COLOR_OPTIONS)[number]["id"];

export type CharacterAppearance = { hat: HatStyle; hair: HairStyle; hairColor: HairColor };

/** Saves from before hair (#376) read with this hair. */
const DEFAULT_HAIR = { hair: "parted", hairColor: "brown" } as const;

export const DEFAULT_CHARACTER_APPEARANCE: CharacterAppearance = { hat: "wayfarer-hood", ...DEFAULT_HAIR };

export function defaultAppearanceForClass(classId: CharacterClassId): CharacterAppearance {
  switch (classId) {
    case "warden": return { hat: "ironcrest-helm", ...DEFAULT_HAIR };
    case "ranger": return { hat: "ranger-cap", ...DEFAULT_HAIR };
    case "arcanist":
    case "necromancer":
    case "rogue":
    case "cleric":
    case "druid": return { hat: "wayfarer-hood", ...DEFAULT_HAIR };
  }
}
const SAVE_SCHEMA_VERSION = 1;
const SAVE_KEY_PREFIX = "mmorpg.preview-character.v1";

export type SavedCharacterV1 = {
  schemaVersion: typeof SAVE_SCHEMA_VERSION;
  characterId: string;
  appearance: CharacterAppearance;
};

export type CharacterStorage = Pick<Storage, "getItem" | "setItem">;

export function rotateYawOffset(yaw: number, x: number, z: number): [number, number] {
  const cos = Math.cos(yaw);
  const sin = Math.sin(yaw);
  return [x * cos + z * sin, -x * sin + z * cos];
}

export function isHatStyle(value: unknown): value is HatStyle {
  return HAT_OPTIONS.some((option) => option.id === value);
}

export function isHairStyle(value: unknown): value is HairStyle {
  return HAIR_OPTIONS.some((option) => option.id === value);
}

export function isHairColor(value: unknown): value is HairColor {
  return HAIR_COLOR_OPTIONS.some((option) => option.id === value);
}

export function hatOption(style: HatStyle) {
  const option = HAT_OPTIONS.find((candidate) => candidate.id === style);
  if (!option) throw new Error(`Unknown hat style: ${style}`);
  return option;
}

export function equipmentForAppearance(
  character: CharacterPreview,
  appearance: CharacterAppearance,
): EquipmentItem[] {
  const hat = hatOption(appearance.hat);
  return character.equipment.map((item) =>
    item.slot === "Head" ? { slot: "Head", name: hat.name, accent: hat.accent } : { ...item },
  );
}

export function storageKeyForCharacter(characterId: string): string {
  if (!characterId) throw new Error("Character ID is required for local save storage");
  return `${SAVE_KEY_PREFIX}.${characterId}`;
}

export function saveCharacter(
  storage: CharacterStorage,
  characterId: string,
  appearance: CharacterAppearance,
): SavedCharacterV1 {
  hatOption(appearance.hat);
  if (!isHairStyle(appearance.hair) || !isHairColor(appearance.hairColor)) {
    throw new Error("Character hair is invalid");
  }
  const saved: SavedCharacterV1 = {
    schemaVersion: SAVE_SCHEMA_VERSION,
    characterId,
    appearance: { ...appearance },
  };
  storage.setItem(storageKeyForCharacter(characterId), JSON.stringify(saved));
  return saved;
}

export function loadCharacter(storage: CharacterStorage, characterId: string): SavedCharacterV1 | null {
  const raw = storage.getItem(storageKeyForCharacter(characterId));
  if (raw === null) return null;

  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new Error("Saved character is not valid JSON");
  }

  if (!isRecord(value) || value.schemaVersion !== SAVE_SCHEMA_VERSION) {
    throw new Error("Unsupported saved character schema");
  }
  if (value.characterId !== characterId) {
    throw new Error("Saved character identity does not match the selected character");
  }
  if (!isRecord(value.appearance) || !isHatStyle(value.appearance.hat)) {
    throw new Error("Saved character appearance is invalid");
  }
  const { hair = DEFAULT_HAIR.hair, hairColor = DEFAULT_HAIR.hairColor } = value.appearance;
  if (!isHairStyle(hair) || !isHairColor(hairColor)) {
    throw new Error("Saved character hair is invalid");
  }

  return {
    schemaVersion: SAVE_SCHEMA_VERSION,
    characterId,
    appearance: { hat: value.appearance.hat, hair, hairColor },
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
