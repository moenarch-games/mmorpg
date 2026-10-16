import { describe, expect, test } from "bun:test";
import type { EntityState } from "../src/replication";
import { parseSessionRoute } from "../src/session/route";
import {
  CONNECT_TIMEOUT_MS,
  RECONNECT_SETTLE_MS,
  SNAPSHOT_TIMEOUT_MS,
  OnlineZoneSource,
  MAX_RECEIVED_PROJECTIONS,
} from "../src/world/online-zone-source";
import { FakeZoneHost, ManualClock, TICK_MS, type FakeZoneHostOptions } from "./support/fake-zone-host";
import { worldSourceContract } from "./support/world-source-contract";

const EAST = 16_384;
const MOVE_EAST = { kind: "move", forward: 1, strafe: 0, facing: EAST } as const;
const JUMP = "0902";
/** The joining player's first command: `ChooseClass` for the default male Warden. */
const CLASS_CHOICE = "090b0001";

function online(options: FakeZoneHostOptions = {}) {
  const clock = new ManualClock();
  const host = new FakeZoneHost(clock, options).start();
  const source = new OnlineZoneSource({
    route: parseSessionRoute(host.url),
    contentRevision: 2n,
    connector: host.connector,
    clock,
  });
  const subject = {
    source,
    elapse: (seconds: number) => clock.advance(seconds * 1_000),
    settle: <T>(promise: Promise<T>) => clock.until(promise),
  };
  return { clock, host, source, subject };
}

/** Host time passes and the page renders a frame. */
async function frame(clock: ManualClock, source: OnlineZoneSource, milliseconds = TICK_MS): Promise<void> {
  await clock.advance(milliseconds);
  source.advance();
}

function self(source: OnlineZoneSource): EntityState {
  const projection = source.latestProjection();
  const entity = projection?.entities.find((candidate) => candidate.entityId === projection.viewerId);
  if (!entity) throw new Error("The viewer is missing from its projection");
  return entity;
}

// Zero grace, like a host that removes a closed session's unit at the next tick.
worldSourceContract("OnlineZoneSource over a protocol-faithful zone host", () => online().subject, () => {
  const { host, subject } = online();
  host.contentRevision = 3n;
  return { ...subject, repair: () => { host.contentRevision = 2n; } };
});

describe("OnlineZoneSource", () => {
  test("joins as the welcomed player and sends intent as strictly increasing command frames", async () => {
    const { clock, host, source } = online();
    const player = await clock.until(source.join());
    expect(player).toBe(1);
    expect(host.opened).toEqual([host.url]);
    source.sendCommand(MOVE_EAST);
    source.sendCommand({ kind: "jump" });
    await frame(clock, source);
    source.sendCommand({ ...MOVE_EAST, forward: 0 });
    expect(host.applied).toEqual([
      { playerId: 1, sequence: 1, payload: CLASS_CHOICE },
      { playerId: 1, sequence: 2, payload: "090101004000" },
      { playerId: 1, sequence: 3, payload: JUMP },
      { playerId: 1, sequence: 4, payload: "090100004000" },
    ]);
    expect(self(source).position[0]).toBeGreaterThan(0);
  });

  test("a queued intent learns the sequence it is sent under, behind one still in flight", async () => {
    const { clock, host, source } = online();
    await clock.until(source.join());
    host.toHost = () => [];
    // The class choice took sequence 1; the jump goes out as 2 and waits for its acknowledgement.
    expect(source.sendCommand({ kind: "jump" })).toBe(2);
    expect(source.sendCommand({ kind: "sell-item", npc: 3, bagSlot: 0, quantity: 1 })).toBe(3);
    host.toHost = (datagram) => [datagram];
    for (let frames = 0; frames < 6; frames += 1) {
      await frame(clock, source);
    }
    expect(host.applied.map(({ sequence }) => sequence)).toEqual([1, 2, 3]);
    expect(source.latestProjection()?.acknowledgedSequence).toBe(3);
  });

  test("a lost jump is resent until acknowledged, applied once, and movement waits behind it", async () => {
    const { clock, host, source } = online();
    await clock.until(source.join());
    let lose = true;
    host.toHost = (datagram) => {
      const jump = datagram.byteLength === 10 && datagram[9] === 2;
      if (jump && lose) {
        lose = false;
        return [];
      }
      return jump ? [datagram, datagram] : [datagram];
    };
    source.sendCommand({ kind: "jump" });
    source.sendCommand(MOVE_EAST);
    expect(host.applied.map(({ payload }) => payload)).toEqual([CLASS_CHOICE]);
    for (let frames = 0; frames < 6; frames += 1) {
      await frame(clock, source);
    }
    expect(host.applied.map(({ sequence, payload }) => [sequence, payload])).toEqual([[1, CLASS_CHOICE], [2, JUMP], [3, "090101004000"]]);
  });

  test("targeting and attack intents are resent until acknowledged and applied once, in order", async () => {
    const { clock, host, source } = online();
    await clock.until(source.join());
    let sent = 0;
    // The first intent's first copy and every second datagram after it are lost.
    host.toHost = (datagram) => (sent++ % 2 === 0 ? [] : [datagram]);
    source.sendCommand({ kind: "select-target", target: { kind: "creature", id: 7 } });
    source.sendCommand({ kind: "start-attack" });
    source.sendCommand({ kind: "stop-attack" });
    source.sendCommand({ kind: "release-spirit" });
    for (let frames = 0; frames < 20; frames += 1) {
      await frame(clock, source);
    }
    expect(host.applied.map(({ sequence, payload }) => [sequence, payload])).toEqual([
      [1, CLASS_CHOICE], [2, "09030200000007"], [3, "0904"], [4, "0905"], [5, "0906"],
    ]);
  });

  test("fragmented projections are reassembled and verified", async () => {
    // A one-player projection frame is 70 bytes: it arrives in fragments of at most 40.
    const { clock, host, source } = online({ datagramBytes: 40 });
    const sizes: number[] = [];
    host.toClient = (datagram) => {
      sizes.push(datagram.byteLength);
      return [datagram];
    };
    await clock.until(source.join());
    await frame(clock, source, 5 * TICK_MS);
    expect(source.latestProjection()?.tick).toBe(host.tick);
    expect(Math.max(...sizes)).toBeLessThanOrEqual(40);
    expect(sizes.length).toBeGreaterThanOrEqual(2 * Number(host.tick));
  });

  test("a projection from another content revision fails the join closed with a clear message", async () => {
    const { clock, host, source } = online({ contentRevision: 3n });
    await expect(clock.until(source.join())).rejects.toThrow("runs content revision 3, but this page loaded revision 2");
    expect(source.latestProjection()).toBeNull();
    // The refused player's session was closed, not left connected.
    await clock.advance(2 * TICK_MS);
    expect(host.players()).toEqual([]);
  });

  test("a projection that turns invalid while playing fails closed on the next frame", async () => {
    const { clock, host, source } = online();
    await clock.until(source.join());
    host.contentRevision = 4n;
    await clock.advance(TICK_MS);
    expect(() => source.advance()).toThrow("content revision 4");
    // The failure is reported once; the source is unjoined and can join again.
    expect(() => source.advance()).not.toThrow();
    expect(source.latestProjection()).toBeNull();
    expect(() => source.sendCommand({ kind: "jump" })).toThrow("Join the world");
    host.contentRevision = 2n;
    expect(await clock.until(source.join())).toBe(2);
  });

  test("forged snapshot frames fail closed", async () => {
    const { clock, host, source } = online();
    await clock.until(source.join());
    host.toClient = (datagram) => {
      const forged = datagram.slice();
      forged[forged.length - 1]! ^= 0xff;
      return [forged];
    };
    await clock.advance(TICK_MS);
    expect(() => source.sendCommand(MOVE_EAST)).toThrow("snapshot hash mismatch");
  });

  test("a class choice no projection acknowledged is chosen again first on the resumed connection", async () => {
    const { clock, host, source } = online({ graceTicks: 300n });
    let dropClassChoice = true;
    host.toHost = (datagram) => (dropClassChoice && datagram.byteLength === 12 && datagram[9] === 11 ? [] : [datagram]);
    const player = await clock.until(source.join());
    await frame(clock, source, 3 * TICK_MS);
    expect(host.applied).toEqual([]);
    dropClassChoice = false;
    host.lose(player);
    await clock.advance(0);
    await frame(clock, source, RECONNECT_SETTLE_MS);
    await frame(clock, source);
    await frame(clock, source);
    expect(source.linkState()).toBe("connected");
    expect(host.applied.map(({ sequence, payload }) => [sequence, payload])).toEqual([[2, CLASS_CHOICE], [3, "090100000000"]]);
  });

  test("projections waiting for a paused page are bounded to the newest second", async () => {
    const { clock, source } = online();
    await clock.until(source.join());
    source.advance();
    // A backgrounded page stops calling advance while projections keep arriving.
    await clock.advance(5 * MAX_RECEIVED_PROJECTIONS * TICK_MS);
    const latest = source.latestProjection()!;
    const received = source.advance();
    expect(received.length).toBe(MAX_RECEIVED_PROJECTIONS);
    expect(received.at(-1)).toBe(latest);
    expect(received.every((view, index) => index === 0 || view.tick > received[index - 1]!.tick)).toBe(true);
    expect(source.advance()).toEqual([]);
  });

  test("a clean close by the host is an interruption that resumes the same player", async () => {
    const { clock, host, source } = online({ graceTicks: 300n });
    const player = await clock.until(source.join());
    await frame(clock, source, 3 * TICK_MS);
    host.closeSession(player);
    await clock.advance(0);
    expect(source.linkState()).toBe("reconnecting");
    await frame(clock, source, RECONNECT_SETTLE_MS);
    await frame(clock, source);
    expect(source.linkState()).toBe("connected");
    expect(host.connectionEpoch(player)).toBe(2);
    expect(host.players()).toEqual([player]);
  });

  test("a lost connection resumes the same player on a new epoch, stopped, without replaying intent", async () => {
    const { clock, host, source } = online({ graceTicks: 300n });
    const player = await clock.until(source.join());
    source.sendCommand(MOVE_EAST);
    await frame(clock, source, 3 * TICK_MS);
    const before = source.latestProjection();
    host.lose(player);
    await clock.advance(0);
    expect(source.linkState()).toBe("reconnecting");
    // The last known scene stays visible; intent during the interruption is dropped.
    expect(source.latestProjection()).toBe(before);
    expect(source.sample().length).toBe(1);
    source.sendCommand({ kind: "jump" });
    const sequencesBefore = host.applied.length;
    await frame(clock, source, RECONNECT_SETTLE_MS - 1);
    expect(host.opened.length).toBe(1);
    await frame(clock, source, 1);
    expect(host.opened.length).toBe(2);
    expect(host.opened[1]).toMatch(/\/game\/matches\/zone-1\/reconnect\/[0-9a-f]{32}$/);
    await frame(clock, source);
    expect(source.linkState()).toBe("connected");
    expect(host.connectionEpoch(player)).toBe(2);
    expect(host.players()).toEqual([player]);
    // The resumed connection starts with a stopped move at the last facing, sequenced after everything sent.
    expect(host.applied.slice(sequencesBefore)).toEqual([{ playerId: player, sequence: 3, payload: "090100004000" }]);
    expect(host.unit(player)?.forward).toBe(0);
    // Presentation restarted from the new connection's projections.
    expect(source.sample()).toEqual(source.latestProjection()!.entities);
    source.sendCommand({ kind: "jump" });
    expect(host.applied.at(-1)).toEqual({ playerId: player, sequence: 4, payload: JUMP });
  });

  test("a resume the host refuses fails closed and never admits a new player", async () => {
    const { clock, host, source } = online({ graceTicks: 300n });
    const player = await clock.until(source.join());
    host.lose(player);
    host.refuseSessions = true;
    await clock.advance(RECONNECT_SETTLE_MS + TICK_MS);
    let failure: Error | null = null;
    try {
      source.advance();
    } catch (error) {
      failure = error as Error;
    }
    expect(failure?.message).toBe("The zone host did not resume this session.");
    // The one-time token never leaks into what the page shows.
    expect(failure?.message).not.toMatch(/[0-9a-f]{32}/);
    expect(host.opened.filter((url) => url === host.url)).toEqual([host.url]);
    expect(host.opened.length).toBe(2);
    expect(source.linkState()).toBe("connected");
    expect(source.latestProjection()).toBeNull();
  });

  test("five seconds without projections triggers the resume", async () => {
    const { clock, host, source } = online({ graceTicks: 900n });
    const player = await clock.until(source.join());
    host.toClient = () => [];
    await frame(clock, source, SNAPSHOT_TIMEOUT_MS - TICK_MS);
    expect(source.linkState()).toBe("connected");
    await frame(clock, source, 2 * TICK_MS);
    expect(source.linkState()).toBe("reconnecting");
    host.toClient = (datagram) => [datagram];
    await frame(clock, source, RECONNECT_SETTLE_MS + 2 * TICK_MS);
    expect(source.linkState()).toBe("connected");
    expect(host.connectionEpoch(player)).toBe(2);
  });

  test("a resume is bounded by the host's reconnect grace", async () => {
    const { clock, host, source } = online({ graceTicks: 3n });
    const player = await clock.until(source.join());
    host.lose(player);
    await clock.advance(RECONNECT_SETTLE_MS);
    expect(() => source.advance()).toThrow("Resuming the session timed out");
    expect(host.opened.length).toBe(1);
  });

  test("a second interruption after a completed resume resumes again", async () => {
    const { clock, host, source } = online({ graceTicks: 300n });
    const player = await clock.until(source.join());
    for (const epoch of [2, 3]) {
      host.lose(player);
      await frame(clock, source, RECONNECT_SETTLE_MS + 2 * TICK_MS);
      expect(source.linkState()).toBe("connected");
      expect(host.connectionEpoch(player)).toBe(epoch);
    }
  });

  test("leaving cancels a pending resume and a pending join", async () => {
    const { clock, host, source } = online({ graceTicks: 300n });
    const player = await clock.until(source.join());
    host.lose(player);
    source.leave();
    await clock.advance(1_000);
    expect(host.opened.length).toBe(1);
    expect(source.linkState()).toBe("connected");
    expect(() => source.advance()).not.toThrow();

    const joining = source.join();
    source.leave();
    await expect(joining).rejects.toThrow("cancelled");
    await clock.advance(1_000);
    expect(source.latestProjection()).toBeNull();
    // Both admitted units were closed; they leave after the grace.
    await clock.advance(11_000);
    expect(host.players()).toEqual([]);
  });

  test("a join without a projection times out", async () => {
    const { clock, host, source } = online();
    host.stop();
    const joining = source.join();
    let settled = false;
    joining.then(() => undefined, () => { settled = true; });
    await clock.advance(CONNECT_TIMEOUT_MS - 1);
    expect(settled).toBe(false);
    await clock.advance(1);
    await expect(joining).rejects.toThrow("did not admit this player");
  });

  test("a refused admission rejects the join", async () => {
    const { clock, host, source } = online();
    host.refuseSessions = true;
    await expect(clock.until(source.join())).rejects.toThrow("Could not join the zone host: Opening handshake failed.");
  });

  test("samples run two ticks behind the newest projection and hold it when projections stall", async () => {
    const { clock, host, source } = online();
    const player = await clock.until(source.join());
    source.sendCommand(MOVE_EAST);
    for (let ticks = 0; ticks < 6; ticks += 1) {
      await frame(clock, source);
    }
    const newest = self(source).position[0];
    const sampled = source.sample().find((entity) => entity.entityId === player)!.position[0];
    expect(sampled).toBeCloseTo(newest - 2 * 21, 6);
    host.toClient = () => [];
    await frame(clock, source, TICK_MS / 2);
    expect(source.sample().find((entity) => entity.entityId === player)!.position[0]).toBeCloseTo(newest - 1.5 * 21, 6);
    await frame(clock, source, 3 * TICK_MS);
    expect(source.sample().find((entity) => entity.entityId === player)!.position[0]).toBe(newest);
  });

  test("another player's unit moves in this player's projections and leaves after the host's grace", async () => {
    const clock = new ManualClock();
    const host = new FakeZoneHost(clock, { graceTicks: 30n }).start();
    const connect = () => new OnlineZoneSource({ route: parseSessionRoute(host.url), contentRevision: 2n, connector: host.connector, clock });
    const mover = connect();
    const watcher = connect();
    const moverId = await clock.until(mover.join());
    await clock.until(watcher.join());
    const seen = () => watcher.latestProjection()?.entities.find((entity) => entity.entityId === moverId);
    const start = seen()!.position[0];
    mover.sendCommand(MOVE_EAST);
    await frame(clock, mover, 10 * TICK_MS);
    watcher.advance();
    expect(seen()!.position[0]).toBeGreaterThan(start + 100);
    mover.leave();
    await clock.advance(30 * TICK_MS);
    watcher.advance();
    expect(seen()).toBeDefined();
    await clock.advance(3 * TICK_MS);
    watcher.advance();
    expect(seen()).toBeUndefined();
  });
});
