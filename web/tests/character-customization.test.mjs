import { describe, expect, test } from "bun:test";
import { PREVIEW_CHARACTER } from "../src/character-selection.ts";
import {
  DEFAULT_CHARACTER_APPEARANCE,
  HAT_OPTIONS,
  equipmentForAppearance,
  loadCharacter,
  rotateYawOffset,
  saveCharacter,
  storageKeyForCharacter,
} from "../src/character-customization.ts";

function memoryStorage() {
  const values = new Map();
  return {
    getItem(key) { return values.get(key) ?? null; },
    setItem(key, value) { values.set(key, value); },
  };
}

describe("character customization prototype", () => {
  test("offers the class outfit's headwear and three MVP hat styles", () => {
    expect(HAT_OPTIONS.map((option) => option.id)).toEqual([
      "class",
      "wayfarer-hood",
      "ranger-cap",
      "ironcrest-helm",
      "none",
    ]);
  });

  test("changes only the head equipment for appearance", () => {
    const equipment = equipmentForAppearance(PREVIEW_CHARACTER, { hat: "ironcrest-helm" });
    expect(equipment[0]).toEqual({ slot: "Head", name: "Ironcrest Helm", accent: "#858e91" });
    expect(equipment.slice(1)).toEqual(PREVIEW_CHARACTER.equipment.slice(1));
  });

  test("rotates attachment offsets with the avatar yaw", () => {
    expect(rotateYawOffset(0, 0.58, 0.04)).toEqual([0.58, 0.04]);

    const quarterTurn = rotateYawOffset(Math.PI / 2, 0.58, 0.04);
    expect(quarterTurn[0]).toBeCloseTo(0.04);
    expect(quarterTurn[1]).toBeCloseTo(-0.58);

    const halfTurn = rotateYawOffset(Math.PI, 0.58, 0.04);
    expect(halfTurn[0]).toBeCloseTo(-0.58);
    expect(halfTurn[1]).toBeCloseTo(-0.04);
  });

  test("round-trips a versioned character save by exact character identity", () => {
    const storage = memoryStorage();
    const saved = saveCharacter(storage, PREVIEW_CHARACTER.id, { hat: "ranger-cap", hair: "buzzed", hairColor: "blond", face: { face: "face.broad", nose: "nose.long" } });
    expect(saved).toEqual({
      schemaVersion: 2,
      characterId: PREVIEW_CHARACTER.id,
      appearance: { hat: "ranger-cap", hair: "buzzed", hairColor: "blond", face: { face: "face.broad", nose: "nose.long" } },
    });
    expect(loadCharacter(storage, PREVIEW_CHARACTER.id)).toEqual(saved);
  });

  test("returns null when no saved character exists", () => {
    expect(loadCharacter(memoryStorage(), PREVIEW_CHARACTER.id)).toBeNull();
    expect(DEFAULT_CHARACTER_APPEARANCE).toEqual({ hat: "wayfarer-hood", hair: "parted", hairColor: "brown", face: {} });
  });

  test("fails closed for mismatched or malformed saved character data", () => {
    const storage = memoryStorage();
    const key = storageKeyForCharacter(PREVIEW_CHARACTER.id);
    storage.setItem(key, JSON.stringify({
      schemaVersion: 1,
      characterId: "another-character",
      appearance: { hat: "ranger-cap" },
    }));
    expect(() => loadCharacter(storage, PREVIEW_CHARACTER.id)).toThrow("identity does not match");

    storage.setItem(key, JSON.stringify({
      schemaVersion: 1,
      characterId: PREVIEW_CHARACTER.id,
      appearance: { hat: "wizard-hat" },
    }));
    expect(() => loadCharacter(storage, PREVIEW_CHARACTER.id)).toThrow("appearance is invalid");
  });

  test("saves from before hair read with the starter hair", () => {
    const storage = memoryStorage();
    storage.setItem(storageKeyForCharacter(PREVIEW_CHARACTER.id), JSON.stringify({
      schemaVersion: 1,
      characterId: PREVIEW_CHARACTER.id,
      appearance: { hat: "ranger-cap" },
    }));
    expect(loadCharacter(storage, PREVIEW_CHARACTER.id)?.appearance).toEqual({ hat: "ranger-cap", hair: "parted", hairColor: "brown", face: {} });
  });

  test("version 1 saves read with the default face; malformed faces fail closed", () => {
    const storage = memoryStorage();
    const key = storageKeyForCharacter(PREVIEW_CHARACTER.id);
    storage.setItem(key, JSON.stringify({
      schemaVersion: 1,
      characterId: PREVIEW_CHARACTER.id,
      appearance: { hat: "none", hair: "long", hairColor: "black", face: { face: "ignored" } },
    }));
    expect(loadCharacter(storage, PREVIEW_CHARACTER.id)?.appearance.face).toEqual({});

    storage.setItem(key, JSON.stringify({
      schemaVersion: 2,
      characterId: PREVIEW_CHARACTER.id,
      appearance: { hat: "none", hair: "long", hairColor: "black", face: { eyes: 3 } },
    }));
    expect(() => loadCharacter(storage, PREVIEW_CHARACTER.id)).toThrow("face is invalid");
  });
});
