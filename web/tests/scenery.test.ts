import { describe, expect, test } from "bun:test";
import { createLocalWorld, type LocalZoneModule } from "../src/world/local-world";
import { PROP_KINDS, createSceneryProvider, decodeScenery } from "../src/world/scenery";
import { catalogJson } from "./support/catalog";
import { fixtureExport, fixtureScenery } from "./support/scenery-fixture";

const decode = (patch: Record<string, unknown> = {}) => fixtureScenery(patch);

describe("scenery export boundary (format v3)", () => {
  test("decodes terrain, the far ring, prop records, structures, roads and areas", () => {
    const scenery = decode();
    expect(scenery.contentRevision).toBe(7n);
    expect(scenery.terrain.columns).toBe(9);
    expect(scenery.farTerrain.columns).toBe(5);
    expect(scenery.props.map((prop) => prop.kind)).toEqual([...PROP_KINDS]);
    const keep = scenery.props[0]!;
    expect(keep).toEqual({
      kind: "keep",
      position: [-1_200, 0, -1_200],
      yaw: 0,
      scale: 1,
      halfExtents: [900, 500, 600],
      collider: { id: 100, center: [-1_200, 500, -1_200], halfExtents: [900, 500, 600] },
    });
    const grass = scenery.props.find((prop) => prop.kind === "grass-tuft")!;
    expect(grass.collider).toBeNull();
    expect(grass.scale).toBeCloseTo(1.1, 9);
    expect(scenery.roads).toEqual([{ name: "Test Road", halfWidth: 150, points: [[-1_600, 0], [0, 0], [1_600, 400]] }]);
    expect(scenery.areas[0]?.name).toBe("Greyhaven Outpost");
  });

  test("record order in the kind table does not matter, names do", () => {
    const reversed = [...PROP_KINDS].reverse();
    const props = (fixtureExport().props as number[][]).map(([kind, ...rest]) => [PROP_KINDS.length - 1 - kind!, ...rest]);
    expect(decode({ propKinds: reversed, props }).props.map((prop) => prop.kind)).toEqual([...PROP_KINDS]);
  });

  test("fails closed on unknown or malformed content", () => {
    const base = fixtureExport();
    const terrain = base.terrain as Record<string, unknown>;
    const [record] = base.props as number[][];
    const [structure] = base.structures as Record<string, unknown>[];
    for (const patch of [
      { format: "other" },
      { version: 1 },
      { version: 2 },
      { presentationFingerprint: 1 },
      { presentationFingerprint: "0123456789ABCDEf" },
      { presentationFingerprint: "123456789abcdef" },
      { contentRevision: 7 },
      { contentRevision: "07" },
      { extra: true },
      { propKinds: [...PROP_KINDS, "dragon"] },
      { propKinds: [...PROP_KINDS, "keep"] },
      { props: [[99, 0, 0, 0, 0, 1_000, 1, 1, 1]] },
      { props: [[0, 0, 0, 0, 0, 1_000, 0, 1, 1]] },
      { props: [[0, 0, 0, 0, 65_536, 1_000, 1, 1, 1]] },
      { props: [[0, 0, 0, 0, 0, 0, 1, 1, 1]] },
      { props: [[0, 0, 0, 0, 0, 1_000, 1, 1]] },
      { props: [[0, 0.5, 0, 0, 0, 1_000, 1, 1, 1]] },
      { props: [record], structures: [structure, structure] },
      { structures: [{ ...structure, prop: 999 }] },
      { structures: [{ ...structure, extra: 1 }] },
      { roads: [{ name: "Stub", halfWidth: 150, points: [[0, 0]] }] },
      { roads: [{ name: "Thin", halfWidth: 0, points: [[0, 0], [1, 1]] }] },
      { terrain: { ...terrain, heights: [0] } },
      { terrain: { ...terrain, biomes: (terrain.biomes as number[]).map(() => 42) } },
      { farTerrain: undefined },
      { areas: [{ id: 1, name: "Inverted", minXz: [1, 0], maxXz: [0, 0] }] },
      { biomes: [{ id: 0, name: "a", color: "#000000" }, { id: 0, name: "b", color: "#000000" }] },
    ]) {
      expect(() => decode(patch), JSON.stringify(patch).slice(0, 80)).toThrow("Invalid scenery");
    }
    expect(() => decodeScenery("{")).toThrow("Invalid scenery");
  });
});

describe("scenery provider and local world", () => {
  const content = (areaId: number | undefined) => ({
    scenery: () => JSON.stringify(fixtureExport()),
    areaAt: () => areaId,
    reliefAt: (x: number, z: number) => x + z,
  });

  test("routes area and relief queries to the WASM owners", () => {
    expect(createSceneryProvider(content(1)).areaAt(0, 0)?.name).toBe("Greyhaven Outpost");
    expect(createSceneryProvider(content(undefined)).areaAt(0, 0)).toBeNull();
    expect(() => createSceneryProvider(content(9)).areaAt(0, 0)).toThrow("missing");
    expect(createSceneryProvider(content(1)).reliefAt(1.4, 2.6)).toBe(4);
  });

  test("refuses scenery or a catalog from another content revision than the zone", () => {
    const module = (revision: bigint, catalogRevision = "7"): LocalZoneModule => ({
      ...content(1),
      catalog: () => JSON.stringify(catalogJson(catalogRevision)),
      LocalZone: class {
        join() { return 1; }
        leave() { return true; }
        submit() { return true; }
        tick() { return 1n; }
        projection(): Uint8Array { throw new Error("unused"); }
        contentRevision() { return revision; }
      },
    });
    expect(() => createLocalWorld(module(8n))).toThrow("Scenery content revision 7 does not match");
    expect(() => createLocalWorld(module(7n, "8"))).toThrow("Catalog content revision 8 does not match");
    const world = createLocalWorld(module(7n));
    expect(world.scenery.scenery.contentRevision).toBe(7n);
    expect(world.catalog.creatureTemplates.get(1)?.name).toBe("Timber Wolf");
  });
});
