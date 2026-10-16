import { expect, test } from "bun:test";
import type { WorldCommand } from "../../src/command-wire";
import type { EntityState, ZoneSnapshot } from "../../src/replication";
import type { WorldSource } from "../../src/world/world-source";

const ownUnit = (entities: readonly EntityState[], player: number) =>
  entities.find((entity) => entity.kind === "player" && entity.entityId === player);

/**
 * A source under test plus control of its zone's time. A local zone advances
 * only through `advance`; an online source's host runs on its own clock,
 * which `elapse` moves and `settle` runs until a pending promise settles.
 */
export type ContractSubject = {
  source: WorldSource;
  /** Lets `seconds` of host time pass (a no-op for a local zone). */
  elapse(seconds: number): Promise<void>;
  /** Resolves or rejects with `promise`, running host time until it settles. */
  settle<T>(promise: Promise<T>): Promise<T>;
};

/** A subject whose joins are refused until `repair()`, e.g. because its first projection is rejected. */
export type RefusingSubject = ContractSubject & { repair(): void };

/** A subject for a source that needs no host time, like a local zone. */
export function immediate(source: WorldSource): ContractSubject {
  return { source, elapse: async () => undefined, settle: (promise) => promise };
}

/** Behaviour every `WorldSource` owes the presentation layer, local or online. */
export function worldSourceContract(
  name: string,
  create: () => ContractSubject,
  createRefusing: () => RefusingSubject,
): void {
  /** Host time passes, then the presentation loop observes it. */
  const pass = async ({ source, elapse }: ContractSubject, seconds: number) => {
    await elapse(seconds);
    source.advance(seconds);
  };

  test(`${name}: a joined player receives projections addressed to it that contain it`, async () => {
    const subject = create();
    const { source } = subject;
    expect(source.latestProjection()).toBeNull();
    expect(source.sample()).toEqual([]);
    const player = await subject.settle(source.join());
    const projection = source.latestProjection();
    expect(projection?.viewerId).toBe(player);
    expect(ownUnit(projection?.entities ?? [], player)).toBeDefined();
    expect(ownUnit(source.sample(), player)).toBeDefined();
    expect(source.linkState()).toBe("connected");
    source.leave();
  });

  test(`${name}: joining twice or commanding without a player fails closed`, async () => {
    const subject = create();
    const { source } = subject;
    const move: WorldCommand = { kind: "move", forward: 1, strafe: 0, facing: 0 };
    expect(() => source.sendCommand(move)).toThrow();
    await subject.settle(source.join());
    await expect(subject.settle(source.join())).rejects.toThrow();
    source.sendCommand(move);
    source.sendCommand({ kind: "jump" });
    source.leave();
    expect(() => source.sendCommand(move)).toThrow();
    source.leave();
  });

  test(`${name}: advancing time moves projections forward and never back`, async () => {
    const subject = create();
    const { source } = subject;
    await subject.settle(source.join());
    const ticks: bigint[] = [];
    const record = () => ticks.push((source.latestProjection() as ZoneSnapshot).tick);
    record();
    for (const seconds of [0, 0.01, 0.05, 0.05, 0.2]) {
      await pass(subject, seconds);
      record();
    }
    expect(ticks.at(-1)! > ticks[0]!).toBe(true);
    for (let index = 1; index < ticks.length; index += 1) {
      expect(ticks[index]! >= ticks[index - 1]!).toBe(true);
    }
    source.leave();
  });

  test(`${name}: a refused join leaves the source unjoined so entry can be retried`, async () => {
    const subject = createRefusing();
    const { source } = subject;
    await expect(subject.settle(source.join())).rejects.toThrow();
    expect(source.latestProjection()).toBeNull();
    expect(source.sample()).toEqual([]);
    expect(() => source.sendCommand({ kind: "jump" })).toThrow();
    await pass(subject, 0.1);
    expect(source.latestProjection()).toBeNull();
    source.leave();
    subject.repair();
    const player = await subject.settle(source.join());
    const projection = source.latestProjection();
    expect(projection?.viewerId).toBe(player);
    // The refused entry left no unit behind in the world.
    const players = projection?.entities.filter((entity) => entity.kind === "player");
    expect(players?.map((entity) => entity.entityId)).toEqual([player]);
    source.sendCommand({ kind: "jump" });
    source.leave();
  });

  test(`${name}: leaving clears presentation and the next join is a new player`, async () => {
    const subject = create();
    const { source } = subject;
    const first = await subject.settle(source.join());
    await pass(subject, 0.1);
    source.leave();
    expect(source.latestProjection()).toBeNull();
    expect(source.sample()).toEqual([]);
    await pass(subject, 0.1);
    expect(source.latestProjection()).toBeNull();
    const second = await subject.settle(source.join());
    expect(second).not.toBe(first);
    const projection = source.latestProjection();
    expect(projection?.viewerId).toBe(second);
    expect(ownUnit(projection?.entities ?? [], first)).toBeUndefined();
    source.leave();
  });
}
