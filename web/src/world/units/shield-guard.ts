import type { WorldCommand } from "../../command-wire";
import type { GuardState, StrikeHeight, ZoneSnapshot } from "../../replication";

/**
 * The Warden's held shield guard input (#156, Atlas 5.2.29/5.2.31/5.2.37): holding the separately
 * bound guard action raises the shield, vertical mouse motion since the press picks the lane (up
 * is high; the shared mouse-owner rules read it, #636), and a deliberate release or any retirement
 * lowers it. The client only names the held lane; the zone alone decides whether the guard protects
 * (grounded, not broken) and what it blocks.
 */

/** Right-stick deflection past which a pad-held guard aims high or low. */
export const GUARD_STICK_DEFLECTION = 0.5;
/** Standard-mapping left trigger and right stick vertical axis. */
const LEFT_TRIGGER = 6;
const RIGHT_STICK_Y = 3;

/** The parts of a standard-mapping pad the guard reads. */
export type GuardPad = { readonly buttons: readonly { readonly value: number }[]; readonly axes: readonly number[] };

/**
 * The lane a pad holding the left trigger aims with its right stick (owner answer #156.Q1 A): up
 * high, centred middle, down low. `null` while no pad holds the trigger, so the pointer aims.
 */
export function padGuardLane(pads: readonly (GuardPad | null)[]): StrikeHeight | null {
  let lane: StrikeHeight | null = null;
  let deflection = -1;
  for (const pad of pads) {
    if (!pad || (pad.buttons[LEFT_TRIGGER]?.value ?? 0) < 0.5) {
      continue;
    }
    const y = Number.isFinite(pad.axes[RIGHT_STICK_Y]) ? (pad.axes[RIGHT_STICK_Y] as number) : 0;
    if (Math.abs(y) > deflection) {
      deflection = Math.abs(y);
      lane = y <= -GUARD_STICK_DEFLECTION ? "high" : y >= GUARD_STICK_DEFLECTION ? "low" : "middle";
    }
  }
  return lane;
}

/** Whether the viewer can raise a shield guard: a living Warden. */
export function canGuard(snapshot: ZoneSnapshot): boolean {
  return !snapshot.viewer.dead && snapshot.viewer.classChoice?.classId === "warden";
}

export function guardCommand(lane: StrikeHeight | null): WorldCommand {
  return { kind: "guard", lane };
}

/** How the HUD and the world show the authoritative guard. */
export type GuardDisplay = { lane: StrikeHeight; state: "protecting" | "suspended" | "broken" } | null;

export function guardDisplay(guard: GuardState): GuardDisplay {
  if (guard.raised === null) {
    return null;
  }
  const state = guard.broken > 0 ? "broken" : guard.protecting ? "protecting" : "suspended";
  return { lane: guard.raised, state };
}

/**
 * One held guard: the lane the mouse aims (from the shared mouse-owner rules), a pad's override, and
 * the lane last sent to the zone.
 */
export class ShieldGuardHold {
  #pointerLane: StrikeHeight | null = null;
  #sent: StrikeHeight | null = null;
  #padLane: StrikeHeight | null = null;

  get active(): boolean {
    return this.#pointerLane !== null;
  }

  begin(): void {
    this.#pointerLane = "middle";
  }

  /** While held, the mouse's lane. */
  aimWithPointer(lane: StrikeHeight): void {
    if (this.#pointerLane !== null) {
      this.#pointerLane = lane;
    }
  }

  /** A pad holding the guard aims it with its right stick, overriding the pointer; `null` returns it. */
  aimWithPad(lane: StrikeHeight | null): void {
    this.#padLane = lane;
  }

  lane(): StrikeHeight | null {
    return this.#pointerLane === null ? null : (this.#padLane ?? this.#pointerLane);
  }

  /** The lane to send now, if it differs from the last one sent; `undefined` means nothing to send. */
  pending(): StrikeHeight | null | undefined {
    const lane = this.lane();
    if (lane === this.#sent) {
      return undefined;
    }
    this.#sent = lane;
    return lane;
  }

  /** Ends the hold; true when the zone holds a raised lane that must now be lowered. */
  end(): boolean {
    const raised = this.#sent !== null;
    this.#pointerLane = null;
    this.#sent = null;
    return raised;
  }
}
