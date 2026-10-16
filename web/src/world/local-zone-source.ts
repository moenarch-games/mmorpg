import { encodeCommand, type WorldCommand } from "../command-wire";
import { FixedTickClock } from "../demo-clock";
import { decodeSnapshot, SnapshotBuffer, type EntityState, type ZoneSnapshot } from "../replication";
import { classChoiceCodes, DEFAULT_JOIN_CHARACTER, type JoinCharacter, type LinkState, type WorldSource } from "./world-source";

/** The subset of the wasm-bindgen `LocalZone` this source drives. */
export type LocalZoneHandle = {
  /** Spawns a player and submits its class choice as command sequence 1. */
  join(classId: number, sex: number): number;
  leave(player: number): boolean;
  submit(player: number, sequence: number, command: Uint8Array): boolean;
  tick(): bigint;
  projection(player: number): Uint8Array;
};

const MAX_SEQUENCE = 0xffff_ffff;

/**
 * A single-player world hosted in this page: the shared Rust zone simulation
 * compiled to WASM, advanced in fixed 30 Hz ticks from a bounded accumulator.
 * Every tick's encoded player-scoped projection goes through the same decoder
 * and history a network source uses; canonical state never reaches here.
 */
export class LocalZoneSource implements WorldSource {
  readonly #zone: LocalZoneHandle;
  readonly #clock = new FixedTickClock();
  readonly #history = new SnapshotBuffer();
  #player: number | null = null;
  #sequence = 0;
  #latest: ZoneSnapshot | null = null;

  constructor(zone: LocalZoneHandle) {
    this.#zone = zone;
  }

  // Async only for the contract: a local join completes before it returns its promise.
  async join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    if (this.#player !== null) {
      throw new Error("Already in the world; leave before joining again.");
    }
    const choice = classChoiceCodes(character);
    const player = this.#zone.join(choice.classId, choice.sex);
    this.#clock.reset();
    this.#history.reset();
    this.#latest = null;
    try {
      this.#publish(player);
    } catch (error) {
      // Entry is all or nothing: a rejected first projection removes the unit again.
      this.#zone.leave(player);
      throw error;
    }
    this.#player = player;
    // The class choice was sequence 1.
    this.#sequence = 1;
    return player;
  }

  leave(): void {
    const player = this.#player;
    if (player === null) {
      return;
    }
    this.#player = null;
    this.#latest = null;
    this.#history.reset();
    this.#clock.reset();
    this.#zone.leave(player);
  }

  sendCommand(command: WorldCommand): number {
    const player = this.#requirePlayer();
    if (this.#sequence >= MAX_SEQUENCE) {
      throw new Error("Command sequences are exhausted for this player.");
    }
    const payload = encodeCommand(command);
    this.#sequence += 1;
    if (!this.#zone.submit(player, this.#sequence, payload)) {
      throw new Error(`The local zone ignored fresh sequence ${this.#sequence}.`);
    }
    return this.#sequence;
  }

  advance(deltaSeconds: number): readonly ZoneSnapshot[] {
    const player = this.#player;
    const due = this.#clock.advance(deltaSeconds);
    if (player === null) {
      return [];
    }
    const received: ZoneSnapshot[] = [];
    for (let tick = 0; tick < due; tick += 1) {
      this.#zone.tick();
      received.push(this.#publish(player));
    }
    return received;
  }

  latestProjection(): ZoneSnapshot | null {
    return this.#latest;
  }

  linkState(): LinkState {
    return "connected";
  }

  sample(): readonly EntityState[] {
    const latest = this.#latest;
    if (!latest) {
      return [];
    }
    // One tick of interpolation delay: blend from the previous tick toward the latest.
    const renderTick = latest.tick > 0n ? latest.tick - 1n : 0n;
    return this.#history.sample(renderTick, this.#clock.fraction);
  }

  #requirePlayer(): number {
    if (this.#player === null) {
      throw new Error("Join the world before sending commands.");
    }
    return this.#player;
  }

  #publish(player: number): ZoneSnapshot {
    const snapshot = decodeSnapshot(this.#zone.projection(player));
    if (snapshot.viewerId !== player) {
      throw new Error("The local zone addressed a projection to another player.");
    }
    this.#latest = snapshot;
    this.#history.push(snapshot);
    return snapshot;
  }
}
