import init, * as generated from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { LocalCharacters } from "./local-characters";
import { createLocalWorld, type LocalWorld } from "./local-world";
import { ScenarioSource, type ScenarioChoice } from "./scenario-source";
import { markStartup } from "../startup-marks";
import type { ActionGraph, LabOptions } from "./action-lab";
import type { ZoneWorld } from "./world-source";

/**
 * Fetches and instantiates the generated `mmorpg-wasm` module, then builds the
 * local world over this browser's character storage.
 */
export async function loadLocalWorld(characters: LocalCharacters): Promise<LocalWorld> {
  markStartup("wasm-fetch");
  await init();
  markStartup("wasm-ready");
  const world = createLocalWorld(generated, characters);
  markStartup("world-created");
  return world;
}

/** The authored action graph and the lab's setup choices (#381); call after the module loaded. */
export function actionLabDefinitions(): { graph: ActionGraph; options: LabOptions } {
  return { graph: JSON.parse(generated.actionGraph()) as ActionGraph, options: JSON.parse(generated.actionLabOptions()) as LabOptions };
}

/** The area id of a stored local character's committed location, or null (#240). */
export function committedArea(characters: LocalCharacters, characterId: string): number | null {
  return generated.committedArea(characters.entries, characterId) ?? null;
}

/** The committed level of a stored local character, or null if it was never played. */
export function committedLevel(characters: LocalCharacters, characterId: string): number | null {
  return generated.committedLevel(characters.entries, characterId) ?? null;
}

/**
 * The review scenario world (#371) over the loaded local world's scenery and
 * catalog: a fresh scenario zone per join, nothing stored. Throws for an
 * unknown scenario name.
 */
export function createScenarioWorld(local: LocalWorld, choice: ScenarioChoice): { world: ZoneWorld; source: ScenarioSource; area: number | null } {
  const lab = choice.lab;
  if (lab) {
    const create = () => generated.LocalZone.forActionLab(lab.className, lab.level, lab.gear, lab.mainHand, lab.resource ?? undefined, choice.seed);
    // Validate the setup now, so an illegal one is reported before entry.
    create().free();
    const source = new ScenarioSource(create);
    return { world: { ...local, source }, source, area: generated.actionLabArea() ?? null };
  }
  const area = generated.scenarioArea(choice.name);
  // Validate the name now, so a typo is reported before entry.
  generated.LocalZone.forScenario(choice.name, choice.seed).free();
  const source = new ScenarioSource(() => generated.LocalZone.forScenario(choice.name, choice.seed));
  return { world: { ...local, source }, source, area: area ?? null };
}
