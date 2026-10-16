import type { Axis, WorldCommand } from "../command-wire";
import { TICK_HZ } from "../replication";

/**
 * Local movement intent. `jumps` counts Space presses so coalesced frames
 * can merge presses but never replay them.
 */
export type MovementInput = {
  forward: Axis;
  strafe: Axis;
  facing: number;
  jumps: number;
};

/** Periodic resend of the current intent, like the native client's heartbeat. */
export const RESEND_INTERVAL_MS = 50;
/** Facing-only changes (camera drags) go out at most once per server tick. */
export const FACING_INTERVAL_MS = 1000 / TICK_HZ;

type SentMove = { forward: Axis; strafe: Axis; facing: number };

/**
 * Decides which commands the current input needs, mirroring the native
 * client's outbox: a jump when the press counter advanced, then a move when
 * forward/strafe changed, a facing change is due, or the resend interval
 * elapsed. While the player is dead it sends no held movement: the intent
 * is zero at the last sent facing and presses are dropped, so nothing held
 * through death moves the released spirit. It performs no I/O.
 */
export class MovementOutbox {
  #sent: SentMove | null = null;
  #sentAt = Number.NEGATIVE_INFINITY;
  #jumps: number;

  constructor(input: MovementInput) {
    this.#jumps = input.jumps;
  }

  /** Starts a new session: only current input counts, earlier presses are dropped. */
  reset(input: MovementInput): void {
    this.#jumps = input.jumps;
    this.#sent = null;
    this.#sentAt = Number.NEGATIVE_INFINITY;
  }

  /** Commands for the current input; `dead` comes from the latest projection. */
  update(held: MovementInput, nowMs: number, dead = false): WorldCommand[] {
    const commands: WorldCommand[] = [];
    const input = dead ? { forward: 0 as const, strafe: 0 as const, facing: this.#sent?.facing ?? held.facing, jumps: held.jumps } : held;
    if (input.jumps !== this.#jumps) {
      this.#jumps = input.jumps;
      if (!dead) {
        commands.push({ kind: "jump" });
      }
    }
    const sent = this.#sent;
    const elapsed = nowMs - this.#sentAt;
    const intentChanged = sent === null || sent.forward !== input.forward || sent.strafe !== input.strafe;
    const facingDue = (sent === null || sent.facing !== input.facing) && elapsed >= FACING_INTERVAL_MS;
    if (intentChanged || facingDue || elapsed >= RESEND_INTERVAL_MS) {
      this.#sent = { forward: input.forward, strafe: input.strafe, facing: input.facing };
      this.#sentAt = nowMs;
      commands.push({ kind: "move", ...this.#sent });
    }
    return commands;
  }
}
