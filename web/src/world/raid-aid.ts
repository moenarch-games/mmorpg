import type { RaidSignal } from "../replication";
import { zoneConstants } from "../zone-constants";

/**
 * Night raid aftermath aid (#531, Atlas 5.1.51): after a lost raid the projection's raid signal
 * names the authored aid tasks still open (breaches in the barn, injured guards by the farmhouse),
 * by task index (#882). Interact within reach of an open task's spot asks the zone to do it; the zone
 * checks again and owns the outcome.
 */

export interface AidTask {
  readonly x: number;
  readonly z: number;
  /** The injured guard to tend, or 0 for a breach to mend. */
  readonly npc: number;
}

/** The first open aid task within the zone's aid reach of feet at `(x, z)` units, or null. */
export function reachableAidTask(signal: RaidSignal | null | undefined, tasks: readonly AidTask[], x: number, z: number): number | null {
  const open = signal?.aid ?? [];
  const reach = zoneConstants().aidReachUnits;
  const index = tasks.findIndex((task, index) => open.includes(index) && (task.x - x) ** 2 + (task.z - z) ** 2 <= reach ** 2);
  return index < 0 ? null : index;
}

/** Whether a lost raid's aftermath still has open aid tasks. */
export function aftermathOpen(signal: RaidSignal | null | undefined): boolean {
  return (signal?.phase ?? 0) === 0 && (signal?.aid.length ?? 0) > 0;
}

/** One raid site's spots (#530, #1019): its approximate disturbance circle and its aid tasks, in task order. */
export interface RaidSiteSpots {
  readonly area: { readonly x: number; readonly z: number; readonly radius: number };
  readonly aid: readonly AidTask[];
}

/** Reads `raidArea()` (`[x, z, radius]` per site) and `raidAid()` (`[site, x, z, npc]` per task) into the sites. */
export function raidSites(areas: ArrayLike<number>, tasks: ArrayLike<number>): RaidSiteSpots[] {
  const sites = Array.from({ length: Math.floor(areas.length / 3) }, (_, index) => ({
    area: { x: areas[index * 3]!, z: areas[index * 3 + 1]!, radius: areas[index * 3 + 2]! },
    aid: [] as AidTask[],
  }));
  for (let index = 0; index + 3 < tasks.length; index += 4) {
    sites[tasks[index]!]?.aid.push({ x: tasks[index + 1]!, z: tasks[index + 2]!, npc: tasks[index + 3]! });
  }
  return sites;
}

/**
 * The raid site a player at `(x, z)` units sees (#1019): the nearest one. The zone's sites lie far apart and each one's
 * signal reaches only its own surroundings, so the raid signal a player receives is always the nearest site's.
 */
export function raidSiteAt(sites: readonly RaidSiteSpots[], x: number, z: number): RaidSiteSpots | null {
  let nearest: RaidSiteSpots | null = null;
  let best = Infinity;
  for (const site of sites) {
    const distance = (site.area.x - x) ** 2 + (site.area.z - z) ** 2;
    if (distance < best) {
      best = distance;
      nearest = site;
    }
  }
  return nearest;
}
