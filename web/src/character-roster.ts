import {
  MAX_CHARACTER_SLOTS,
  PREVIEW_CHARACTER,
  isCharacterClassId,
  isCharacterSex,
  localCharacterPreview,
  type CharacterPreview,
} from "./character-selection";
import { storageKeyForCharacter } from "./character-customization";
import { bindingProfileKey } from "./settings/binding-profiles";

const ROSTER_SCHEMA_VERSION = 1;
const ROSTER_KEY = "mmorpg.offline-roster.v1";
const MAX_ROSTER_BYTES = 8192;

export type CharacterRosterStorage = Pick<Storage, "getItem" | "setItem">;

type StoredCharacter = {
  id: string;
  name: string;
  classId: unknown;
  sex: unknown;
};

function record(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Local character roster is invalid.");
  }
  const result = value as Record<string, unknown>;
  if (Object.keys(result).length !== keys.length || keys.some((key) => !Object.hasOwn(result, key))) {
    throw new Error("Local character roster has missing or unsupported fields.");
  }
  return result;
}

function decodeStoredCharacter(value: unknown): CharacterPreview {
  const stored = record(value, ["id", "name", "classId", "sex"]) as StoredCharacter;
  if (typeof stored.id !== "string" || typeof stored.name !== "string" ||
      !isCharacterClassId(stored.classId) || !isCharacterSex(stored.sex)) {
    throw new Error("Local character roster contains an invalid character.");
  }
  return localCharacterPreview(stored.id, stored.name, stored.classId, stored.sex);
}

export function characterRosterStorageKey(): string {
  return ROSTER_KEY;
}

/**
 * Storage key of the retired offline game checkpoint (position and waystone
 * progress). The browser no longer writes or loads these; existing ones still
 * retire their character ID so a new character never reuses it.
 */
export function legacyCheckpointKey(characterId: string): string {
  if (!characterId || characterId.length > 128) throw new Error("A valid character ID is required.");
  return `mmorpg.offline-demo.v1.${characterId}`;
}

export function loadCreatedCharacters(storage: CharacterRosterStorage): CharacterPreview[] {
  const raw = storage.getItem(ROSTER_KEY);
  if (raw === null) {
    return [];
  }
  if (raw.length > MAX_ROSTER_BYTES || new TextEncoder().encode(raw).byteLength > MAX_ROSTER_BYTES) {
    throw new Error("Local character roster is too large.");
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("Local character roster is not valid JSON.");
  }
  const envelope = record(parsed, ["schemaVersion", "characters"]);
  if (envelope.schemaVersion !== ROSTER_SCHEMA_VERSION || !Array.isArray(envelope.characters)) {
    throw new Error("Unsupported local character roster schema.");
  }
  if (envelope.characters.length > MAX_CHARACTER_SLOTS - 1) {
    throw new Error("Local character roster exceeds the available slots.");
  }

  const characters = envelope.characters.map(decodeStoredCharacter);
  const ids = new Set<string>([PREVIEW_CHARACTER.id]);
  const names = new Set<string>([PREVIEW_CHARACTER.name.normalize("NFKC").toLowerCase()]);
  for (const character of characters) {
    const name = character.name.normalize("NFKC").toLowerCase();
    if (ids.has(character.id) || names.has(name)) {
      throw new Error("Local character roster contains duplicate identity.");
    }
    ids.add(character.id);
    names.add(name);
  }
  return characters;
}

export function saveCreatedCharacters(
  storage: CharacterRosterStorage,
  characters: readonly CharacterPreview[],
): void {
  if (characters.length > MAX_CHARACTER_SLOTS - 1) {
    throw new Error("Local character roster exceeds the available slots.");
  }
  const ids = new Set<string>();
  const names = new Set<string>();
  const stored = characters.map((character) => {
    const validated = localCharacterPreview(character.id, character.name, character.classId, character.sex);
    const name = validated.name.normalize("NFKC").toLowerCase();
    if (ids.has(validated.id) || names.has(name)) {
      throw new Error("Local character roster contains duplicate identity.");
    }
    ids.add(validated.id);
    names.add(name);
    return {
      id: validated.id,
      name: validated.name,
      classId: validated.classId,
      sex: validated.sex,
    };
  });
  const raw = JSON.stringify({ schemaVersion: ROSTER_SCHEMA_VERSION, characters: stored });
  if (new TextEncoder().encode(raw).byteLength > MAX_ROSTER_BYTES) {
    throw new Error("Local character roster is too large.");
  }
  storage.setItem(ROSTER_KEY, raw);
}

/**
 * True while a legacy game checkpoint, an appearance save or a key-binding profile still exists for `characterId`.
 * Such an identity must not be reissued to a new character, even when the roster
 * entry itself was lost or reset. Unreadable storage retains nothing.
 */
export function hasRetainedCharacterSaves(storage: CharacterRosterStorage, characterId: string): boolean {
  try {
    return storage.getItem(legacyCheckpointKey(characterId)) !== null ||
      storage.getItem(storageKeyForCharacter(characterId)) !== null ||
      storage.getItem(bindingProfileKey(characterId)) !== null;
  } catch {
    return false;
  }
}

/**
 * Combines the roster another tab may have written with this tab's in-memory view.
 * Stored characters come first in stored order; in-memory local characters that are
 * absent from storage are kept after them, so neither side's characters are dropped.
 */
export function mergeStoredRoster(
  inMemory: readonly CharacterPreview[],
  stored: readonly CharacterPreview[],
): CharacterPreview[] {
  const storedIds = new Set(stored.map((character) => character.id));
  return [
    PREVIEW_CHARACTER,
    ...stored,
    ...inMemory.filter((character) => character.id !== PREVIEW_CHARACTER.id && !storedIds.has(character.id)),
  ];
}
