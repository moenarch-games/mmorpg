/** The tile pyramid's whole Web Mercator world is this square, centred on the origin (metres). */
const HALF_WORLD = 1024;
/** MapLibre's world is 512 CSS pixels wide at zoom 0, whatever the raster tile size. */
const WORLD_PIXELS_AT_ZOOM_0 = 512;

export function worldToLngLat(x: number, z: number): [number, number] {
  return [(x / HALF_WORLD) * 180, (Math.atan(Math.sinh((-z / HALF_WORLD) * Math.PI)) * 180) / Math.PI];
}

export function lngLatToWorld(longitude: number, latitude: number): { x: number; z: number } {
  const mercator = Math.log(Math.tan(Math.PI / 4 + (latitude * Math.PI) / 360));
  return { x: (longitude / 180) * HALF_WORLD, z: (-mercator / Math.PI) * HALF_WORLD };
}

export function metresPerPixel(zoom: number): number {
  return (HALF_WORLD * 2) / (WORLD_PIXELS_AT_ZOOM_0 * 2 ** zoom);
}

export type Selection = { minX: number; minZ: number; maxX: number; maxZ: number; pixelsPerMetre: number };

/** A whole-metre square of `size` metres around a point, as the backend accepts. */
export function selectionAround(x: number, z: number, size: number, pixelsPerMetre: number): Selection {
  const minX = Math.round(x - size / 2);
  const minZ = Math.round(z - size / 2);
  return { minX, minZ, maxX: minX + size, maxZ: minZ + size, pixelsPerMetre };
}
