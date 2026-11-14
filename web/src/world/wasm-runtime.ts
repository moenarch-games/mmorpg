import * as generated from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import { fetchRelease, readDerived, writeDerived } from "../asset-cache/derived";
import { loadWasm } from "../wasm";
import { LocalCharacters } from "./local-characters";
import { createLocalWorld, type LocalWorld } from "./local-world";
import { createSceneryProvider, type SceneryProvider } from "./scenery";
import { ScenarioSource, type ScenarioChoice, type ScenarioZoneHandle } from "./scenario-source";
import { markStartup } from "../startup-marks";
import type { ActionGraph, LabOptions, LabSetup } from "./action-lab";
import type { ConfigCheck, LabComparison, LabConfig, LabNetwork, LabRun, LabScenario } from "./lab-experiment";
import type { ZoneWorld } from "./world-source";
import type { PhysiqueModel } from "../physique";

/** The derived-cache name of the presented scenery's terrain grids. */
const TERRAIN_GRIDS = "terrain-grids";

/**
 * Fetches and instantiates the generated `mmorpg-wasm` module, then builds the
 * local world over this browser's character storage.
 */
export async function loadLocalWorld(characters: LocalCharacters): Promise<LocalWorld> {
  markStartup("wasm-fetch");
  await loadWasm();
  markStartup("wasm-ready");
  // The terrain grids are the slowest pure derivation of the presented scenery; a later visit reuses them per release
  // (#526). Without a manifest or storage, or on a stale or damaged entry, the module derives them.
  const release = await fetchRelease(import.meta.env.BASE_URL);
  const storage = typeof caches === "undefined" ? undefined : caches;
  const cached = release ? await readDerived(storage, location.origin, release, TERRAIN_GRIDS) : null;
  const seeded = cached !== null && generated.seedTerrainGrids(cached);
  const world = createLocalWorld(generated, characters);
  markStartup("world-created");
  if (release && !seeded) { void writeDerived(storage, location.origin, release, TERRAIN_GRIDS, generated.terrainGridCache()); }
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
 * client; call after the module loaded. `kit` is the Redbrand kit's descriptor when it was
 * fetched (#675): its outfit joins the male's and dresses the bandits. `hairKit` is the hair kit's
 * descriptor when it was fetched (#677): its pieces are the male's hair and beards.
 */
export function figureAppearance(character: string, appearance: unknown, kit: unknown = null, hairKit: unknown = null): unknown {
  const optional = (value: unknown) => (value === null ? undefined : JSON.stringify(value));
  return JSON.parse(generated.figureAppearance(character, JSON.stringify(appearance), optional(kit), optional(hairKit)));
}

/**
 * The accessory-bone chains a figure's `humanoid-appearance` declares (Atlas 6.6.39, `sway::chains_of`):
 * shared with the native client; call after the module loaded.
 */
export function declaredChains(appearance: unknown): { joints: string[]; limit: number }[] {
  return JSON.parse(generated.declaredChains(JSON.stringify(appearance))) as { joints: string[]; limit: number }[];
}

/** Shared bounded right-arm fit after the provider mixes a timed sword's authored clips. */
export function swordArmPose(name: string, progress: number, weight: number, frames: Float64Array, height: number, floor: number): Float64Array {
  return generated.swordArmPose(name, progress, weight, frames, height, floor);
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

/** `mmorpg-core`'s physique model (#634): sliders, bound and preset vectors; call after the module loaded. */
export function physiqueModel(): PhysiqueModel {
  return JSON.parse(generated.physiqueModel()) as PhysiqueModel;
}

/** `mmorpg-core`'s combined, clamped modifiers of `values` in basis points, in `PhysiqueModel.attributes` order. */
export function physiqueModifiers(values: readonly number[]): number[] {
  return Array.from(generated.physiqueModifiers(Uint16Array.from(values)));
}

/** The committed physique of a stored local character, or null if it was never played. */
export function committedPhysique(characters: LocalCharacters, characterId: string): number[] | null {
  const values = generated.committedPhysique(characters.entries, characterId);
  return values ? Array.from(values) : null;
}

/**
 * Saves `values` to a stored local character under its lease while this tab holds it; rejects
 * while another tab plays it. Resolves false, changing nothing, for a character never stored.
 */
export async function customizePhysique(characters: LocalCharacters, characterId: string, values: readonly number[]): Promise<boolean> {
  const release = await characters.hold(characterId);
  try {
    return generated.customizePhysique(characters.entries, characterId, Uint16Array.from(values), characters.nextZoneEpoch());
  } finally {
    release();
  }
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
