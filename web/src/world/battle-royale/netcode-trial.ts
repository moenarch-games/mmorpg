import { encodeCommand, encodeObservedCommand, isTimingSensitive, type WorldCommand } from "../../command-wire";
import { FixedTickClock } from "../../demo-clock";
import { decodeSnapshot, ProjectionDecoder, ticksFromMs, type EntityState, type ZoneSnapshot } from "../../replication";
import type { ScenarioZoneHandle } from "../scenario-source";
import { interpolated, type LinkState, type ShownUnits, type WorldSource } from "../world-source";

/**
 * The arena netcode options a tester compares (#680, Atlas 5.9.i680); nothing is assumed to win.
 * `baseline` is today's online path: zone authority, every unit drawn 2 ticks behind the newest
 * projection (the shared interpolation, #1058), the own body included. `predicted` moves the own body at once with the zone's own
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

/**
 * The predicted own body (#680) a source drives: commands as it sends them, `step()` and
 * `record()` once per local tick, and every projection it receives, the newest restarting the
 * prediction from the zone's own answer. `ownBody` puts the own body where the prediction stands,
 * between its last two recorded ticks.
 */
export class OwnBodyPrediction {
  readonly #handle: PredictionHandle;
  #reconciled = false;
  /** The predicted own body after the previous and the latest local tick: `[x, y, z, facing]`. */
  #predicted: [Float32Array | null, Float32Array | null] = [null, null];
  readonly #body = new Float64Array(7);

  constructor(handle: PredictionHandle) { this.#handle = handle; }

  /** `bytes` are the plain or observed zone command wire bytes sent under `sequence`. */
  sent(sequence: number, bytes: Uint8Array): void { this.#handle.sent(sequence, bytes); }

  /** One local tick of the prediction. */
  step(): void { this.#handle.step(); }

  /** Every projection received, in order: each acknowledges, the newest reconciles. */
  receive(snapshots: readonly ZoneSnapshot[]): void {
    for (const snapshot of snapshots) { this.#handle.acknowledge(snapshot.tick, snapshot.acknowledgedSequence); }
    const newest = snapshots.at(-1);
    const own = newest && ownEntity(newest, newest.viewerId);
    if (!newest || !own) { return; }
    const [x, y, z] = own.position;
    const [vx, vy, vz] = own.velocity;
    this.#handle.reconcile(newest.tick, x, y, z, vx, vy, vz, own.facing);
    this.#reconciled = true;
  }

  /** Remembers where the prediction stands after this local tick, once it was reconciled. */
  record(): void {
    if (!this.#reconciled) { return; }
    const position = this.#handle.position();
    this.#predicted = [this.#predicted[1], position.length === 4 ? position : null];
  }

  /**
   * The viewer's own body where the prediction puts it, `alpha` into the tick, as the unit
   * presenter takes it (`[x, y, z, facing, vx, vy, vz]`, #1058); null before it reconciled.
   */
  ownBody(alpha: number): Float64Array | null {
    const [previous, current] = this.#predicted;
    if (!current) { return null; }
    const from = previous ?? current;
    const at = (axis: number) => from[axis]! + (current[axis]! - from[axis]!) * alpha;
    const body = this.#body;
    body[0] = at(0); body[1] = at(1); body[2] = at(2); body[3] = current[3]!;
    for (let axis = 0; axis < 3; axis += 1) { body[4 + axis] = current[axis]! - from[axis]!; }
    return body;
  }

  free(): void { this.#handle.free(); }
}

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
  #zone: TrialZoneHandle | null = null;
  #prediction: OwnBodyPrediction | null = null;
  #player: number | null = null;
  #partner: number | null = null;
  #partnerSequence = 0;
  #sequence = 0;
  /** Local ticks run since the match began. */
  #ticks = 0;
  #uplink: { due: number; sequence: number; bytes: Uint8Array }[] = [];
  #downlink: { due: number; bytes: Uint8Array }[] = [];
  #latest: ZoneSnapshot | null = null;
  /** The joined session's projections as its delayed downlink delivers them, decoded once in WASM (#1009). */
  #projections: ProjectionDecoder | null = null;

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
    this.#prediction = this.#trial.option === "baseline" ? null : new OwnBodyPrediction(this.#createPrediction());
    this.#player = player;
    this.#clock.reset();
    this.#projections = new ProjectionDecoder();
    this.#sequence = 0;
    this.#partnerSequence = 0;
    this.#ticks = 0;
    this.#uplink = [];
    this.#downlink = [];
    // The joining projection arrives at once, like an online welcome, with or without prediction.
    const welcome = this.#receive(zone.projection(player));
    this.#prediction?.receive([welcome]);
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
    this.#prediction?.receive(received);
    this.#prediction?.record();
    return received;
  }

  #receive(bytes: Uint8Array): ZoneSnapshot {
    this.#projections ??= new ProjectionDecoder();
    const snapshot = this.#projections.decode(bytes);
    this.#latest = snapshot;
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

  shown(): ShownUnits {
    // The own body is drawn where the prediction puts it, between its last two ticks.
    const fraction = this.#clock.fraction;
    return interpolated(fraction, this.#prediction?.ownBody(fraction) ?? null);
  }
}
