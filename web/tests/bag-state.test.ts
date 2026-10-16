import { expect, test } from "bun:test";
import type { InventorySlot } from "../src/replication";
import { BagState } from "../src/world/units/bag-state";
import { HEALTHY_VIEWER, playerEntity, testSnapshot } from "./support/snapshots";

function bag(fur = 3): InventorySlot[] {
  return [{ itemId: 1, quantity: fur }, { itemId: 2, quantity: 1 }, ...Array<null>(14).fill(null)];
}

function projection(tick: number, revision = 1n, inventory: readonly InventorySlot[] | null = bag()) {
  return testSnapshot({ zoneId: 1, contentRevision: 4n, viewerId: 1, tick: BigInt(tick),
    acknowledgedSequence: 0, entities: [playerEntity(1, [0, 90, 0])], inventoryRevision: revision, inventory });
}

test("moves create intent while received slots remain unchanged", () => {
  const state = new BagState();
  state.update(projection(0));
  const before = state.slots;
  expect(state.move(0, 15, 2)).toEqual({ kind: "move-item", source: 0, destination: 15, quantity: 2 });
  expect(state.slots).toBe(before);
  expect(state.slots?.[0]).toEqual({ itemId: 1, quantity: 3 });
  const next = bag(1); next[15] = { itemId: 1, quantity: 2 };
  state.update(projection(1, 2n, next));
  expect(state.slots).toEqual(next);
  expect(state.feedback).toBe("Bag updated.");
});

test("omitted sheets retain slots but a missed revision disables intent until periodic recovery", () => {
  const state = new BagState();
  expect(state.move(0, 1, 1)).toBeNull();
  state.update(projection(0));
  state.update(projection(1, 1n, null));
  expect(state.ready).toBe(true);
  state.update(projection(2, 2n, null));
  expect(state.slots).toEqual(bag());
  expect(state.ready).toBe(false);
  expect(state.move(0, 15, 1)).toBeNull();
  state.update(projection(10, 2n, bag(1)));
  expect(state.ready).toBe(true);
  expect(state.slots).toEqual(bag(1));
});

test("stale ticks, lower revisions and other session identities cannot replace a bag", () => {
  const state = new BagState();
  const latest = projection(10, 2n, bag(1));
  state.update(latest);
  for (const invalid of [projection(9), projection(10), projection(11),
    { ...projection(12, 3n), viewerId: 2 }, { ...projection(12, 3n), zoneId: 2 },
    { ...projection(12, 3n), contentRevision: 5n }]) {
    expect(state.update(invalid)).toBe(false);
    expect(state.slots).toEqual(bag(1));
  }
  state.reset(projection(0));
  expect(state.slots).toBeNull();
  expect(state.feedback).toBe("");
  expect(state.update({ ...latest, viewerId: 2 })).toBe(false);
  expect(state.update(projection(0))).toBe(true);
  expect(state.slots).toEqual(bag());
});

test("UI input bounds and death cannot issue item grants or mutate cached stacks", () => {
  const state = new BagState();
  state.update(projection(0));
  for (const [source, destination, quantity] of [[-1, 0, 1], [16, 0, 1], [0, 16, 1],
    [15, 0, 1], [0, 1, 0], [0, 1, 4], [0, 1, 1.5], [0, 1, NaN]]) {
    expect(state.move(source!, destination!, quantity!)).toBeNull();
  }
  state.update({ ...projection(1), viewer: { ...HEALTHY_VIEWER, health: 0, dead: true } });
  expect(state.canMove).toBe(false);
  expect(state.move(0, 15, 1)).toBeNull();
  expect(state.slots).toEqual(bag());
});

test("refusal feedback survives omitted sheets and copying isolates received records", () => {
  const state = new BagState();
  const received = bag();
  state.update(projection(0, 1n, received));
  received[0]!.quantity = 99;
  expect(state.slots).toEqual(bag());
  state.move(0, 1, 1);
  state.update({ ...projection(1, 1n, null), events: [{ kind: "error", code: "invalid-inventory-move", target: null }] });
  expect(state.feedback).toContain("refused");
  state.update(projection(2, 1n, null));
  expect(state.feedback).toContain("refused");
  expect(state.slots).toEqual(bag());
});

test("equipment and stat totals arrive and are retained with the bag sheet", () => {
  const state = new BagState();
  const stats = { stamina: 0, strength: 2, agility: 2, intellect: 0 };
  const equipment = [2, null, null, null, null, null];
  const unequipped = bag(); unequipped[1] = null;
  state.update({ ...projection(0, 2n, unequipped), equipment, stats });
  expect(state.equipment).toEqual(equipment);
  expect(state.stats).toEqual(stats);
  equipment[0] = null;
  expect(state.equipment?.[0]).toBe(2);
  state.update(projection(1, 2n, null));
  expect(state.equipment).toEqual([2, null, null, null, null, null]);
  expect(state.stats).toEqual(stats);
  state.reset(projection(0));
  expect(state.equipment).toBeNull();
  expect(state.stats).toBeNull();
});
