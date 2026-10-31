import type { RaidSignal } from "../replication";

/**
 * Night raid aftermath aid (#531, Atlas 5.1.51): after a lost raid the projection's raid signal
 * names the authored aid tasks still open (breaches in the barn, injured guards by the farmhouse),
 * one bit per task. Interact within reach of an open task's spot asks the zone to do it; the zone
 * checks again and owns the outcome.
 */

/** The zone's aid reach (3 m, `raid::AID_REACH_UNITS`) in units. */
export const AID_REACH_UNITS = 300;

export interface AidTask {
  readonly x: number;
  readonly z: number;
  /** The injured guard to tend, or 0 for a breach to mend. */
  readonly npc: number;
}

/** The first open aid task within reach of feet at `(x, z)` units, or null. */
export function reachableAidTask(signal: RaidSignal | null | undefined, tasks: readonly AidTask[], x: number, z: number): number | null {
  const open = signal?.aid ?? 0;
  const index = tasks.findIndex((task, index) => (open & (1 << index)) !== 0 && (task.x - x) ** 2 + (task.z - z) ** 2 <= AID_REACH_UNITS ** 2);
  return index < 0 ? null : index;
}

/** Whether a lost raid's aftermath still has open aid tasks. */
export function aftermathOpen(signal: RaidSignal | null | undefined): boolean {
  return (signal?.phase ?? 0) === 0 && (signal?.aid ?? 0) !== 0;
}
