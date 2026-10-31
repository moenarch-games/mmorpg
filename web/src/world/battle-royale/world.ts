import { ArenaPresentation, LocalZone, UnitPresenter } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { decodeCatalog } from "../catalog";
import type { LocalWorld } from "../local-world";
import { decodeScenery, type SceneryProvider } from "../scenery";
import type { ZoneServer } from "../online-config";
import { ScenarioSource } from "../scenario-source";
import { webTransportConnector } from "../../session/webtransport";
import { classChoiceCodes, type JoinCharacter, type ZoneWorld } from "../world-source";
import type { ArenaEquipment } from "./protocol";
import { BattleRoyaleSource } from "./source";

export type ArenaWorld = {
  world: ZoneWorld;
  source: BattleRoyaleSource;
  maximumLevel: number;
  standardEquipment(character: JoinCharacter): ArenaEquipment;
};

/** The local practice arena (#550): the shared zone simulation in WASM, reset to a fresh life. */
export type PracticeWorld = { world: ZoneWorld; source: ScenarioSource };

/** Values come from Rust admission defaults, not a second client-side gear catalog. */
export function equipmentSlots(ids: ArrayLike<number>): ArenaEquipment {
  if (ids.length !== 6) { throw new Error("The arena build must have six equipment slots."); }
  const slot = (index: number): number | null => ids[index] === 0 ? null : ids[index]!;
  return [slot(0), slot(1), slot(2), slot(3), slot(4), slot(5)];
}

/** The shared fortress drawn as graybox (#552): the same Rust meshes and unit presenters as the ordinary world. */
function arenaScenery(local: LocalWorld, presentation: ArenaPresentation): SceneryProvider {
  const emptyMotion = new Float32Array();
  return {
    scenery: decodeScenery(presentation.scenery()),
    staticScene: {
      staticScene: () => presentation.staticScene(),
      staticSceneVertices: () => presentation.staticSceneVertices(),
      staticSceneIndices: () => presentation.staticSceneIndices(),
      staticSceneModels: () => presentation.staticSceneModels(),
      staticSceneMotion: () => emptyMotion,
      staticSceneShown: () => new Uint8Array(),
      surfaceY: () => 0,
    },
    unitPresenter: (player) => UnitPresenter.forArena(player),
    buildings: { presenters: () => [], reachableDoor: () => 0 },
    shrine: { entries: [], placements: [] },
    outpost: { entries: [], placements: [] },
    settlement: { entries: [], placements: [] },
    nature: { entries: [], placements: [], lods: () => { throw new Error("The arena has no trees or rocks."); } },
    ambient: { entries: [], clips: [], anchors: [], frame: () => emptyMotion },
    importedRestClip: (kind) => local.scenery.importedRestClip(kind),
    areaAt: () => null,
    reachablePieces: () => [],
    raidArea: null,
    raidAid: [],
    raidPrep: null,
    waystoneAt: () => 0,
    waystones: [],
    reliefAt: () => 0,
    regionModels: () => [],
  };
}

/** Called after the ordinary world initializes WASM. Models and animation use the same presenters. */
export function createArenaWorld(
  local: LocalWorld,
  server: ZoneServer,
  selectedEquipment: (character: JoinCharacter) => ArenaEquipment | null,
): ArenaWorld {
  const presentation = new ArenaPresentation();
  const standardEquipment = (character: JoinCharacter): ArenaEquipment =>
    equipmentSlots(presentation.standardEquipment(classChoiceCodes(character).classId));
  const source = new BattleRoyaleSource({
    route: server.route,
    connector: webTransportConnector(server.certificateHash),
    contentRevision: presentation.contentRevision(),
    equipment: (character) => selectedEquipment(character) ?? standardEquipment(character),
  });
  return {
    world: { source, scenery: arenaScenery(local, presentation), catalog: decodeCatalog(presentation.catalog()), name: "Battle Royale", arena: true },
    source,
    maximumLevel: presentation.maximumLevel(),
    standardEquipment,
  };
}

/**
 * The practice arena (#550): no match host, account or stored character. Each join or reset
 * admits `build()` (read then, so builds change only between lives) at the maximum level in front
 * of stationary training targets; combat and health are the shared `mmorpg-core` rules.
 */
export function createPracticeWorld(
  local: LocalWorld,
  build: () => { character: JoinCharacter; equipment: ArenaEquipment },
): PracticeWorld {
  const presentation = new ArenaPresentation();
  const source = new ScenarioSource(() => {
    const { character, equipment } = build();
    const codes = classChoiceCodes(character);
    return LocalZone.forPracticeArena(codes.classId, codes.sex, Uint16Array.from(equipment, (id) => id ?? 0));
  });
  return {
    world: { source, scenery: arenaScenery(local, presentation), catalog: decodeCatalog(presentation.catalog()), name: "Battle Royale practice", arena: true },
    source,
  };
}
