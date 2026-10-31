import * as generated from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { loadWasm } from "../wasm";
import { LocalCharacters } from "./local-characters";
import { createLocalWorld, type LocalWorld } from "./local-world";
import { createSceneryProvider, type SceneryProvider } from "./scenery";
import { ScenarioSource, type ScenarioChoice, type ScenarioZoneHandle } from "./scenario-source";
import { markStartup } from "../startup-marks";
import type { ActionGraph, LabOptions, LabSetup } from "./action-lab";
import type { ConfigCheck, LabComparison, LabConfig, LabNetwork, LabRun, LabScenario } from "./lab-experiment";
import type { ZoneWorld } from "./world-source";

/**
 * Fetches and instantiates the generated `mmorpg-wasm` module, then builds the
 * local world over this browser's character storage.
 */
export async function loadLocalWorld(characters: LocalCharacters): Promise<LocalWorld> {
  markStartup("wasm-fetch");
  await loadWasm();
  markStartup("wasm-ready");
  const world = createLocalWorld(generated, characters);
  markStartup("world-created");
  return world;
}

/**
 * Presents zone `zoneId` (#145: an online character went to another zone's host) and returns that
 * zone's scenery. The page's worlds share the module's presentation, so earlier providers describe
 * it too from now on; call after the module loaded.
 */
export function presentZone(zoneId: number): SceneryProvider {
  generated.presentZone(zoneId);
  return createSceneryProvider(generated);
}

/** The combat-sounds SoundManifest with mmorpg's cues added (#447); call after the module loaded. */
export function soundManifest(bundle: string): string {
  return generated.soundManifest(bundle);
}

/**
 * The `humanoid-appearance` the Ranger figure `character` draws from (#39, #493): the bundle's
 * own, with the game's humanoid enemy presets beside the male figure's, shared with the native
 * client; call after the module loaded.
 */
export function figureAppearance(character: string, appearance: unknown): unknown {
  return JSON.parse(generated.figureAppearance(character, JSON.stringify(appearance)));
}

/**
 * A unit's own seeded variation of the preset `id` (asset-tooling #380) over `appearance`: the
 * same man the native client draws; call after the module loaded.
 */
export function varyPreset(appearance: unknown, id: string, seed: number): unknown {
  return JSON.parse(generated.varyPreset(JSON.stringify(appearance), id, seed));
}

/** The authored action graph and the lab's setup choices (#381); call after the module loaded. */
export function actionLabDefinitions(): { graph: ActionGraph; options: LabOptions } {
  return { graph: JSON.parse(generated.actionGraph()) as ActionGraph, options: JSON.parse(generated.actionLabOptions()) as LabOptions };
}

/** What the Action Lab's experiments (#382) run in the shared simulation; call after the module loaded. */
export type LabExperiments = {
  /** The production graph as an editable configuration. */
  baseline(): LabConfig;
  check(config: LabConfig): ConfigCheck;
  /** The rule values that differ from `from` to `to`, as `path: old → new` lines (#383). */
  changes(from: LabConfig, to: LabConfig): string[];
  /** Replays `scenario` under `config` across a simulated `network` (#415). */
  run(scenario: LabScenario, config: LabConfig, network: LabNetwork): LabRun;
  compare(scenario: LabScenario, a: LabConfig, b: LabConfig, network: LabNetwork): LabComparison;
  /** Fresh lab worlds of `setup` and `seed` under `config` (production rules for null). */
  zoneFactory(setup: LabSetup, seed: bigint, config: LabConfig | null): () => ScenarioZoneHandle;
  contentRevision(): string;
};

export const labExperiments: LabExperiments = {
  baseline: () => JSON.parse(generated.actionLabBaseline()) as LabConfig,
  check: (config) => JSON.parse(generated.actionLabCheck(JSON.stringify(config))) as ConfigCheck,
  changes: (from, to) => JSON.parse(generated.actionLabChanges(JSON.stringify(from), JSON.stringify(to))) as string[],
  run: (scenario, config, network) =>
    JSON.parse(generated.actionLabRun(JSON.stringify(scenario), JSON.stringify(config), JSON.stringify(network))) as LabRun,
  compare: (scenario, a, b, network) =>
    JSON.parse(generated.actionLabCompare(JSON.stringify(scenario), JSON.stringify(a), JSON.stringify(b), JSON.stringify(network))) as LabComparison,
  zoneFactory: (setup, seed, config) => () => generated.LocalZone.forActionLab(
    setup.className, setup.level, setup.gear, setup.mainHand, setup.resource ?? undefined, seed, config ? JSON.stringify(config) : undefined),
  contentRevision: () => {
    const zone = new generated.LocalZone();
    try { return String(zone.contentRevision()); } finally { zone.free(); }
  },
};

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
    const create = labExperiments.zoneFactory(lab, choice.seed, null);
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
