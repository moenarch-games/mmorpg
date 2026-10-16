import { expect, test } from "bun:test";
import type { EntityRef } from "../src/entity-ref";
import type { EntityState, ErrorCode, InventorySlot, Vector3, ZoneSnapshot } from "../src/replication";
import { decodeCatalog } from "../src/world/catalog";
import { BagState } from "../src/world/units/bag-state";
import { nearestVendor, VENDOR_REACH_UNITS, VendorState } from "../src/world/units/vendor-state";
import { catalogJson } from "./support/catalog";
import { HEALTHY_VIEWER, NO_FLAGS, playerEntity, testSnapshot } from "./support/snapshots";

// The test catalog's vendor (NPC 3) offers the Worn Dagger for 9 copper; NPC 6 is a guard.
const catalog = decodeCatalog(JSON.stringify(catalogJson()));

function npc(id: number, position: Vector3): EntityState {
  return { kind: "npc", entityId: id, appearance: id, position, velocity: [0, 0, 0], facing: 0, level: 5, healthPercent: 100, flags: NO_FLAGS };
}

function bag(): InventorySlot[] {
  return [{ itemId: 1, quantity: 3 }, { itemId: 2, quantity: 1 }, ...Array<null>(14).fill(null)];
}

function projection(tick: number, fields: Partial<ZoneSnapshot> = {}, vendorAt: Vector3 = [300, 0, 0]): ZoneSnapshot {
  return testSnapshot({
    zoneId: 1, contentRevision: 4n, viewerId: 1, tick: BigInt(tick), acknowledgedSequence: 0,
    entities: [playerEntity(1, [0, 90, 0]), npc(6, [100, 0, 0]), npc(3, vendorAt)],
    inventoryRevision: 1n, inventory: bag(), viewer: { ...HEALTHY_VIEWER, copper: 10 }, ...fields,
  });
}

function error(code: ErrorCode, target: EntityRef | null) {
  return [{ kind: "error" as const, code, target }];
}

test("the nearest projected catalog vendor is offered, with reach measured in XZ like the zone", () => {
  const near = nearestVendor(projection(0), catalog);
  expect(near).toEqual({ npc: 3, name: "Innkeeper Bram Tolliver", inReach: true, offers: [{ item: 2, price: 9 }] });
  expect(nearestVendor(projection(0, {}, [VENDOR_REACH_UNITS, 0, 0]), catalog)?.inReach).toBe(true);
  expect(nearestVendor(projection(0, {}, [VENDOR_REACH_UNITS + 1, 0, 0]), catalog)?.inReach).toBe(false);
  // The guard is nearer but sells nothing.
  expect(nearestVendor(projection(0, { entities: [playerEntity(1, [0, 90, 0]), npc(6, [10, 0, 0])] }), catalog)).toBeNull();
});

test("an explicitly selected non-vendor or missing NPC never redirects trade to nearby Bram", () => {
  for (const id of [1, 6, 999]) {
    const snapshot = projection(1, { viewer: { ...HEALTHY_VIEWER, target: { kind: "npc", id } } });
    expect(nearestVendor(snapshot, catalog)).toBeNull();
  }
});

test("buying and selling send intent and wait for the sheet without predicting copper or items", () => {
  const bags = new BagState();
  const vendor = new VendorState();
  const first = projection(0);
  bags.update(first);
  vendor.update(first, bags);
  const near = nearestVendor(first, catalog);
  expect(vendor.canTrade(near, bags)).toBe(true);
  expect(vendor.buy(near, 0, bags)).toEqual({ kind: "buy-item", npc: 3, offer: 0, quantity: 1 });
  expect(vendor.pending).toBe(true);
  expect(vendor.copper).toBe(10);
  // One trade at a time.
  expect(vendor.sell(near, 0, bags)).toBeNull();
  const bought = [...bag()];
  bought[2] = { itemId: 2, quantity: 1 };
  const second = projection(1, { inventoryRevision: 2n, inventory: bought, viewer: { ...HEALTHY_VIEWER, copper: 1 } });
  bags.update(second);
  vendor.update(second, bags);
  expect(vendor.feedback).toBe("Purchased.");
  expect(vendor.copper).toBe(1);
  // Selling sells the whole stack in the slot.
  expect(vendor.sell(near, 0, bags)).toEqual({ kind: "sell-item", npc: 3, bagSlot: 0, quantity: 3 });
  const sold = [...bought];
  sold[0] = null;
  const third = projection(2, { inventoryRevision: 3n, inventory: sold, viewer: { ...HEALTHY_VIEWER, copper: 4 } });
  bags.update(third);
  vendor.update(third, bags);
  expect(vendor.feedback).toBe("Sold.");
  expect(vendor.sell(near, 0, bags)).toBeNull();
});

test("offers beyond the copper balance, out of reach, while dead or with a paused bag are not offered", () => {
  const bags = new BagState();
  const vendor = new VendorState();
  const poor = projection(0, { viewer: { ...HEALTHY_VIEWER, copper: 8 } });
  bags.update(poor);
  vendor.update(poor, bags);
  expect(vendor.buy(nearestVendor(poor, catalog), 0, bags)).toBeNull();
  expect(vendor.buy(nearestVendor(poor, catalog), 1, bags)).toBeNull();
  const far = projection(1, {}, [VENDOR_REACH_UNITS + 1, 0, 0]);
  bags.update(far);
  vendor.update(far, bags);
  expect(vendor.canTrade(nearestVendor(far, catalog), bags)).toBe(false);
  const dead = projection(2, { viewer: { ...HEALTHY_VIEWER, copper: 10, health: 0, dead: true } });
  bags.update(dead);
  vendor.update(dead, bags);
  expect(vendor.sell(nearestVendor(dead, catalog), 0, bags)).toBeNull();
  const paused = projection(3, { inventoryRevision: 2n, inventory: null });
  bags.update(paused);
  vendor.update(paused, bags);
  expect(vendor.canTrade(nearestVendor(paused, catalog), bags)).toBe(false);
});

test("refusals addressed to the vendor read as trade feedback; other units' refusals do not", () => {
  const cases: [ErrorCode, string][] = [
    ["not-enough-money", "enough copper"],
    ["inventory-full", "bags cannot hold"],
    ["invalid-vendor", "does not trade"],
    ["out-of-range", "Move closer"],
    ["money-overflow", "more copper"],
    ["invalid-inventory-move", "refused"],
  ];
  for (const [code, message] of cases) {
    const bags = new BagState();
    const vendor = new VendorState();
    const first = projection(0);
    bags.update(first);
    vendor.update(first, bags);
    expect(vendor.sell(nearestVendor(first, catalog), 1, bags)).not.toBeNull();
    const unrelated = projection(1, { events: error(code, { kind: "creature", id: 3 }) });
    bags.update(unrelated);
    vendor.update(unrelated, bags);
    expect(vendor.pending).toBe(true);
    const refused = projection(2, { events: error(code, { kind: "npc", id: 3 }) });
    bags.update(refused);
    vendor.update(refused, bags);
    expect(vendor.feedback).toContain(message);
    expect(vendor.pending).toBe(false);
  }
});

test("a refused trade whose feedback was lost unlocks once a later projection acknowledges it", () => {
  const bags = new BagState();
  const vendor = new VendorState();
  const first = projection(0, { acknowledgedSequence: 3 });
  bags.update(first);
  vendor.update(first, bags);
  expect(vendor.sell(nearestVendor(first, catalog), 0, bags)).not.toBeNull();
  // The source queued the trade behind another command: an acknowledgement of 4 is not its answer.
  vendor.sent(5);
  const waiting = projection(1, { acknowledgedSequence: 4, inventory: null });
  bags.update(waiting);
  vendor.update(waiting, bags);
  expect(vendor.pending).toBe(true);
  const acknowledged = projection(2, { acknowledgedSequence: 5, inventory: null });
  bags.update(acknowledged);
  vendor.update(acknowledged, bags);
  expect(vendor.pending).toBe(false);
  expect(vendor.feedback).toContain("did not trade");
  expect(vendor.canTrade(nearestVendor(acknowledged, catalog), bags)).toBe(true);
});

test("a confirmed trade wins over uncorrelated capacity feedback, and trades wait for pending bag moves", () => {
  const bags = new BagState();
  const vendor = new VendorState();
  const first = projection(0);
  bags.update(first);
  vendor.update(first, bags);
  expect(vendor.sell(nearestVendor(first, catalog), 0, bags)).not.toBeNull();
  const sold = [...bag()];
  sold[0] = null;
  const confirmed = projection(1, {
    inventoryRevision: 2n, inventory: sold, viewer: { ...HEALTHY_VIEWER, copper: 13 },
    events: [{ kind: "error", code: "too-many-intents", target: null }],
  });
  bags.update(confirmed);
  vendor.update(confirmed, bags);
  expect(vendor.feedback).toBe("Sold.");
  // A pending bag move could swap the slot a sale names.
  expect(bags.move(1, 5, 1)).not.toBeNull();
  expect(vendor.canTrade(nearestVendor(confirmed, catalog), bags)).toBe(false);
  expect(vendor.sell(nearestVendor(confirmed, catalog), 1, bags)).toBeNull();
});
