import { encodeCommand, type WorldCommand } from "../command-wire";
import { FixedTickClock } from "../demo-clock";
import { decodeSnapshot, SnapshotBuffer, type EntityState, type ZoneSnapshot } from "../replication";
import type { LinkState, WorldSource } from "./world-source";

/** The subset of a wasm-bindgen `LocalZone` made by `LocalZone.forScenario` that this source drives. */
export type ScenarioZoneHandle = {
  readonly scenarioPlayer: number | undefined;
  submit(player: number, sequence: number, command: Uint8Array): boolean;
  tick(): bigint;
  projection(player: number): Uint8Array;
  free(): void;
};

/** A review scenario chosen by URL (#371): `?scenario=<name>&seed=<n>`. */
export type ScenarioChoice = { name: string; seed: bigint };

/** The page's scenario choice, or null for ordinary entry. A malformed seed throws. */
export function scenarioChoice(search: string): ScenarioChoice | null {
  const query = new URLSearchParams(search);
  const name = query.get("scenario");
  if (!name) {
    return null;
  }
  const seed = query.get("seed") ?? "0";
  if (!/^\d{1,20}$/.test(seed) || BigInt(seed) > 0xffff_ffff_ffff_ffffn) {
    throw new Error(`Scenario seed "${seed}" is not an unsigned 64-bit integer.`);
  }
  return { name, seed: BigInt(seed) };
}

/**
 * A local review scenario (#371): the shared zone simulation in WASM, started
 * fresh in a named situation with an ephemeral character. Like
 * `LocalZoneSource` it ticks at 30 Hz and publishes only decoded projections,
 * but nothing is leased or stored, and `reset()` restarts the same scenario
 * and seed. It is never a network host's input.
 */
export class ScenarioSource implements WorldSource {
  readonly #createZone: () => ScenarioZoneHandle;
  #zone: ScenarioZoneHandle | null = null;
  #player: number | null = null;
  readonly #clock = new FixedTickClock();
  readonly #history = new SnapshotBuffer();
  #sequence = 0;
  #latest: ZoneSnapshot | null = null;

  constructor(createZone: () => ScenarioZoneHandle) {
    this.#createZone = createZone;
  }

  async join(): Promise<number> {
    if (this.#player !== null) {
      throw new Error("Already in the scenario; leave before joining again.");
    }
    const zone = this.#createZone();
    const player = zone.scenarioPlayer;
    if (player === undefined) {
      zone.free();
      throw new Error("The scenario zone has no scenario character.");
    }
    this.#zone = zone;
    this.#clock.reset();
    this.#history.reset();
    this.#sequence = 0;
    this.#publish(player);
    this.#player = player;
    return player;
  }

  /** Discards the running world and starts the scenario again from its seed. */
  async reset(): Promise<number> {
    this.leave();
    return this.join();
  }

  leave(): void {
    this.#zone?.free();
    this.#zone = null;
    this.#player = null;
    this.#latest = null;
    this.#history.reset();
    this.#clock.reset();
  }

  sendCommand(command: WorldCommand): number {
    const player = this.#requirePlayer();
    this.#sequence += 1;
    if (!this.#requireZone().submit(player, this.#sequence, encodeCommand(command))) {
      throw new Error(`The scenario zone ignored fresh sequence ${this.#sequence}.`);
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
    const renderTick = latest.tick > 0n ? latest.tick - 1n : 0n;
    return this.#history.sample(renderTick, this.#clock.fraction);
  }

  #requireZone(): ScenarioZoneHandle {
    if (!this.#zone) {
      throw new Error("Join the scenario before using its zone.");
    }
    return this.#zone;
  }

  #requirePlayer(): number {
    if (this.#player === null) {
      throw new Error("Join the scenario before sending commands.");
    }
    return this.#player;
  }

  #publish(player: number): ZoneSnapshot {
    const snapshot = decodeSnapshot(this.#requireZone().projection(player));
    if (snapshot.viewerId !== player) {
      throw new Error("The scenario zone addressed a projection to another player.");
    }
    this.#latest = snapshot;
    this.#history.push(snapshot);
    return snapshot;
  }
}
