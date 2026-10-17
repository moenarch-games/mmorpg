import { encodeCommand, type WorldCommand } from "../command-wire";
import { FixedTickClock } from "../demo-clock";
import { decodeSnapshot, SnapshotBuffer, type EntityState, type ZoneSnapshot } from "../replication";
import type { CharacterEntries, LocalCharacters } from "./local-characters";
import { classChoiceCodes, DEFAULT_JOIN_CHARACTER, type JoinCharacter, type LinkState, type WorldSource } from "./world-source";

/** The subset of the wasm-bindgen `LocalZone` this source drives. */
export type LocalZoneHandle = {
  /** Leases and spawns a stored character, creating it with the class choice if it was never stored. */
  enter(entries: CharacterEntries, characterId: string, classId: number, sex: number, zoneEpoch: bigint): number;
  /** Commits the player's character. */
  save(entries: CharacterEntries, player: number): bigint;
  /** Commits and releases the player's character and removes its unit. */
  exit(entries: CharacterEntries, player: number): bigint;
  submit(player: number, sequence: number, command: Uint8Array): boolean;
  tick(): bigint;
  projection(player: number): Uint8Array;
};

const MAX_SEQUENCE = 0xffff_ffff;
/** Ticks between commits of the played character (5 s). */
export const SAVE_INTERVAL_TICKS = 150;

/**
 * A single-player world hosted in this page: the shared Rust zone simulation
 * compiled to WASM, advanced in fixed 30 Hz ticks from a bounded accumulator.
 * Every tick's encoded player-scoped projection goes through the same decoder
 * and history a network source uses; canonical state never reaches here.
 *
 * The played character is durable: entry holds it for this tab and leases it
 * from the page's character storage, progress is committed every few seconds
 * and on `persist()`, and leaving commits and releases it.
 */
export class LocalZoneSource implements WorldSource {
  readonly #zone: LocalZoneHandle;
  readonly #characters: LocalCharacters;
  readonly #clock = new FixedTickClock();
  readonly #history = new SnapshotBuffer();
  #player: number | null = null;
  #release: (() => void) | null = null;
  #joining = false;
  #cancelled = false;
  #sequence = 0;
  #ticksSinceSave = 0;
  #latest: ZoneSnapshot | null = null;

  constructor(zone: LocalZoneHandle, characters: LocalCharacters) {
    this.#zone = zone;
    this.#characters = characters;
  }

  async join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    if (this.#player !== null || this.#joining) {
      throw new Error("Already in the world; leave before joining again.");
    }
    this.#joining = true;
    this.#cancelled = false;
    try {
      const release = await this.#characters.hold(character.id);
      let player: number;
      try {
        if (this.#cancelled) {
          throw new Error("Entry was cancelled.");
        }
        const choice = classChoiceCodes(character);
        player = this.#zone.enter(this.#characters.entries, character.id, choice.classId, choice.sex, this.#characters.nextZoneEpoch());
      } catch (error) {
        release();
        throw error;
      }
      this.#clock.reset();
      this.#history.reset();
      this.#latest = null;
      try {
        this.#publish(player);
      } catch (error) {
        // Entry is all or nothing: a rejected first projection hands the character back.
        try {
          this.#zone.exit(this.#characters.entries, player);
        } finally {
          release();
        }
        throw error;
      }
      this.#player = player;
      this.#release = release;
      this.#sequence = 0;
      this.#ticksSinceSave = 0;
      return player;
    } finally {
      this.#joining = false;
    }
  }

  /** Commits the played character now, e.g. before the page goes away. */
  persist(): void {
    if (this.#player !== null) {
      this.#zone.save(this.#characters.entries, this.#player);
      this.#ticksSinceSave = 0;
    }
  }

  leave(): void {
    const player = this.#player;
    if (player === null) {
      this.#cancelled = this.#joining;
      return;
    }
    const release = this.#release;
    this.#player = null;
    this.#release = null;
    this.#latest = null;
    this.#history.reset();
    this.#clock.reset();
    try {
      this.#zone.exit(this.#characters.entries, player);
    } finally {
      release?.();
    }
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
      this.#ticksSinceSave += 1;
      if (this.#ticksSinceSave >= SAVE_INTERVAL_TICKS) {
        this.persist();
      }
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
