import { ArenaPresentation, UnitPresenter } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { decodeCatalog } from "../catalog";
import type { LocalWorld } from "../local-world";
import { decodeScenery, type SceneryProvider } from "../scenery";
import type { ZoneServer } from "../online-config";
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

/** Values come from Rust admission defaults, not a second client-side gear catalog. */
export function equipmentSlots(ids: ArrayLike<number>): ArenaEquipment {
  if (ids.length !== 6) { throw new Error("The arena build must have six equipment slots."); }
  const slot = (index: number): number | null => ids[index] === 0 ? null : ids[index]!;
  return [slot(0), slot(1), slot(2), slot(3), slot(4), slot(5)];
}

/** Called after the ordinary world initializes WASM. Models and animation use the same presenters. */
export function createArenaWorld(
  local: LocalWorld,
  server: ZoneServer,
  selectedEquipment: (character: JoinCharacter) => ArenaEquipment | null,
): ArenaWorld {
  const presentation = new ArenaPresentation();
  const emptyMotion = new Float32Array();
  const scenery: SceneryProvider = {
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
    inn: { entries: () => [], presenter: () => ({ frame: () => emptyMotion }), reachableDoor: () => 0 },
    shrine: { entries: [], placements: [] },
    outpost: { entries: [], placements: [] },
    ambient: { entries: [], clips: [], anchors: [], frame: () => emptyMotion },
    importedRestClip: (kind) => local.scenery.importedRestClip(kind),
    areaAt: () => null,
    reachablePieces: () => [],
    reliefAt: () => 0,
    regionModels: () => [],
  };
  const standardEquipment = (character: JoinCharacter): ArenaEquipment =>
    equipmentSlots(presentation.standardEquipment(classChoiceCodes(character).classId));
  const source = new BattleRoyaleSource({
    route: server.route,
    connector: webTransportConnector(server.certificateHash),
    equipment: (character) => selectedEquipment(character) ?? standardEquipment(character),
  });
  return {
    world: { source, scenery, catalog: decodeCatalog(presentation.catalog()), name: "Battle Royale" },
    source,
    maximumLevel: presentation.maximumLevel(),
    standardEquipment,
  };
}
