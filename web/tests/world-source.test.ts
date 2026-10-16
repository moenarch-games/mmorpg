import { describe, expect, test } from "bun:test";
import { encodeCommand } from "../src/command-wire";
import { MAX_CATCH_UP_TICKS } from "../src/demo-clock";
import { LocalZoneSource, type LocalZoneHandle } from "../src/world/local-zone-source";
import { FACING_INTERVAL_MS, MovementOutbox, RESEND_INTERVAL_MS, type MovementInput } from "../src/world/movement-outbox";
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
import { encodeTestSnapshot } from "./support/snapshot-encoder";
import { playerEntity, testSnapshot } from "./support/snapshots";
import { immediate, worldSourceContract } from "./support/world-source-contract";

/** Records what `LocalZoneSource` asks of the WASM zone; returns well-formed projections. */
class RecordingZone implements LocalZoneHandle {
  readonly submitted: { player: number; sequence: number; bytes: string }[] = [];
  readonly left: number[] = [];
  /** `[player, class, sex]` of every join; the zone submits the choice as sequence 1. */
  readonly joins: [number, number, number][] = [];
  ticks = 0;
  viewerOverride: number | null = null;
  /** While set, projections are truncated bytes the strict decoder rejects. */
  corruptProjections = false;
  #nextPlayer = 1;
  #players = new Set<number>();

  join(classId: number, sex: number): number {
    const player = this.#nextPlayer;
    this.#nextPlayer += 1;
    this.#players.add(player);
    this.joins.push([player, classId, sex]);
    return player;
  }

  leave(player: number): boolean {
    this.left.push(player);
    return this.#players.delete(player);
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
    return encodeTestSnapshot(testSnapshot({
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
worldSourceContract("LocalZoneSource over a recording zone", () => immediate(new LocalZoneSource(new RecordingZone())), () => {
  const zone = new RecordingZone();
  zone.corruptProjections = true;
  return { ...immediate(new LocalZoneSource(zone)), repair: () => { zone.corruptProjections = false; } };
});

describe("LocalZoneSource", () => {
  test("sends encoded commands with strictly increasing sequences per player", async () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(zone);
    const first = await source.join({ classId: "arcanist", sex: "female" });
    source.sendCommand({ kind: "move", forward: 1, strafe: -1, facing: 16_384 });
    source.sendCommand({ kind: "jump" });
    source.leave();
    const second = await source.join();
    source.sendCommand({ kind: "jump" });
    // Joining spent sequence 1 on the class choice; the default is a male Warden.
    expect(zone.joins).toEqual([[first, 2, 0], [second, 0, 1]]);
    expect(zone.submitted).toEqual([
      { player: first, sequence: 2, bytes: "090101ff4000" },
      { player: first, sequence: 3, bytes: Buffer.from(encodeCommand({ kind: "jump" })).toString("hex") },
      { player: second, sequence: 2, bytes: "0902" },
    ]);
    expect(zone.left).toEqual([first]);
  });

  test("runs fixed ticks from a bounded accumulator only while joined", async () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(zone);
    source.advance(1);
    expect(zone.ticks).toBe(0);
    await source.join();
    source.advance(0.5 / 30);
    expect(zone.ticks).toBe(0);
    source.advance(0.5 / 30 + 1e-9);
    expect(zone.ticks).toBe(1);
    source.advance(10);
    expect(zone.ticks).toBe(1 + MAX_CATCH_UP_TICKS);
    expect(source.latestProjection()?.tick).toBe(BigInt(1 + MAX_CATCH_UP_TICKS));
  });

  test("samples interpolate one tick behind the latest projection", async () => {
    const zone = new RecordingZone();
    const source = new LocalZoneSource(zone);
    await source.join();
    source.advance(2 / 30 + 1e-9);
    expect(zone.ticks).toBe(2);
    expect(source.sample()[0]?.position[0]).toBeCloseTo(21, 3);
    source.advance(0.5 / 30);
    expect(source.sample()[0]?.position[0]).toBeCloseTo(31.5, 3);
  });

  test("a projection addressed to another player is rejected", async () => {
    const zone = new RecordingZone();
    zone.viewerOverride = 99;
    await expect(new LocalZoneSource(zone).join()).rejects.toThrow("another player");
  });

  test("a rejected first projection removes the unit it just added from the zone", async () => {
    for (const [fault, message] of [["corrupt", "Truncated snapshot"], ["misaddressed", "another player"]] as const) {
      const zone = new RecordingZone();
      if (fault === "corrupt") zone.corruptProjections = true;
      else zone.viewerOverride = 99;
      const source = new LocalZoneSource(zone);
      await expect(source.join()).rejects.toThrow(message);
      expect(zone.left).toEqual([1]);
      zone.corruptProjections = false;
      zone.viewerOverride = null;
      expect(await source.join()).toBe(2);
      source.sendCommand({ kind: "jump" });
      expect(zone.submitted).toEqual([{ player: 2, sequence: 2, bytes: "0902" }]);
    }
  });
});

describe("movement outbox", () => {
  const input = (patch: Partial<MovementInput> = {}): MovementInput => ({ forward: 0, strafe: 0, facing: 0, jumps: 0, ...patch });

  test("sends current intent first, then only changes, and resends on the heartbeat", () => {
    const outbox = new MovementOutbox(input());
    expect(outbox.update(input(), 0)).toEqual([{ kind: "move", forward: 0, strafe: 0, facing: 0 }]);
    expect(outbox.update(input(), 10)).toEqual([]);
    expect(outbox.update(input({ forward: 1 }), 11)).toEqual([{ kind: "move", forward: 1, strafe: 0, facing: 0 }]);
    expect(outbox.update(input({ forward: 1 }), 11 + RESEND_INTERVAL_MS - 1)).toEqual([]);
    expect(outbox.update(input({ forward: 1 }), 11 + RESEND_INTERVAL_MS)).toEqual([{ kind: "move", forward: 1, strafe: 0, facing: 0 }]);
  });

  test("throttles facing-only changes to one per server tick", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input({ forward: 1 }), 0);
    expect(outbox.update(input({ forward: 1, facing: 100 }), 1)).toEqual([]);
    expect(outbox.update(input({ forward: 1, facing: 200 }), FACING_INTERVAL_MS)).toEqual([
      { kind: "move", forward: 1, strafe: 0, facing: 200 },
    ]);
  });

  test("each advance of the press counter sends exactly one jump before the move", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input(), 0);
    expect(outbox.update(input({ jumps: 3 }), 1)).toEqual([{ kind: "jump" }]);
    expect(outbox.update(input({ jumps: 3 }), 2)).toEqual([]);
    expect(outbox.update(input({ jumps: 4, strafe: 1 }), 3)).toEqual([
      { kind: "jump" },
      { kind: "move", forward: 0, strafe: 1, facing: 0 },
    ]);
  });

  test("the dead send no held movement or jumps; living again resumes the held keys", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input({ forward: 1, strafe: -1, facing: 300 }), 0);
    // Death lets go at once, at the facing last sent, whatever is held or pressed.
    expect(outbox.update(input({ forward: 1, strafe: -1, facing: 900, jumps: 1 }), 1, true)).toEqual([
      { kind: "move", forward: 0, strafe: 0, facing: 300 },
    ]);
    expect(outbox.update(input({ forward: 1, facing: 900, jumps: 2 }), 2, true)).toEqual([]);
    expect(outbox.update(input({ forward: 1, facing: 900, jumps: 2 }), 1 + RESEND_INTERVAL_MS, true)).toEqual([
      { kind: "move", forward: 0, strafe: 0, facing: 300 },
    ]);
    // Presses made while dead are never replayed after release.
    expect(outbox.update(input({ forward: 1, facing: 900, jumps: 2 }), 1 + RESEND_INTERVAL_MS + 1)).toEqual([
      { kind: "move", forward: 1, strafe: 0, facing: 900 },
    ]);
  });

  test("a new session drops earlier presses and resends intent", () => {
    const outbox = new MovementOutbox(input());
    outbox.update(input({ forward: 1 }), 0);
    outbox.reset(input({ jumps: 5 }));
    expect(outbox.update(input({ jumps: 5 }), 1)).toEqual([{ kind: "move", forward: 0, strafe: 0, facing: 0 }]);
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
