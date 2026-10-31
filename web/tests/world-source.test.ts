import { describe, expect, test } from "bun:test";
import { encodeCommand } from "../src/command-wire";
import { MAX_CATCH_UP_TICKS } from "../src/demo-clock";
import { TICK_HZ } from "../src/replication";
import { LocalZoneSource, type LocalZoneHandle } from "../src/world/local-zone-source";
import { MovementOutbox } from "../src/world/movement-outbox";
import {
  GROUND_CLEARANCE_METRES,
  MAX_DISTANCE_METRES,
  MIN_DISTANCE_METRES,
  ORBIT_RADIANS_PER_PIXEL,
  IDLE_INTENT,
  OrbitCamera,
  movementInput,
  type HeldIntent,
} from "../src/world/orbit-camera";
import { FakeWorldSource } from "./support/fake-world-source";
import { testCharacters } from "./support/local-characters";
import { encodeSnapshot, PROTOCOL_BYTE } from "./support/wire";
import { playerEntity, testSnapshot } from "./support/snapshots";
import { immediate, worldSourceContract } from "./support/world-source-contract";

/** The protocol byte every command leads with, as hex. */
const P = PROTOCOL_BYTE.toString(16).padStart(2, "0");
/** The shared movement outbox resends the held intent every 50 ms (`mmorpg-client-model`, #543). */
const RESEND_INTERVAL_MS = 50;

/** Records what `LocalZoneSource` asks of the WASM zone; returns well-formed projections. */
class RecordingZone implements LocalZoneHandle {
  readonly submitted: { player: number; sequence: number; bytes: string }[] = [];
  readonly left: number[] = [];
  /** `[player, character, class, sex]` of every entry. */
  readonly joins: [number, string, number, number][] = [];
  saves = 0;
  ticks = 0;
  viewerOverride: number | null = null;
  /** While set, projections are truncated bytes the strict decoder rejects. */
  corruptProjections = false;
  #nextPlayer = 1;
  #players = new Set<number>();

  enter(_entries: unknown, characterId: string, classId: number, sex: number): number {
    const player = this.#nextPlayer;
    this.#nextPlayer += 1;
    this.#players.add(player);
    this.joins.push([player, characterId, classId, sex]);
    return player;
  }

  save(): bigint {
    this.saves += 1;
    return BigInt(this.saves);
  }

  checkpoint(): Uint8Array {
    return Uint8Array.of(this.ticks);
  }

  exit(_entries: unknown, player: number): bigint {
    this.left.push(player);
    this.#players.delete(player);
    return 0n;
  }

  submit(player: number, sequence: number, command: Uint8Array): boolean {
    this.submitted.push({ player, sequence, bytes: Buffer.from(command).toString("hex") });
    return true;
  }

  tick(): bigint {
    this.ticks += 1;
    return BigInt(this.ticks);
  }

  projection(player: number): Uint8Array {
    if (!this.#players.has(player)) throw new Error("unknown player");
    if (this.corruptProjections) return new Uint8Array([3]);
    const viewer = this.viewerOverride ?? player;
    return encodeSnapshot(testSnapshot({
      zoneId: 1, tick: BigInt(this.ticks), contentRevision: 1n, acknowledgedSequence: 0, viewerId: viewer,
      entities: [playerEntity(viewer, [this.ticks * 21, 90, 0], [21, 0, 0])],
    }));
  }
}

worldSourceContract("FakeWorldSource", () => immediate(new FakeWorldSource()), () => {
  const source = new FakeWorldSource();
  source.refuseJoins = true;
  return { ...immediate(source), repair: () => { source.refuseJoins = false; } };
});
worldSourceContract("LocalZoneSource over a recording zone", () => immediate(new LocalZoneSource(() => new RecordingZone(), testCharacters())), () => {
  const zone = new RecordingZone();
  zone.corruptProjections = true;
  return { ...immediate(new LocalZoneSource(() => zone, testCharacters())), repair: () => { zone.corruptProjections = false; } };
});

describe("LocalZoneSource", () => {
  test("sends encoded commands with strictly increasing sequences per player", async () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(() => zone, testCharacters());
    const first = await source.join({ id: "local-2", classId: "arcanist", sex: "female" });
    source.sendCommand({ kind: "move", forward: 1, strafe: -1, facing: 16_384 });
    source.sendCommand({ kind: "jump" });
    source.leave();
    const second = await source.join();
    source.sendCommand({ kind: "jump" });
    // The default is a male Warden.
    expect(zone.joins).toEqual([[first, "local-2", 2, 0], [second, "default-warden", 0, 1]]);
    expect(zone.submitted).toEqual([
      { player: first, sequence: 1, bytes: `${P}0101ff4000` },
      { player: first, sequence: 2, bytes: Buffer.from(encodeCommand({ kind: "jump" })).toString("hex") },
      { player: second, sequence: 1, bytes: `${P}02` },
    ]);
    expect(zone.left).toEqual([first]);
  });

  test("runs fixed ticks from a bounded accumulator only while joined", async () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(() => zone, testCharacters());
    source.advance(1);
    expect(zone.ticks).toBe(0);
    await source.join();
    source.advance(0.5 / TICK_HZ);
    expect(zone.ticks).toBe(0);
    source.advance(0.5 / TICK_HZ + 1e-9);
    expect(zone.ticks).toBe(1);
    source.advance(10);
    expect(zone.ticks).toBe(1 + MAX_CATCH_UP_TICKS);
    expect(source.latestProjection()?.tick).toBe(BigInt(1 + MAX_CATCH_UP_TICKS));
  });

  test("samples interpolate one tick behind the latest projection", async () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(() => zone, testCharacters());
    await source.join();
    source.advance(2 / TICK_HZ + 1e-9);
    expect(zone.ticks).toBe(2);
    expect(source.sample()[0]?.position[0]).toBeCloseTo(21, 3);
    source.advance(0.5 / TICK_HZ);
    expect(source.sample()[0]?.position[0]).toBeCloseTo(31.5, 3);
  });

  test("a projection addressed to another player is rejected", async () => {
    const zone = new RecordingZone();
    zone.viewerOverride = 99;
    await expect(new LocalZoneSource(() => zone, testCharacters()).join()).rejects.toThrow("another player");
  });

  test("a rejected first projection removes the unit it just added from the zone", async () => {
    for (const [fault, message] of [["corrupt", "truncated"], ["misaddressed", "another player"]] as const) {
      const zone = new RecordingZone();
      if (fault === "corrupt") zone.corruptProjections = true;
      else zone.viewerOverride = 99;
      const source = new LocalZoneSource(() => zone, testCharacters());
      await expect(source.join()).rejects.toThrow(message);
      expect(zone.left).toEqual([1]);
      zone.corruptProjections = false;
      zone.viewerOverride = null;
      expect(await source.join()).toBe(2);
      source.sendCommand({ kind: "jump" });
      expect(zone.submitted).toEqual([{ player: 2, sequence: 1, bytes: `${P}02` }]);
    }
  });
});

/** Held movement as the semantic controls report it. */
function held(forward: HeldIntent["forward"], strafe: HeldIntent["strafe"]): HeldIntent {
  return { forward, strafe, steering: forward !== 0 || strafe !== 0 };
}

describe("camera-relative input", () => {
  test("while movement is held the character adopts the camera heading", () => {
    const camera = new OrbitCamera();
    camera.orbit(Math.PI / 2 / ORBIT_RADIANS_PER_PIXEL, 0);
    const heading = camera.facing();
    expect(heading).not.toBe(0);
    expect(movementInput(held(1, 0), heading, 0, 2)).toEqual({ forward: 1, strafe: 0, facing: heading, jumps: 2 });
    expect(movementInput(held(0, -1), heading, 0, 0)).toEqual({ forward: 0, strafe: -1, facing: heading, jumps: 0 });
    // Opposite movement cancels the run but still steers.
    expect(movementInput({ forward: 0, strafe: 0, steering: true }, heading, 0, 0).facing).toBe(heading);
  });

  test("while idle the character keeps its last facing, whatever the camera does", () => {
    expect(movementInput(IDLE_INTENT, 49_152, 16_384, 0)).toEqual({ forward: 0, strafe: 0, facing: 16_384, jumps: 0 });
    // Jumping alone does not steer.
    expect(movementInput(IDLE_INTENT, 49_152, 16_384, 1).facing).toBe(16_384);
  });

  test("orbiting while idle sends no new facing; the next step runs along the new view", () => {
    const camera = new OrbitCamera();
    const outbox = new MovementOutbox(movementInput(IDLE_INTENT, camera.facing(), 0, 0));
    expect(outbox.update(movementInput(IDLE_INTENT, camera.facing(), 0, 0), 0)).toEqual([
      { kind: "move", forward: 0, strafe: 0, facing: 0 },
    ]);
    camera.orbit(-Math.PI / 2 / ORBIT_RADIANS_PER_PIXEL, 0);
    const heading = camera.facing();
    expect(outbox.update(movementInput(IDLE_INTENT, heading, 0, 0), RESEND_INTERVAL_MS)).toEqual([
      { kind: "move", forward: 0, strafe: 0, facing: 0 },
    ]);
    const running = movementInput(held(1, 0), heading, 0, 0);
    expect(outbox.update(running, RESEND_INTERVAL_MS + 1)).toEqual([{ kind: "move", forward: 1, strafe: 0, facing: heading }]);
    // Stopping keeps the heading the character last ran along.
    expect(movementInput(IDLE_INTENT, 0, running.facing, 0).facing).toBe(heading);
  });

  test("the default orbit view looks over the shoulder toward +Z", () => {
    const camera = new OrbitCamera();
    const view = camera.view([1, 0.9, -2]);
    expect(view.target).toEqual([1, 1.5, -2]);
    expect(view.eye[2]).toBeLessThan(view.target[2]);
    expect(view.eye[1]).toBeGreaterThan(view.target[1]);
    expect(Math.hypot(...view.eye.map((value, axis) => value - view.target[axis]!))).toBeCloseTo(camera.distance, 6);
    expect(camera.facing()).toBe(0);
  });

  test("dragging right turns the heading toward the character's right", () => {
    const camera = new OrbitCamera();
    camera.orbit(Math.PI / 2 / ORBIT_RADIANS_PER_PIXEL, 0);
    expect(Math.abs(camera.facing() - 49_152)).toBeLessThanOrEqual(2);
    camera.orbit(-Math.PI / ORBIT_RADIANS_PER_PIXEL, 0);
    expect(Math.abs(camera.facing() - 16_384)).toBeLessThanOrEqual(2);
  });

  test("pitch and zoom are clamped and ignore non-finite input", () => {
    const camera = new OrbitCamera();
    camera.orbit(0, -1e9);
    const low = camera.view([0, 0, 0]);
    expect(low.eye[1]).toBeGreaterThan(low.target[1]);
    camera.zoom(1e3);
    expect(camera.targetDistance).toBe(MIN_DISTANCE_METRES);
    camera.update(0, true);
    expect(camera.distance).toBe(MIN_DISTANCE_METRES);
    camera.zoom(-1e3);
    camera.update(0, true);
    expect(camera.distance).toBe(MAX_DISTANCE_METRES);
    camera.zoom(Number.NaN);
    camera.orbit(Number.NaN, 0);
    camera.update(Number.NaN);
    expect(camera.distance).toBe(MAX_DISTANCE_METRES);
    expect(Number.isFinite(camera.facing())).toBe(true);
  });

  test("zoom eases toward its target and settles exactly", () => {
    const camera = new OrbitCamera();
    const start = camera.distance;
    camera.zoom(3);
    const target = camera.targetDistance;
    expect(target).toBeLessThan(start);
    expect(camera.distance).toBe(start);
    camera.update(1 / 60);
    expect(camera.distance).toBeLessThan(start);
    expect(camera.distance).toBeGreaterThan(target);
    for (let frame = 0; frame < 120; frame += 1) {
      camera.update(1 / 60);
    }
    expect(camera.distance).toBe(target);
  });

  test("the eye never sinks below the ground under it; nothing else collides", () => {
    const camera = new OrbitCamera();
    const flat = camera.view([0, 1, 0], () => 0);
    expect(flat).toEqual(camera.view([0, 1, 0]));
    const hill = camera.view([0, 1, 0], () => 12);
    expect(hill.eye[1]).toBeCloseTo(12 + GROUND_CLEARANCE_METRES, 9);
    expect(hill.eye[0]).toBe(flat.eye[0]);
    expect(hill.eye[2]).toBe(flat.eye[2]);
    expect(hill.target).toEqual(flat.target);
  });

  test("left drag looks around freely; right drag turns the character with the view", () => {
    const camera = new OrbitCamera();
    camera.orbit(Math.PI / 2 / ORBIT_RADIANS_PER_PIXEL, 0);
    const heading = camera.facing();
    // Right drag: the character adopts the view even while standing.
    expect(movementInput(IDLE_INTENT, heading, 16_384, 0, "turn").facing).toBe(heading);
    // Left drag: running keeps the character's own heading.
    expect(movementInput(held(1, 0), heading, 16_384, 0, "orbit")).toEqual({ forward: 1, strafe: 0, facing: 16_384, jumps: 0 });
    camera.lookAlong(Math.PI);
    expect(camera.facing()).toBe(32_768);
    expect(camera.heading).toBeCloseTo(Math.PI, 12);
  });
});
