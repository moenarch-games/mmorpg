import { encodeCommandFrame } from "./frames";

/** How a command reaches the zone host over unreliable datagrams. */
export type Delivery =
  /**
   * Held state such as movement: a newer command replaces an unsent older
   * one. It is sent once; the caller repeats held state periodically, so a
   * lost datagram is corrected by the next one.
   */
  | "latest"
  /**
   * A discrete intent such as a jump: sent under its own sequence and resent
   * byte for byte until a projection acknowledges it.
   */
  | "reliable";

/** An unacknowledged reliable command is resent this often, like the movement heartbeat. */
export const RELIABLE_RESEND_MS = 50;
/** Reliable commands waiting at once; further ones are dropped, like the host's per-tick intent bound. */
export const MAX_QUEUED_RELIABLE = 16;
const MAX_SEQUENCE = 0xffff_ffff;

type Queued = { payload: Uint8Array; delivery: Delivery };
type InFlight = { sequence: number; frame: Uint8Array; sentAt: number };

/** Deterministic counters describing what an outbox did. */
export type OutboxStats = {
  /** Frames sent under a fresh sequence. */
  sent: number;
  /** Repeats of an unacknowledged reliable frame. */
  resent: number;
  /** Held-state commands replaced by a newer one before they were sent. */
  coalesced: number;
  /** Reliable commands refused because the queue was full. */
  dropped: number;
};

/**
 * Sequencing and delivery of game-server command frames. It performs no I/O:
 * `poll` returns the datagrams to send now.
 *
 * The host applies a command only when its sequence is above the newest one it
 * applied, and acknowledges that newest sequence in the player's projection.
 * Every frame therefore carries a fresh, strictly increasing sequence, except
 * the resend of an unacknowledged reliable command, which repeats its frame
 * exactly; the host ignores the copy once it applied the original. While a
 * reliable command is unacknowledged nothing with a higher sequence is sent,
 * so an acknowledgement at or above its sequence proves the host applied it:
 * a later command can never overtake it and make it stale unseen. Held-state
 * commands submitted meanwhile coalesce behind it.
 */
export class SequencedOutbox {
  #sequence: number;
  #queue: Queued[] = [];
  #inFlight: InFlight | null = null;
  readonly #stats: OutboxStats = { sent: 0, resent: 0, coalesced: 0, dropped: 0 };

  /** `lastSequence` is the highest sequence already used for the player; a new player starts at 0. */
  constructor(lastSequence = 0) {
    if (!Number.isInteger(lastSequence) || lastSequence < 0 || lastSequence > MAX_SEQUENCE) {
      throw new Error("The last command sequence must be a u32.");
    }
    this.#sequence = lastSequence;
  }

  /** The highest sequence sent so far; it survives `restart`. */
  get sequence(): number {
    return this.#sequence;
  }

  /** The sequence of the reliable command awaiting acknowledgement, if any. */
  get awaiting(): number | null {
    return this.#inFlight?.sequence ?? null;
  }

  /** Commands waiting to be sent. */
  get queued(): number {
    return this.#queue.length;
  }

  stats(): OutboxStats {
    return { ...this.#stats };
  }

  /**
   * Queues a command and returns the sequence it will be sent under, or `null` when a reliable one
   * was dropped because the queue is full. Queued commands take sequences in queue order and only a
   * trailing held state coalesces, so a reliable command's sequence is fixed once queued; a
   * `restart` forgets it unsent.
   */
  submit(payload: Uint8Array, delivery: Delivery): number | null {
    if (delivery === "reliable") {
      const reliable = this.#queue.filter((queued) => queued.delivery === "reliable").length;
      if (reliable >= MAX_QUEUED_RELIABLE) {
        this.#stats.dropped += 1;
        return null;
      }
    } else if (this.#queue.at(-1)?.delivery === "latest") {
      // Only the tail coalesces, so held state never jumps ahead of an intent queued before it.
      this.#queue.pop();
      this.#stats.coalesced += 1;
    }
    this.#queue.push({ payload: payload.slice(), delivery });
    return this.#sequence + this.#queue.length;
  }

  /** Records the host's acknowledged sequence from a projection. */
  acknowledge(sequence: number): void {
    if (this.#inFlight && sequence >= this.#inFlight.sequence) {
      this.#inFlight = null;
    }
  }

  /**
   * The datagrams due at `nowMs`, in order: a resend of the unacknowledged
   * reliable command when its interval elapsed, or queued commands up to and
   * including the next reliable one. Throws when sequences are exhausted.
   */
  poll(nowMs: number): Uint8Array[] {
    const inFlight = this.#inFlight;
    if (inFlight) {
      if (nowMs - inFlight.sentAt < RELIABLE_RESEND_MS) {
        return [];
      }
      inFlight.sentAt = nowMs;
      this.#stats.resent += 1;
      return [inFlight.frame];
    }
    const datagrams: Uint8Array[] = [];
    for (let next = this.#queue.shift(); next; next = this.#queue.shift()) {
      if (this.#sequence >= MAX_SEQUENCE) {
        throw new Error("Command sequences are exhausted for this player.");
      }
      this.#sequence += 1;
      const frame = encodeCommandFrame(this.#sequence, next.payload);
      datagrams.push(frame);
      this.#stats.sent += 1;
      if (next.delivery === "reliable") {
        this.#inFlight = { sequence: this.#sequence, frame, sentAt: nowMs };
        break;
      }
    }
    return datagrams;
  }

  /**
   * Starts over on a resumed connection: queued and unacknowledged commands
   * are forgotten, never replayed, and `barrier` goes out first as a reliable
   * command. Sequences continue from the highest one sent, acknowledged or not.
   */
  restart(barrier: Uint8Array): void {
    this.#queue = [];
    this.#inFlight = null;
    this.submit(barrier, "reliable");
  }
}
