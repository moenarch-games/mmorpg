import { describe, expect, test } from "bun:test";
import corpus from "../../fixtures/character-names.json";
import { comparableName, normalizeCharacterName } from "../src/character-selection";

function stored(raw: string): string | null {
  try {
    return normalizeCharacterName(raw);
  } catch {
    return null;
  }
}

// The same corpus mmorpg-client-model's character_name.rs checks (#1245).
describe("local character-name policy", () => {
  test("keeps and refuses the corpus names as the native client does", () => {
    for (const { raw, stored: expected } of corpus.names) {
      expect([raw, stored(raw)]).toEqual([raw, expected]);
    }
  });

  test("compares names as the native client does", () => {
    for (const [a, b] of corpus.same) {
      expect(comparableName(a)).toBe(comparableName(b));
    }
    for (const [a, b] of corpus.different) {
      expect(comparableName(a)).not.toBe(comparableName(b));
    }
  });
});
