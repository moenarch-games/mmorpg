/**
 * The world map's zone features (#1063) as GeoJSON for the Maps library: the named areas'
 * traced outlines and one marker shape per point-of-interest kind. The Maps library draws
 * points only as circles, so each kind's marker is a small polygon sized in screen pixels,
 * rebuilt when the zoom changes. Pure: the caller supplies where a zone position lies.
 */

/** `[longitude, latitude]` in degrees. */
export type LonLat = [number, number];
/** Where a zone position (metres, x east, z south) lies on the map. */
export type Project = (x: number, z: number) => LonLat;
export type Feature = GeoJSON.Feature<GeoJSON.Geometry, Record<string, unknown>>;
/** The exploration map's point kinds, as `mmorpg-client-model` names them. */
export type PointKind = "waystone" | "dungeon" | "landmark" | "service" | "quest";

/** Each kind's fill and outline; the legend uses the same. */
export const POINT_STYLES: Record<PointKind, { fill: string; stroke: string; label: string }> = {
  waystone: { fill: "#6ec8e6", stroke: "#1d1a14", label: "Waystone" },
  dungeon: { fill: "#5c463a", stroke: "#f0e1c8", label: "Cave or hold" },
  landmark: { fill: "#c8c4ba", stroke: "#1d1a14", label: "Landmark" },
  service: { fill: "#e8b240", stroke: "#1d1a14", label: "Service" },
  quest: { fill: "#ffd34d", stroke: "#1d1a14", label: "Quest" },
};

const DEGREES_PER_METRE = 1 / 111_319.49; // at the equator, where the map sits

/** An area's rings (metres, each open) as a map polygon whose rings are closed. */
export function areaPolygon(id: string, rings: readonly (readonly (readonly number[])[])[], project: Project, properties: Record<string, unknown>): Feature {
  const coordinates = rings.map((ring) => {
    const closed = ring.length > 0 ? [...ring, ring[0] as readonly number[]] : [];
    return closed.map(([x = 0, z = 0]) => project(x, z));
  });
  return { type: "Feature", id, geometry: { type: "Polygon", coordinates }, properties };
}

/** The unit outline of a kind's marker, x east and y north, about one unit across. */
function outline(kind: PointKind): [number, number][] {
  const around = (count: number, radius: (i: number) => number, turn = 0) =>
    Array.from({ length: count }, (_, i): [number, number] => {
      const angle = turn + (i / count) * 2 * Math.PI;
      return [radius(i) * Math.sin(angle), radius(i) * Math.cos(angle)];
    });
  switch (kind) {
    case "waystone":
      return [[0, 0.62], [0.38, 0], [0, -0.62], [-0.38, 0]];
    case "dungeon":
      return [[0, 0.5], [0.55, -0.42], [-0.55, -0.42]];
    case "landmark":
      return [[-0.4, -0.4], [0.4, -0.4], [0.4, 0.4], [-0.4, 0.4]];
    case "service":
      return around(10, () => 0.42);
    case "quest":
      return around(10, (i) => (i % 2 === 0 ? 0.6 : 0.26));
  }
}

/** A marker for a point of `kind` at `at`, `size` metres across, as a map polygon. */
export function pointMarker(id: string, kind: PointKind, at: LonLat, size: number, properties: Record<string, unknown>): Feature {
  const [lon, lat] = at;
  const scale = size * DEGREES_PER_METRE;
  const ring = outline(kind).map(([east, north]): LonLat => [lon + east * scale, lat + north * scale]);
  ring.push(ring[0] as LonLat);
  return { type: "Feature", id, geometry: { type: "Polygon", coordinates: [ring] }, properties: { ...properties, kind } };
}

/** Metres per screen pixel at `zoom` (Web Mercator, 512 px tiles, at the equator). */
export function metresPerPixel(zoom: number): number {
  return 40_075_016.7 / (512 * 2 ** zoom);
}
