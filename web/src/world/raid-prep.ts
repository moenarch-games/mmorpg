import type { RaidNotice, RaidSignal } from "../replication";

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
    // A defence's end (#1019): what Osric says of his people.
    case "stood":
      return "Farmer Osric Mill: \"They're falling back, and my people are standing. We held!\"";
    case "overrun":
      return "Farmer Osric Mill: \"Too many of my people are down. Once they're tended and the barn is whole, we try again.\"";
  }
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
