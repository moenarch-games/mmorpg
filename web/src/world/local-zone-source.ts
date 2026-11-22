import { encodeCommand, type WorldCommand } from "../command-wire";
import { FixedTickClock } from "../demo-clock";
import { ProjectionDecoder, ticksFromMs, type ZoneSnapshot } from "../replication";
import type { CharacterEntries, LocalCharacters } from "./local-characters";
import { classChoiceCodes, DEFAULT_JOIN_CHARACTER, interpolated, type JoinCharacter, type LinkState, type ShownUnits, type WorldSource } from "./world-source";

/** The subset of the wasm-bindgen `LocalZone` this source drives. */
export type LocalZoneHandle = {
  /** Leases and spawns a stored character, creating it with the class choice if it was never stored. */
  enter(entries: CharacterEntries, characterId: string, classId: number, sex: number, zoneEpoch: bigint, physique?: Uint16Array): number;
  /** Commits the player's character. */
  save(entries: CharacterEntries, player: number): bigint;
  /** Commits and releases the player's character and removes its unit. */
  exit(entries: CharacterEntries, player: number): bigint;
  submit(player: number, sequence: number, command: Uint8Array): boolean;
  tick(): bigint;
  projection(player: number): Uint8Array;
  /** The world without its players, to save. */
  checkpoint(): Uint8Array;
};

/** A local zone resumed from a saved world checkpoint, or a fresh one without it; throws for an unusable checkpoint. */
export type LocalZoneFactory = (checkpoint: Uint8Array | undefined) => LocalZoneHandle;

const MAX_SEQUENCE = 0xffff_ffff;
/** Ticks between commits of the played character (5 s). */
export const SAVE_INTERVAL_TICKS = ticksFromMs(5_000);

/**
 * A single-player world hosted in this page: the shared Rust zone simulation
 * compiled to WASM, advanced in fixed `TICK_HZ` ticks from a bounded accumulator.
 * Every tick's encoded player-scoped projection goes through the same decoder
 * a network source uses, and the unit presenter interpolates it like theirs;
 * canonical state never reaches here.
 *
 * The played character is durable: entry holds it for this tab and leases it
 * from the page's character storage, progress is committed every few seconds
 * and on `persist()`, and leaving commits and releases it. So is the world
 * (#40): entry holds the browser's one local world for this tab and resumes
 * it from its saved checkpoint, which is saved with every commit. A saved
 * world this build cannot resume (another version or content) is replaced by
 * a fresh one, and `worldNotice` says so.
 */
export class LocalZoneSource implements WorldSource {
  readonly #createZone: LocalZoneFactory;
  #zone: LocalZoneHandle | null = null;
  readonly #characters: LocalCharacters;
  /** Why the saved world was replaced at the last entry, if it was. */
  worldNotice: string | null = null;
  readonly #clock = new FixedTickClock();
  #player: number | null = null;
  #release: (() => void) | null = null;
  #releaseWorld: (() => void) | null = null;
  /** The zone this page left and the checkpoint it saved then. */
  #idle: { zone: LocalZoneHandle; saved: Uint8Array } | null = null;
  #joining = false;
  #cancelled = false;
  #sequence = 0;
  #ticksSinceSave = 0;
  #latest: ZoneSnapshot | null = null;
  /** The joined session's projections, decoded once in WASM (#1009). */
  #projections: ProjectionDecoder | null = null;

  constructor(createZone: LocalZoneFactory, characters: LocalCharacters) {
    this.#createZone = createZone;
    this.#characters = characters;
  }

  /**
   * The saved world, or a fresh one when it cannot be resumed. The zone this
   * page left is reused while the saved world is still the one it saved, so its
   * session player IDs keep increasing.
   */
  #openWorld(): LocalZoneHandle {
    const saved = this.#characters.loadWorld();
    const idle = this.#idle;
    this.#idle = null;
    if (idle && saved && sameBytes(saved, idle.saved)) {
      return idle.zone;
    }
    if (saved) {
      try {
        return this.#createZone(saved);
      } catch (error) {
        console.warn("The saved local world cannot be resumed; starting a fresh one.", error);
        this.worldNotice = "The saved world is from another version and was reset.";
      }
    }
    return this.#createZone(undefined);
  }

  async join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    if (this.#player !== null || this.#joining) {
      throw new Error("Already in the world; leave before joining again.");
    }
    this.#joining = true;
    this.#cancelled = false;
    this.worldNotice = null;
    try {
      const releaseWorld = await this.#characters.holdWorld();
      let release: () => void;
      try {
        release = await this.#characters.hold(character.id);
      } catch (error) {
        releaseWorld();
        throw error;
      }
      const releaseBoth = () => {
        release();
        releaseWorld();
      };
      let zone: LocalZoneHandle;
      let player: number;
      try {
        if (this.#cancelled) {
          throw new Error("Entry was cancelled.");
        }
        zone = this.#openWorld();
        const choice = classChoiceCodes(character);
        const physique = character.physique ? Uint16Array.from(character.physique) : undefined;
        player = zone.enter(this.#characters.entries, character.id, choice.classId, choice.sex, this.#characters.nextZoneEpoch(), physique);
      } catch (error) {
        releaseBoth();
        throw error;
      }
      this.#zone = zone;
      this.#clock.reset();
      this.#latest = null;
      this.#projections = new ProjectionDecoder();
      try {
        this.#publish(player);
      } catch (error) {
        // Entry is all or nothing: a rejected first projection hands the character back.
        try {
          zone.exit(this.#characters.entries, player);
        } finally {
          this.#zone = null;
          releaseBoth();
        }
        throw error;
      }
      this.#player = player;
      this.#release = release;
      this.#releaseWorld = releaseWorld;
      this.#sequence = 0;
      this.#ticksSinceSave = 0;
      return player;
    } finally {
      this.#joining = false;
    }
  }

  /** Commits the played character and saves the world now, e.g. before the page goes away. */
  persist(): void {
    const zone = this.#zone;
    if (this.#player !== null && zone) {
      zone.save(this.#characters.entries, this.#player);
      this.#characters.saveWorld(zone.checkpoint());
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
    const releaseWorld = this.#releaseWorld;
    const zone = this.#zone;
    this.#player = null;
    this.#release = null;
    this.#releaseWorld = null;
    this.#zone = null;
    this.#latest = null;
    this.#clock.reset();
    try {
      if (zone) {
        zone.exit(this.#characters.entries, player);
        const saved = zone.checkpoint();
        this.#characters.saveWorld(saved);
        this.#idle = { zone, saved };
      }
    } finally {
      release?.();
      releaseWorld?.();
    }
  }

  sendCommand(command: WorldCommand): number {
    const player = this.#requirePlayer();
    if (this.#sequence >= MAX_SEQUENCE) {
      throw new Error("Command sequences are exhausted for this player.");
    }
    const payload = encodeCommand(command);
    this.#sequence += 1;
    if (!this.#requireZone().submit(player, this.#sequence, payload)) {
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
      this.#requireZone().tick();
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

  shown(): ShownUnits {
    // The fraction of a tick the clock accumulated since the latest local tick.
    return interpolated(this.#clock.fraction);
  }

  #requireZone(): LocalZoneHandle {
    if (!this.#zone) {
      throw new Error("Join the world before using the local zone.");
    }
    return this.#zone;
  }

  #requirePlayer(): number {
    if (this.#player === null) {
      throw new Error("Join the world before sending commands.");
    }
    return this.#player;
  }

  #publish(player: number): ZoneSnapshot {
    this.#projections ??= new ProjectionDecoder();
    const snapshot = this.#projections.local(this.#requireZone(), player);
    if (snapshot.viewerId !== player) {
      throw new Error("The local zone addressed a projection to another player.");
    }
    this.#latest = snapshot;
    return snapshot;
  }
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}
