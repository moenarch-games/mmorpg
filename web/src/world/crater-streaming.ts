import type { Area } from "./scenery";
import type { PlacedKitSource } from "./placed-kit";

/** The authored Outpost area's boundary is a conservative proxy for the basin floor (#423).
 * It is only an art-loading hint: the zone's terrain and collision remain authoritative. */
const OUTPOST_NAME = "Greyhaven Outpost";
/** Begin fetching beyond the cliffs before a player reaches the edge (18 m of lead time). */
export const EXIT_LOOKAHEAD_UNITS = 1_800;

export function isOutpostArea(area: Area | undefined): boolean {
  return area?.name === OUTPOST_NAME;
}

/** At the sheltered village floor, nearby area bounds must not trigger hidden rim art. */
export function shelteredOutpost(at: readonly [number, number], areas: readonly Area[]): Area | null {
  const outpost = areas.find((area) => isOutpostArea(area));
  if (!outpost) { return null; }
  const edge = Math.min(
    at[0] - outpost.minXz[0], outpost.maxXz[0] - at[0],
    at[1] - outpost.minXz[1], outpost.maxXz[1] - at[1],
  );
  return edge > EXIT_LOOKAHEAD_UNITS ? outpost : null;
}

/** Whether a placed model's anchor is within `paddingUnits` of an area's XZ bounds. */
export function kitNearArea(source: PlacedKitSource, area: Area, unitsPerMetre: number, paddingUnits: number): boolean {
  return source.placements.some(({ matrix }) => {
    const x = matrix[12]! * unitsPerMetre;
    const z = matrix[14]! * unitsPerMetre;
    const dx = Math.max(area.minXz[0] - x, 0, x - area.maxXz[0]);
    const dz = Math.max(area.minXz[1] - z, 0, z - area.maxXz[1]);
    return dx * dx + dz * dz <= paddingUnits * paddingUnits;
  });
}

/** Trigger a not-yet-needed kit when walking near one of its real placements. */
export function kitNearPoint(source: PlacedKitSource, at: readonly [number, number], unitsPerMetre: number, radiusUnits: number): boolean {
  return source.placements.some(({ matrix }) => {
    const dx = matrix[12]! * unitsPerMetre - at[0];
    const dz = matrix[14]! * unitsPerMetre - at[1];
    return dx * dx + dz * dz <= radiusUnits * radiusUnits;
  });
}
