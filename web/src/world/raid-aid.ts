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
