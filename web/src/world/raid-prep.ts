import type { RaidSignal } from "../replication";

/**
 * Forewarning and preparation before a night raid (#532, Atlas 5.1.59–5.1.62): on some evenings
 * the projection's raid signal says the scouts' tracks lie on the approach, whether the viewer
 * read them, and which preparations the zone accepted for the coming raid. Interact in reach asks
 * the zone to take the next step; the zone checks again and owns the outcome. No timer, target
 * or guarantee is shown: the signs may come to nothing.
 */

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

/**
 * What a guard or resident says while the signs are about (5.1.61, 5.1.62): a rumour before the
 * viewer looked, the lookout's ask once they did, and the guards' readiness once warned.
 */
export function forewarningLine(name: string, signal: RaidSignal, lookout: boolean): string {
  if (signal.warned) {
    return `${name}: "The watch is posted. If you have a moment, brace the barn's walls at Millbrook."`;
  }
  if (signal.scouted && lookout) {
    return `${name}: "Tracks on the Rim Trail, you say? Tell me what you saw." (Interact to report.)`;
  }
  return `${name}: "Folk on the Rim Trail saw strangers watching Millbrook Farm at dusk. Could be nothing."`;
}
