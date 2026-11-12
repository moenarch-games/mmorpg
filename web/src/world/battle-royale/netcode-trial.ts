import { encodeCommand, encodeObservedCommand, isTimingSensitive, type WorldCommand } from "../../command-wire";
import { FixedTickClock } from "../../demo-clock";
import { decodeSnapshot, SnapshotBuffer, ticksFromMs, type EntityState, type ZoneSnapshot } from "../../replication";
import { INTERPOLATION_DELAY_TICKS } from "../online-zone-source";
import type { ScenarioZoneHandle } from "../scenario-source";
import type { LinkState, WorldSource } from "../world-source";

/**
 * The arena netcode options a tester compares (#680, Atlas 5.9.i680); nothing is assumed to win.
 * `baseline` is today's online path: zone authority, every unit drawn 67 ms behind the newest
 * projection, the own body included. `predicted` moves the own body at once with the zone's own
 * movement rules and reconciles it against `acknowledged_sequence`. `rewind` adds the match's
 * capped server rewind of timed swings' hits on bodies; clashes stay in the zone's present.
 */
export type NetcodeOption = "baseline" | "predicted" | "rewind";

export const NETCODE_OPTIONS: readonly { id: NetcodeOption; name: string }[] = [
  { id: "baseline", name: "1 · Baseline (interpolated)" },
  { id: "predicted", name: "2 · Predicted movement" },
  { id: "rewind", name: "3 · Predicted + hit rewind" },
];

/** Simulated round trips for local practice, split into uplink and downlink ticks; no jitter. */
export const DELAY_PRESETS: readonly { name: string; roundTripMs: number }[] = [
  { name: "No delay", roundTripMs: 0 },
  { name: "50 ms", roundTripMs: 50 },
  { name: "100 ms", roundTripMs: 100 },
  { name: "150 ms", roundTripMs: 150 },
];

/** One practice match's netcode choice: the option and the simulated round trip. */
export type NetcodeTrial = { option: NetcodeOption; roundTripMs: number };

export function isNetcodeOption(value: string | null): value is NetcodeOption {
  return NETCODE_OPTIONS.some((option) => option.id === value);
}

/** `?netcode=baseline|predicted|rewind&delay=<ms>`, or null without a netcode option. */
export function netcodeTrialFromQuery(search: string): NetcodeTrial | null {
  const query = new URLSearchParams(search);
  const option = query.get("netcode");
  if (!isNetcodeOption(option)) { return null; }
  const delay = Number(query.get("delay") ?? "0");
  const preset = DELAY_PRESETS.find((candidate) => candidate.roundTripMs === delay);
  return { option, roundTripMs: preset?.roundTripMs ?? 0 };
}

/** The uplink and downlink ticks of a simulated round trip. */
export function linkTicks(roundTripMs: number): { up: number; down: number } {
  const total = ticksFromMs(roundTripMs);
  const up = Math.floor(total / 2);
  return { up, down: total - up };
}

/** A `LocalZone.forArenaTrial` world: the trial's practice arena and its sparring partner. */
export type TrialZoneHandle = ScenarioZoneHandle & { readonly sparringPartner: number | undefined };

/** The subset of the wasm `ArenaMovementPrediction` this source drives. */
export type PredictionHandle = {
  sent(sequence: number, command: Uint8Array): void;
  step(): void;
  acknowledge(tick: bigint, acknowledged: number): void;
  reconcile(tick: bigint, x: number, y: number, z: number, vx: number, vy: number, vz: number, facing: number): void;
  position(): Float32Array;
  free(): void;
};

const YAW_STEPS = 65_536;
/** The sparring partner closes to this distance (units), then circles, and swings within reach. */
const PARTNER_CLOSE = 200;
const PARTNER_REACH = 240;
const PARTNER_STRAFE_TICKS = 90;
const PARTNER_SWING_TICKS = 150;

function yawToward(dx: number, dz: number): number {
  const yaw = Math.round((Math.atan2(dx, dz) / (2 * Math.PI)) * YAW_STEPS);
  return ((yaw % YAW_STEPS) + YAW_STEPS) % YAW_STEPS;
}

function ownEntity(snapshot: ZoneSnapshot, id: number): EntityState | undefined {
  return snapshot.entities.find((entity) => entity.kind === "player" && entity.entityId === id);
}

/**
 * The local practice arena under a netcode option and a simulated round trip (#680): the shared
 * zone simulation in WASM stays the one authority; this source only delays commands to it and
 * projections from it, by whole ticks, as a network would. A sparring partner (a second player
 * the page drives in the zone, without delay) closes in, circles and swings, so movement, hits and
 * clashes can be felt. Nothing is stored; `reset()` starts a fresh match.
 */
export class NetcodeTrialSource implements WorldSource {
  readonly #createZone: () => TrialZoneHandle;
  readonly #createPrediction: () => PredictionHandle;
  readonly #trial: NetcodeTrial;
  readonly #up: number;
  readonly #down: number;
  readonly #clock = new FixedTickClock();
  readonly #history = new SnapshotBuffer();
  #zone: TrialZoneHandle | null = null;
  #prediction: PredictionHandle | null = null;
  #player: number | null = null;
  #partner: number | null = null;
  #partnerSequence = 0;
  #sequence = 0;
  /** Local ticks run since the match began. */
  #ticks = 0;
  #uplink: { due: number; sequence: number; bytes: Uint8Array }[] = [];
  #downlink: { due: number; bytes: Uint8Array }[] = [];
  #latest: ZoneSnapshot | null = null;
  /** The predicted own body after the previous and the latest local tick: `[x, y, z, facing]`. */
  #predicted: [Float32Array | null, Float32Array | null] = [null, null];

  constructor(createZone: () => TrialZoneHandle, createPrediction: () => PredictionHandle, trial: NetcodeTrial) {
    this.#createZone = createZone;
    this.#createPrediction = createPrediction;
    this.#trial = trial;
    ({ up: this.#up, down: this.#down } = linkTicks(trial.roundTripMs));
  }

  get trial(): NetcodeTrial { return this.#trial; }

  async join(): Promise<number> {
    if (this.#player !== null) { throw new Error("Already in the practice arena; leave before joining again."); }
    const zone = this.#createZone();
    const player = zone.scenarioPlayer;
    if (player === undefined) {
      zone.free();
      throw new Error("The trial arena has no practice character.");
    }
    this.#zone = zone;
    this.#partner = zone.sparringPartner ?? null;
    this.#prediction = this.#trial.option === "baseline" ? null : this.#createPrediction();
    this.#player = player;
    this.#clock.reset();
    this.#history.reset();
    this.#sequence = 0;
    this.#partnerSequence = 0;
    this.#ticks = 0;
    this.#uplink = [];
    this.#downlink = [];
    this.#predicted = [null, null];
    // The joining projection arrives at once, like an online welcome.
    this.#receive(zone.projection(player));
    return player;
  }

  async reset(): Promise<number> {
    this.leave();
    return this.join();
  }

  leave(): void {
    this.#zone?.free();
    this.#prediction?.free();
    this.#zone = null;
    this.#prediction = null;
    this.#player = null;
    this.#partner = null;
    this.#latest = null;
    this.#history.reset();
  }

  sendCommand(command: WorldCommand): number | null {
    if (this.#player === null) { throw new Error("Join the practice arena before sending commands."); }
    const observed = this.#latest?.tick;
    const bytes = observed !== undefined && isTimingSensitive(command)
      ? encodeObservedCommand(command, observed)
      : encodeCommand(command);
    this.#sequence += 1;
    this.#uplink.push({ due: this.#ticks + 1 + this.#up, sequence: this.#sequence, bytes });
    this.#prediction?.sent(this.#sequence, bytes);
    return this.#sequence;
  }

  advance(deltaSeconds: number): readonly ZoneSnapshot[] {
    const due = this.#clock.advance(deltaSeconds);
    const received: ZoneSnapshot[] = [];
    for (let tick = 0; tick < due && this.#zone; tick += 1) {
      received.push(...this.#tickOnce());
    }
    return received;
  }

  #tickOnce(): ZoneSnapshot[] {
    const zone = this.#zone!;
    const player = this.#player!;
    this.#ticks += 1;
    while (this.#uplink.length > 0 && this.#uplink[0]!.due <= this.#ticks) {
      const { sequence, bytes } = this.#uplink.shift()!;
      zone.submit(player, sequence, bytes);
    }
    this.#drivePartner(zone);
    zone.tick();
    this.#downlink.push({ due: this.#ticks + this.#down, bytes: zone.projection(player) });
    this.#prediction?.step();
    const received: ZoneSnapshot[] = [];
    while (this.#downlink.length > 0 && this.#downlink[0]!.due <= this.#ticks) {
      received.push(this.#receive(this.#downlink.shift()!.bytes));
    }
    const newest = received.at(-1);
    const own = newest && ownEntity(newest, player);
    if (this.#prediction && newest && own) {
      const [x, y, z] = own.position;
      const [vx, vy, vz] = own.velocity;
      this.#prediction.reconcile(newest.tick, x, y, z, vx, vy, vz, own.facing);
    }
    if (this.#prediction) {
      const position = this.#prediction.position();
      this.#predicted = [this.#predicted[1], position.length === 4 ? position : null];
    }
    return received;
  }

  #receive(bytes: Uint8Array): ZoneSnapshot {
    const snapshot = decodeSnapshot(bytes);
    this.#latest = snapshot;
    this.#history.push(snapshot);
    this.#prediction?.acknowledge(snapshot.tick, snapshot.acknowledgedSequence);
    return snapshot;
  }

  /** The sparring partner reads its own projection and closes in, circles and swings. */
  #drivePartner(zone: TrialZoneHandle): void {
    const partner = this.#partner;
    const player = this.#player;
    if (partner === null || player === null || this.#ticks % 6 !== 0) { return; }
    const view = decodeSnapshot(zone.projection(partner));
    const self = ownEntity(view, partner);
    const target = ownEntity(view, player);
    if (view.viewer.dead || !self || !target) { return; }
    const dx = target.position[0] - self.position[0];
    const dz = target.position[2] - self.position[2];
    const distance = Math.hypot(dx, dz);
    const facing = yawToward(dx, dz);
    const strafe = Math.floor(this.#ticks / PARTNER_STRAFE_TICKS) % 2 === 0 ? 1 : -1;
    const commands: WorldCommand[] = [distance > PARTNER_CLOSE
      ? { kind: "move", forward: 1, strafe: 0, facing }
      : { kind: "move", forward: 0, strafe, facing }];
    if (distance <= PARTNER_REACH && this.#ticks % PARTNER_SWING_TICKS === 0) {
      commands.push({ kind: "directed-strike", ability: 1, facing, aim: { side: "centre", height: "middle" } });
    }
    for (const command of commands) {
      this.#partnerSequence += 1;
      zone.submit(partner, this.#partnerSequence, encodeCommand(command));
    }
  }

  latestProjection(): ZoneSnapshot | null {
    return this.#latest;
  }

  linkState(): LinkState {
    return "connected";
  }

  sample(): readonly EntityState[] {
    const latest = this.#latest;
    if (!latest) { return []; }
    const tick = latest.tick - BigInt(INTERPOLATION_DELAY_TICKS);
    const entities = tick < 0n ? this.#history.sample(0n) : this.#history.sample(tick, this.#clock.fraction);
    const [previous, current] = this.#predicted;
    if (!this.#prediction || !current) { return entities; }
    // The own body is drawn where the prediction puts it, between its last two ticks.
    const from = previous ?? current;
    const alpha = this.#clock.fraction;
    const at = (axis: number) => from[axis]! + (current[axis]! - from[axis]!) * alpha;
    const velocity = (axis: number) => Math.max(-128, Math.min(127, Math.round(current[axis]! - from[axis]!)));
    return entities.map((entity) => entity.kind === "player" && entity.entityId === latest.viewerId
      ? { ...entity, position: [at(0), at(1), at(2)], velocity: [velocity(0), velocity(1), velocity(2)], facing: current[3]! }
      : entity);
  }
}
