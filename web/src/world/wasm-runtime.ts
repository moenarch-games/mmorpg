import init, * as generated from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { LocalCharacters } from "./local-characters";
import { createLocalWorld, type LocalWorld } from "./local-world";

/**
 * Fetches and instantiates the generated `mmorpg-wasm` module, then builds the
 * local world over this browser's character storage.
 */
export async function loadLocalWorld(characters: LocalCharacters): Promise<LocalWorld> {
  await init();
  return createLocalWorld(generated, characters);
}

/** The area id of a stored local character's committed location, or null (#240). */
export function committedArea(characters: LocalCharacters, characterId: string): number | null {
  return generated.committedArea(characters.entries, characterId) ?? null;
}

/** The committed level of a stored local character, or null if it was never played. */
export function committedLevel(characters: LocalCharacters, characterId: string): number | null {
  return generated.committedLevel(characters.entries, characterId) ?? null;
}
