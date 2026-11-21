import type { ExplorationMap } from "../generated/mmorpg-wasm/mmorpg_wasm.js";
import type { ZoneSnapshot } from "../replication";
import { wasm } from "../wasm";
import type { MinimapExploration } from "./minimap";

// The exploration map (#983) is the shared client model's (`mmorpg-client-model`), the same the
// native client draws: named areas, the discovered count and approximate quest areas, from the
// viewer's discovered areas (waystone sheet) and quest sheet only.

export type { ExplorationMap, MapArea, QuestHint } from "../generated/mmorpg-wasm/mmorpg_wasm.js";

const NO_QUESTS = { completed: [], entries: [], markers: [] };

/** How far from the player discovered landmarks show on the minimap, in metres. */
const LANDMARK_RANGE_METRES = 160;

/** Keeps the latest exploration map, rebuilt only when the discovered areas or the quest sheet change. */
export class Exploration {
  #key = "";
  #map: ExplorationMap | null = null;

  /** The map for `projection`; null before any projection. */
  update(projection: ZoneSnapshot): ExplorationMap {
    const sheet = projection.quests ?? NO_QUESTS;
    const key = JSON.stringify([projection.viewer.discoveries, sheet.entries]);
    if (!this.#map || key !== this.#key) {
      this.#key = key;
      this.#map = wasm.explorationMap(Uint32Array.from(projection.viewer.discoveries), sheet);
    }
    return this.#map;
  }

  get map(): ExplorationMap | null {
    return this.#map;
  }

  reset(): void {
    this.#key = "";
    this.#map = null;
  }
}

/** The minimap's part of `map` around `self` (metres): every quest hint and the discovered landmarks in range. */
export function minimapExploration(map: ExplorationMap, self: { x: number; z: number }): MinimapExploration {
  return {
    hints: map.hints.map((hint) => ({ x: hint.center[0], z: hint.center[1], radius: hint.radius })),
    landmarks: map.areas
      .filter((area) => area.discovered)
      .map((area) => ({ x: (area.min[0] + area.max[0]) / 2, z: (area.min[1] + area.max[1]) / 2, name: area.name }))
      .filter((landmark) => Math.hypot(landmark.x - self.x, landmark.z - self.z) <= LANDMARK_RANGE_METRES),
  };
}
