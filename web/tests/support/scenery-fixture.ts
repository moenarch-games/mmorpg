import { PROP_KINDS, decodeScenery, type PropKind, type Scenery } from "../../src/world/scenery";

/**
 * A small `mmorpg.scenery` v2 export: a 9 × 9 terrain grid every 4 m with a
 * plaza, a road, a field and a lake, a coarse far ring, one prop of every
 * kind (structures carry their collider box) and one road.
 */
const BIOMES = [
  { id: 0, name: "meadow", color: "#5c843e" },
  { id: 1, name: "road", color: "#8c704c" },
  { id: 2, name: "plaza", color: "#96886e" },
  { id: 3, name: "shore", color: "#c4b484" },
  { id: 4, name: "lake bed", color: "#787058" },
  { id: 5, name: "farmland", color: "#705436" },
  { id: 6, name: "woods", color: "#3e502c" },
  { id: 7, name: "rock", color: "#7c7870" },
  { id: 8, name: "snow", color: "#eceef2" },
];

/** Structure kinds get a collider; the box is centred on the prop, standing on its feet. */
const STRUCTURES: ReadonlySet<PropKind> = new Set([
  "keep", "inn", "house", "smithy", "barn", "farmhouse", "windmill", "well", "palisade", "gate-post", "waystone",
  "gravestone", "cliff", "mine-entrance", "tent", "crate", "campfire", "rock-large",
]);

const HALF_EXTENTS: Partial<Record<PropKind, [number, number, number]>> = {
  keep: [900, 500, 600],
  inn: [700, 300, 500],
  house: [400, 250, 300],
  smithy: [300, 225, 350],
  barn: [600, 350, 800],
  farmhouse: [400, 250, 350],
  windmill: [250, 400, 250],
  well: [100, 50, 100],
  palisade: [1_000, 150, 30],
  "gate-post": [40, 200, 40],
  waystone: [50, 125, 50],
  gravestone: [30, 50, 10],
  cliff: [500, 600, 400],
  "mine-entrance": [300, 210, 50],
  tent: [150, 110, 150],
  crate: [50, 50, 50],
  campfire: [60, 15, 60],
  "crop-row": [2_090, 25, 30],
  dock: [500, 15, 90],
  "tree-oak": [40, 250, 40],
  "tree-pine": [35, 250, 35],
  "tree-birch": [30, 250, 30],
};

export function fixtureExport(patch: Record<string, unknown> = {}): Record<string, unknown> {
  const columns = 9;
  const heights: number[] = [];
  const biomes: number[] = [];
  for (let row = 0; row < columns; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      // A gentle rise toward +X with rock and snow on the last columns.
      heights.push(column >= 7 ? (column - 6) * 2_600 : (column * 7 + row * 3) % 40);
      biomes.push(column === 8 ? 8 : column === 7 ? 7 : row === 4 ? 1 : column <= 1 && row <= 2 ? 2 : column >= 5 && row >= 6 ? 5 : row <= 1 && column >= 4 ? 4 : row === 2 && column >= 4 ? 3 : column === 0 && row >= 6 ? 6 : 0);
    }
  }
  const far = 5;
  const farHeights = Array.from({ length: far * far }, (_, index) => 3_000 + (index % far) * 2_000);
  const farBiomes = farHeights.map((height) => (height >= 5_000 ? 8 : 7));
  const props: number[][] = [];
  const structures: Record<string, unknown>[] = [];
  PROP_KINDS.forEach((kind, index) => {
    const [hx, hy, hz] = HALF_EXTENTS[kind] ?? [40, 30, 40];
    const x = -1_200 + (index % 6) * 500;
    const z = -1_200 + Math.floor(index / 6) * 500;
    const yaw = STRUCTURES.has(kind) ? 0 : (index * 7_919) % 65_536;
    props.push([index, x, 0, z, yaw, STRUCTURES.has(kind) ? 1_000 : 900 + index * 10, hx, hy, hz]);
    if (STRUCTURES.has(kind)) {
      structures.push({ prop: index, colliderId: 100 + index, center: [x, hy, z], halfExtents: [hx, hy, hz] });
    }
  });
  return {
    format: "mmorpg.scenery",
    version: 3,
    source: "fixture",
    contentRevision: "7",
    presentationFingerprint: "0123456789abcdef",
    unitsPerMetre: 100,
    playerHalfExtents: [30, 90, 30],
    terrain: { originXz: [-1_600, -1_600], step: 400, columns, rows: columns, heights, biomes },
    farTerrain: { originXz: [-4_000, -4_000], step: 2_000, columns: far, rows: far, heights: farHeights, biomes: farBiomes },
    biomes: BIOMES,
    propKinds: [...PROP_KINDS],
    props,
    structures,
    roads: [{ name: "Test Road", halfWidth: 150, points: [[-1_600, 0], [0, 0], [1_600, 400]] }],
    water: [{ centerXz: [600, -1_200], radiiXz: [700, 400], surfaceY: 15 }],
    areas: [{ id: 1, name: "Greyhaven Outpost", minXz: [-1_100, -1_100], maxXz: [7_300, 4_100] }],
    ...patch,
  };
}

export function fixtureScenery(patch: Record<string, unknown> = {}): Scenery {
  return decodeScenery(JSON.stringify(fixtureExport(patch)));
}
