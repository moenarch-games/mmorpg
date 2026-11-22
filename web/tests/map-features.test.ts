import { describe, expect, test } from "bun:test";
import { areaPolygon, metresPerPixel, pointMarker, POINT_STYLES, type PointKind } from "../src/world/map-features";

const flat = (x: number, z: number): [number, number] => [x, -z];

describe("world map zone features (#1063)", () => {
  test("an area's open rings become closed polygon rings, holes kept", () => {
    const outer = [[0, 0], [10, 0], [10, 10], [0, 10]];
    const hole = [[2, 2], [4, 2], [4, 4]];
    const feature = areaPolygon("area-1", [outer, hole], flat, { name: "Vale" });
    expect(feature.geometry.type).toBe("Polygon");
    const rings = (feature.geometry as GeoJSON.Polygon).coordinates;
    expect(rings).toHaveLength(2);
    expect(rings[0]).toHaveLength(5);
    expect(rings[0]?.[0]).toEqual(rings[0]?.[4]);
    expect(rings[0]?.[2]).toEqual([10, -10]);
    expect(rings[1]).toHaveLength(4);
    expect(feature.properties.name).toBe("Vale");
  });

  test("every point kind has its own closed marker shape and style", () => {
    const kinds = Object.keys(POINT_STYLES) as PointKind[];
    expect(kinds).toEqual(["waystone", "dungeon", "landmark", "service", "quest"]);
    const shapes = kinds.map((kind) => {
      const marker = pointMarker(`p-${kind}`, kind, [0, 0], 20, { name: kind });
      const ring = (marker.geometry as GeoJSON.Polygon).coordinates[0] ?? [];
      expect(ring[0]).toEqual(ring[ring.length - 1]);
      expect(marker.properties.kind).toBe(kind);
      // About `size` metres across: no vertex further than the size from the centre.
      for (const [lon = 0, lat = 0] of ring) expect(Math.hypot(lon, lat) * 111_319.49).toBeLessThanOrEqual(20);
      return JSON.stringify(ring);
    });
    expect(new Set(shapes).size).toBe(kinds.length);
    expect(new Set(kinds.map((kind) => POINT_STYLES[kind].fill)).size).toBe(kinds.length);
  });

  test("markers keep their screen size as the map zooms", () => {
    expect(metresPerPixel(13) / metresPerPixel(14)).toBeCloseTo(2);
  });
});
