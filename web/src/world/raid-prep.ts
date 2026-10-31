import type { RaidNotice, RaidSignal } from "../replication";

/**
 * Forewarning and preparation before a night raid (#532, Atlas 5.1.59–5.1.62): on some evenings
 * the projection's raid signal says the scouts' tracks lie on the approach, whether the viewer
 * read them, and which preparations the zone accepted for the coming raid. Interact in reach asks
 * the zone to take the next step; the zone checks again and owns the outcome. No timer, target
 * or guarantee is shown: the signs may come to nothing.
 */

/** The zone's reach for the tracks and the braces (3 m) and for talking to the lookout (5 m), in units. */
export const TRACKS_REACH_UNITS = 300;
export const BRACE_REACH_UNITS = 300;
export const LOOKOUT_REACH_UNITS = 500;
/** The readiness bit of the warned guards; brace `k` is bit `k + 1`. */
export const READINESS_WARNED = 1;

export interface RaidPrepSpots {
  readonly tracks: { readonly x: number; readonly z: number };
  /** The guard who takes reports. */
  readonly lookout: number;
  readonly braces: readonly { readonly x: number; readonly z: number }[];
}

/** Reads `raidPrep()`: tracks `[x, z]`, the lookout's NPC ID, then each brace's `[x, z]`. */
export function prepSpots(flat: ArrayLike<number>): RaidPrepSpots | null {
  if (flat.length < 3) {
    return null;
  }
  const braces = Array.from({ length: Math.floor((flat.length - 3) / 2) }, (_, index) => ({ x: flat[3 + index * 2]!, z: flat[4 + index * 2]! }));
  return { tracks: { x: flat[0]!, z: flat[1]! }, lookout: flat[2]!, braces };
}

function within(at: { x: number; z: number }, x: number, z: number, reach: number): boolean {
  return (at.x - x) ** 2 + (at.z - z) ** 2 <= reach ** 2;
}

/**
 * The `RaidPrepare` step feet at `(x, z)` units can take, or null: 1 reports read tracks to the
 * lookout standing at `lookoutAt`, 0 reads the tracks, `2 + k` braces barn wall `k` once warned.
 */
export function reachablePrepStep(signal: RaidSignal | null | undefined, spots: RaidPrepSpots | null, x: number, z: number, lookoutAt: { x: number; z: number } | null): number | null {
  if (!signal?.signs || !spots) {
    return null;
  }
  const warned = (signal.readiness & READINESS_WARNED) !== 0;
  if (signal.scouted && !warned && lookoutAt && within(lookoutAt, x, z, LOOKOUT_REACH_UNITS)) {
    return 1;
  }
  if (!signal.scouted && within(spots.tracks, x, z, TRACKS_REACH_UNITS)) {
    return 0;
  }
  const brace = warned ? spots.braces.findIndex((at, index) => (signal.readiness & (1 << (index + 1))) === 0 && within(at, x, z, BRACE_REACH_UNITS)) : -1;
  return brace < 0 ? null : 2 + brace;
}

/** What a raid acknowledgment says; the lookout's lines never claim more than the outcome (5.1.60). */
export function raidNoticeText(notice: RaidNotice): string {
  switch (notice) {
    case "tracks":
      return "Fresh boot prints and a dropped Redbrand rag: someone has been watching Millbrook Farm. The east gate guard should hear of this.";
    case "warned":
      return "Greyhaven Guard: \"Redbrand scouts on the Rim Trail? I'll post the watch. Brace the barn's walls if you can; it may come to nothing.\"";
    case "braced":
      return "You wedge a timber against the barn wall.";
    case "held":
      return "Greyhaven Guard: \"The barn stands. Being ready for them bought us the time.\"";
    case "fell":
      return "Greyhaven Guard: \"We were ready and it slowed them, but they broke into the barn all the same.\"";
  }
}

/**
 * What a guard or resident says while the signs are about (5.1.61, 5.1.62): a rumour before the
 * viewer looked, the lookout's ask once they did, and the guards' readiness once warned.
 */
export function forewarningLine(name: string, signal: RaidSignal, lookout: boolean): string {
  if ((signal.readiness & READINESS_WARNED) !== 0) {
    return `${name}: "The watch is posted. If you have a moment, brace the barn's walls at Millbrook."`;
  }
  if (signal.scouted && lookout) {
    return `${name}: "Tracks on the Rim Trail, you say? Tell me what you saw." (Interact to report.)`;
  }
  return `${name}: "Folk on the Rim Trail saw strangers watching Millbrook Farm at dusk. Could be nothing."`;
}
