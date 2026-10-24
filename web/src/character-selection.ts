export type EquipmentItem = {
  slot: string;
  name: string;
  accent: string;
};

export type CharacterClassId = "warden" | "ranger" | "arcanist" | "necromancer" | "rogue";
export type CharacterSex = "male" | "female";

export type CharacterClassDefinition = {
  id: CharacterClassId;
  name: string;
  summary: string;
  starterEquipment: readonly EquipmentItem[];
};

export const CHARACTER_CLASSES: readonly CharacterClassDefinition[] = [
  {
    id: "warden",
    name: "Warden",
    summary: "Armored frontline fighter",
    starterEquipment: [
      { slot: "Head", name: "Wayfarer's Hood", accent: "#b7c6bd" },
      { slot: "Shoulders", name: "Greywatch Mantle", accent: "#6e8f84" },
      { slot: "Chest", name: "Warden's Brigandine", accent: "#718d84" },
      { slot: "Hands", name: "Ashwood Grips", accent: "#8b7255" },
      { slot: "Feet", name: "Trailbound Boots", accent: "#745d49" },
      { slot: "Main hand", name: "Oathkeeper Blade", accent: "#c7b66d" },
    ],
  },
  {
    id: "ranger",
    name: "Ranger",
    summary: "Mobile ranged scout",
    starterEquipment: [
      { slot: "Head", name: "Ranger's Cap", accent: "#6f875f" },
      { slot: "Shoulders", name: "Pinewatch Mantle", accent: "#617c63" },
      { slot: "Chest", name: "Trailseeker Jerkin", accent: "#72845d" },
      { slot: "Hands", name: "Tracker Gloves", accent: "#8a7354" },
      { slot: "Feet", name: "Pathfinder Boots", accent: "#66523f" },
      { slot: "Main hand", name: "Ashwood Longbow", accent: "#a88355" },
    ],
  },
  {
    id: "arcanist",
    name: "Arcanist",
    summary: "Arcane ranged spellcaster",
    starterEquipment: [
      { slot: "Head", name: "Wayfarer's Hood", accent: "#b7c6bd" },
      { slot: "Shoulders", name: "Starweave Mantle", accent: "#817aa1" },
      { slot: "Chest", name: "Adept's Robe", accent: "#6e718f" },
      { slot: "Hands", name: "Runed Gloves", accent: "#776886" },
      { slot: "Feet", name: "Softstep Boots", accent: "#5f536a" },
      { slot: "Main hand", name: "Emberglass Staff", accent: "#bd895f" },
    ],
  },
  {
    id: "necromancer",
    name: "Necromancer",
    summary: "Shadow caster who drains life",
    starterEquipment: [
      { slot: "Head", name: "Wayfarer's Hood", accent: "#b7c6bd" },
      { slot: "Shoulders", name: "Ossuary Mantle", accent: "#4b3e5e" },
      { slot: "Chest", name: "Duskweave Robe", accent: "#3d3548" },
      { slot: "Hands", name: "Gravebound Wraps", accent: "#5a4b6a" },
      { slot: "Feet", name: "Ashen Slippers", accent: "#2c2536" },
      { slot: "Main hand", name: "Bonewood Staff", accent: "#e6dfc8" },
    ],
  },
  {
    id: "rogue",
    name: "Rogue",
    summary: "Dagger fighter who poisons and stuns",
    starterEquipment: [
      { slot: "Head", name: "Wayfarer's Hood", accent: "#b7c6bd" },
      { slot: "Shoulders", name: "Shadowstitch Mantle", accent: "#4f4038" },
      { slot: "Chest", name: "Cutpurse Jerkin", accent: "#45413d" },
      { slot: "Hands", name: "Quickfinger Gloves", accent: "#3a2e26" },
      { slot: "Feet", name: "Hushstep Boots", accent: "#2a2826" },
      { slot: "Main hand", name: "Steel Dirk", accent: "#c4c9cc" },
    ],
  },
];

export const MAX_CHARACTER_SLOTS = 8;

export type CharacterPreview = {
  id: string;
  name: string;
  race: string;
  sex: CharacterSex;
  classId: CharacterClassId;
  className: string;
  level: number;
  location: string;
  equipment: readonly EquipmentItem[];
};

export type CharacterCreationDraft = {
  name: string;
  sex: CharacterSex;
  classId: CharacterClassId;
};

export function isCharacterClassId(value: unknown): value is CharacterClassId {
  return CHARACTER_CLASSES.some((candidate) => candidate.id === value);
}

export function isCharacterSex(value: unknown): value is CharacterSex {
  return value === "male" || value === "female";
}

export function characterClass(classId: CharacterClassId): CharacterClassDefinition {
  const definition = CHARACTER_CLASSES.find((candidate) => candidate.id === classId);
  if (!definition) {
    throw new Error(`Unknown character class: ${classId}`);
  }
  return definition;
}

export function starterEquipmentForClass(classId: CharacterClassId): EquipmentItem[] {
  return characterClass(classId).starterEquipment.map((item) => ({ ...item }));
}

export function normalizeCharacterName(value: string): string {
  const normalized = value.trim().replace(/\s+/g, " ");
  const length = Array.from(normalized).length;
  if (length < 2 || length > 24) {
    throw new Error("Character name must be between 2 and 24 characters.");
  }
  if (!/^[\p{L}][\p{L}\p{M} '\u2019-]*$/u.test(normalized)) {
    throw new Error("Character name may contain letters, spaces, apostrophes, and hyphens.");
  }
  return normalized;
}

function comparableName(value: string): string {
  return value.normalize("NFKC").toLowerCase();
}

/** Upper bound on local ID numbers, so retired identities cannot make allocation unbounded. */
export const MAX_LOCAL_CHARACTER_ID = 1024;

/**
 * Allocates the lowest free local ID that is neither in the roster nor retired.
 * An ID is retired while any per-character save still references it, so a new
 * character never inherits a former character's checkpoint or appearance.
 */
function nextLocalCharacterId(
  existing: readonly CharacterPreview[],
  isRetired: (id: string) => boolean,
): string {
  const ids = new Set(existing.map((character) => character.id));
  for (let slot = 1; slot <= MAX_LOCAL_CHARACTER_ID; slot += 1) {
    const candidate = `local-${slot}`;
    if (!ids.has(candidate) && !isRetired(candidate)) {
      return candidate;
    }
  }
  throw new Error("No local character slot is available.");
}

export function localCharacterPreview(
  id: string,
  name: string,
  classId: CharacterClassId,
  sex: CharacterSex,
): CharacterPreview {
  if (!/^local-[1-9][0-9]*$/.test(id)) {
    throw new Error("Local character ID is invalid.");
  }
  const normalizedName = normalizeCharacterName(name);
  const definition = characterClass(classId);
  return {
    id,
    name: normalizedName,
    race: "Human",
    sex,
    classId,
    className: definition.name,
    level: 1,
    location: "Greyhaven Outpost",
    equipment: starterEquipmentForClass(classId),
  };
}

export function createCharacterPreview(
  draft: CharacterCreationDraft,
  existing: readonly CharacterPreview[],
  isRetired: (id: string) => boolean = () => false,
): CharacterPreview {
  if (existing.length >= MAX_CHARACTER_SLOTS) {
    throw new Error(`All ${MAX_CHARACTER_SLOTS} character slots are full.`);
  }
  const name = normalizeCharacterName(draft.name);
  if (existing.some((candidate) => comparableName(candidate.name) === comparableName(name))) {
    throw new Error("A character with that name already exists in this local roster.");
  }
  if (!isCharacterClassId(draft.classId)) {
    throw new Error("Choose one of the available classes.");
  }
  if (!isCharacterSex(draft.sex)) {
    throw new Error("Choose male or female.");
  }
  return localCharacterPreview(nextLocalCharacterId(existing, isRetired), name, draft.classId, draft.sex);
}

export function draftCharacterPreview(draft: CharacterCreationDraft): CharacterPreview {
  const definition = characterClass(draft.classId);
  const rawName = draft.name.trim().replace(/\s+/g, " ");
  return {
    id: "draft",
    name: rawName || "New character",
    race: "Human",
    sex: draft.sex,
    classId: draft.classId,
    className: definition.name,
    level: 1,
    location: "Greyhaven Outpost",
    equipment: starterEquipmentForClass(draft.classId),
  };
}

export const PREVIEW_CHARACTER: CharacterPreview = {
  id: "aelric-stormward",
  name: "Aelric Stormward",
  race: "Human",
  sex: "male",
  classId: "warden",
  className: "Warden",
  level: 18,
  location: "Greyhaven Outpost",
  equipment: starterEquipmentForClass("warden"),
};

export type EntryState =
  | { phase: "character-selection"; selectedCharacterId: string }
  | { phase: "world"; characterId: string };

export function initialEntryState(character: CharacterPreview = PREVIEW_CHARACTER): EntryState {
  return { phase: "character-selection", selectedCharacterId: character.id };
}

/**
 * Presentation-only transition for the offline browser demo.
 * Online entry must still authenticate the durable character and acquire a server route.
 */
export function enterPreviewWorld(
  state: EntryState,
  character: CharacterPreview = PREVIEW_CHARACTER,
): EntryState {
  if (state.phase === "world") {
    return state;
  }
  if (state.selectedCharacterId !== character.id) {
    throw new Error("Selected character is not available in this preview roster");
  }
  return { phase: "world", characterId: character.id };
}
