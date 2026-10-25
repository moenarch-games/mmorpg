import { encodeCommand, type WorldCommand } from "../command-wire";
import { FixedTickClock } from "../demo-clock";
import { decodeSnapshot, SnapshotBuffer, ticksFromMs, type EntityState, type ZoneSnapshot } from "../replication";
import type { LinkState, WorldSource } from "./world-source";
import { LAB_SCENARIO, labSetupFromQuery, type LabSetup } from "./action-lab";

/** The subset of a wasm-bindgen `LocalZone` made by `LocalZone.forScenario` that this source drives. */
export type ScenarioZoneHandle = {
  readonly scenarioPlayer: number | undefined;
  submit(player: number, sequence: number, command: Uint8Array): boolean;
  tick(): bigint;
  projection(player: number): Uint8Array;
  /** The Action Lab's read of the action graph after the last tick, as JSON (#381). */
  actionInspection?(player: number): string | undefined;
  free(): void;
};

/** One recorded tick of a scenario: the projection and, in the Action Lab, the graph inspection JSON. */
export type ScenarioFrame = { tick: bigint; snapshot: ZoneSnapshot; inspection: string | null };

/** Recorded ticks kept for stepping back and scrubbing: 30 s. */
export const SCENARIO_HISTORY_TICKS = ticksFromMs(30_000);

/**
 * A review scenario chosen by URL (#371): `?scenario=<name>&seed=<n>`. The Action Lab (#381) is
 * `?scenario=action-lab` with its character setup in further parameters.
 */
export type ScenarioChoice = { name: string; seed: bigint; lab: LabSetup | null };

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
  return { name, seed: BigInt(seed), lab: name === LAB_SCENARIO ? labSetupFromQuery(search) : null };
}

/**
 * A local review scenario (#371): the shared zone simulation in WASM, started
 * fresh in a named situation with an ephemeral character. Like
 * `LocalZoneSource` it ticks at `TICK_HZ` and publishes only decoded projections,
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
  #frames: ScenarioFrame[] = [];
  #paused = false;
  #pendingSteps = 0;
  #scrub: bigint | null = null;

  constructor(createZone: () => ScenarioZoneHandle) {
    this.#createZone = createZone;
  }

  /** Whether the world is paused: no ticks run except explicit steps. */
  get paused(): boolean {
    return this.#paused;
  }

  /** Pauses or resumes the world; resuming returns the view to the live tick. */
  setPaused(paused: boolean): void {
    this.#paused = paused;
    if (!paused) {
      this.#scrub = null;
      this.#pendingSteps = 0;
      this.#clock.reset();
    }
  }

  /** Pauses and runs exactly one tick at the next advance; the view returns to the live tick. */
  step(): void {
    this.#paused = true;
    this.#scrub = null;
    this.#pendingSteps += 1;
  }

  /** The recorded ticks, oldest first. */
  get frames(): readonly ScenarioFrame[] {
    return this.#frames;
  }

  /** The tick the view shows: a scrubbed past tick, or null for live. */
  get scrubTick(): bigint | null {
    return this.#scrub;
  }

  /**
   * Shows recorded tick `tick` (clamped to the history) while paused, or the live tick for null.
   * Scrubbing pauses; the world never rewinds, only the view does.
   */
  scrubTo(tick: bigint | null): void {
    if (tick === null || this.#frames.length === 0) {
      this.#scrub = null;
      return;
    }
    const first = this.#frames[0]!.tick;
    const last = this.#frames.at(-1)!.tick;
    this.#paused = true;
    this.#scrub = tick >= last ? null : tick < first ? first : tick;
  }

  /** The recorded tick the view shows: the scrubbed one, or the latest. */
  shownFrame(): ScenarioFrame | null {
    const scrub = this.#scrub;
    if (scrub === null) {
      return this.#frames.at(-1) ?? null;
    }
    return this.#frames.find((frame) => frame.tick === scrub) ?? null;
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
    this.#frames = [];
    this.#scrub = null;
    this.#pendingSteps = 0;
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
    let due: number;
    if (this.#paused) {
      due = this.#pendingSteps;
      this.#pendingSteps = 0;
    } else {
      due = this.#clock.advance(deltaSeconds);
    }
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
    if (this.#scrub !== null) {
      return this.shownFrame()?.snapshot.entities ?? latest.entities;
    }
    if (this.#paused) {
      // A paused or stepped world shows exactly its last tick.
      return latest.entities;
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
    const inspection = this.#requireZone().actionInspection?.(player) ?? null;
    this.#frames.push({ tick: snapshot.tick, snapshot, inspection });
    if (this.#frames.length > SCENARIO_HISTORY_TICKS) {
      this.#frames.shift();
    }
    return snapshot;
  }
}
